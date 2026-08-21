//! L2 — fingerprint matching of registered sensitive documents.
//!
//! Registers sensitive documents by embedding and cosine-matches outbound
//! content against them, via any
//! [`EmbeddingProvider`](crate::embedding::EmbeddingProvider) (e.g. the
//! kernel's BGE-M3 fastembed backend).
//!
//! `match_content` returns `Result<Option<..>>` rather than a bare `Option`
//! deliberately: embedding is fallible (model load, ONNX runtime), and an
//! error silently flattened to `None` would read as a false "clean" verdict
//! on a DLP path.

use crate::embedding::{EmbeddingProvider, SearchHit, cosine_similarity};
use crate::error::Result;
use std::sync::Arc;

/// Default cosine threshold for [`FingerprintIndex::new`].
pub const DEFAULT_THRESHOLD: f64 = 0.85;

/// Cosine-matches outbound content against registered sensitive documents.
///
/// Registration embeds with the **document** prefix; matching embeds with the
/// **query** prefix (see `EmbeddingProvider::embed` / `embed_document` for
/// the asymmetric-model rationale).
// ponytail: linear cosine scan — μs at hundreds of docs, ~ms at 10k
// (1024-dim). Switch to `TurbovecIndex` (feature `vector-index`) past ~10k
// registered documents.
pub struct FingerprintIndex {
    provider: Arc<dyn EmbeddingProvider>,
    docs: Vec<(u64, Vec<f32>)>,
    threshold: f64,
}

impl FingerprintIndex {
    /// New index with the default match threshold (0.85).
    pub fn new(provider: Arc<dyn EmbeddingProvider>) -> Self {
        Self::with_threshold(provider, DEFAULT_THRESHOLD)
    }

    /// New index with an explicit cosine threshold (higher = stricter).
    pub fn with_threshold(provider: Arc<dyn EmbeddingProvider>, threshold: f64) -> Self {
        Self {
            provider,
            docs: Vec::new(),
            threshold,
        }
    }

    /// Register (or re-register — appends) a sensitive document under
    /// `doc_id`.
    pub fn register(&mut self, doc_id: u64, text: &str) -> Result<()> {
        let vector = self.provider.embed_document(text)?.vector;
        self.docs.push((doc_id, vector));
        Ok(())
    }

    /// Number of registered document vectors.
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    /// Whether nothing is registered.
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// Embed `content` (query prefix) and return the best registered document
    /// at or above the threshold, or `Ok(None)` when nothing matches.
    pub fn match_content(&self, content: &str) -> Result<Option<SearchHit>> {
        if self.docs.is_empty() {
            return Ok(None);
        }
        let query = self.provider.embed(content)?.vector;
        let best = self
            .docs
            .iter()
            .filter_map(|&(id, ref v)| {
                let score = cosine_similarity(&query, v);
                (score >= self.threshold).then_some(SearchHit {
                    id,
                    score: score as f32,
                })
            })
            .max_by(|a, b| {
                a.score
                    .partial_cmp(&b.score)
                    .expect("scores are finite (cosine of finite vectors)")
            });
        Ok(best)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::EmbeddingResult;
    use crate::embedding::types::normalize;

    /// Deterministic fake: bag-of-characters vector over 8 dims — no model
    /// download, similar texts → similar vectors.
    struct FakeProvider;

    fn fake_vector(text: &str) -> Vec<f32> {
        let mut v = vec![0f32; 8];
        for b in text.bytes() {
            v[(b % 8) as usize] += 1.0;
        }
        normalize(&mut v);
        v
    }

    impl EmbeddingProvider for FakeProvider {
        fn dim(&self) -> usize {
            8
        }
        fn embed(&self, text: &str) -> Result<EmbeddingResult> {
            Ok(EmbeddingResult {
                vector: fake_vector(text),
                text_preview: text.chars().take(16).collect(),
            })
        }
        fn name(&self) -> &str {
            "fake"
        }
    }

    #[test]
    fn near_copy_matches_registered_doc() {
        let mut index = FingerprintIndex::new(Arc::new(FakeProvider));
        index.register(1, "confidential merger memo draft").unwrap();
        index.register(2, "public weather forecast notes").unwrap();

        let hit = index
            .match_content("confidential merger memo final")
            .unwrap()
            .expect("near-copy should match");
        assert_eq!(hit.id, 1);
        assert!(hit.score >= DEFAULT_THRESHOLD as f32);
    }

    #[test]
    fn unrelated_content_returns_none() {
        let mut index = FingerprintIndex::new(Arc::new(FakeProvider));
        index.register(1, "confidential merger memo draft").unwrap();
        assert!(index.match_content("zzz qqq xxx www").unwrap().is_none());
    }

    #[test]
    fn empty_index_returns_none() {
        let index = FingerprintIndex::new(Arc::new(FakeProvider));
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
        assert!(index.match_content("anything").unwrap().is_none());
    }

    #[test]
    fn stricter_threshold_blocks_even_identical_text() {
        let mut index = FingerprintIndex::with_threshold(Arc::new(FakeProvider), 2.0);
        index.register(1, "confidential merger memo draft").unwrap();
        // Cosine can never reach 2.0 — even the identical text is excluded.
        assert!(
            index
                .match_content("confidential merger memo draft")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn zero_threshold_returns_best_doc() {
        let mut index = FingerprintIndex::with_threshold(Arc::new(FakeProvider), 0.0);
        index.register(7, "alpha").unwrap();
        let hit = index
            .match_content("beta")
            .unwrap()
            .expect("zero threshold admits any nonzero-overlap candidate");
        assert_eq!(hit.id, 7);
    }

    #[test]
    fn fingerprint_index_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FingerprintIndex>();
    }
}
