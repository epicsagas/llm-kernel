//! Knowledge graph with SQLite persistence, FTS5 search, and smart recall.
//!
//! Provides a complete knowledge graph layer on top of SQLite:
//!
//! - **Types**: [`GraphNode`], [`GraphEdge`], [`ScoredNode`], [`GraphStats`]
//! - **Schema**: [`init_graph_schema`] — creates tables, FTS5, indexes
//! - **CRUD**: node/edge insert, read, update, delete
//! - **Search**: FTS5 full-text search and dynamic filtering
//! - **Recall**: [`smart_recall`] — composite scoring with recency, importance, access, FTS, graph boost
//! - **Traversal**: [`graph_neighbors`] (1-hop), [`related_nodes`] (BFS via recursive CTE)
//! - **Lifecycle**: [`decay_importance`], [`tag_stale_nodes`], [`compute_stats`]
//!
//! All functions take `&rusqlite::Connection` — no hardcoded paths.
//!
//! ```no_run
//! use rusqlite::Connection;
//! use llm_kernel::graph::{init_graph_schema, upsert_node, smart_recall, GraphNode};
//!
//! let conn = Connection::open_in_memory().unwrap();
//! init_graph_schema(&conn).unwrap();
//!
//! upsert_node(&conn, &GraphNode {
//!     id: "rust-ownership".into(),
//!     node_type: "concept".into(),
//!     title: "Rust Ownership Model".into(),
//!     body: "Ownership, borrowing, and lifetimes...".into(),
//!     tags: vec!["rust".into(), "memory-safety".into()],
//!     projects: vec!["my-project".into()],
//!     agents: vec![],
//!     created: "2026-01-01T00:00:00Z".into(),
//!     updated: "2026-01-01T00:00:00Z".into(),
//!     importance: 0.8,
//!     access_count: 0,
//!     accessed_at: String::new(),
//! }).unwrap();
//!
//! let results = smart_recall(&conn, Some("my-project"), Some("ownership"), 5).unwrap();
//! for scored in &results {
//!     println!("{:.2} — {}", scored.score, scored.node.title);
//! }
//! ```

pub mod dedup;
pub mod lifecycle;
pub mod recall;
pub mod schema;
pub mod search;
pub mod store;
pub mod traversal;
pub mod types;

#[cfg(feature = "graph-async")]
pub mod async_graph;

#[cfg(feature = "graph-pool")]
pub mod async_pool;
#[cfg(feature = "graph-pool")]
pub use async_pool::AsyncPoolGraph;

// Re-export primary types and functions
pub use dedup::{find_duplicate, upsert_node_dedup};
pub use lifecycle::{compute_stats, decay_importance, tag_stale_nodes, touch_node, touch_nodes};
pub use recall::smart_recall;
pub use schema::{GRAPH_SCHEMA_VERSION, init_graph_schema, migrate_graph};
pub use search::{query_nodes, search_nodes};
pub use store::{
    append_edge, delete_edge, delete_node, read_edges, read_node, read_nodes, upsert_node,
};
pub use traversal::{build_graph, graph_neighbors, related_nodes};
pub use types::{
    Graph, GraphEdge, GraphNode, GraphNodeSummary, GraphStats, ScoredNode, validate_uuid,
};
