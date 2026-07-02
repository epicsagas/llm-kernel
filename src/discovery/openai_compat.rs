use serde::Deserialize;

use crate::error::{KernelError, Result};

#[derive(Debug, Clone, Deserialize)]
struct OpenAIModelEntry {
    id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAIResponse {
    data: Vec<OpenAIModelEntry>,
}

/// Fetch available model IDs from an OpenAI-compatible endpoint.
pub fn fetch_openai_compatible_models(base_url: &str) -> Result<Vec<String>> {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let config = ureq::config::Config::builder()
        .timeout_global(Some(std::time::Duration::from_secs(2)))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut resp = agent.get(&url).call().map_err(KernelError::discovery)?;

    let payload: OpenAIResponse = resp
        .body_mut()
        .read_json()
        .map_err(KernelError::discovery)?;
    Ok(payload.data.into_iter().map(|m| m.id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_openai_response_multiple_models() {
        let raw = r#"{"object":"list","data":[{"id":"gpt-4o","object":"model"},{"id":"gpt-4o-mini","object":"model"}]}"#;
        let resp: OpenAIResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(resp.data.len(), 2);
        assert_eq!(resp.data[0].id, "gpt-4o");
        assert_eq!(resp.data[1].id, "gpt-4o-mini");
    }

    #[test]
    fn parse_openai_response_empty() {
        let raw = r#"{"data":[]}"#;
        let resp: OpenAIResponse = serde_json::from_str(raw).unwrap();
        assert!(resp.data.is_empty());
    }

    #[test]
    fn parse_model_entry_only_id_required() {
        let raw = r#"{"id":"mistral-7b-instruct"}"#;
        let entry: OpenAIModelEntry = serde_json::from_str(raw).unwrap();
        assert_eq!(entry.id, "mistral-7b-instruct");
    }

    #[test]
    fn url_construction_no_double_slash() {
        let base = "http://localhost:1234/";
        let expected = "http://localhost:1234/v1/models";
        let url = format!("{}/v1/models", base.trim_end_matches('/'));
        assert_eq!(url, expected);
    }

    #[test]
    fn parse_lmstudio_style_response() {
        let raw = r#"{"data":[{"id":"lmstudio-community/gemma-3-4b-it-GGUF"},{"id":"bartowski/Phi-4-mini-instruct-GGUF"}]}"#;
        let resp: OpenAIResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(resp.data.len(), 2);
        assert!(resp.data[0].id.contains('/'));
    }
}
