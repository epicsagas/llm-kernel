//! Cross-engine search federation over multiple vector backends.
//!
//! Federation queries several [`AsyncVectorIndex`](crate::embedding::AsyncVectorIndex)
//! backends concurrently and merges their results with the existing fusion
//! functions ([`rrf_fuse`],
//! [`weighted_sum_fuse`]).
//!
//! # Why RRF is the default
//!
//! Heterogeneous backends score on incompatible scales: Qdrant (cosine
//! distance) returns scores in `[0, 1]`; Elasticsearch knn `_score` is
//! `(1 + cosine) / 2`, also in `[0, 1]` but a *different* monotonic transform;
//! the in-memory `TurbovecIndex` returns raw cosine in `[-1, 1]`. Score-based
//! fusion (weighted sum) over these raw values ranks documents incorrectly
//! because a `0.3` from one backend is not comparable to a `0.3` from another.
//!
//! [`FusionStrategy::Rrf`] is **rank-based** (`1/(k + rank)`), so it is
//! scale-invariant — it fuses heterogeneous backends correctly with **no**
//! normalization. That is why it is the default. [`FusionStrategy::WeightedSum`]
//! is opt-in: it normalizes each list with min-max first, which is only correct
//! when every backend scores on a comparable scale, so it carries a caveat.

use crate::search::SearchResult;
use crate::search::fusion::{normalize_minmax, weighted_sum_fuse};
use crate::search::rrf_fuse;

/// How federated result lists are merged.
///
/// Defaults to [`FusionStrategy::Rrf`] (rank-based, scale-invariant). See the
/// [module docs](self) for why this matters across heterogeneous backends.
#[derive(Debug, Clone)]
pub enum FusionStrategy {
    /// Reciprocal Rank Fusion with constant `k` (typically 60). Rank-based, so
    /// no score normalization is required across backends.
    Rrf {
        /// RRF smoothing constant (larger = flatter).
        k: u32,
    },
    /// Weighted sum of min-max-normalized per-list scores. Each list is
    /// normalized in isolation before summing, so this is only correct when
    /// every backend scores on a comparable scale — otherwise prefer
    /// [`FusionStrategy::Rrf`].
    WeightedSum,
}

impl Default for FusionStrategy {
    fn default() -> Self {
        FusionStrategy::Rrf { k: 60 }
    }
}

/// Fuse pre-fetched result lists with no I/O.
///
/// Lets a synchronous backend (e.g. the in-memory
/// [`TurbovecIndex`](crate::embedding::TurbovecIndex)) participate in
/// federation: the caller searches it directly, then folds its list in here
/// alongside lists gathered from async backends (or any source). All backends
/// contribute equally (weight `1.0`).
///
/// ```
/// use llm_kernel::search::{SearchResult, federation::{federate_results, FusionStrategy}};
///
/// let qdrant = vec![SearchResult { id: "1".into(), score: 0.9, text: String::new() }];
/// let es = vec![SearchResult { id: "1".into(), score: 0.97, text: String::new() }];
/// let turbovec = vec![SearchResult { id: "1".into(), score: 0.3, text: String::new() }];
///
/// let merged = federate_results(&[qdrant, es, turbovec], &FusionStrategy::default());
/// assert_eq!(merged.len(), 1); // shared id deduped, not tripled
/// ```
pub fn federate_results(
    lists: &[Vec<SearchResult>],
    strategy: &FusionStrategy,
) -> Vec<SearchResult> {
    match strategy {
        FusionStrategy::Rrf { k } => rrf_fuse(lists, *k),
        FusionStrategy::WeightedSum => {
            let normed: Vec<Vec<SearchResult>> = lists
                .iter()
                .map(|l| {
                    let mut c = l.clone();
                    normalize_minmax(&mut c);
                    c
                })
                .collect();
            let weights = vec![1.0_f32; normed.len()];
            weighted_sum_fuse(&normed, &weights)
        }
    }
}

// ---------------------------------------------------------------------------
// Async federation over AsyncVectorIndex backends (needs the `federation` feature).
// ---------------------------------------------------------------------------

#[cfg(feature = "federation")]
mod federated {
    use std::sync::Arc;
    use std::time::Duration;

