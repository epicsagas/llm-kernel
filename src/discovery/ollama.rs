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
