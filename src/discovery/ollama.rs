use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
struct OllamaTag {
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct OllamaResponse {
    models: Vec<OllamaTag>,
}

/// Fetch available model names from an Ollama instance.
pub fn fetch_ollama_models(base_url: &str) -> anyhow::Result<Vec<String>> {
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let config = ureq::config::Config::builder()
        .timeout_global(Some(std::time::Duration::from_secs(2)))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut resp = agent.get(&url).call()?;

    let payload: OllamaResponse = resp.body_mut().read_json()?;
    Ok(payload.models.into_iter().map(|m| m.name).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ollama_response_multiple_models() {
        let raw = r#"{"models":[{"name":"llama3.2:latest"},{"name":"mistral:7b"},{"name":"phi4:latest"}]}"#;
        let resp: OllamaResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(resp.models.len(), 3);
        assert_eq!(resp.models[0].name, "llama3.2:latest");
        assert_eq!(resp.models[2].name, "phi4:latest");
    }

    #[test]
    fn parse_ollama_response_empty() {
        let raw = r#"{"models":[]}"#;
        let resp: OllamaResponse = serde_json::from_str(raw).unwrap();
        assert!(resp.models.is_empty());
    }

    #[test]
    fn parse_ollama_tag_preserves_name() {
        let raw = r#"{"name":"deepseek-r1:8b"}"#;
        let tag: OllamaTag = serde_json::from_str(raw).unwrap();
        assert_eq!(tag.name, "deepseek-r1:8b");
    }

    #[test]
    fn url_construction_trims_trailing_slash() {
        // The function appends /api/tags — verify no double-slash for trailing-slash inputs.
        let base = "http://localhost:11434/";
        let expected = "http://localhost:11434/api/tags";
        let url = format!("{}/api/tags", base.trim_end_matches('/'));
        assert_eq!(url, expected);
    }
}
