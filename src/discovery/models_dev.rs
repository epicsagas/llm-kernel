//! Client for the [models.dev](https://github.com/anomalyco/models.dev) model catalog API.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const MODELS_DEV_URL: &str = "https://models.dev/api.json";

/// Token limits reported by models.dev for a single model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelLimits {
    /// Maximum context window in tokens.
    pub context: Option<u64>,
    /// Maximum input tokens per request.
    pub input: Option<u64>,
    /// Maximum output tokens per request.
    pub output: Option<u64>,
}

/// A single model entry from the models.dev catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    /// Unique model identifier (e.g. `"anthropic/claude-3-5-sonnet"`).
    pub id: String,
    /// Human-readable model name.
    pub name: String,
    /// Provider that hosts this model (e.g. `"anthropic"`).
    pub provider_id: String,
    /// Token limits for this model.
    pub limits: Option<ModelLimits>,
}

/// Top-level response payload from the models.dev API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevPayload {
    /// All models returned by the API.
    pub models: Vec<ModelEntry>,
}

/// Fetch models from models.dev API and optionally cache to disk.
pub fn fetch_and_cache(cache_path: &str) -> anyhow::Result<ModelsDevPayload> {
    let config = ureq::config::Config::builder()
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut resp = agent.get(MODELS_DEV_URL).call()?;

    let payload: ModelsDevPayload = resp.body_mut().read_json()?;

    if let Some(parent) = Path::new(cache_path).parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&payload)?;
    fs::write(cache_path, json)?;
    Ok(payload)
}

/// Load previously cached models.dev data.
pub fn load_cache(cache_path: &str) -> anyhow::Result<Option<ModelsDevPayload>> {
    if !Path::new(cache_path).exists() {
        return Ok(None);
    }
    let data = fs::read_to_string(cache_path)?;
    let payload: ModelsDevPayload = serde_json::from_str(&data)?;
    Ok(Some(payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mock_payload() {
        let raw = r#"{
            "models": [
                {
                    "id": "anthropic/claude-3-5-sonnet",
                    "name": "Claude 3.5 Sonnet",
                    "provider_id": "anthropic",
                    "limits": {
                        "context": 200000,
                        "input": 200000,
                        "output": 8192
                    }
                }
            ]
        }"#;
        let payload: ModelsDevPayload = serde_json::from_str(raw).unwrap();
        assert_eq!(payload.models.len(), 1);
        assert_eq!(payload.models[0].id, "anthropic/claude-3-5-sonnet");
    }
}
