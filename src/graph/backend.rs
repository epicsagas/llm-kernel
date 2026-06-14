//! Backend-agnostic graph trait and SQLite implementation.
//!
//! [`GraphBackend`] is a sync, object-safe trait covering the primitive node/edge
//! operations every graph backend must support. It deliberately exposes **no
//! `rusqlite` types**, so a PostgreSQL or in-memory backend can implement it
//! (see the v0.8.0 roadmap). [`SqliteGraph`] is the bundled implementation: it
//! wraps a single mutex-guarded connection and delegates to the existing
//! free-function graph API in [`crate::graph`].
//!
//! The existing free functions (`upsert_node(&conn, …)`, `search_nodes(&conn, …)`,
//! …) are unchanged — callers that already own a `Connection` keep using them.
//! [`SqliteGraph`] simply packages a connection behind the trait for users who
//! want backend-agnostic graph access.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::Result;
use crate::graph::recall::smart_recall;
use crate::graph::schema::{init_graph_schema, migrate_graph, schema_version};
use crate::graph::search::{query_nodes, search_nodes};
use crate::graph::store::{
    append_edge, delete_edge, delete_node, edges_for_node, remove_edges_for_node, upsert_node,
};
use crate::graph::traversal::related_nodes;
use crate::graph::types::{GraphEdge, GraphNode, ScoredNode};

