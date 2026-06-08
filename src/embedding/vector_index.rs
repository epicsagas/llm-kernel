//! Vector index trait for compressed approximate nearest neighbor search.
//!
//! Defines the abstract interface that concrete implementations (e.g.,
//! `llm-kernel-vector-index` with TurboQuant) must satisfy. This module has
//! **zero external dependencies** — implementations live in separate crates.
//!
//! ```
//! use llm_kernel::embedding::vector_index::SearchHit;
//!
//! let hit = SearchHit { id: 42, score: 0.95 };
//! assert_eq!(hit.id, 42);
//! ```

use std::path::Path;

use anyhow::Result;

/// A single search hit from vector index lookup.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchHit {
    /// External identifier for the matched vector.
    pub id: u64,
    /// Similarity score (higher = more similar).
    pub score: f32,
}

/// Trait for compressed vector indexes.
///
/// Implementations provide approximate nearest neighbor search with
/// quantization-based compression. Follows the same pattern as
/// [`EmbeddingProvider`](crate::embedding::EmbeddingProvider).
///
/// The trait is defined here with zero dependencies. Concrete implementations
/// live in separate crates (e.g., `llm-kernel-vector-index` with TurboQuant).
///
/// This trait is fully object-safe — `load` is intentionally not included
/// because it requires `Self: Sized`. Concrete types provide their own
/// `load` inherent methods instead.
pub trait VectorIndex: Send + Sync {
    /// Add vectors with auto-assigned sequential IDs.
    fn add(&mut self, vectors: &[Vec<f32>]) -> Result<()>;

    /// Add vectors with explicit external IDs.
    fn add_with_ids(&mut self, vectors: &[Vec<f32>], ids: &[u64]) -> Result<()>;

    /// Search for the `k` nearest neighbors of `query`.
    fn search(&self, query: &[f32], k: usize) -> Result<Vec<SearchHit>>;

    /// Search restricted to an allowlist of candidate IDs.
    ///
    /// Useful for hybrid retrieval: first narrow candidates via BM25 or
    /// metadata filter, then dense-rerank within that set.
    fn search_filtered(&self, query: &[f32], k: usize, allowlist: &[u64])
    -> Result<Vec<SearchHit>>;

    /// Number of vectors currently indexed.
    fn len(&self) -> usize;

    /// Whether the index is empty.
    fn is_empty(&self) -> bool;

    /// Vector dimensionality.
    fn dim(&self) -> usize;

    /// Persist the index to disk.
    fn save(&self, path: &Path) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_hit_fields() {
        let hit = SearchHit {
            id: 42,
            score: 0.95,
        };
        assert_eq!(hit.id, 42);
        assert!((hit.score - 0.95).abs() < f32::EPSILON);
    }

    #[test]
    fn search_hit_copy() {
        let hit = SearchHit { id: 1, score: 0.5 };
        let copied = hit; // Copy semantics — no .clone() needed
        assert_eq!(copied.id, hit.id);
        assert_eq!(copied.score, hit.score);
    }
}
