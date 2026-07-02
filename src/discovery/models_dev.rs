//! Client for the [models.dev](https://github.com/anomalyco/models.dev) model catalog API.
//!
//! Fetches the live catalog from `https://models.dev/api.json`, whose top-level
//! shape is a provider-keyed map:
//!
//! ```json
//! { "<provider_id>": { "id": ..., "env": [...], "api": ...,
//!                      "models": { "<model_id>": { "cost": {...}, "limit": {...}, ... } } } }
//! ```
//!
//! Results are flattened into [`ModelEntry`] records carrying the full
//! models.dev metadata (pricing, limits, modalities, capabilities) so they can
//! feed [`crate::provider::ProviderIndex::with_discovered`].

use crate::error::{KernelError, Result};
use crate::provider::{ModelCapabilities, ModelCost, ModelDescriptor, ModelLimit, ModelModalities};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

const MODELS_DEV_URL: &str = "https://models.dev/api.json";

// ---------------------------------------------------------------------------
// Discovery result types
// ---------------------------------------------------------------------------

/// Token limits reported by a discovery source for a single model.
///
/// All fields are optional so heterogeneous sources (models.dev, Ollama,
/// OpenAI-compatible) can report only what they know.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelLimits {
    /// Maximum context window in tokens.
    pub context: Option<u64>,
    /// Maximum input tokens per request.
    pub input: Option<u64>,
    /// Maximum output tokens per request.
    pub output: Option<u64>,
}

/// A single model entry discovered from a remote catalog.
///
/// `id`, `name`, and `provider_id` are always present; the remaining fields are
/// optional so sparse sources can populate only what they know. The richer
/// fields mirror [`ModelDescriptor`] so a discovered entry can feed
/// [`crate::provider::ProviderIndex::estimate_cost`] via
/// [`crate::provider::ProviderIndex::with_discovered`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelEntry {
    /// Unique model identifier (e.g. `"anthropic/claude-3-5-sonnet"`).
    pub id: String,
    /// Human-readable model name.
    pub name: String,
    /// Provider that hosts this model (e.g. `"anthropic"`).
    pub provider_id: String,
    /// Model family grouping.
    #[serde(default)]
    pub family: Option<String>,
    /// ISO 8601 release date.
    #[serde(default)]
    pub release_date: Option<String>,
    /// Knowledge cutoff date (ISO 8601).
    #[serde(default)]
    pub knowledge: Option<String>,
    /// Token limits for context and output.
    #[serde(default)]
    pub limits: Option<ModelLimits>,
    /// Per-million-token pricing.
    #[serde(default)]
    pub cost: Option<ModelCost>,
    /// Input and output modalities.
    #[serde(default)]
    pub modalities: Option<ModelModalities>,
    /// Capability flags (tool calling, streaming, etc.).
    #[serde(default)]
    pub capabilities: Option<ModelCapabilities>,
}

impl From<ModelEntry> for ModelDescriptor {
    /// Project a discovery entry onto a catalog model descriptor.
    ///
    /// `limits` is carried over only when both `context` and `output` are known
    /// (the catalog representation requires both). Pricing, modalities, and
    /// capabilities pass through unchanged.
    fn from(entry: ModelEntry) -> Self {
        let limit = entry.limits.and_then(|l| match (l.context, l.output) {
            (Some(context), Some(output)) => Some(ModelLimit { context, output }),
            _ => None,
        });
        ModelDescriptor {
            id: entry.id,
            name: entry.name,
            family: entry.family,
            release_date: entry.release_date,
            cost: entry.cost,
            limit,
            modalities: entry.modalities,
            capabilities: entry.capabilities,
            knowledge: entry.knowledge,
        }
    }
}

// ---------------------------------------------------------------------------
// models.dev raw schema (private — deserialized, then transformed)
// ---------------------------------------------------------------------------