/// Sync, object-safe trait for graph backends.
///
/// Methods cover node/edge CRUD, FTS search, filtered query, and schema
/// migration. No method exposes `rusqlite` types, so the trait is implementable
/// by any backend. `dyn GraphBackend` is usable.
pub trait GraphBackend: Send + Sync {
    /// Insert or replace a node.
    fn upsert_node(&self, node: &GraphNode) -> Result<()>;
    /// Read a single node by ID (`None` if absent).
    fn read_node(&self, id: &str) -> Result<Option<GraphNode>>;
    /// Delete a node by ID. Returns `true` if a row was removed.
    fn delete_node(&self, id: &str) -> Result<bool>;
    /// FTS5 full-text search, ranked by importance DESC.
    fn search_nodes(&self, query: &str, limit: usize) -> Result<Vec<GraphNode>>;
    /// Dynamic filter by tag / node_type / project.
    #[allow(clippy::too_many_arguments)]
    fn query_nodes(
        &self,
        tag: Option<&str>,
        node_type: Option<&str>,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<GraphNode>>;
    /// Composite recall — rank nodes by recency, importance, access, FTS, and
    /// graph boost. The canonical high-level read path for "what's relevant".
    fn smart_recall(
        &self,
        project: Option<&str>,
        hint: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ScoredNode>>;
    /// BFS-traverse up to `depth` hops from `start_id`, returning related node
    /// IDs (excluding the start).
    fn related_nodes(&self, start_id: &str, depth: usize) -> Result<Vec<String>>;
    /// Append an edge (duplicates by edge ID are ignored).
    fn append_edge(&self, edge: &GraphEdge) -> Result<()>;
    /// Read edges where the given node is source or target.
    fn edges_for_node(&self, node_id: &str) -> Result<Vec<GraphEdge>>;
    /// Delete an edge by ID. Returns `true` if a row was removed.
    fn delete_edge(&self, id: &str) -> Result<bool>;
    /// Remove every edge connected to a node.
    fn remove_edges_for_node(&self, node_id: &str) -> Result<()>;

    /// Recorded schema version for this backend.
    fn current_version(&self) -> Result<u32>;
    /// Apply pending migrations up to the backend's latest schema version.
    /// Returns the resulting version.
    fn migrate(&self) -> Result<u32>;
}

/// SQLite-backed [`GraphBackend`] over one mutex-guarded connection.
///
/// Opening applies the schema and runs any pending migrations, so a database
/// created by an older `llm-kernel` is upgraded transparently on open.
pub struct SqliteGraph {
    conn: Mutex<Connection>,
}

impl SqliteGraph {
    /// Open (or create) a graph database at `path`, applying schema + migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = open_with_schema(path.as_ref())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Create an in-memory graph (useful for tests and ephemeral stores).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(store_err)?;
        init_graph_schema(&conn)?;
        let current = schema_version(&conn)?;
        migrate_graph(&conn, current)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Lock helper: recover the guard even if a previous holder panicked.
    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Open a file-backed connection, apply the schema, then run pending migrations.
fn open_with_schema(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path).map_err(store_err)?;
    init_graph_schema(&conn)?;
    let current = schema_version(&conn)?;
    migrate_graph(&conn, current)?;
    Ok(conn)
}

fn store_err(e: rusqlite::Error) -> crate::error::KernelError {
    crate::error::KernelError::Store(e.to_string())
}

impl GraphBackend for SqliteGraph {
    fn upsert_node(&self, node: &GraphNode) -> Result<()> {
        let c = self.lock();
        upsert_node(&c, node)
    }

    fn read_node(&self, id: &str) -> Result<Option<GraphNode>> {
        let c = self.lock();
        crate::graph::store::read_node(&c, id)
    }

    fn delete_node(&self, id: &str) -> Result<bool> {
        let c = self.lock();
        delete_node(&c, id)
    }

    fn search_nodes(&self, query: &str, limit: usize) -> Result<Vec<GraphNode>> {
        let c = self.lock();
        search_nodes(&c, query, limit)
    }

    fn query_nodes(
        &self,
        tag: Option<&str>,
        node_type: Option<&str>,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<GraphNode>> {
        let c = self.lock();
        query_nodes(&c, tag, node_type, project, limit)
    }

    fn smart_recall(
        &self,
        project: Option<&str>,
        hint: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ScoredNode>> {
        let c = self.lock();
        smart_recall(&c, project, hint, limit)
    }

    fn related_nodes(&self, start_id: &str, depth: usize) -> Result<Vec<String>> {
        let c = self.lock();
        Ok(related_nodes(&c, start_id, depth))
    }

    fn append_edge(&self, edge: &GraphEdge) -> Result<()> {
        let c = self.lock();
        append_edge(&c, edge)
    }

    fn edges_for_node(&self, node_id: &str) -> Result<Vec<GraphEdge>> {
        let c = self.lock();
        edges_for_node(&c, node_id)
    }

    fn delete_edge(&self, id: &str) -> Result<bool> {
        let c = self.lock();
        delete_edge(&c, id)
    }

    fn remove_edges_for_node(&self, node_id: &str) -> Result<()> {
        let c = self.lock();
        remove_edges_for_node(&c, node_id)
    }

    fn current_version(&self) -> Result<u32> {
        let c = self.lock();
        schema_version(&c)
    }

    fn migrate(&self) -> Result<u32> {
        let c = self.lock();
        let current = schema_version(&c)?;
        migrate_graph(&c, current)
    }
}

/// CJK-aware convenience methods (available with the `graph-cjk` feature).
#[cfg(feature = "graph-cjk")]
impl SqliteGraph {
    /// CJK (or mixed) search via contiguous substring matching.
    ///
    /// Delegates to [`crate::graph::cjk::search_nodes_cjk`]; see its docs for
    /// the matching semantics.
    pub fn search_nodes_cjk(&self, query: &str, limit: usize) -> Result<Vec<GraphNode>> {
        let c = self.lock();
        crate::graph::cjk::search_nodes_cjk(&c, query, limit)
    }

    /// Segment a string for CJK tokenization (exposed for callers that want to
    /// pre-process queries or inspect tokenization).
    pub fn segment_cjk(text: &str) -> String {
        crate::graph::cjk::segment_cjk(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_node(id: &str) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            node_type: "concept".to_string(),
            title: format!("Node {id}"),
            body: "graph backend test body".to_string(),
            tags: vec!["backend".to_string()],
            projects: vec![],
            agents: vec![],
            created: "2026-01-01T00:00:00Z".to_string(),
            updated: "2026-01-01T00:00:00Z".to_string(),
            importance: 0.5,
            access_count: 0,
            accessed_at: String::new(),
        }
    }

    /// AC4: a node round-trips through the trait, and the trait is usable as
    /// `dyn GraphBackend` (object-safety) with no `rusqlite` in the surface.
    #[test]
    fn dyn_backend_round_trips_node() {
        let backend: Box<dyn GraphBackend> = Box::new(SqliteGraph::open_in_memory().unwrap());
        assert!(backend.read_node("n1").unwrap().is_none());
        backend.upsert_node(&sample_node("n1")).unwrap();
        let loaded = backend.read_node("n1").unwrap().unwrap();
        assert_eq!(loaded.title, "Node n1");
        assert_eq!(loaded.tags, vec!["backend".to_string()]);
        assert!(backend.delete_node("n1").unwrap());
        assert!(backend.read_node("n1").unwrap().is_none());
    }

    /// AC5: a fresh backend reports the current schema version.
    #[test]
    fn fresh_backend_reports_current_version() {
        let backend = SqliteGraph::open_in_memory().unwrap();
        assert_eq!(
            backend.current_version().unwrap(),
            crate::graph::schema::GRAPH_SCHEMA_VERSION
        );
    }

    /// AC5: search through the trait finds an inserted node by title.
    #[test]
    fn backend_search_finds_node() {
        let backend = SqliteGraph::open_in_memory().unwrap();
        backend.upsert_node(&sample_node("rust")).unwrap();
        let hits = backend.search_nodes("graph backend", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "rust");
    }

    /// The composite recall path is reachable through the trait.
    #[test]
    fn backend_smart_recall_finds_relevant() {
        let backend = SqliteGraph::open_in_memory().unwrap();
        let mut n = sample_node("rust");
        n.body = "rust ownership borrow checker".to_string();
        backend.upsert_node(&n).unwrap();
        let recalled = backend.smart_recall(None, Some("ownership"), 5).unwrap();
        assert!(recalled.iter().any(|s| s.node.id == "rust"));
    }

    /// The composite traversal path is reachable through the trait.
    #[test]
    fn backend_related_nodes_traverses_edges() {
        let backend = SqliteGraph::open_in_memory().unwrap();
        backend.upsert_node(&sample_node("a")).unwrap();
        backend.upsert_node(&sample_node("b")).unwrap();
        backend
            .append_edge(&GraphEdge {
                id: "e1".into(),
                source: "a".into(),
                target: "b".into(),
                relation: "related".into(),
                weight: 1.0,
                ts: "2026-01-01T00:00:00Z".into(),
            })
            .unwrap();
        let related = backend.related_nodes("a", 2).unwrap();
        assert!(related.contains(&"b".to_string()));
    }
}
