use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// models.dev-compatible model descriptor types
// ---------------------------------------------------------------------------

/// Per-million-token pricing for a model.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ModelCost {
    /// Price per million input (prompt) tokens in USD.
    pub input: f64,
    /// Price per million output (completion) tokens in USD.
    pub output: f64,
    /// Price per million cache-read tokens, if the provider supports prompt caching.
    #[serde(default)]
    pub cache_read: Option<f64>,
    /// Price per million cache-write tokens, if the provider supports prompt caching.
    #[serde(default)]
    pub cache_write: Option<f64>,
}

/// Token limits for a model.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ModelLimit {
    /// Maximum context window in tokens (prompt + completion).
    pub context: u64,
    /// Maximum output (completion) tokens per request.
    pub output: u64,
}

/// Input/output modalities a model supports.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ModelModalities {
    /// Accepted input modalities (e.g. `["text", "image"]`).
    pub input: Vec<String>,
    /// Produced output modalities (e.g. `["text"]`).
    pub output: Vec<String>,
}

/// Capability flags for a model.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ModelCapabilities {
    /// Whether the model accepts file/image attachments.
    #[serde(default)]
    pub attachment: bool,
    /// Whether the model supports extended reasoning / chain-of-thought.
    #[serde(default)]
    pub reasoning: bool,
    /// Whether the model accepts a `temperature` parameter.
    #[serde(default)]
    pub temperature: bool,
    /// Whether the model supports tool/function calling.
    #[serde(default)]
    pub tool_call: bool,
    /// Whether the model supports streaming responses (SSE).
    #[serde(default = "default_true")]
    pub streaming: bool,
}

fn default_true() -> bool {
    true
}

/// A model offered by a provider (models.dev-compatible).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ModelDescriptor {
    /// Unique model identifier (e.g. `"gpt-4o"`, `"claude-sonnet-4-6"`).
    pub id: String,
    /// Human-readable model name.
    pub name: String,
    /// Model family grouping (e.g. `"gpt-4"`, `"claude-3"`).
    #[serde(default)]
    pub family: Option<String>,
    /// ISO 8601 date the model was released.
    #[serde(default)]
    pub release_date: Option<String>,
    /// Pricing information per million tokens.
    #[serde(default)]
    pub cost: Option<ModelCost>,
    /// Token limits for context and output.
    #[serde(default)]
    pub limit: Option<ModelLimit>,
    /// Input and output modalities.
    #[serde(default)]
    pub modalities: Option<ModelModalities>,
    /// Capability flags (tool calling, streaming, etc.).
    #[serde(default)]
    pub capabilities: Option<ModelCapabilities>,
    /// Knowledge cutoff date (ISO 8601).
    #[serde(default)]
    pub knowledge: Option<String>,
}

// ---------------------------------------------------------------------------
// Provider service descriptor
// ---------------------------------------------------------------------------

/// Describes an LLM provider service with all metadata needed to connect and use it.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ServiceDescriptor {
    /// Unique provider identifier (e.g. `"openai"`, `"anthropic"`).
    pub id: String,
    /// Human-readable display name.
    #[serde(rename = "display_name")]
    pub display_name: String,
    /// Short description of the provider.
    pub description: String,
    /// Provider category (e.g. `"cloud"`, `"local"`).
    pub category: String,
    /// Provider family used to group related providers.
    pub family: String,
    /// Authentication mode: `"none"`, `"literal"`, or `"secret"`.
    #[serde(rename = "auth_mode")]
    pub auth_mode: String,
    /// Environment variable name that holds the API key (empty if not required).
    #[serde(rename = "key_var", skip_serializing_if = "String::is_empty", default)]
    pub key_var: String,
    /// Literal auth token embedded in the catalog (only set when `auth_mode = "literal"`).
    #[serde(
        rename = "literal_auth_token",
        skip_serializing_if = "String::is_empty",
        default
    )]
    pub literal_auth_token: String,
    /// Base URL for the provider's web interface.
    #[serde(rename = "base_url")]
    pub base_url: String,
    /// Default model ID used when no model override is specified.
    #[serde(rename = "default_model")]
    pub default_model: String,
    /// Named model tiers mapping tier name → model ID (e.g. `"fast"` → `"gpt-4o-mini"`).
    #[serde(rename = "model_tiers", default)]
    pub model_tiers: HashMap<String, String>,
    /// Legacy list of available model choices (claudy-specific).
    #[serde(rename = "model_choices", default)]
    pub model_choices: Vec<ModelChoice>,
    /// URL used to test connectivity to the provider.
    #[serde(rename = "test_url")]
    pub test_url: String,
    /// Setup instructions shown to the user during first-time configuration.
    #[serde(default)]
    pub setup: Vec<String>,
    /// Usage examples shown to the user in the install wizard.
    #[serde(default)]
    pub usage: Vec<String>,

    // models.dev-compatible fields
    /// API base URL override (models.dev-compatible field).
    #[serde(default)]
    pub api_base_url: Option<String>,
    /// npm package name (models.dev-compatible field, for AI coding tools).
    #[serde(default)]
    pub npm_package: Option<String>,
    /// Link to provider documentation.
    #[serde(default)]
    pub doc_url: Option<String>,
    /// Full list of models offered by this provider.
    #[serde(default)]
    pub models: Vec<ModelDescriptor>,
}

