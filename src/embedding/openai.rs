//! OpenAI text-embedding provider (sync, via ureq).

use serde::Deserialize;

use crate::embedding::types::{EmbeddingProvider, EmbeddingResult};
use crate::error::{KernelError, Result};

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    index: usize,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

/// OpenAI embedding provider.
///
/// Uses `text-embedding-3-small` (1536-dim) by default.
/// Swap model to `text-embedding-3-large` (3072-dim) for higher accuracy.
///
/// `api_key` is not exposed via `Debug` to prevent accidental logging.
pub struct OpenAIEmbeddingClient {
    api_key: String,
    model: String,
    dim: usize,
}

impl OpenAIEmbeddingClient {
    /// Create with `text-embedding-3-small` (1536 dimensions).
    pub fn new_small(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: "text-embedding-3-small".into(),
            dim: 1536,
        }
    }

    /// Create with `text-embedding-3-large` (3072 dimensions).
    pub fn new_large(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: "text-embedding-3-large".into(),
            dim: 3072,
        }
    }

    /// Create with an explicit model name and embedding dimension.
    ///
    /// Use this for legacy models (`text-embedding-ada-002`), reduced-dimension
    /// variants, or any future model not covered by [`new_small`](Self::new_small)
    /// and [`new_large`](Self::new_large).
    ///
    /// # Panics
    ///
    /// Panics if `dim` is zero.
    pub fn new_with_model(
        api_key: impl Into<String>,
        model: impl Into<String>,
        dim: usize,
    ) -> Self {
        assert!(dim > 0, "dim must be non-zero");
        Self {
            api_key: api_key.into(),
            model: model.into(),
            dim,
        }
    }

    /// Create from environment variable `OPENAI_API_KEY`.
    ///
    /// Always uses `text-embedding-3-small` (1536-dim). For a different model
    /// use [`new_with_model`](Self::new_with_model) after reading the key manually.
    pub fn from_env() -> Result<Self> {
        let key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| KernelError::Embedding("OPENAI_API_KEY not set".into()))?;
        Ok(Self::new_small(key))
    }
}

use super::types::text_preview;

impl EmbeddingProvider for OpenAIEmbeddingClient {
    fn dim(&self) -> usize {
        self.dim
    }

    fn name(&self) -> &str {
        &self.model
    }

