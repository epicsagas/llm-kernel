//! Async wrappers for the knowledge graph.
//!
//! All operations run on the Tokio blocking thread pool via
//! [`tokio::task::spawn_blocking`], keeping the async executor free.
//!
//! # Usage
//!
//! ```no_run
//! use std::sync::{Arc, Mutex};
//! use rusqlite::Connection;
//! use llm_kernel::graph::{init_graph_schema, GraphNode};
//! use llm_kernel::graph::async_graph::AsyncGraph;
//!
//! # async fn example() {
//! let conn = Connection::open_in_memory().unwrap();
//! init_graph_schema(&conn).unwrap();
//! let graph = AsyncGraph::new(conn);
//!
//! graph.upsert_node(GraphNode {
//!     id: "n1".into(),
//!     node_type: "concept".into(),
//!     title: "Example".into(),
//!     body: "body text".into(),
//!     tags: vec![],
//!     projects: vec![],
//!     agents: vec![],
//!     created: "2026-01-01T00:00:00Z".into(),
//!     updated: "2026-01-01T00:00:00Z".into(),
//!     importance: 0.5,
//!     access_count: 0,
//!     accessed_at: String::new(),
//! }).await.unwrap();
//! # }
//! ```

use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use tokio::task;

use crate::error::{KernelError, Result};
use crate::graph::types::{GraphEdge, GraphNode, GraphStats, ScoredNode};

/// Async handle to a knowledge graph backed by a `rusqlite::Connection`.
///
/// Internally wraps the connection in `Arc<Mutex<_>>` so it can be shared
/// across async tasks. All blocking SQL calls are offloaded to the Tokio
/// blocking-thread pool.
#[derive(Clone)]
pub struct AsyncGraph {
    conn: Arc<Mutex<Connection>>,
}

impl AsyncGraph {
    /// Wrap an already-initialised connection.
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    /// Open (or create) a database at `path` and initialise the graph schema.
    pub async fn open(path: impl Into<String>) -> Result<Self> {
        let path = path.into();
        task::spawn_blocking(move || {
            let conn = Connection::open(&path).map_err(|e| KernelError::Store(e.to_string()))?;
            crate::graph::schema::init_graph_schema(&conn)?;
            Ok(Self::new(conn))
        })
        .await
        .map_err(|e| KernelError::Store(e.to_string()))?
    }

    fn with_conn<F, T>(&self, f: F) -> task::JoinHandle<Result<T>>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        task::spawn_blocking(move || {
            let guard = conn
                .lock()
                .map_err(|_| KernelError::Store("mutex poisoned".into()))?;
            f(&guard)
        })
    }

    /// Insert or replace a node.
    pub async fn upsert_node(&self, node: GraphNode) -> Result<()> {
        self.with_conn(move |c| crate::graph::store::upsert_node(c, &node))
            .await
            .map_err(|e| KernelError::Store(e.to_string()))?
    }

    /// Read a node by ID. Returns `None` if not found.
    pub async fn read_node(&self, id: impl Into<String>) -> Result<Option<GraphNode>> {
        let id = id.into();
        self.with_conn(move |c| crate::graph::store::read_node(c, &id))
            .await
            .map_err(|e| KernelError::Store(e.to_string()))?
    }

    /// Delete a node by ID. Returns `true` if a row was deleted.
    pub async fn delete_node(&self, id: impl Into<String>) -> Result<bool> {
        let id = id.into();
        self.with_conn(move |c| crate::graph::store::delete_node(c, &id))
            .await
            .map_err(|e| KernelError::Store(e.to_string()))?
    }

    /// Append an edge (duplicates by ID are ignored).
    pub async fn append_edge(&self, edge: GraphEdge) -> Result<()> {
        self.with_conn(move |c| crate::graph::store::append_edge(c, &edge))
            .await
            .map_err(|e| KernelError::Store(e.to_string()))?
    }

    /// Delete an edge by ID. Returns `true` if a row was deleted.
    pub async fn delete_edge(&self, id: impl Into<String>) -> Result<bool> {
        let id = id.into();
        self.with_conn(move |c| crate::graph::store::delete_edge(c, &id))
            .await
            .map_err(|e| KernelError::Store(e.to_string()))?
    }

    /// Run smart recall with composite scoring.
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
        .map_err(|e| KernelError::Store(e.to_string()))?
    }

    /// Full-text search over node titles and bodies.
    pub async fn search_nodes(
        &self,
        query: impl Into<String>,
        limit: usize,
    ) -> Result<Vec<GraphNode>> {
        let query = query.into();
        self.with_conn(move |c| crate::graph::search::search_nodes(c, &query, limit))
            .await
            .map_err(|e| KernelError::Store(e.to_string()))?
    }

    /// Compute graph statistics (node/edge counts, avg importance).
    pub async fn stats(&self) -> Result<GraphStats> {
        self.with_conn(crate::graph::lifecycle::compute_stats)
            .await
            .map_err(|e| KernelError::Store(e.to_string()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::schema::init_graph_schema;

    fn mem_graph() -> AsyncGraph {
        let conn = Connection::open_in_memory().unwrap();
        init_graph_schema(&conn).unwrap();
        AsyncGraph::new(conn)
    }

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

    #[tokio::test]
    async fn upsert_and_read() {
        let g = mem_graph();
        g.upsert_node(node("n1")).await.unwrap();
        let loaded = g.read_node("n1").await.unwrap().unwrap();
        assert_eq!(loaded.id, "n1");
    }

    #[tokio::test]
    async fn read_missing_returns_none() {
        let g = mem_graph();
        assert!(g.read_node("ghost").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn delete_node() {
        let g = mem_graph();
        g.upsert_node(node("n1")).await.unwrap();
        assert!(g.delete_node("n1").await.unwrap());
        assert!(g.read_node("n1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn append_and_delete_edge() {
        let g = mem_graph();
        g.upsert_node(node("a")).await.unwrap();
        g.upsert_node(node("b")).await.unwrap();
        g.append_edge(GraphEdge {
            id: "e1".into(),
            source: "a".into(),
            target: "b".into(),
            relation: "related".into(),
            weight: 1.0,
            ts: "2026-01-01T00:00:00Z".into(),
        })
        .await
        .unwrap();
        assert!(g.delete_edge("e1").await.unwrap());
    }

    #[tokio::test]
    async fn smart_recall_async() {
        let g = mem_graph();
        g.upsert_node(node("x")).await.unwrap();
        let results = g.smart_recall(None, None, 10).await.unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn stats_returns_counts() {
        let g = mem_graph();
        g.upsert_node(node("a")).await.unwrap();
        g.upsert_node(node("b")).await.unwrap();
        let s = g.stats().await.unwrap();
        assert_eq!(s.total_nodes, 2);
        assert_eq!(s.total_edges, 0);
    }

    #[tokio::test]
    async fn clone_shares_connection() {
        let g = mem_graph();
        let g2 = g.clone();
        g.upsert_node(node("n1")).await.unwrap();
        // g2 sees the same DB
        assert!(g2.read_node("n1").await.unwrap().is_some());
    }
}
