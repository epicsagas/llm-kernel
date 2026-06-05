use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// A model choice offered by a provider.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelDescriptor {
    pub id: String,
    pub description: String,
}

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
    pub model_choices: Vec<ModelDescriptor>,
    #[serde(rename = "test_url")]
    pub test_url: String,
    #[serde(default)]
    pub setup: Vec<String>,
    #[serde(default)]
    pub usage: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct IndexPayload {
    providers: Vec<ServiceDescriptor>,
}

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
}
