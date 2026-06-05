//! Multi-connection async pool for the knowledge graph.
//!
//! Unlike `AsyncGraph` (single `Arc<Mutex<Connection>>`),
//! this module maintains a bounded pool of rusqlite connections gated by a
//! tokio `Semaphore`. Multiple read queries can execute concurrently in WAL
//! mode, while the semaphore bounds total concurrency.
//!
//! ```no_run
//! use llm_kernel::graph::AsyncPoolGraph;
//!
//! # #[tokio::main]
//! # async fn main() -> llm_kernel::error::Result<()> {
//! let pool = AsyncPoolGraph::open("my.db", 4).await?;
//! pool.upsert_node(llm_kernel::graph::GraphNode {
//!     id: "n1".into(),
//!     node_type: "concept".into(),
//!     title: "Example".into(),
//!     body: String::new(),
//!     tags: vec![],
//!     projects: vec![],
//!     agents: vec![],
//!     created: "2026-01-01T00:00:00Z".into(),
//!     updated: "2026-01-01T00:00:00Z".into(),
//!     importance: 0.5,
//!     access_count: 0,
//!     accessed_at: String::new(),
//! }).await?;
//! # Ok(())
//! # }
//! ```

use std::path::Path;
use std::sync::LazyLock;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use rusqlite::{Connection, OpenFlags};
use tokio::sync::Semaphore;
use tokio::task;

/// Monotonic counter for unique shared-memory database names.
static MEM_POOL_ID: LazyLock<AtomicU64> = LazyLock::new(|| AtomicU64::new(0));

use crate::error::{KernelError, Result};
use crate::graph::types::{GraphEdge, GraphNode, GraphStats, ScoredNode};

// ── Pool inner state ────────────────────────────────

struct PoolInner {
    idle: Mutex<Vec<Connection>>,
    path: String,
    /// true for in-memory pools — uses shared-cache URI so all connections
    /// see the same data.
    shared_mem: bool,
}

impl PoolInner {
    fn take(&self) -> Result<Connection> {
        if let Ok(mut guard) = self.idle.lock()
            && let Some(conn) = guard.pop()
        {
            return Ok(conn);
        }
        if self.shared_mem {
            Connection::open_with_flags(
                &self.path,
                OpenFlags::SQLITE_OPEN_READ_WRITE
                    | OpenFlags::SQLITE_OPEN_CREATE
                    | OpenFlags::SQLITE_OPEN_URI
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .map_err(|e| KernelError::Store(e.to_string()))
        } else {
            Connection::open(&self.path).map_err(|e| KernelError::Store(e.to_string()))
        }
    }

    fn return_conn(&self, conn: Connection) {
        if let Ok(mut guard) = self.idle.lock() {
            guard.push(conn);
        }
        // If lock fails (poisoned), drop the connection — it will be recreated on next take.
    }
}

// ── AsyncPoolGraph ──────────────────────────────────

/// Bounded async connection pool for the knowledge graph.
///
/// Uses a `Semaphore` to bound concurrency and a `Mutex<Vec<Connection>>`
/// for idle connection reuse. Each method acquires a permit, takes (or creates)
/// a connection, runs the operation via `spawn_blocking`, then returns the
/// connection to the pool.
#[derive(Clone)]
pub struct AsyncPoolGraph {
    inner: Arc<PoolInner>,
    sem: Arc<Semaphore>,
}

impl AsyncPoolGraph {
    /// Open (or create) a database and initialise the graph schema.
    ///
    /// `max_conns` bounds the number of concurrent operations.
    pub async fn open(path: impl AsRef<Path>, max_conns: usize) -> Result<Self> {
        let path_str = path
            .as_ref()
            .to_str()
            .ok_or_else(|| KernelError::Store("invalid path".into()))?
            .to_string();

        // Create parent dirs + open first connection + apply schema
        let path_for_open = path_str.clone();
        let first_conn = task::spawn_blocking(move || -> Result<Connection> {
            if let Some(parent) = Path::new(&path_for_open).parent() {
                std::fs::create_dir_all(parent)?;
            }
            let conn =
                Connection::open(&path_for_open).map_err(|e| KernelError::Store(e.to_string()))?;
            crate::graph::schema::init_graph_schema(&conn)?;
            Ok(conn)
        })
        .await
        .map_err(|e| KernelError::Store(e.to_string()))??;

        let inner = Arc::new(PoolInner {
            idle: Mutex::new(vec![first_conn]),
            path: path_str,
            shared_mem: false,
        });

        Ok(Self {
            inner,
            sem: Arc::new(Semaphore::new(max_conns.max(1))),
        })
    }

