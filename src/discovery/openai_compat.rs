use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
struct OpenAIModelEntry {
    id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAIResponse {
    data: Vec<OpenAIModelEntry>,
}

/// Fetch available model IDs from an OpenAI-compatible endpoint.
pub fn fetch_openai_compatible_models(base_url: &str) -> anyhow::Result<Vec<String>> {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let config = ureq::config::Config::builder()
        .timeout_global(Some(std::time::Duration::from_secs(2)))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut resp = agent.get(&url).call()?;

    let payload: OpenAIResponse = resp.body_mut().read_json()?;
    Ok(payload.data.into_iter().map(|m| m.id).collect())
}