/// Legacy model choice (claudy-specific: id + description).
/// Retained for backward compatibility with existing catalog.json entries.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ModelChoice {
    /// Model identifier.
    pub id: String,
    /// Short description of the model.
    pub description: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct IndexPayload {
    providers: Vec<ServiceDescriptor>,
}

// ---------------------------------------------------------------------------
// Provider index
// ---------------------------------------------------------------------------

/// Immutable provider catalog with O(1) lookup by id.
///
/// The catalog is compiled into the binary from `catalog.json` via `include_str!`.
/// Access it through [`ProviderIndex::embedded()`].
#[derive(Debug, Clone)]
pub struct ProviderIndex {
    entries: Vec<ServiceDescriptor>,
    index: HashMap<String, usize>,
}

impl ProviderIndex {
    fn from_payload(payload: IndexPayload) -> Self {
        Self::from_providers(payload.providers)
    }

    /// Build a [`ProviderIndex`] from an explicit list of providers.
    ///
    /// Useful for tests, overlays, or merging discovered providers into the
    /// embedded catalog. Provider order is preserved. The embedded catalog has
    /// no duplicate ids; if duplicates are passed, [`ProviderIndex::get`]
    /// resolves to the last occurrence while [`ProviderIndex::all`] retains
    /// every entry.
    pub fn from_providers(providers: Vec<ServiceDescriptor>) -> Self {
        let index: HashMap<String, usize> = providers
            .iter()
            .enumerate()
            .map(|(i, p)| (p.id.clone(), i))
            .collect();
        Self {
            entries: providers,
            index,
        }
    }