    /// Create an in-memory pool with schema applied. Useful for tests.
    ///
    /// Uses SQLite shared-cache mode so all connections in the pool see the
    /// same data (plain `:memory:` creates an independent DB per connection).
    pub async fn open_in_memory(max_conns: usize) -> Result<Self> {
        let id = MEM_POOL_ID.fetch_add(1, Ordering::Relaxed);
        let uri = format!("file:llm_kernel_pool_{id}?mode=memory&cache=shared");
        let uri_clone = uri.clone();
        let conn = task::spawn_blocking(move || -> Result<Connection> {
            let conn = Connection::open_with_flags(
                &uri_clone,
                OpenFlags::SQLITE_OPEN_READ_WRITE
                    | OpenFlags::SQLITE_OPEN_CREATE
                    | OpenFlags::SQLITE_OPEN_URI
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .map_err(|e| KernelError::Store(e.to_string()))?;
            crate::graph::schema::init_graph_schema(&conn)?;
            Ok(conn)
        })
        .await
        .map_err(|e| KernelError::Store(e.to_string()))??;

        let inner = Arc::new(PoolInner {
            idle: Mutex::new(vec![conn]),
            path: uri,
            shared_mem: true,
        });

        Ok(Self {
            inner,
            sem: Arc::new(Semaphore::new(max_conns.max(1))),
        })
    }

    /// Execute a closure with a pooled connection.
    async fn with_conn<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let _permit = self
            .sem
            .acquire()
            .await
            .map_err(|_| KernelError::Store("semaphore closed".into()))?;

        let inner = Arc::clone(&self.inner);
        task::spawn_blocking(move || {
            let conn = inner.take()?;
            let result = f(&conn);
            inner.return_conn(conn);
            result
        })
        .await
        .map_err(|e| KernelError::Store(e.to_string()))?
    }

    // ── Node CRUD ───────────────────────────────────

    /// Insert or replace a node.
    pub async fn upsert_node(&self, node: GraphNode) -> Result<()> {
        self.with_conn(move |c| crate::graph::store::upsert_node(c, &node))
            .await
    }

    /// Read a node by ID. Returns `None` if not found.
    pub async fn read_node(&self, id: impl Into<String>) -> Result<Option<GraphNode>> {
        let id = id.into();
        self.with_conn(move |c| crate::graph::store::read_node(c, &id))
            .await
    }

    /// Read all nodes (limited to 10 000).
    pub async fn read_nodes(&self) -> Result<Vec<GraphNode>> {
        self.with_conn(|c| crate::graph::store::read_nodes_limited(c, 10_000))
            .await
    }

    /// Delete a node by ID. Returns `true` if a row was deleted.
    pub async fn delete_node(&self, id: impl Into<String>) -> Result<bool> {
        let id = id.into();
        self.with_conn(move |c| crate::graph::store::delete_node(c, &id))
            .await
    }

    // ── Edge CRUD ───────────────────────────────────

    /// Append an edge (duplicates by ID are ignored).
    pub async fn append_edge(&self, edge: GraphEdge) -> Result<()> {
        self.with_conn(move |c| crate::graph::store::append_edge(c, &edge))
            .await
    }

    /// Read all edges (limited to 10 000).
    pub async fn read_edges(&self) -> Result<Vec<GraphEdge>> {
        self.with_conn(|c| crate::graph::store::read_edges(c, 10_000))
            .await
    }

    /// Delete an edge by ID. Returns `true` if a row was deleted.
    pub async fn delete_edge(&self, id: impl Into<String>) -> Result<bool> {
        let id = id.into();
        self.with_conn(move |c| crate::graph::store::delete_edge(c, &id))
            .await
    }

    // ── Search & Recall ─────────────────────────────

    /// Full-text search over node titles and bodies.
    pub async fn search_nodes(
        &self,
        query: impl Into<String>,
        limit: usize,
    ) -> Result<Vec<GraphNode>> {
        let query = query.into();
        self.with_conn(move |c| crate::graph::search::search_nodes(c, &query, limit))
            .await
    }