    use futures_util::future::join_all;

    use crate::embedding::{AsyncVectorIndex, SearchHit};
    use crate::error::{KernelError, Result};
    use crate::search::SearchResult;
    use crate::search::fusion::{normalize_minmax, weighted_sum_fuse};
    use crate::search::rrf_fuse;

    use super::FusionStrategy;

    /// One backend in a [`FederatedSearch`]: the index and its fusion weight.
    struct Backend {
        index: Arc<dyn AsyncVectorIndex>,
        weight: f32,
    }

    /// Map u64-keyed [`SearchHit`]s into the String-id [`SearchResult`] shape the
    /// fusion functions expect, canonicalizing the id so a shared document
    /// merges across backends rather than appearing multiple times.
    fn hits_to_results(hits: Vec<SearchHit>) -> Vec<SearchResult> {
        hits.into_iter()
            .map(|h| SearchResult {
                id: h.id.to_string(),
                score: h.score,
                text: String::new(),
            })
            .collect()
    }

    /// Concurrent search over multiple [`AsyncVectorIndex`] backends.
    ///
    /// Queries every backend at once, applies a per-backend timeout so one slow
    /// remote cannot stall the whole query, drops failing or timed-out backends
    /// with an observable `tracing::warn!`, and merges the survivors with the
    /// configured [`FusionStrategy`]. If **every** backend fails, returns
    /// [`KernelError::Search`].
    ///
    /// Synchronous backends (e.g. `TurbovecIndex`) participate via
    /// [`federate_results`](super::federate_results) instead — search them
    /// directly and fold the list in.
    pub struct FederatedSearch {
        backends: Vec<Backend>,
        strategy: FusionStrategy,
        timeout: Duration,
    }

    impl Default for FederatedSearch {
        fn default() -> Self {
            Self {
                backends: Vec::new(),
                strategy: FusionStrategy::default(),
                timeout: Duration::from_secs(5),
            }
        }
    }

    impl FederatedSearch {
        /// Create an empty federated search (default strategy RRF k=60, 5s timeout).
        pub fn new() -> Self {
            Self::default()
        }

        /// Add a backend with a fusion weight (used only by
        /// [`FusionStrategy::WeightedSum`]; ignored by RRF).
        #[must_use]
        pub fn with_backend(mut self, index: Arc<dyn AsyncVectorIndex>, weight: f32) -> Self {
            self.backends.push(Backend { index, weight });
            self
        }

        /// Set the fusion strategy (default [`FusionStrategy::Rrf`] k=60).
        #[must_use]
        pub fn strategy(mut self, strategy: FusionStrategy) -> Self {
            self.strategy = strategy;
            self
        }

        /// Set the per-backend query timeout (default 5s). A backend that
        /// exceeds it is dropped with a warning rather than blocking the query.
        #[must_use]
        pub fn timeout(mut self, timeout: Duration) -> Self {
            self.timeout = timeout;
            self
        }

