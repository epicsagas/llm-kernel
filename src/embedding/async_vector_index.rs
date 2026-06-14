//! Async vector index trait for remote/shared backends.
//!
//! The existing [`VectorIndex`](crate::embedding::VectorIndex) is a synchronous,
//! in-process trait (`&mut self`, blocking `search`). That fits compressed
//! in-memory indexes (TurboQuant) but not remote vector services such as
//! Qdrant or Elasticsearch, whose clients are **async-only** and naturally
//! shared (`&self`) rather than exclusively borrowed.
//!
//! [`AsyncVectorIndex`] is the async, object-safe counterpart for those
//! backends. It mirrors the useful subset of [`VectorIndex`] — add, remove,
//! search, filtered search, length, dimensionality — and omits `save` because
//! remote backends persist server-side (just as [`VectorIndex`] omits `load`
//! to stay object-safe). Concrete implementations live in this crate behind
//! feature flags (e.g. the `qdrant` feature at `src/embedding/qdrant.rs`);
//! Elasticsearch will implement it in v0.9.0.
//!
//! The trait has no concrete dependencies beyond `async_trait`. It is defined
//! behind the `embedding` feature so the shared contract stays in the kernel
//! while the heavy client crates remain opt-in.

use anyhow::Result;

use crate::embedding::vector_index::SearchHit;

/// Async, object-safe vector index for remote/shared backends.
///
/// Implementations are remote vector services (Qdrant, Elasticsearch, …) whose
/// clients are async and shareable. Use `dyn AsyncVectorIndex` to abstract over
/// concrete backends.
///
/// IDs are always explicit — remote indexes do not auto-assign sequential IDs
/// the way an in-memory index can, so callers supply the `u64` external IDs.
#[async_trait::async_trait]
pub trait AsyncVectorIndex: Send + Sync {
    /// Upsert vectors keyed by their explicit external IDs.
    ///
    /// Re-upserting an existing ID replaces its vector. `vectors.len()` must
    /// equal `ids.len()`.
    async fn add(&self, vectors: &[Vec<f32>], ids: &[u64]) -> Result<()>;

    /// Remove vectors by their external IDs.
    ///
    /// IDs that do not exist are silently ignored. An empty slice is a no-op.
    async fn remove(&self, ids: &[u64]) -> Result<()>;

    /// Search for the `k` nearest neighbors of `query`.
    async fn search(&self, query: &[f32], k: usize) -> Result<Vec<SearchHit>>;

    /// Search restricted to an allowlist of candidate IDs.
    ///
    /// Mirrors [`VectorIndex::search_filtered`](crate::embedding::VectorIndex::search_filtered):
    /// narrow candidates (e.g. by metadata or BM25), then dense-rerank.
    async fn search_filtered(
        &self,
        query: &[f32],
        k: usize,
        allowlist: &[u64],
    ) -> Result<Vec<SearchHit>>;

    /// Number of vectors currently indexed.
    async fn len(&self) -> Result<usize>;

    /// Whether the index is empty.
    async fn is_empty(&self) -> Result<bool> {
        Ok(self.len().await? == 0)
    }

    /// Vector dimensionality.
    fn dim(&self) -> usize;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The trait is object-safe: a blanket stub demonstrates `dyn
    /// AsyncVectorIndex` compiles. The real backends live behind feature flags
    /// (e.g. the `qdrant` feature at `src/embedding/qdrant.rs`).
    struct StubIndex {
        d: usize,
    }

    #[async_trait::async_trait]
    impl AsyncVectorIndex for StubIndex {
        async fn add(&self, _vectors: &[Vec<f32>], _ids: &[u64]) -> Result<()> {
            Ok(())
        }
        async fn remove(&self, _ids: &[u64]) -> Result<()> {
            Ok(())
        }
        async fn search(&self, _query: &[f32], _k: usize) -> Result<Vec<SearchHit>> {
            Ok(Vec::new())
        }
        async fn search_filtered(
            &self,
            _query: &[f32],
            _k: usize,
            _allowlist: &[u64],
        ) -> Result<Vec<SearchHit>> {
            Ok(Vec::new())
        }
        async fn len(&self) -> Result<usize> {
            Ok(0)
        }
        fn dim(&self) -> usize {
            self.d
        }
    }

    /// AC2: `dyn AsyncVectorIndex` is usable (object-safety) and the default
    /// `is_empty` method composes over `len`.
    #[tokio::test]
    async fn dyn_async_vector_index_object_safe() {
        let idx: Box<dyn AsyncVectorIndex> = Box::new(StubIndex { d: 4 });
        assert_eq!(idx.dim(), 4);
        idx.add(&[vec![0.0; 4]], &[1]).await.unwrap();
        assert!(idx.is_empty().await.unwrap());
    }
}