/// A raw models.dev model object. Unknown fields (`reasoning_options`,
/// `interleaved`, `structured_output`, `open_weights`, `last_updated`,
/// `cost.tiers`, `cost.context_over_200k`) are silently ignored by serde.
#[derive(Debug, Clone, Deserialize)]
struct RawModel {
    id: String,
    name: String,
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    knowledge: Option<String>,
    // Flat capability bools — models.dev puts these at the model top level, not
    // nested under a `capabilities` object.
    #[serde(default)]
    attachment: Option<bool>,
    #[serde(default)]
    reasoning: Option<bool>,
    #[serde(default)]
    temperature: Option<bool>,
    #[serde(default)]
    tool_call: Option<bool>,
    #[serde(default)]
    limit: Option<RawLimit>,
    #[serde(default)]
    cost: Option<ModelCost>,
    #[serde(default)]
    modalities: Option<ModelModalities>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawLimit {
    #[serde(default)]
    context: Option<u64>,
    #[serde(default)]
    output: Option<u64>,
}

/// A raw models.dev provider object.
#[derive(Debug, Clone, Deserialize)]
struct RawProvider {
    #[serde(default)]
    #[allow(dead_code)]
    env: Vec<String>,
    #[serde(default)]
    npm: Option<String>,
    #[serde(default)]
    api: Option<String>,
    #[serde(default)]
    doc: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    name: Option<String>,
    #[serde(default)]
    models: HashMap<String, RawModel>,
}

/// Top-level response payload from the models.dev API: a provider-keyed map.
///
/// **Breaking change:** this was previously `{ models: Vec<ModelEntry> }`, a
/// shape that never matched the live API. The real response is a map of
/// provider id → provider object; this type now mirrors it. The on-disk cache
/// written by [`fetch_and_cache`] is byte-identical to the upstream payload.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelsDevPayload(HashMap<String, RawProvider>);

impl ModelsDevPayload {
    /// Flatten every provider into discovery [`ModelEntry`] records, in
    /// deterministic (sorted provider id, sorted model id) order.
    #[must_use]
    pub fn entries(&self) -> Vec<ModelEntry> {
        let mut provider_ids: Vec<&String> = self.0.keys().collect();
        provider_ids.sort();
        provider_ids
            .into_iter()
            .flat_map(|pid| provider_to_entries(pid, &self.0[pid]))
            .collect()
    }

    /// Return the catalog model descriptors for one upstream provider key, in
    /// sorted model-id order. Returns an empty vec if the key is absent.
    #[must_use]
    pub fn provider_models(&self, provider_key: &str) -> Vec<ModelDescriptor> {
        self.0
            .get(provider_key)
            .map(|p| provider_to_entries(provider_key, p))
            .unwrap_or_default()
            .into_iter()
            .map(ModelDescriptor::from)
            .collect()
    }

    /// Return the upstream provider's `api` base URL, if advertised.
    #[must_use]
    pub fn provider_api_base(&self, provider_key: &str) -> Option<&str> {
        self.0.get(provider_key).and_then(|p| p.api.as_deref())
    }

    /// Return the upstream provider's npm package name, if any.
    #[must_use]
    pub fn provider_npm(&self, provider_key: &str) -> Option<&str> {
        self.0.get(provider_key).and_then(|p| p.npm.as_deref())
    }

