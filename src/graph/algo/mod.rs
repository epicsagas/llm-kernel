//! Graph algorithms operating on an in-memory CSR snapshot.
//!
//! These algorithms (PageRank, connected components, shortest path, similarity)
//! are pure-Rust, zero-dependency, and backend-agnostic: they consume a
//! [`CsrGraph`] built from the edge set rather than running SQL per step. This
//! keeps the iterative math identical across the SQLite and PostgreSQL
//! backends — the same zero-drift principle as `recall.rs`'s `compute_recency`.
//!
//! Available on the `graph` feature with no extra feature gate (the algorithms
//! add no dependencies).

pub mod community;
pub mod csr;
pub mod pagerank;
pub mod path;
pub mod similarity;

pub use community::{
    LABEL_PROPAGATION_ITERS, connected_components, label_propagation, label_propagation_default,
};
pub use csr::CsrGraph;
pub use pagerank::{
    PAGERANK_DAMPING, PAGERANK_EPS, PAGERANK_ITERS, pagerank, pagerank_default, pagerank_scores,
};
pub use path::{SHORTEST_PATH_W_MIN, dijkstra, shortest_path, shortest_path_ids};
pub use similarity::{adamic_adar, common_neighbors, jaccard_similarity, link_prediction};
