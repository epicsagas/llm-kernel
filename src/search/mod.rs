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

pub mod fusion;
pub mod provider;
pub mod rrf;
pub mod types;

pub use fusion::{combmnz_fuse, normalize_minmax, weighted_sum_fuse};
pub use provider::{KeywordIndex, SearchProvider};
pub use rrf::rrf_fuse;
pub use types::SearchResult;

#[cfg(test)]
mod tests {
    use super::*;

    /// Two distinct providers compose via the existing RRF fusion with the
    /// expected merged top result (AC1).
    #[test]
    fn two_providers_compose_via_rrf() {
        let keywords = KeywordIndex::new(vec![
            ("shared".to_string(), "rust async runtime tokio".to_string()),
            (
                "kw_only".to_string(),
                "rust ownership borrowing".to_string(),
            ),
        ]);
        let semantic = KeywordIndex::new(vec![
            ("shared".to_string(), "rust async runtime tokio".to_string()),
            ("sem_only".to_string(), "rust green threads mio".to_string()),
        ]);

        let kw_results = keywords.search("rust async", 10).unwrap();
        let sem_results = semantic.search("rust async", 10).unwrap();

        let fused = rrf_fuse(&[kw_results, sem_results], 60);
        assert!(!fused.is_empty());
        // "shared" appears at rank 0 in both providers -> highest RRF score.
        assert_eq!(fused[0].id, "shared");
    }
}
