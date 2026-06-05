use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const MODELS_DEV_URL: &str = "https://models.dev/api.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelLimits {
    pub context: Option<u64>,
    pub input: Option<u64>,
    pub output: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    pub limits: Option<ModelLimits>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevPayload {
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