        /// Run `query` against every backend concurrently, merge survivors.
        ///
        /// Each backend is queried for `2 * k` results (over-fetch) so RRF
        /// rank-credit is preserved for a document that ranks just below `k` in
        /// one backend but near the top in another; the fused list is then
        /// truncated to the requested `k`. A per-backend timeout drops slow or
        /// failing backends with an observable `tracing::warn!` rather than
        /// stalling the query.
        ///
        /// Returns the fused result list (at most `k` items).
        /// [`KernelError::Search`] is returned only when *no* backend succeeded;
        /// one or more survivors yield a partial (but non-empty) merged result.
        pub async fn search(&self, query: &[f32], k_req: usize) -> Result<Vec<SearchResult>> {
            if self.backends.is_empty() {
                return Ok(Vec::new());
            }

            // Snapshot (index, weight) so each future is self-contained.
            let entries: Vec<(Arc<dyn AsyncVectorIndex>, f32)> = self
                .backends
                .iter()
                .map(|b| (b.index.clone(), b.weight))
                .collect();
            let timeout = self.timeout;

            // Over-fetch each backend so RRF rank-credit is preserved for
            // documents that rank just below k in one backend but appear near
            // the top in another. Standard RRF practice: fetch ~2k, fuse, then
            // truncate the merged list to the requested k (done after fusion).
            // `saturating_mul` guards usize overflow and yields 0 for k == 0.
            let fetch_k = k_req.saturating_mul(2);

            let futs = entries.into_iter().map(|(index, weight)| {
                let q = query.to_vec();
                async move {
                    match tokio::time::timeout(timeout, index.search(&q, fetch_k)).await {
                        Ok(Ok(hits)) => Some((weight, hits)),
                        Ok(Err(e)) => {
                            tracing::warn!("federated backend errored; excluding: {e}");
                            None
                        }
                        Err(_elapsed) => {
                            tracing::warn!(
                                "federated backend timed out after {:?}; excluding",
                                timeout
                            );
                            None
                        }
                    }
                }
            });
            let collected: Vec<Option<(f32, Vec<SearchHit>)>> = join_all(futs).await;

            let ok: Vec<(f32, Vec<SearchHit>)> = collected.into_iter().flatten().collect();
            if ok.is_empty() {
                return Err(KernelError::Search(
                    "all federated backends failed or timed out".into(),
                ));
            }

            // Adapt u64-keyed hits into the String-id SearchResult shape fusion
            // expects, canonicalizing the id so a shared document merges across
            // backends rather than appearing multiple times. `ok` is consumed
            // once: RRF needs only the lists, WeightedSum additionally needs the
            // per-backend weights (collected inside that arm). Note: the RRF
            // smoothing constant is named `k` by the `FusionStrategy::Rrf`
            // variant, which is why the requested count is `k_req` here — the
            // two must not be confused at the truncation step.
            let mut fused = match self.strategy {
                FusionStrategy::Rrf { k } => {
                    let lists: Vec<Vec<SearchResult>> = ok
                        .into_iter()
                        .map(|(_w, hits)| hits_to_results(hits))
                        .collect();
                    rrf_fuse(&lists, k)
                }
                FusionStrategy::WeightedSum => {
                    let mut lists: Vec<Vec<SearchResult>> = Vec::with_capacity(ok.len());
                    let mut weights: Vec<f32> = Vec::with_capacity(ok.len());
                    for (w, hits) in ok {
                        let mut list = hits_to_results(hits);
                        normalize_minmax(&mut list);
                        lists.push(list);
                        weights.push(w);
                    }
                    weighted_sum_fuse(&lists, &weights)
                }
            };
            // The over-fetch produced lists longer than requested; trim the
            // fused output back to exactly `k_req`. `truncate` is a no-op if
            // the fused list is already shorter (e.g. a backend with fewer than
            // `fetch_k` documents).
            fused.truncate(k_req);
            Ok(fused)
        }
    }
}

#[cfg(feature = "federation")]
pub use federated::FederatedSearch;

#[cfg(test)]
mod tests {
    use super::*;

    fn hits(ids: &[(&str, f32)]) -> Vec<SearchResult> {
        ids.iter()
            .map(|(id, score)| SearchResult {
                id: (*id).to_string(),
                score: *score,
                text: String::new(),
            })
            .collect()
    }

    /// AC5: RRF is scale-invariant — heterogeneous raw-score scales (Qdrant
    /// cosine, ES `(1+cos)/2`, TurboVec raw cosine) fuse correctly under the
    /// default strategy with no manual normalization. A document ranked #1 in
    /// all three lists tops the merge regardless of wildly different scores.
    #[test]
    fn rrf_fuses_heterogeneous_scales_correctly() {
        // Qdrant cosine [0,1]
        let qdrant = hits(&[("shared", 0.90), ("a", 0.50)]);
        // ES _score (1+cos)/2 [0,1] — note 0.97 here corresponds to cos≈0.94
        let es = hits(&[("shared", 0.97), ("b", 0.70)]);
        // TurboVec raw cosine [-1,1] — shared only scores 0.3 on this scale
        let turbovec = hits(&[("shared", 0.30), ("c", -0.50)]);

        let merged = federate_results(&[qdrant, es, turbovec], &FusionStrategy::default());

        // "shared" is rank 0 in all three → highest RRF score.
        assert_eq!(merged[0].id, "shared");
    }