    /// Return the upstream provider's documentation URL, if any.
    #[must_use]
    pub fn provider_doc(&self, provider_key: &str) -> Option<&str> {
        self.0.get(provider_key).and_then(|p| p.doc.as_deref())
    }
}

/// Flatten one models.dev provider into discovery entries (sorted model order).
fn provider_to_entries(provider_id: &str, provider: &RawProvider) -> Vec<ModelEntry> {
    let mut model_ids: Vec<&String> = provider.models.keys().collect();
    model_ids.sort();
    model_ids
        .into_iter()
        .filter_map(|mid| {
            provider
                .models
                .get(mid)
                .map(|m| raw_to_entry(provider_id, m))
        })
        .collect()
}

/// Transform one raw models.dev model into a discovery entry.
fn raw_to_entry(provider_id: &str, m: &RawModel) -> ModelEntry {
    let capabilities = Some(ModelCapabilities {
        attachment: m.attachment.unwrap_or(false),
        reasoning: m.reasoning.unwrap_or(false),
        temperature: m.temperature.unwrap_or(false),
        tool_call: m.tool_call.unwrap_or(false),
        // models.dev has no per-model streaming flag; default true (matches
        // the catalog convention and `ModelCapabilities`'s serde default).
        streaming: true,
    });
    let limits = m.limit.as_ref().map(|l| ModelLimits {
        context: l.context,
        // models.dev `limit` has no `input` field.
        input: None,
        output: l.output,
    });
    ModelEntry {
        id: m.id.clone(),
        name: m.name.clone(),
        provider_id: provider_id.to_string(),
        family: m.family.clone(),
        release_date: m.release_date.clone(),
        knowledge: m.knowledge.clone(),
        limits,
        cost: m.cost.clone(),
        modalities: m.modalities.clone(),
        capabilities,
    }
}

// ---------------------------------------------------------------------------
// Fetch + cache
// ---------------------------------------------------------------------------

/// Fetch the raw response body from `url` with a bounded timeout.
fn http_get(url: &str) -> Result<String> {
    let config = ureq::config::Config::builder()
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut resp = agent.get(url).call().map_err(KernelError::discovery)?;
    resp.body_mut()
        .read_to_string()
        .map_err(KernelError::discovery)
}

/// Fetch and parse the models.dev catalog from `url` (no disk cache).
///
/// **Trust boundary:** the URL is used verbatim with no host allowlist; pass
/// only admin-configured values.
pub fn fetch_from(url: &str) -> Result<ModelsDevPayload> {
    let body = http_get(url)?;
    serde_json::from_str(&body).map_err(Into::into)
}

/// Fetch and parse the catalog from the default models.dev endpoint (no cache).
pub fn fetch() -> Result<ModelsDevPayload> {
    fetch_from(MODELS_DEV_URL)
}

/// Fetch models from the models.dev API and cache the raw payload to disk.
///
/// The cache file is written byte-identical to the upstream response, so it
/// diffs cleanly against `https://models.dev/api.json` and round-trips through
/// [`load_cache`].
pub fn fetch_and_cache(cache_path: &str) -> Result<ModelsDevPayload> {
    let body = http_get(MODELS_DEV_URL)?;

    if let Some(parent) = Path::new(cache_path).parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(cache_path, &body)?;

    serde_json::from_str(&body).map_err(Into::into)
}

/// Load previously cached models.dev data.
pub fn load_cache(cache_path: &str) -> Result<Option<ModelsDevPayload>> {
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

    fn sample_payload() -> &'static str {
        r#"{
            "anthropic": {
                "id": "anthropic",
                "env": ["ANTHROPIC_API_KEY"],
                "npm": "@ai-sdk/anthropic",
                "name": "Anthropic",
                "doc": "https://docs.anthropic.com",
                "models": {
                    "claude-opus-4-5": {
                        "id": "claude-opus-4-5",
                        "name": "Claude Opus 4.5",
                        "family": "claude-opus",
                        "attachment": true,
                        "reasoning": true,
                        "reasoning_options": [{"type": "effort"}],
                        "tool_call": true,
                        "temperature": true,
                        "interleaved": {"field": "reasoning_content"},
                        "knowledge": "2025-03-31",
                        "release_date": "2025-11-24",
                        "last_updated": "2025-11-24",
                        "modalities": {"input": ["text", "image", "pdf"], "output": ["text"]},
                        "open_weights": false,
                        "limit": {"context": 200000, "output": 64000},
                        "cost": {"input": 5, "output": 25, "cache_read": 0.5, "cache_write": 6.25}
                    }
                }
            },
            "openai": {
                "id": "openai",
                "env": ["OPENAI_API_KEY"],
                "models": {
                    "gpt-4o-mini": {
                        "id": "gpt-4o-mini",
                        "name": "GPT-4o mini",
                        "tool_call": true,
                        "temperature": true,
                        "limit": {"context": 128000, "output": 16384},
                        "cost": {"input": 0.15, "output": 0.6, "tiers": [{"min_tokens": 0}]}
                    }
                }
            }
        }"#
    }

    #[test]
    fn test_parse_real_schema() {
        let payload: ModelsDevPayload = serde_json::from_str(sample_payload()).unwrap();
        let entries = payload.entries();
        assert_eq!(entries.len(), 2);
        // Sorted: anthropic/claude-opus-4-5 before openai/gpt-4o-mini.
        assert_eq!(entries[0].id, "claude-opus-4-5");
        assert_eq!(entries[0].provider_id, "anthropic");
        assert_eq!(entries[1].id, "gpt-4o-mini");
        assert_eq!(entries[1].provider_id, "openai");
    }

    #[test]
    fn test_unknown_fields_are_ignored() {
        // reasoning_options, interleaved, last_updated, open_weights,
        // cost.tiers must be tolerated (serde skips unknown fields).
        let payload: ModelsDevPayload = serde_json::from_str(sample_payload()).unwrap();
        let opus = payload
            .provider_models("anthropic")
            .into_iter()
            .find(|m| m.id == "claude-opus-4-5")
            .unwrap();
        assert!(opus.capabilities.as_ref().unwrap().attachment);
        assert!(opus.capabilities.as_ref().unwrap().reasoning);
        assert!(opus.capabilities.as_ref().unwrap().streaming); // defaulted
        assert_eq!(opus.cost.as_ref().unwrap().cache_read, Some(0.5));
    }

    #[test]
    fn test_partial_cost_and_optional_cache() {
        let payload: ModelsDevPayload = serde_json::from_str(sample_payload()).unwrap();
        let mini = payload
            .provider_models("openai")
            .into_iter()
            .find(|m| m.id == "gpt-4o-mini")
            .unwrap();
        let cost = mini.cost.as_ref().unwrap();
        assert_eq!(cost.input, 0.15);
        assert_eq!(cost.output, 0.6);
        // No cache fields upstream → None.
        assert_eq!(cost.cache_read, None);
        assert_eq!(cost.cache_write, None);
    }

    #[test]
    fn test_limit_has_no_input_after_transform() {
        let payload: ModelsDevPayload = serde_json::from_str(sample_payload()).unwrap();
        let entry = payload
            .entries()
            .into_iter()
            .find(|e| e.id == "claude-opus-4-5")
            .unwrap();
        let limits = entry.limits.unwrap();
        assert_eq!(limits.context, Some(200_000));
        assert_eq!(limits.output, Some(64_000));
        assert_eq!(limits.input, None); // models.dev limit has no input
    }

    #[test]
    fn test_from_entry_to_descriptor_requires_both_limits() {
        // Both context + output present → descriptor.limit is Some.
        let full = ModelEntry {
            id: "m".to_string(),
            name: "M".to_string(),
            provider_id: "p".to_string(),
            family: None,
            release_date: None,
            knowledge: None,
            limits: Some(ModelLimits {
                context: Some(1000),
                input: None,
                output: Some(500),
            }),
            cost: None,
            modalities: None,
            capabilities: None,
        };
        let d: ModelDescriptor = full.into();
        assert_eq!(d.limit.as_ref().unwrap().context, 1000);
        assert_eq!(d.limit.as_ref().unwrap().output, 500);

        // Missing output → descriptor.limit is None.
        let partial = ModelEntry {
            limits: Some(ModelLimits {
                context: Some(1000),
                input: None,
                output: None,
            }),
            ..ModelEntry {
                id: "m".to_string(),
                name: "M".to_string(),
                provider_id: "p".to_string(),
                family: None,
                release_date: None,
                knowledge: None,
                limits: None,
                cost: None,
                modalities: None,
                capabilities: None,
            }
        };
        let d2: ModelDescriptor = partial.into();
        assert!(d2.limit.is_none());
    }

    #[test]
    fn test_provider_models_sorted_and_absent_key() {
        let payload: ModelsDevPayload = serde_json::from_str(sample_payload()).unwrap();
        let anthropic = payload.provider_models("anthropic");
        assert_eq!(anthropic.len(), 1);
        assert!(payload.provider_models("nonexistent").is_empty());
    }

    #[test]
    fn test_provider_api_base() {
        let payload: ModelsDevPayload = serde_json::from_str(sample_payload()).unwrap();
        // openai has no `api` in the fixture; anthropic fixture omits it too.
        assert_eq!(payload.provider_api_base("anthropic"), None);
    }
}