    fn embed(&self, text: &str) -> Result<EmbeddingResult> {
        let config = ureq::config::Config::builder()
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .build();
        let agent = ureq::Agent::new_with_config(config);

        let body = serde_json::json!({
            "model": self.model,
            "input": text,
        });

        let mut resp = agent
            .post("https://api.openai.com/v1/embeddings")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .send_json(body)
            .map_err(KernelError::embedding)?;

        let payload: EmbeddingResponse = resp
            .body_mut()
            .read_json()
            .map_err(KernelError::embedding)?;

        let vector = payload
            .data
            .into_iter()
            .next()
            .ok_or_else(|| KernelError::Embedding("empty embedding response".into()))?
            .embedding;

        Ok(EmbeddingResult {
            vector,
            text_preview: text_preview(text),
        })
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<EmbeddingResult>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let config = ureq::config::Config::builder()
            .timeout_global(Some(std::time::Duration::from_secs(60)))
            .build();
        let agent = ureq::Agent::new_with_config(config);

        let body = serde_json::json!({
            "model": self.model,
            "input": texts,
        });

        let mut resp = agent
            .post("https://api.openai.com/v1/embeddings")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .send_json(body)
            .map_err(KernelError::embedding)?;

        let payload: EmbeddingResponse = resp
            .body_mut()
            .read_json()
            .map_err(KernelError::embedding)?;

        // The OpenAI API does not guarantee that `data` is returned in input
        // order; sort by `index` before zipping with `texts`.
        let mut data = payload.data;
        data.sort_unstable_by_key(|d| d.index);

        if data.len() != texts.len() {
            return Err(KernelError::Embedding(format!(
                "API returned {} embeddings for {} inputs",
                data.len(),
                texts.len()
            )));
        }

        let results = data
            .into_iter()
            .zip(texts.iter())
            .map(|(item, &text)| EmbeddingResult {
                vector: item.embedding,
                text_preview: text_preview(text),
            })
            .collect();

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_client_has_correct_dim() {
        let client = OpenAIEmbeddingClient::new_small("test-key");
        assert_eq!(client.dim(), 1536);
        assert_eq!(client.name(), "text-embedding-3-small");
    }

    #[test]
    fn large_client_has_correct_dim() {
        let client = OpenAIEmbeddingClient::new_large("test-key");
        assert_eq!(client.dim(), 3072);
        assert_eq!(client.name(), "text-embedding-3-large");
    }

    #[test]
    fn from_env_fails_without_key() {
        // SAFETY: `remove_var` is unsafe in Rust 2024 because env mutation is
        // not thread-safe. Tests run in their own process and this binary does
        // not spawn threads that read OPENAI_API_KEY concurrently.
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
        assert!(OpenAIEmbeddingClient::from_env().is_err());
    }

    #[test]
    fn parse_embedding_response() {
        let raw = r#"{"object":"list","data":[{"object":"embedding","embedding":[0.1,-0.2,0.3],"index":0}],"model":"text-embedding-3-small","usage":{"prompt_tokens":5,"total_tokens":5}}"#;
        let payload: EmbeddingResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(payload.data.len(), 1);
        assert_eq!(payload.data[0].embedding, vec![0.1f32, -0.2, 0.3]);
        assert_eq!(payload.data[0].index, 0);
    }

    #[test]
    fn embed_batch_reorders_by_index() {
        // Simulate an out-of-order API response (index 1 before index 0).
        let raw = r#"{"object":"list","data":[{"object":"embedding","embedding":[0.2],"index":1},{"object":"embedding","embedding":[0.1],"index":0}],"model":"text-embedding-3-small","usage":{"prompt_tokens":2,"total_tokens":2}}"#;
        let mut payload: EmbeddingResponse = serde_json::from_str(raw).unwrap();
        payload.data.sort_unstable_by_key(|d| d.index);
        assert_eq!(payload.data[0].embedding, vec![0.1f32]);
        assert_eq!(payload.data[1].embedding, vec![0.2f32]);
    }

    #[test]
    fn new_with_model_sets_name_and_dim() {
        let client = OpenAIEmbeddingClient::new_with_model("key", "text-embedding-ada-002", 1536);
        assert_eq!(client.dim(), 1536);
        assert_eq!(client.name(), "text-embedding-ada-002");
    }

    #[test]
    fn new_with_model_custom_dim() {
        let client = OpenAIEmbeddingClient::new_with_model("key", "text-embedding-3-small", 512);
        assert_eq!(client.dim(), 512);
        assert_eq!(client.name(), "text-embedding-3-small");
    }

    #[test]
    #[should_panic(expected = "dim must be non-zero")]
    fn new_with_model_zero_dim_panics() {
        OpenAIEmbeddingClient::new_with_model("key", "text-embedding-ada-002", 0);
    }

    #[test]
    fn preview_ascii_truncated() {
        let long = "a".repeat(100);
        let preview = text_preview(&long);
        assert!(preview.ends_with('…'));
        assert_eq!(preview.chars().filter(|&c| c != '…').count(), 64);
    }

    #[test]
    fn preview_short_not_truncated() {
        assert_eq!(text_preview("hello"), "hello");
    }

    #[test]
    fn preview_multibyte_no_panic() {
        // Each Korean char is 3 bytes; byte-slicing at 64 would panic.
        let korean = "안녕하세요".repeat(20);
        let preview = text_preview(&korean);
        assert!(preview.ends_with('…'));
        assert_eq!(preview.chars().filter(|&c| c != '…').count(), 64);
    }
}
