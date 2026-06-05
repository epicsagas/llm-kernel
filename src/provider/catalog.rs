use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// models.dev-compatible model descriptor types
// ---------------------------------------------------------------------------

/// Per-million-token pricing for a model.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelCost {
    pub input: f64,
    pub output: f64,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
}

/// Token limits for a model.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelLimit {
    pub context: u64,
    pub output: u64,
}

/// Input/output modalities a model supports.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelModalities {
    pub input: Vec<String>,
    pub output: Vec<String>,
}

/// Capability flags for a model.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelCapabilities {
    #[serde(default)]
    pub attachment: bool,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub temperature: bool,
    #[serde(default)]
    pub tool_call: bool,
    #[serde(default = "default_true")]
    pub streaming: bool,
}

fn default_true() -> bool {
    true
}

/// A model offered by a provider (models.dev-compatible).
#[derive(Debug, Clone, Deserialize)]
pub struct ModelDescriptor {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub cost: Option<ModelCost>,
    #[serde(default)]
    pub limit: Option<ModelLimit>,
    #[serde(default)]
    pub modalities: Option<ModelModalities>,
    #[serde(default)]
    pub capabilities: Option<ModelCapabilities>,
    #[serde(default)]
    pub knowledge: Option<String>,
}

// ---------------------------------------------------------------------------
// Provider service descriptor
// ---------------------------------------------------------------------------

/// Describes an LLM provider service with all metadata needed to connect and use it.
#[derive(Debug, Clone, Deserialize)]
pub struct ServiceDescriptor {
    pub id: String,
    #[serde(rename = "display_name")]
    pub display_name: String,
    pub description: String,
    pub category: String,
    pub family: String,
    #[serde(rename = "auth_mode")]
    pub auth_mode: String,
    #[serde(rename = "key_var", skip_serializing_if = "String::is_empty", default)]
    pub key_var: String,
    #[serde(
        rename = "literal_auth_token",
        skip_serializing_if = "String::is_empty",
        default
    )]
    pub literal_auth_token: String,
    #[serde(rename = "base_url")]
    pub base_url: String,
    #[serde(rename = "default_model")]
    pub default_model: String,
    #[serde(rename = "model_tiers", default)]
    pub model_tiers: HashMap<String, String>,
    #[serde(rename = "model_choices", default)]
    pub model_choices: Vec<ModelChoice>,
    #[serde(rename = "test_url")]
    pub test_url: String,
    #[serde(default)]
    pub setup: Vec<String>,
    #[serde(default)]
    pub usage: Vec<String>,

    // models.dev-compatible fields
    #[serde(default)]
    pub api_base_url: Option<String>,
    #[serde(default)]
    pub npm_package: Option<String>,
    #[serde(default)]
    pub doc_url: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelDescriptor>,
}

/// Legacy model choice (claudy-specific: id + description).
/// Retained for backward compatibility with existing catalog.json entries.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelChoice {
    pub id: String,
    pub description: String,
}

#[derive(Debug, Deserialize)]
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
        let index: HashMap<String, usize> = payload
            .providers
            .iter()
            .enumerate()
            .map(|(i, p)| (p.id.clone(), i))
            .collect();
        Self {
            entries: payload.providers,
            index,
        }
    }

    /// Access the static catalog embedded at compile time.
    pub fn embedded() -> &'static ProviderIndex {
        &EMBEDDED
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
}
