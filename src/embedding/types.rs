//! Embedding types and trait definitions.

/// A single embedding result.
#[derive(Debug, Clone)]
pub struct EmbeddingResult {
    /// The embedding vector.
    pub vector: Vec<f32>,
    /// Source text (for debugging).
    pub text_preview: String,
}

impl EmbeddingResult {
    /// Dimensionality of the embedding vector.
    pub fn dim(&self) -> usize {
        self.vector.len()
    }

    /// Compute cosine similarity between two embedding results.
    pub fn cosine_similarity(&self, other: &EmbeddingResult) -> f32 {
        cosine_similarity(&self.vector, &other.vector)
    }
}

/// Compute cosine similarity between two f32 vectors.
///
/// Single-pass accumulation: dot product and both squared norms
/// are computed simultaneously to avoid three separate iterations.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (dot, sum_a, sum_b) = a
        .iter()
        .zip(b.iter())
        .fold((0.0f32, 0.0f32, 0.0f32), |(dot, sa, sb), (&x, &y)| {
            (dot + x * y, sa + x * x, sb + y * y)
        });
    let denom = sum_a.sqrt() * sum_b.sqrt();
    if denom == 0.0 { 0.0 } else { dot / denom }
}

/// Trait for embedding providers.
///
/// Implementations may use local models (candle, ONNX) or remote APIs (OpenAI).
pub trait EmbeddingProvider: Send + Sync {
    /// The dimensionality of the embedding vectors.
    fn dim(&self) -> usize;

    /// Embed a single text string.
    fn embed(&self, text: &str) -> anyhow::Result<EmbeddingResult>;

    /// Embed multiple texts in batch.
    ///
    /// The default implementation calls [`embed`](Self::embed) for each text.
    /// Returns an error on the **first** failure — successful results up to
    /// that point are discarded. For fine-grained error handling, call
    /// [`embed`](Self::embed) individually and collect results manually.
    fn embed_batch(&self, texts: &[&str]) -> anyhow::Result<Vec<EmbeddingResult>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }

    /// Provider name for display.
    fn name(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_similarity_identical() {
        let v = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!((cosine_similarity(&a, &b)).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_opposite() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        assert!((cosine_similarity(&a, &b) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_empty() {
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }

    #[test]
    fn cosine_similarity_unequal_len() {
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 2.0]), 0.0);
    }

    #[test]
    fn embedding_result_similarity() {
        let a = EmbeddingResult {
            vector: vec![1.0, 0.0],
            text_preview: "a".into(),
        };
        let b = EmbeddingResult {
            vector: vec![0.0, 1.0],
            text_preview: "b".into(),
        };
        assert!((a.cosine_similarity(&b)).abs() < 1e-6);
    }

    #[test]
    fn embedding_result_dim() {
        let e = EmbeddingResult {
            vector: vec![1.0, 0.0, 0.5],
            text_preview: "test".into(),
        };
        assert_eq!(e.dim(), 3);
    }
}