    /// Access the static catalog embedded at compile time.
    pub fn embedded() -> &'static ProviderIndex {
        &EMBEDDED
    }

    /// Return a new catalog where discovered model entries overlay this one.
    ///
    /// For a discovered entry whose `provider_id` matches an existing provider,
    /// its model is merged into that provider (replacing on id collision,
    /// appending otherwise). Entries whose `provider_id` is not in the catalog
    /// are gathered under a synthetic `"discovered"` provider.
    ///
    /// This resolves the catalog↔discovery gap: once merged, discovered models
    /// are visible to [`ProviderIndex::find_model`] and
    /// [`ProviderIndex::estimate_cost`]. The catalog is not mutated; an owned
    /// [`ProviderIndex`] is returned.
    #[cfg(feature = "discovery")]
    pub fn with_discovered(&self, discovered: &[crate::discovery::ModelEntry]) -> ProviderIndex {
        let mut entries: Vec<ServiceDescriptor> = self.entries.clone();
        let mut synthetic: Option<ServiceDescriptor> = None;

        for entry in discovered {
            let model: ModelDescriptor = entry.clone().into();
            match self.index.get(&entry.provider_id).copied() {
                Some(idx) => {
                    let provider = &mut entries[idx];
                    if let Some(pos) = provider.models.iter().position(|m| m.id == model.id) {
                        provider.models[pos] = model;
                    } else {
                        provider.models.push(model);
                    }
                }
                None => {
                    let synth = synthetic.get_or_insert_with(|| ServiceDescriptor {
                        id: "discovered".to_string(),
                        display_name: "Discovered".to_string(),
                        description: "Runtime-discovered models not present in the embedded \
                                      catalog."
                            .to_string(),
                        category: "discovered".to_string(),
                        family: "discovered".to_string(),
                        auth_mode: "secret".to_string(),
                        key_var: String::new(),
                        literal_auth_token: String::new(),
                        base_url: String::new(),
                        default_model: String::new(),
                        model_tiers: HashMap::new(),
                        model_choices: vec![],
                        test_url: String::new(),
                        setup: vec![],
                        usage: vec![],
                        api_base_url: None,
                        npm_package: None,
                        doc_url: None,
                        models: vec![],
                    });
                    synth.models.push(model);
                }
            }
        }

        if let Some(synth) = synthetic {
            entries.push(synth);
        }

        ProviderIndex::from_providers(entries)
    }

    /// Return all providers in catalog order.
    pub fn all(&self) -> &[ServiceDescriptor] {
        &self.entries
    }

    /// Return all provider IDs.
    pub fn ids(&self) -> Vec<String> {
        self.entries.iter().map(|p| p.id.clone()).collect()
    }

    /// Look up a provider by ID. O(1).
    pub fn get(&self, id: &str) -> Option<&ServiceDescriptor> {
        self.index.get(id).map(|&i| &self.entries[i])
    }

    /// Unique categories in catalog order.
    pub fn categories(&self) -> Vec<String> {
        self.entries
            .iter()
            .scan(HashSet::new(), |seen, p| {
                Some(if seen.insert(p.category.clone()) {
                    Some(p.category.clone())
                } else {
                    None
                })
            })
            .flatten()
            .collect()
    }

    /// Filter providers by category.
    pub fn providers_by_category(&self, category: &str) -> Vec<&ServiceDescriptor> {
        self.entries
            .iter()
            .filter(|p| p.category == category)
            .collect()
    }

    /// Collect all secret key variable names from providers that require one.
    pub fn builtin_secret_keys(&self) -> HashSet<String> {
        self.entries
            .iter()
            .filter(|p| !p.key_var.is_empty())
            .map(|p| p.key_var.clone())
            .collect()
    }

    /// Get models for a specific provider.
    pub fn models_for(&self, provider_id: &str) -> &[ModelDescriptor] {
        self.get(provider_id)
            .map(|p| p.models.as_slice())
            .unwrap_or(&[])
    }

    /// Find a model by ID across all providers.
    /// Returns the first match (provider, model).
    pub fn find_model(&self, model_id: &str) -> Option<(&ServiceDescriptor, &ModelDescriptor)> {
        self.entries
            .iter()
            .find_map(|p| p.models.iter().find(|m| m.id == model_id).map(|m| (p, m)))
    }

    /// Estimate the USD cost of an LLM call given token counts.
    ///
    /// Looks up `model_id` across all providers and computes:
    /// `(input_price * prompt_tokens + output_price * completion_tokens) / 1_000_000`
    ///
    /// Returns `None` if the model is not found or has no pricing data.
    pub fn estimate_cost(
        &self,
        model_id: &str,
        prompt_tokens: u32,
        completion_tokens: u32,
    ) -> Option<f64> {
        let (_, model) = self.find_model(model_id)?;
        let cost = model.cost.as_ref()?;
        Some(
            cost.input * prompt_tokens as f64 / 1_000_000.0
                + cost.output * completion_tokens as f64 / 1_000_000.0,
        )
    }
}