    /// AC6: a shared id present in all backends is deduped (merged) once, and
    /// accumulates rank-credit so it outranks any single-backend document.
    #[test]
    fn shared_id_is_deduped_and_boosted() {
        let qdrant = hits(&[("shared", 1.0), ("only_q", 0.9)]);
        let es = hits(&[("shared", 1.0)]);
        let turbovec = hits(&[("shared", 1.0)]);

        let merged = federate_results(&[qdrant, es, turbovec], &FusionStrategy::default());

        // "shared" appears exactly once.
        let shared_count = merged.iter().filter(|r| r.id == "shared").count();
        assert_eq!(shared_count, 1);
        assert_eq!(merged.len(), 2); // shared + only_q
        // shared: rank 0 in three lists; only_q: rank 1 in one list.
        let shared_score = merged.iter().find(|r| r.id == "shared").unwrap().score;
        let only_q_score = merged.iter().find(|r| r.id == "only_q").unwrap().score;
        assert!(shared_score > only_q_score);
    }

    #[test]
    fn weighted_sum_strategy_runs() {
        let a = hits(&[("x", 0.0), ("y", 1.0)]);
        let b = hits(&[("y", 1.0), ("z", 0.4)]);
        let merged = federate_results(&[a, b], &FusionStrategy::WeightedSum);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].id, "y");
    }
}

