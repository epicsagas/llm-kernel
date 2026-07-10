//! AI agent memory graph — SQLite/PostgreSQL-backed long-term memory with FTS5
//! search, smart recall, and graph-structured relevance boosting.
//!
//! The module is tuned for agent *memory*: nodes carry importance and decaying
//! recency/access signals, and are recalled by composite scoring (recency +
//! importance + access + FTS + graph boost), with CSR algorithms ([`algo`])
//! surfacing structurally central memories. The niche is comparable to
//! **Zep / Mem0 / Letta**, but local-first and vault-based rather than a hosted
//! service.
//!
//! From v0.19 the trait also serves **general directed-graph workloads** —
//! citation networks, document backlinks, dependency graphs — via batch edge
//! writes ([`GraphBackend::append_edges`]), directional / relation-filtered
//! lookups ([`GraphBackend::edges_for_node_dir`],
//! [`GraphBackend::neighbors_weighted`]), and filtered BFS
//! ([`GraphBackend::related_nodes_filtered`]); [`EdgeDirection`] selects
//! out / in / both. For pure topology with no memory semantics, `petgraph` or
//! a graph database remains a better fit.
//!
//! Provides a complete knowledge graph layer on top of SQLite:
//!
//! - **Types**: [`GraphNode`], [`GraphEdge`], [`ScoredNode`], [`GraphStats`]
//! - **Schema**: [`init_graph_schema`] — creates tables, FTS5, indexes
//! - **CRUD**: node/edge insert, read, update, delete
//! - **Search**: FTS5 full-text search and dynamic filtering
//! - **Recall**: [`smart_recall`] — composite scoring with recency, importance, access, FTS, graph boost
//! - **Traversal**: [`graph_neighbors`] (1-hop), [`related_nodes`] (BFS via recursive CTE)
//! - **Algorithms**: pure-Rust CSR algorithms in [`algo`] — [`pagerank()`], [`connected_components()`], [`label_propagation()`], [`dijkstra()`], [`jaccard_similarity()`]
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

pub mod algo;
pub mod backend;
pub mod dedup;
pub mod lifecycle;
pub mod recall;
pub mod schema;
pub mod search;
pub mod store;
pub mod traversal;
pub mod types;

/// CJK-aware graph search (Rust-side segmentation; no schema change).
#[cfg(feature = "graph-cjk")]
pub mod cjk;

/// PostgreSQL `GraphBackend` (feature `graph-pg`).
#[cfg(feature = "graph-pg")]
pub mod pg;

#[cfg(feature = "graph-async")]
pub mod async_graph;

#[cfg(feature = "graph-pool")]
pub mod async_pool;
#[cfg(feature = "graph-pool")]
pub use async_pool::AsyncPoolGraph;

// Re-export primary types and functions
pub use algo::{
    CsrGraph, LABEL_PROPAGATION_ITERS, PAGERANK_DAMPING, PAGERANK_EPS, PAGERANK_ITERS,
    SHORTEST_PATH_W_MIN, adamic_adar, common_neighbors, connected_components, dijkstra,
    jaccard_similarity, label_propagation, label_propagation_default, link_prediction, pagerank,
    pagerank_default, pagerank_scores, shortest_path, shortest_path_ids,
};
pub use backend::{GraphBackend, SqliteGraph};
pub use dedup::{find_duplicate, upsert_node_dedup};
pub use lifecycle::{compute_stats, decay_importance, tag_stale_nodes, touch_node, touch_nodes};
pub use recall::smart_recall;
pub use schema::{GRAPH_SCHEMA_VERSION, init_graph_schema, migrate_graph, schema_version};
pub use search::{query_nodes, search_nodes};
pub use store::{
    append_edge, delete_edge, delete_node, read_edges, read_node, read_nodes, upsert_node,
};
pub use traversal::{build_graph, graph_neighbors, related_nodes};
pub use types::{
    EdgeDirection, Graph, GraphEdge, GraphNode, GraphNodeSummary, GraphStats, ScoredNode,
    validate_uuid,
};

#[cfg(feature = "graph-cjk")]
pub use cjk::{search_nodes_cjk, segment_cjk};

#[cfg(feature = "graph-pg")]
pub use pg::PgGraph;