/// Static catalog compiled into the binary from `catalog.json`.
static EMBEDDED: LazyLock<ProviderIndex> = LazyLock::new(|| {
    let raw = include_str!("catalog.json");
    let payload: IndexPayload = serde_json::from_str(raw).expect("catalog.json is valid");
    ProviderIndex::from_payload(payload)
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedded_loads() {
        let catalog = ProviderIndex::embedded();
        assert!(!catalog.all().is_empty());
    }

    #[test]
    fn test_get_known_provider() {
        let catalog = ProviderIndex::embedded();
        // catalog.json contains "zai" (first provider with key_var)
        let p = catalog.get("zai").expect("zai should exist");
        assert_eq!(p.id, "zai");
        assert!(!p.base_url.is_empty());
        assert!(!p.default_model.is_empty());
    }

    #[test]
    fn test_get_unknown_returns_none() {
        let catalog = ProviderIndex::embedded();
        assert!(catalog.get("nonexistent_provider_xyz").is_none());
    }

    #[test]
    fn test_categories_no_duplicates() {
        let catalog = ProviderIndex::embedded();
        let cats = catalog.categories();
        let mut seen = HashSet::new();
        for c in &cats {
            assert!(seen.insert(c.clone()), "duplicate category: {}", c);
        }
    }

    #[test]
    fn test_builtin_secret_keys() {
        let catalog = ProviderIndex::embedded();
        let keys = catalog.builtin_secret_keys();
        assert!(!keys.is_empty(), "should contain at least one secret key");
        assert!(
            keys.contains("ZAI_API_KEY"),
            "should contain ZAI_API_KEY, got: {:?}",
            keys
        );
    }

    #[test]
    fn test_providers_by_category() {
        let catalog = ProviderIndex::embedded();
        let cats = catalog.categories();
        if let Some(cat) = cats.first() {
            let providers = catalog.providers_by_category(cat);
            assert!(!providers.is_empty());
            for p in &providers {
                assert_eq!(p.category, *cat);
            }
        }
    }

    #[test]
    fn test_models_for_provider() {
        let catalog = ProviderIndex::embedded();
        let models = catalog.models_for("zai");
        assert!(!models.is_empty(), "zai should have models");
        // First model should have an id
        assert!(!models[0].id.is_empty());
    }

    #[test]
    fn test_models_for_unknown_provider() {
        let catalog = ProviderIndex::embedded();
        let models = catalog.models_for("nonexistent_provider_xyz");
        assert!(models.is_empty());
    }

    #[test]
    fn test_find_model() {
        let catalog = ProviderIndex::embedded();
        let (provider, model) = catalog.find_model("glm-5").expect("glm-5 should be found");
        assert_eq!(model.id, "glm-5");
        assert!(
            provider.id == "zai" || provider.id == "zai-cn",
            "glm-5 should belong to a Z.AI provider, got: {}",
            provider.id
        );
    }

    #[test]
    fn test_find_model_unknown() {
        let catalog = ProviderIndex::embedded();
        assert!(catalog.find_model("nonexistent-model-xyz").is_none());
    }

    #[test]
    fn test_model_has_pricing() {
        let catalog = ProviderIndex::embedded();
        let (_, model) = catalog.find_model("glm-5").expect("glm-5 should exist");
        let cost = model.cost.as_ref().expect("glm-5 should have cost");
        assert!(cost.input > 0.0, "input cost should be positive");
        assert!(cost.output > 0.0, "output cost should be positive");
    }

    #[test]
    fn test_from_providers_round_trip() {
        let original = ProviderIndex::embedded();
        let rebuilt = ProviderIndex::from_providers(original.entries.clone());
        assert_eq!(rebuilt.ids().len(), original.ids().len());
        // O(1) lookup survives reconstruction.
        assert!(rebuilt.get("zai").is_some());
        assert!(rebuilt.find_model("glm-5").is_some());
    }

    #[test]
    fn test_from_providers_preserves_order() {
        let providers = vec![
            ServiceDescriptor {
                id: "p1".to_string(),
                display_name: "P1".to_string(),
                description: String::new(),
                category: "c".to_string(),
                family: "f".to_string(),
                auth_mode: "none".to_string(),
                key_var: String::new(),
                literal_auth_token: String::new(),
                base_url: String::new(),
                default_model: String::new(),
                model_tiers: HashMap::new(),
                model_choices: vec![],
                test_url: String::new(),
                setup: vec![],
                usage: vec![],
                api_base_url: None,
                npm_package: None,
                doc_url: None,
                models: vec![],
            },
            ServiceDescriptor {
                id: "p2".to_string(),
                display_name: "P2".to_string(),
                ..ProviderIndex::embedded().get("zai").unwrap().clone()
            },
        ];
        let idx = ProviderIndex::from_providers(providers);
        assert_eq!(idx.ids(), vec!["p1".to_string(), "p2".to_string()]);
        assert_eq!(idx.get("p1").unwrap().display_name, "P1");
        assert_eq!(idx.get("p2").unwrap().display_name, "P2");
    }

    #[cfg(feature = "discovery")]
    #[test]
    fn test_with_discovered_merges_and_enables_cost() {
        use crate::discovery::{ModelEntry, ModelLimits};
        use crate::provider::ModelCost;

        let catalog = ProviderIndex::embedded();

        // A model not in the static catalog, attached to an existing provider.
        let fresh = ModelEntry {
            id: "future-model-xyz".to_string(),
            name: "Future Model".to_string(),
            provider_id: "zai".to_string(),
            cost: Some(ModelCost {
                input: 2.0,
                output: 8.0,
                cache_read: None,
                cache_write: None,
            }),
            limits: Some(ModelLimits {
                context: Some(100_000),
                input: None,
                output: Some(4_000),
            }),
            ..Default::default()
        };
        // A model under a provider absent from the catalog → synthetic bucket.
        let orphan = ModelEntry {
            id: "mystery/m1".to_string(),
            name: "Mystery M1".to_string(),
            provider_id: "mystery".to_string(),
            cost: Some(ModelCost {
                input: 1.0,
                output: 1.0,
                cache_read: None,
                cache_write: None,
            }),
            ..Default::default()
        };

        let merged = catalog.with_discovered(&[fresh, orphan]);

        // fresh merged into existing zai → estimate_cost now works.
        assert!(merged.find_model("future-model-xyz").is_some());
        assert_eq!(
            merged.estimate_cost("future-model-xyz", 1_000_000, 1_000_000),
            Some(10.0)
        );

        // orphan landed under a synthetic "discovered" provider.
        assert!(merged.get("discovered").is_some());
        assert!(merged.find_model("mystery/m1").is_some());
        assert_eq!(merged.estimate_cost("mystery/m1", 1_000_000, 0), Some(1.0));

        // The embedded static catalog is not mutated.
        assert!(catalog.find_model("future-model-xyz").is_none());
    }
}