#[cfg(all(test, feature = "federation"))]
mod async_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;

    use crate::embedding::{AsyncVectorIndex, SearchHit};
    use crate::error::{KernelError, Result};
    use crate::search::federation::{FederatedSearch, FusionStrategy};

    /// Configurable stub backend: returns canned hits, optionally fails, or
    /// delays past a timeout.
    struct StubIndex {
        hits: Vec<SearchHit>,
        delay: Option<Duration>,
        fail: bool,
        dim: usize,
    }

    #[async_trait]
    impl AsyncVectorIndex for StubIndex {
        async fn add(&self, _vectors: &[Vec<f32>], _ids: &[u64]) -> Result<()> {
            Ok(())
        }
        async fn remove(&self, _ids: &[u64]) -> Result<()> {
            Ok(())
        }
        async fn search(&self, _query: &[f32], _k: usize) -> Result<Vec<SearchHit>> {
            if let Some(d) = self.delay {
                tokio::time::sleep(d).await;
            }
            if self.fail {
                return Err(KernelError::Embedding("stub backend failure".into()));
            }
            Ok(self.hits.clone())
        }
        async fn search_filtered(
            &self,
            _query: &[f32],
            _k: usize,
            _allowlist: &[u64],
        ) -> Result<Vec<SearchHit>> {
            Ok(self.hits.clone())
        }
        async fn len(&self) -> Result<usize> {
            Ok(self.hits.len())
        }
        fn dim(&self) -> usize {
            self.dim
        }
    }

    fn hit(id: u64, score: f32) -> SearchHit {
        SearchHit { id, score }
    }

    /// AC4: a slow backend that exceeds the timeout is dropped without failing
    /// the query; the fast backend's results still come through.
    #[tokio::test]
    async fn slow_backend_is_dropped_not_blocking() {
        let fast = Arc::new(StubIndex {
            hits: vec![hit(1, 0.9), hit(2, 0.5)],
            delay: None,
            fail: false,
            dim: 4,
        });
        let slow = Arc::new(StubIndex {
            hits: vec![hit(3, 1.0)],
            delay: Some(Duration::from_millis(500)),
            fail: false,
            dim: 4,
        });

        let fed = FederatedSearch::new()
            .with_backend(fast, 1.0)
            .with_backend(slow, 1.0)
            .timeout(Duration::from_millis(50));

        let merged = fed.search(&[1.0, 0.0, 0.0, 0.0], 5).await.unwrap();
        // Only the fast backend contributed; id 3 (from the timed-out backend)
        // is absent, but the query still succeeded.
        assert!(merged.iter().any(|r| r.id == "1"));
        assert!(!merged.iter().any(|r| r.id == "3"));
    }

    /// AC4: a failing backend is excluded; survivors still return results.
    #[tokio::test]
    async fn failing_backend_is_excluded() {
        let good = Arc::new(StubIndex {
            hits: vec![hit(7, 0.8)],
            delay: None,
            fail: false,
            dim: 4,
        });
        let bad = Arc::new(StubIndex {
            hits: vec![],
            delay: None,
            fail: true,
            dim: 4,
        });

        let merged = FederatedSearch::new()
            .with_backend(good, 1.0)
            .with_backend(bad, 1.0)
            .search(&[0.0, 0.0, 0.0, 1.0], 3)
            .await
            .unwrap();
        assert!(merged.iter().any(|r| r.id == "7"));
    }

    /// AC4: when *every* backend fails, `search` returns `Err`.
    #[tokio::test]
    async fn all_backends_failing_returns_err() {
        let bad = Arc::new(StubIndex {
            hits: vec![],
            delay: None,
            fail: true,
            dim: 4,
        });
        let res = FederatedSearch::new()
            .with_backend(bad.clone(), 1.0)
            .with_backend(bad, 1.0)
            .search(&[0.0; 4], 3)
            .await;
        assert!(res.is_err());
    }

    /// AC4/AC5: two healthy backends merge under the default RRF strategy.
    #[tokio::test]
    async fn two_backends_merge_via_rrf() {
        let a = Arc::new(StubIndex {
            hits: vec![hit(1, 0.99), hit(2, 0.4)],
            delay: None,
            fail: false,
            dim: 4,
        });
        let b = Arc::new(StubIndex {
            hits: vec![hit(2, 0.95), hit(3, 0.6)],
            delay: None,
            fail: false,
            dim: 4,
        });
        let merged = FederatedSearch::new()
            .with_backend(a, 1.0)
            .with_backend(b, 1.0)
            .search(&[1.0, 0.0, 0.0, 0.0], 5)
            .await
            .unwrap();
        // id 2 is rank 0/1 across both → top.
        assert_eq!(merged[0].id, "2");
        assert_eq!(merged.len(), 3);
        // Strategy default is RRF k=60.
        assert!(matches!(
            FusionStrategy::default(),
            FusionStrategy::Rrf { k: 60 }
        ));
    }

    /// Guards the refactored WeightedSum async arm (the weights-collection loop
    /// moved inside that arm): two backends with distinct weights still merge,
    /// with results from both backends present.
    #[tokio::test]
    async fn two_backends_merge_via_weighted_sum() {
        let a = Arc::new(StubIndex {
            hits: vec![hit(1, 1.0), hit(2, 0.2)],
            delay: None,
            fail: false,
            dim: 4,
        });
        let b = Arc::new(StubIndex {
            hits: vec![hit(2, 1.0), hit(3, 0.1)],
            delay: None,
            fail: false,
            dim: 4,
        });
        let merged = FederatedSearch::new()
            .with_backend(a, 0.75)
            .with_backend(b, 0.25)
            .strategy(FusionStrategy::WeightedSum)
            .search(&[1.0, 0.0, 0.0, 0.0], 5)
            .await
            .unwrap();
        // a's top (id 1, normalized weight 0.75) leads; both backends present.
        assert!(!merged.is_empty());
        assert_eq!(merged[0].id, "1");
        assert!(merged.iter().any(|r| r.id == "2")); // shared, deduped
        assert!(merged.iter().any(|r| r.id == "3")); // from b
    }

    /// No backends configured → empty result, no error.
    #[tokio::test]
    async fn no_backends_returns_empty() {
        let merged = FederatedSearch::new().search(&[0.0; 4], 3).await.unwrap();
        assert!(merged.is_empty());
    }

    // --- over-fetch / truncate (hardening) ---------------------------------
    //
    // `StubIndex` above ignores its `k` argument, so it cannot exercise the
    // fetch-2k-then-truncate behavior. `RankAwareStub` honors `k` by returning
    // only the first `k` of its canned list, letting us prove the over-fetch
    // preserves RRF rank-credit and the output is truncated to the requested k.

    /// Stub backend that honors the requested `k`: returns the first `k` of its
    /// canned list (clamped to the list length). This mirrors how a real
    /// `AsyncVectorIndex` returns at most `k` neighbors.
    struct RankAwareStub {
        hits: Vec<SearchHit>,
        dim: usize,
    }

    #[async_trait]
    impl AsyncVectorIndex for RankAwareStub {
        async fn add(&self, _vectors: &[Vec<f32>], _ids: &[u64]) -> Result<()> {
            Ok(())
        }
        async fn remove(&self, _ids: &[u64]) -> Result<()> {
            Ok(())
        }
        async fn search(&self, _query: &[f32], k: usize) -> Result<Vec<SearchHit>> {
            Ok(self.hits.iter().take(k).cloned().collect())
        }
        async fn search_filtered(
            &self,
            _query: &[f32],
            k: usize,
            _allowlist: &[u64],
        ) -> Result<Vec<SearchHit>> {
            Ok(self.hits.iter().take(k).cloned().collect())
        }
        async fn len(&self) -> Result<usize> {
            Ok(self.hits.len())
        }
        fn dim(&self) -> usize {
            self.dim
        }
    }

    /// Without over-fetch, a document that ranks just below `k` in one backend
    /// but at the top in another loses its rank-credit: the first backend never
    /// returns it (it asked for only `k`). Over-fetching `2 * k` per backend
    /// lets that document enter both lists, so RRF credits it from both and it
    /// survives the final truncate to `k`.
    #[tokio::test]
    async fn over_fetch_preserves_rank_credit_across_backends() {
        // Backend A ranks `shared` at position 2 (rank index 2) — below k=2,
        // so a bare `k` query would never return it. Backend B ranks `shared`
        // first. With over-fetch (fetch_k = 4), A DOES return `shared`.
        let a = Arc::new(RankAwareStub {
            hits: vec![hit(101, 0.99), hit(102, 0.9), hit(7, 0.8), hit(8, 0.7)],
            dim: 4,
        });
        let b = Arc::new(RankAwareStub {
            hits: vec![hit(7, 1.0), hit(9, 0.6)],
            dim: 4,
        });

        let merged = FederatedSearch::new()
            .with_backend(a, 1.0)
            .with_backend(b, 1.0)
            // k = 2 → each backend is queried for 4; id 7 appears in BOTH
            // lists (rank 2 in A, rank 0 in B) and accumulates rank-credit, so
            // it outranks the single-backend filler docs and makes the top 2.
            .search(&[1.0, 0.0, 0.0, 0.0], 2)
            .await
            .unwrap();

        // Truncated to exactly k = 2.
        assert_eq!(merged.len(), 2);
        // id 7 is present (it appeared in both over-fetched lists). Without
        // over-fetch it would have been dropped by backend A and could not
        // accumulate cross-backend credit.
        assert!(
            merged.iter().any(|r| r.id == "7"),
            "id 7 should survive via over-fetch rank-credit: {merged:?}"
        );
    }

    /// The fused output is truncated to the requested `k`, never more, even
    /// when every backend has far more than `k` documents.
    #[tokio::test]
    async fn fused_output_is_truncated_to_requested_k() {
        let a = Arc::new(RankAwareStub {
            hits: (1..=20).map(|i| hit(i, 1.0 - i as f32 * 0.01)).collect(),
            dim: 4,
        });
        let b = Arc::new(RankAwareStub {
            hits: (21..=40).map(|i| hit(i, 0.5 - i as f32 * 0.01)).collect(),
            dim: 4,
        });
        let merged = FederatedSearch::new()
            .with_backend(a, 1.0)
            .with_backend(b, 1.0)
            .search(&[1.0, 0.0, 0.0, 0.0], 5)
            .await
            .unwrap();
        assert_eq!(merged.len(), 5, "fused output must be truncated to k");
    }

    /// `k == 0` with configured backends yields an empty (non-error) result:
    /// fetch_k == 0 → each backend returns nothing → fused empty → truncate(0).
    #[tokio::test]
    async fn k_zero_with_backends_returns_empty_not_err() {
        let a = Arc::new(RankAwareStub {
            hits: vec![hit(1, 0.9)],
            dim: 4,
        });
        let merged = FederatedSearch::new()
            .with_backend(a, 1.0)
            .search(&[1.0, 0.0, 0.0, 0.0], 0)
            .await
            .unwrap();
        assert!(merged.is_empty());
    }
}
