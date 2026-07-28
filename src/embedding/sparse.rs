//! Sparse (lexical) vectors for hybrid retrieval.
//!
//! A [`SparseVector`] is the learned-lexical counterpart to a dense embedding —
//! for example BGE-M3's sparse head or SPLADE. It pairs vocabulary indices with
//! weights, and is what pgvector stores in a `sparsevec` column.
//!
//! Combining a dense and a sparse branch is what makes hybrid retrieval work:
//! the dense side matches paraphrases, the sparse side matches exact terms
//! (identifiers, statute numbers, rare jargon) that dense vectors blur away.

/// A sparse vector — `indices[i]` carries weight `values[i]`.
///
/// Indices are 0-based vocabulary positions, as emitted by the model. Backends
/// convert to their own convention on write (pgvector's text form is 1-based).
/// Elements are kept sorted by index with zero weights removed, which is what
/// `sparsevec` expects and what makes intersection cheap.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SparseVector {
    indices: Vec<u32>,
    values: Vec<f32>,
}

impl SparseVector {
    /// Build from parallel `indices` / `values`.
    ///
    /// Zero weights are dropped and the remaining elements are sorted by index.
    /// Returns `None` when the two slices disagree in length — a mismatch means
    /// the caller lost the pairing and silently truncating would corrupt scores.
    pub fn new(indices: Vec<u32>, values: Vec<f32>) -> Option<Self> {
        if indices.len() != values.len() {
            return None;
        }
        let mut pairs: Vec<(u32, f32)> = indices
            .into_iter()
            .zip(values)
            .filter(|(_, v)| *v != 0.0)
            .collect();
        pairs.sort_unstable_by_key(|(i, _)| *i);
        let (indices, values) = pairs.into_iter().unzip();
        Some(Self { indices, values })
    }

    /// Vocabulary indices, ascending.
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// Weights, aligned with [`indices`](Self::indices).
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    /// Number of non-zero elements.
    pub fn nnz(&self) -> usize {
        self.indices.len()
    }

    /// Whether the vector carries no weight at all.
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// Keep only the `k` highest-magnitude weights, still sorted by index.
    ///
    /// Lexical vectors from models like BGE-M3 carry a long tail of near-zero
    /// weights that costs storage and search time while contributing almost
    /// nothing to the score. pgvector also refuses to build an HNSW index over
    /// a `sparsevec` beyond a bounded number of non-zero elements (1,000 as of
    /// pgvector 0.8), so long vectors *must* be pruned before indexing.
    pub fn prune_top_k(&self, k: usize) -> SparseVector {
        if k >= self.nnz() {
            return self.clone();
        }
        if k == 0 {
            return SparseVector::default();
        }
        let mut order: Vec<usize> = (0..self.nnz()).collect();
        // Highest magnitude first; ties broken by lower index for determinism.
        order.sort_unstable_by(|&a, &b| {
            self.values[b]
                .abs()
                .total_cmp(&self.values[a].abs())
                .then(self.indices[a].cmp(&self.indices[b]))
        });
        order.truncate(k);
        order.sort_unstable_by_key(|&i| self.indices[i]);
        SparseVector {
            indices: order.iter().map(|&i| self.indices[i]).collect(),
            values: order.iter().map(|&i| self.values[i]).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_rejects_length_mismatch() {
        assert!(SparseVector::new(vec![1, 2], vec![0.5]).is_none());
    }

    #[test]
    fn new_drops_zeros_and_sorts_by_index() {
        let sv = SparseVector::new(vec![5, 1, 3], vec![0.2, 0.9, 0.0]).unwrap();
        assert_eq!(sv.indices(), &[1, 5]);
        assert_eq!(sv.values(), &[0.9, 0.2]);
        assert_eq!(sv.nnz(), 2);
        assert!(!sv.is_empty());
    }

    #[test]
    fn empty_vector_is_empty() {
        let sv = SparseVector::new(vec![], vec![]).unwrap();
        assert!(sv.is_empty());
        assert_eq!(sv.nnz(), 0);
    }

    #[test]
    fn prune_keeps_highest_magnitude_and_index_order() {
        let sv = SparseVector::new(vec![1, 2, 3, 4], vec![0.1, 0.9, -0.7, 0.3]).unwrap();
        let p = sv.prune_top_k(2);
        // 0.9 (idx 2) and -0.7 (idx 3) survive; magnitude, not sign, decides.
        assert_eq!(p.indices(), &[2, 3]);
        assert_eq!(p.values(), &[0.9, -0.7]);
    }

    #[test]
    fn prune_is_noop_when_k_covers_everything() {
        let sv = SparseVector::new(vec![1, 2], vec![0.5, 0.4]).unwrap();
        assert_eq!(sv.prune_top_k(2), sv);
        assert_eq!(sv.prune_top_k(99), sv);
    }

    #[test]
    fn prune_to_zero_yields_empty() {
        let sv = SparseVector::new(vec![1, 2], vec![0.5, 0.4]).unwrap();
        assert!(sv.prune_top_k(0).is_empty());
    }
}