    /// Smart recall with composite scoring.
    pub async fn smart_recall(
        &self,
        project: Option<String>,
        hint: Option<String>,
        limit: usize,
    ) -> Result<Vec<ScoredNode>> {
        self.with_conn(move |c| {
            crate::graph::recall::smart_recall(c, project.as_deref(), hint.as_deref(), limit)
        })
        .await
    }

    // ── Stats ───────────────────────────────────────

    /// Compute graph statistics (node/edge counts, avg importance).
    pub async fn stats(&self) -> Result<GraphStats> {
        self.with_conn(crate::graph::lifecycle::compute_stats).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str) -> GraphNode {
        GraphNode {
            id: id.into(),
            node_type: "concept".into(),
            title: format!("Node {id}"),
            body: "body".into(),
            tags: vec![],
            projects: vec![],
            agents: vec![],
            created: "2026-01-01T00:00:00Z".into(),
            updated: "2026-01-01T00:00:00Z".into(),
            importance: 0.5,
            access_count: 0,
            accessed_at: String::new(),
        }
    }

    async fn mem() -> AsyncPoolGraph {
        AsyncPoolGraph::open_in_memory(2).await.unwrap()
    }

    #[tokio::test]
    async fn upsert_and_read_node() {
        let pool = mem().await;
        pool.upsert_node(node("n1")).await.unwrap();
        let n = pool.read_node("n1").await.unwrap().unwrap();
        assert_eq!(n.id, "n1");
    }

    #[tokio::test]
    async fn read_missing_returns_none() {
        let pool = mem().await;
        assert!(pool.read_node("ghost").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn delete_node() {
        let pool = mem().await;
        pool.upsert_node(node("n1")).await.unwrap();
        assert!(pool.delete_node("n1").await.unwrap());
        assert!(pool.read_node("n1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn append_and_read_edges() {
        let pool = mem().await;
        pool.upsert_node(node("a")).await.unwrap();
        pool.upsert_node(node("b")).await.unwrap();
        pool.append_edge(GraphEdge {
            id: "e1".into(),
            source: "a".into(),
            target: "b".into(),
            relation: "related".into(),
            weight: 1.0,
            ts: "2026-01-01T00:00:00Z".into(),
        })
        .await
        .unwrap();
        let edges = pool.read_edges().await.unwrap();
        assert_eq!(edges.len(), 1);
    }

    #[tokio::test]
    async fn delete_edge() {
        let pool = mem().await;
        pool.append_edge(GraphEdge {
            id: "e1".into(),
            source: "a".into(),
            target: "b".into(),
            relation: "related".into(),
            weight: 1.0,
            ts: "2026-01-01T00:00:00Z".into(),
        })
        .await
        .unwrap();
        assert!(pool.delete_edge("e1").await.unwrap());
        assert!(pool.read_edges().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn search_finds_nodes() {
        let pool = mem().await;
        let mut n = node("n1");
        n.title = "Rust ownership".to_string();
        pool.upsert_node(n).await.unwrap();
        let results = pool.search_nodes("Rust", 10).await.unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn stats_returns_counts() {
        let pool = mem().await;
        pool.upsert_node(node("a")).await.unwrap();
        pool.upsert_node(node("b")).await.unwrap();
        let s = pool.stats().await.unwrap();
        assert_eq!(s.total_nodes, 2);
        assert_eq!(s.total_edges, 0);
    }

    #[tokio::test]
    async fn clone_shares_pool() {
        let pool = mem().await;
        let pool2 = pool.clone();
        pool.upsert_node(node("n1")).await.unwrap();
        assert!(pool2.read_node("n1").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn concurrent_reads() {
        let pool = mem().await;
        pool.upsert_node(node("n1")).await.unwrap();

        let mut handles = vec![];
        for _ in 0..4 {
            let p = pool.clone();
            handles.push(tokio::spawn(async move {
                p.read_node("n1").await.unwrap().is_some()
            }));
        }
        for h in handles {
            assert!(h.await.unwrap());
        }
    }

    #[tokio::test]
    async fn open_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("test.db");
        let pool = AsyncPoolGraph::open(&path, 2).await.unwrap();
        pool.upsert_node(node("n1")).await.unwrap();
        assert!(path.exists());
        drop(pool);
    }
}
