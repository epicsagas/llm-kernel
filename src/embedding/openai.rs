//! OpenAI text-embedding provider (sync, via ureq).

use serde::Deserialize;

use crate::embedding::types::{EmbeddingProvider, EmbeddingResult};

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

/// OpenAI embedding provider.
///
/// Uses `text-embedding-3-small` (1536-dim) by default.
/// Swap model to `text-embedding-3-large` (3072-dim) for higher accuracy.
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

    /// Create from environment variable `OPENAI_API_KEY`.
    pub fn from_env() -> anyhow::Result<Self> {
        let key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| anyhow::anyhow!("OPENAI_API_KEY not set"))?;
        Ok(Self::new_small(key))
    }
}

impl EmbeddingProvider for OpenAIEmbeddingClient {
    fn dim(&self) -> usize {
        self.dim
    }

    fn name(&self) -> &str {
        &self.model
    }

    fn embed(&self, text: &str) -> anyhow::Result<EmbeddingResult> {
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
            .send_json(body)?;

        let payload: EmbeddingResponse = resp.body_mut().read_json()?;

        let vector = payload
            .data
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("empty embedding response"))?
            .embedding;

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
            .send_json(body)?;

        let payload: EmbeddingResponse = resp.body_mut().read_json()?;

        let results = payload
            .data
            .into_iter()
            .zip(texts.iter())
            .map(|(data, &text)| {
                let preview = if text.len() > 64 {
                    format!("{}…", &text[..64])
                } else {
                    text.to_string()
                };
                EmbeddingResult {
                    vector: data.embedding,
                    text_preview: preview,
                }
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
        // Safety: test binary is single-threaded when this runs.
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
        assert!(OpenAIEmbeddingClient::from_env().is_err());
    }

    #[test]
    fn parse_embedding_response() {
        let raw = r#"{"object":"list","data":[{"object":"embedding","embedding":[0.1,-0.2,0.3],"index":0}],"model":"text-embedding-3-small","usage":{"prompt_tokens":5,"total_tokens":5}}"#;
        let payload: EmbeddingResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(payload.data.len(), 1);
        assert_eq!(payload.data[0].embedding, vec![0.1f32, -0.2, 0.3]);
    }

    #[test]
    fn preview_truncated_at_64_chars() {
        let long = "a".repeat(100);
        // Simulate the preview logic used in embed()
        let preview = if long.len() > 64 {
            format!("{}…", &long[..64])
        } else {
            long.clone()
        };
        assert_eq!(preview.len(), 64 + "…".len()); // "…" is 3 bytes in UTF-8
    }
}
