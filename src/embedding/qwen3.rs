//! Qwen3 embedding provider via fastembed-rs candle backend.
//!
//! Uses the candle-nn pure Rust inference engine (no ONNX Runtime).
//! Models are downloaded from HuggingFace on first use.
//!
//! Supported repos: `Qwen/Qwen3-Embedding-0.6B`, `Qwen/Qwen3-Embedding-8B`,
//! `Qwen/Qwen3-VL-Embedding-2B` (text-only mode).
//!
//! ```ignore
//! use llm_kernel::embedding::Qwen3Provider;
//! use llm_kernel::embedding::EmbeddingProvider;
//!
//! let provider = Qwen3Provider::new("Qwen/Qwen3-Embedding-0.6B")?;
//! let result = provider.embed("hello world")?;
//! ```

use crate::embedding::types::{EmbeddingProvider, EmbeddingResult};
use crate::error::{KernelError, Result};

/// Qwen3 embedding provider backed by candle-nn.
///
/// Unlike [`FastembedProvider`](super::FastembedProvider) (ONNX), this uses
/// candle for pure Rust GPU/CPU inference. The `embed()` method takes `&self`,
/// so no `Mutex` is needed.
pub struct Qwen3Provider {
    inner: fastembed::Qwen3TextEmbedding,
    model_id: String,
    dim: usize,
}

/// Default HuggingFace repo for Qwen3-Embedding-0.6B.
pub const QWEN3_EMBEDDING_0_6B: &str = "Qwen/Qwen3-Embedding-0.6B";

/// Default HuggingFace repo for Qwen3-Embedding-8B.
pub const QWEN3_EMBEDDING_8B: &str = "Qwen/Qwen3-Embedding-8B";

/// Default HuggingFace repo for Qwen3-VL-Embedding-2B (text-only mode).
pub const QWEN3_VL_EMBEDDING_2B: &str = "Qwen/Qwen3-VL-Embedding-2B";

/// Default max sequence length for Qwen3 models.
const DEFAULT_MAX_LENGTH: usize = 512;

impl Qwen3Provider {
    /// Create a new provider using CPU with F32 precision.
    ///
    /// Downloads the model from HuggingFace on first call (cached locally).
    pub fn new(model_id: &str) -> Result<Self> {
        Self::with_options(
            model_id,
            candle_core::Device::Cpu,
            candle_core::DType::F32,
            DEFAULT_MAX_LENGTH,
        )
    }

    /// Create with custom device (GPU), dtype, and max sequence length.
    pub fn with_options(
        model_id: &str,
        device: candle_core::Device,
        dtype: candle_core::DType,
        max_length: usize,
    ) -> Result<Self> {
        let te = fastembed::Qwen3TextEmbedding::from_hf(model_id, &device, dtype, max_length)
            .map_err(KernelError::embedding)?;
        let dim = te.config().hidden_size;
        Ok(Self {
            inner: te,
            model_id: model_id.to_string(),
            dim,
        })
    }

    /// The HuggingFace model repo ID.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }
}

impl EmbeddingProvider for Qwen3Provider {
    fn dim(&self) -> usize {
        self.dim
    }

    fn name(&self) -> &str {
        &self.model_id
    }

    fn embed(&self, text: &str) -> Result<EmbeddingResult> {
        let embeddings = self.inner.embed(&[text]).map_err(KernelError::embedding)?;
        let vector = embeddings
            .into_iter()
            .next()
            .ok_or_else(|| KernelError::Embedding("empty embedding output".into()))?;

        let preview = if text.len() > 64 {
            format!("{}…", &text[..64])
        } else {
            text.to_string()
        };
        Ok(EmbeddingResult {
            vector,
            text_preview: preview,
        })
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<EmbeddingResult>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let embeddings = self.inner.embed(texts).map_err(KernelError::embedding)?;
        Ok(embeddings
            .into_iter()
            .zip(texts.iter())
            .map(|(vector, &text)| {
                let preview = if text.len() > 64 {
                    format!("{}…", &text[..64])
                } else {
                    text.to_string()
                };
                EmbeddingResult {
                    vector,
                    text_preview: preview,
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_id_constants() {
        assert_eq!(QWEN3_EMBEDDING_0_6B, "Qwen/Qwen3-Embedding-0.6B");
        assert_eq!(QWEN3_EMBEDDING_8B, "Qwen/Qwen3-Embedding-8B");
        assert_eq!(QWEN3_VL_EMBEDDING_2B, "Qwen/Qwen3-VL-Embedding-2B");
    }

    #[test]
    #[ignore = "requires model download"]
    fn embed_with_qwen3_0_6b() {
        let provider = Qwen3Provider::new(QWEN3_EMBEDDING_0_6B).unwrap();
        let result = provider.embed("hello world").unwrap();
        // Qwen3-Embedding-0.6B has hidden_size that the config reports
        assert!(!result.vector.is_empty());
        assert_eq!(result.vector.len(), provider.dim());
    }

    #[test]
    #[ignore = "requires model download"]
    fn embed_batch_with_qwen3() {
        let provider = Qwen3Provider::new(QWEN3_EMBEDDING_0_6B).unwrap();
        let results = provider
            .embed_batch(&["hello", "world", "foo bar"])
            .unwrap();
        assert_eq!(results.len(), 3);
        for r in &results {
            assert_eq!(r.vector.len(), provider.dim());
        }
    }
}
