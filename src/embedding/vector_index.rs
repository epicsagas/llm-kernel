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

/// How to combine several ranked [`SearchHit`] lists into one — the join point
/// for hybrid retrieval (e.g. a dense branch plus a sparse/lexical branch).
///
/// Choose by whether the branches' scores are comparable. [`Rrf`](Fusion::Rrf)
/// looks only at ranks, so it is safe when the branches use different
/// similarity measures (cosine vs inner product) or wildly different scales;
/// [`Weighted`](Fusion::Weighted) sums raw scores and therefore assumes the
/// branches are already on a shared scale.
#[derive(Debug, Clone, PartialEq)]
pub enum Fusion {
    /// Reciprocal Rank Fusion: `score(d) = Σ 1 / (k + rank_i(d))` over the
    /// lists containing `d`, with 1-based ranks. Larger `k` flattens the
    /// advantage of top ranks; 60 is the conventional default.
    Rrf {
        /// Rank damping constant — larger values flatten the advantage held by
        /// top-ranked hits. 60 is the conventional default.
        k: u32,
    },
    /// Convex combination of raw scores, one weight per input list. A list
    /// that does not contain a hit contributes nothing for it.
    Weighted {
        /// One weight per input list, in the same order as the lists passed to
        /// [`fuse`](Fusion::fuse).
        weights: Vec<f32>,
    },
}

impl Fusion {
    /// Reciprocal Rank Fusion with the conventional `k = 60`.
    pub fn rrf() -> Self {
        Fusion::Rrf { k: 60 }
    }

    /// Fuse ranked lists into a single list, best first.
    ///
    /// Every input list is expected to be sorted best-first already — RRF
    /// derives its ranks from that order. The result uses the same ordering as
    /// [`SearchHit`] itself: descending score, then ascending id.
    ///
    /// # Panics
    ///
    /// [`Weighted`](Fusion::Weighted) panics when the number of weights differs
    /// from the number of lists, mirroring `search::weighted_sum_fuse`: a
    /// mismatch means a branch would be silently dropped from the ranking.
    pub fn fuse(&self, hit_sets: &[Vec<SearchHit>]) -> Vec<SearchHit> {
        use std::collections::HashMap;
        let mut scores: HashMap<u64, f32> = HashMap::new();
        match self {
            Fusion::Rrf { k } => {
                let k = *k as f32;
                for set in hit_sets {
                    for (rank, hit) in set.iter().enumerate() {
                        *scores.entry(hit.id).or_insert(0.0) += 1.0 / (k + rank as f32 + 1.0);
                    }
                }
            }
            Fusion::Weighted { weights } => {
                assert_eq!(
                    hit_sets.len(),
                    weights.len(),
                    "Fusion::Weighted: hit_sets.len() ({}) must equal weights.len() ({})",
                    hit_sets.len(),
                    weights.len(),
                );
                for (set, w) in hit_sets.iter().zip(weights) {
                    for hit in set {
                        *scores.entry(hit.id).or_insert(0.0) += w * hit.score;
                    }
                }
            }
        }
        let mut fused: Vec<SearchHit> = scores
            .into_iter()
            .map(|(id, score)| SearchHit { id, score })
            .collect();
        fused.sort_unstable_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        fused
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
        let mut hits = [
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
        let mut hits = [
            SearchHit { id: 30, score: 0.5 },
            SearchHit { id: 10, score: 0.5 },
            SearchHit { id: 20, score: 0.5 },
        ];
        hits.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(hits[0].id, 10);
        assert_eq!(hits[1].id, 20);
        assert_eq!(hits[2].id, 30);
    }

    fn hit(id: u64, score: f32) -> SearchHit {
        SearchHit { id, score }
    }

    #[test]
    fn rrf_default_uses_k_60() {
        assert_eq!(Fusion::rrf(), Fusion::Rrf { k: 60 });
    }

    #[test]
    fn rrf_rewards_agreement_across_branches() {
        // 20 is only 2nd in the dense branch but appears in both, so it must
        // outrank 10, which is 1st in only one branch.
        let dense = vec![hit(10, 0.99), hit(20, 0.80)];
        let sparse = vec![hit(20, 12.0), hit(30, 9.0)];
        let fused = Fusion::rrf().fuse(&[dense, sparse]);
        assert_eq!(fused[0].id, 20);
        assert_eq!(fused.len(), 3);
    }

    #[test]
    fn rrf_ignores_raw_score_scale() {
        // Sparse inner-product scores dwarf cosine scores; RRF must not care.
        let dense = vec![hit(1, 0.9), hit(2, 0.8)];
        let sparse = vec![hit(1, 900.0), hit(2, 800.0)];
        let a = Fusion::rrf().fuse(&[dense.clone(), sparse]);
        let sparse_small = vec![hit(1, 0.009), hit(2, 0.008)];
        let b = Fusion::rrf().fuse(&[dense, sparse_small]);
        assert_eq!(
            a.iter().map(|h| h.id).collect::<Vec<_>>(),
            b.iter().map(|h| h.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn weighted_sums_scores_per_branch() {
        let dense = vec![hit(1, 1.0)];
        let sparse = vec![hit(2, 1.0)];
        let fused = Fusion::Weighted {
            weights: vec![1.0, 2.0],
        }
        .fuse(&[dense, sparse]);
        // id 2 carries the heavier branch weight.
        assert_eq!(fused[0].id, 2);
        assert!((fused[0].score - 2.0).abs() < 1e-6);
        assert!((fused[1].score - 1.0).abs() < 1e-6);
    }

    #[test]
    #[should_panic(expected = "must equal weights.len()")]
    fn weighted_panics_on_weight_count_mismatch() {
        Fusion::Weighted { weights: vec![1.0] }.fuse(&[vec![hit(1, 1.0)], vec![hit(2, 1.0)]]);
    }

    #[test]
    fn fuse_of_nothing_is_empty() {
        assert!(Fusion::rrf().fuse(&[]).is_empty());
    }
}
