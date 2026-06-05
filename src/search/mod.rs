//! Hybrid search: BM25 + vector similarity with Reciprocal Rank Fusion.
//!
//! Provides a unified search interface that combines keyword (BM25) and
//! semantic (vector) search results using RRF for optimal relevance.
//!
//! ```
//! use llm_kernel::search::{SearchResult, rrf_fuse};
//!
//! let bm25 = vec![
//!     SearchResult { id: "doc1".into(), score: 0.9, text: "Rust programming".into() },
//!     SearchResult { id: "doc2".into(), score: 0.7, text: "Python basics".into() },
//! ];
//! let vector = vec![
//!     SearchResult { id: "doc2".into(), score: 0.95, text: "Python basics".into() },
//!     SearchResult { id: "doc3".into(), score: 0.6, text: "Go concurrency".into() },
//! ];
//!
//! let fused = rrf_fuse(&[bm25, vector], 60);
//! assert!(!fused.is_empty());
//! ```

pub mod rrf;
pub mod types;

pub use rrf::rrf_fuse;
pub use types::SearchResult;
