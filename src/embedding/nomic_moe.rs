//! Nomic V2 MoE embedding provider via fastembed-rs candle backend.
//!
//! Uses the candle-nn pure Rust inference engine (no ONNX Runtime).
//! Models are downloaded from HuggingFace on first use.
//!
//! `nomic-ai/nomic-embed-text-v2-moe` — 475M total / 305M active params,
//! 8 experts with top-2 routing, hidden_size=768.
//!
//! ```ignore
//! use llm_kernel::embedding::NomicMoeProvider;
//! use llm_kernel::embedding::EmbeddingProvider;
//!
//! let provider = NomicMoeProvider::new()?;
//! let result = provider.embed("hello world")?;
//! ```

use crate::embedding::types::{EmbeddingProvider, EmbeddingResult};

/// Nomic V2 MoE embedding provider backed by candle-nn.
///
/// Unlike [`FastembedProvider`](super::FastembedProvider) (ONNX), this uses
/// candle for pure Rust GPU/CPU inference. The `embed()` method takes `&self`,
/// so no `Mutex` is needed.
pub struct NomicMoeProvider {
    inner: fastembed::NomicV2MoeTextEmbedding,
    model_id: String,
    dim: usize,
}

/// Default HuggingFace repo for nomic-embed-text-v2-moe.
pub const NOMIC_EMBED_TEXT_V2_MOE: &str = "nomic-ai/nomic-embed-text-v2-moe";

/// Default max sequence length for Nomic V2 MoE.
const DEFAULT_MAX_LENGTH: usize = 512;

impl NomicMoeProvider {
    /// Create a new provider using CPU with F32 precision.
    ///
    /// Downloads the model from HuggingFace on first call (cached locally).
    pub fn new() -> anyhow::Result<Self> {
        Self::with_options(
            NOMIC_EMBED_TEXT_V2_MOE,
            candle_core::Device::Cpu,
            candle_core::DType::F32,
            DEFAULT_MAX_LENGTH,
        )
    }

    /// Create with custom repo, device, dtype, and max sequence length.
    pub fn with_options(
        model_id: &str,
        device: candle_core::Device,
        dtype: candle_core::DType,
        max_length: usize,
    ) -> anyhow::Result<Self> {
        let te = fastembed::NomicV2MoeTextEmbedding::from_hf(model_id, &device, dtype, max_length)?;
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

impl EmbeddingProvider for NomicMoeProvider {
    fn dim(&self) -> usize {
        self.dim
    }

    fn name(&self) -> &str {
        &self.model_id
    }

    fn embed(&self, text: &str) -> anyhow::Result<EmbeddingResult> {
        let embeddings = self.inner.embed(&[text])?;
        let vector = embeddings
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("empty embedding output"))?;

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

    fn embed_batch(&self, texts: &[&str]) -> anyhow::Result<Vec<EmbeddingResult>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let embeddings = self.inner.embed(texts)?;
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
    fn model_id_constant() {
        assert_eq!(NOMIC_EMBED_TEXT_V2_MOE, "nomic-ai/nomic-embed-text-v2-moe");
    }

    #[test]
    #[ignore = "requires model download"]
    fn embed_with_nomic_moe() {
        let provider = NomicMoeProvider::new().unwrap();
        let result = provider.embed("hello world").unwrap();
        // nomic-embed-text-v2-moe hidden_size = 768
        assert_eq!(result.vector.len(), 768);
        assert_eq!(result.vector.len(), provider.dim());
    }

    #[test]
    #[ignore = "requires model download"]
    fn embed_batch_with_nomic_moe() {
        let provider = NomicMoeProvider::new().unwrap();
        let results = provider
            .embed_batch(&["hello", "world", "foo bar"])
            .unwrap();
        assert_eq!(results.len(), 3);
        for r in &results {
            assert_eq!(r.vector.len(), 768);
        }
    }
}
