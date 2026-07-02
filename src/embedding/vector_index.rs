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

use crate::error::Result;

/// A single search hit from vector index lookup.
///
/// Sorts by **descending** score (highest similarity first). Ties are broken
/// by ascending ID for deterministic ordering.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchHit {
    /// External identifier for the matched vector.
    pub id: u64,
    /// Similarity score (higher = more similar).
    pub score: f32,
}

impl PartialOrd for SearchHit {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        // f32 is not Ord, so we use total_cmp for a total ordering.
        // Reverse score order: highest score first, then ascending ID.
        Some(
            other
                .score
                .total_cmp(&self.score)
                .then_with(|| self.id.cmp(&other.id)),
        )
    }
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

    /// Remove vectors by their external IDs.
    ///
    /// IDs that do not exist in the index are silently ignored.
    /// Passing an empty slice is a no-op.
    fn remove(&mut self, ids: &[u64]) -> Result<()>;

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

    #[test]
    fn search_hit_sort_descending_by_score() {
        let mut hits = vec![
            SearchHit { id: 1, score: 0.3 },
            SearchHit { id: 2, score: 0.9 },
            SearchHit { id: 3, score: 0.5 },
        ];
        hits.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(hits[0].id, 2); // highest score first
        assert_eq!(hits[1].id, 3);
        assert_eq!(hits[2].id, 1);
    }

    #[test]
    fn search_hit_tie_break_by_id() {
        let mut hits = vec![
            SearchHit { id: 30, score: 0.5 },
            SearchHit { id: 10, score: 0.5 },
            SearchHit { id: 20, score: 0.5 },
        ];
        hits.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(hits[0].id, 10);
        assert_eq!(hits[1].id, 20);
        assert_eq!(hits[2].id, 30);
    }
}
