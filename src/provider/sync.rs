//! Catalog sync engine — merge a live models.dev payload into the embedded
//! catalog.
//!
//! The catalog is split into two concerns:
//! - **Connection metadata** (auth, base URL, install-wizard text, model
//!   tiers): llm-kernel-specific, always kept from the catalog.
//! - **Model data** (pricing, limits, modalities, capabilities): the source of
//!   truth is models.dev. `merge_catalog` refreshes it for every provider that
//!   maps to models.dev ([`crate::provider::mapping`]); providers with no
//!   upstream counterpart are left untouched.
//!
//! `merge_catalog` is pure (no I/O) so it is unit-testable without network.

use crate::discovery::ModelsDevPayload;
use crate::provider::mapping::{self, Mapping};
use crate::provider::{ModelDescriptor, ServiceDescriptor};
use serde::{Deserialize, Serialize};

/// Fetch the live models.dev catalog, optionally from a custom URL.
///
/// **Trust boundary:** `api_url` is forwarded verbatim to [`crate::discovery::fetch_from`]
/// — pass only admin-configured values.
pub fn fetch_models_dev(api_url: Option<&str>) -> anyhow::Result<ModelsDevPayload> {
    match api_url {
        Some(url) => crate::discovery::fetch_from(url),
        None => crate::discovery::fetch(),
    }
}

/// Envelope matching the on-disk `catalog.json` shape.
#[derive(Serialize, Deserialize)]
struct Envelope {
    providers: Vec<ServiceDescriptor>,
}

/// Parse a `catalog.json` document into its provider list.
pub fn parse_catalog(json: &str) -> anyhow::Result<Vec<ServiceDescriptor>> {
    let env: Envelope = serde_json::from_str(json)?;
    Ok(env.providers)
}

/// Serialize a provider list back into the canonical `catalog.json` form
/// (pretty-printed, 2-space indent, trailing newline).
pub fn serialize_catalog(providers: &[ServiceDescriptor]) -> anyhow::Result<String> {
    let env = Envelope {
        providers: providers.to_vec(),
    };
    let mut out = serde_json::to_string_pretty(&env)?;
    out.push('\n');
    Ok(out)
}

/// A change in a model's per-million-token pricing across a sync.
#[derive(Debug, Clone, PartialEq)]
pub struct PriceDelta {
    /// Catalog provider id hosting the model.
    pub provider_id: String,
    /// Model id.
    pub model_id: String,
    /// Input price before sync (USD / 1M tokens), if known.
    pub input_before: Option<f64>,
    /// Input price after sync.
    pub input_after: Option<f64>,
    /// Output price before sync.
    pub output_before: Option<f64>,
    /// Output price after sync.
    pub output_after: Option<f64>,
}

/// Summary of what a [`merge_catalog`] pass changed.
#[derive(Debug, Default, Clone)]
pub struct CatalogDiff {
    /// Total providers examined.
    pub providers_seen: usize,
    /// Providers whose model list was refreshed from models.dev.
    pub providers_synced: usize,
    /// Providers left untouched (no models.dev counterpart).
    pub providers_manual: usize,
    /// `(provider_id, model_id)` for models added (in models.dev, not catalog).
    pub models_added: Vec<(String, String)>,
    /// `(provider_id, model_id)` for existing models refreshed from models.dev.
    pub models_updated: Vec<(String, String)>,
    /// `(provider_id, model_id)` for catalog models absent upstream (kept).
    pub models_removed: Vec<(String, String)>,
    /// Pricing changes detected while refreshing.
    pub price_deltas: Vec<PriceDelta>,
}

impl CatalogDiff {
    /// `true` when the merge produced no changes (catalog already in sync).
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.models_added.is_empty()
            && self.models_updated.is_empty()
            && self.price_deltas.is_empty()
    }
}

impl std::fmt::Display for CatalogDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "providers: {} seen ({} synced, {} manual)",
            self.providers_seen, self.providers_synced, self.providers_manual
        )?;
        writeln!(
            f,
            "models: {} added, {} updated, {} catalog-only kept",
            self.models_added.len(),
            self.models_updated.len(),
            self.models_removed.len()
        )?;
        if !self.price_deltas.is_empty() {
            writeln!(f, "pricing changes (USD / 1M tokens):")?;
            for d in &self.price_deltas {
                writeln!(
                    f,
                    "  {}/{}: input {:?}→{:?}, output {:?}→{:?}",
                    d.provider_id,
                    d.model_id,
                    d.input_before,
                    d.input_after,
                    d.output_before,
                    d.output_after
                )?;
            }
        }
        Ok(())
    }
}

/// Merge a models.dev payload into the current catalog.
///
/// Field precedence (per the sync design):
/// - **Provider service fields** (auth, base URL, tiers, setup, ...): catalog wins.
/// - **Provider convenience fields** (`api_base_url`, `npm_package`,
///   `doc_url`): catalog wins; models.dev fills only when the catalog value is
///   empty.
/// - **Model fields** (cost, limit, modalities, capabilities, ...): models.dev
///   wins; catalog-only models (absent upstream) are preserved in place.
///
/// Returns the merged providers and a [`CatalogDiff`]. Pure: no I/O.
pub fn merge_catalog(
    current: &[ServiceDescriptor],
    upstream: &ModelsDevPayload,
) -> anyhow::Result<(Vec<ServiceDescriptor>, CatalogDiff)> {
    let mut diff = CatalogDiff {
        providers_seen: current.len(),
        ..CatalogDiff::default()
    };

    let merged: Vec<ServiceDescriptor> = current
        .iter()
        .map(|svc| merge_provider(svc, upstream, &mut diff))
        .collect();

    Ok((merged, diff))
}

/// Merge one provider: refresh its models from models.dev, keep its service
/// fields, and best-effort fill empty convenience fields.
fn merge_provider(
    svc: &ServiceDescriptor,
    upstream: &ModelsDevPayload,
    diff: &mut CatalogDiff,
) -> ServiceDescriptor {
    let resolved = mapping::resolve(&svc.id);
    match resolved {
        Mapping::Manual => {
            diff.providers_manual += 1;
            // Preserve catalog-only models as "kept" (informational).
            for m in &svc.models {
                diff.models_removed.push((svc.id.clone(), m.id.clone()));
            }
            svc.clone()
        }
        Mapping::Exact | Mapping::Aliased(_) => {
            diff.providers_synced += 1;
            let key = match resolved {
                Mapping::Aliased(k) => k,
                _ => svc.id.as_str(),
            };
            let upstream_models = upstream.provider_models(key);

            let mut merged_models: Vec<ModelDescriptor> = Vec::with_capacity(upstream_models.len());

            // Walk current models in catalog order: refresh from upstream when
            // present, otherwise preserve the catalog entry in place.
            for cm in &svc.models {
                if let Some(um) = upstream_models.iter().find(|m| m.id == cm.id) {
                    if cm != um {
                        record_delta(&svc.id, cm, um, diff);
                        diff.models_updated.push((svc.id.clone(), cm.id.clone()));
                    }
                    merged_models.push(um.clone());
                } else {
                    // Catalog-only model not in models.dev → keep as-is.
                    diff.models_removed.push((svc.id.clone(), cm.id.clone()));
                    merged_models.push(cm.clone());
                }
            }

            // Append upstream models not already present (additive, no deletion).
            for um in &upstream_models {
                if !merged_models.iter().any(|m| m.id == um.id) {
                    diff.models_added.push((svc.id.clone(), um.id.clone()));
                    merged_models.push(um.clone());
                }
            }

            ServiceDescriptor {
                models: merged_models,
                api_base_url: fill_opt(&svc.api_base_url, upstream.provider_api_base(key)),
                npm_package: fill_opt(&svc.npm_package, upstream.provider_npm(key)),
                doc_url: fill_opt(&svc.doc_url, upstream.provider_doc(key)),
                // All other provider service fields kept verbatim.
                ..svc.clone()
            }
        }
    }
}

/// Return the catalog value when present, else lift the upstream value.
fn fill_opt(catalog: &Option<String>, upstream: Option<&str>) -> Option<String> {
    if catalog.as_ref().is_some_and(|s| !s.is_empty()) {
        catalog.clone()
    } else {
        upstream.map(Into::into)
    }
}

/// Record a [`PriceDelta`] when input or output pricing changed.
fn record_delta(
    provider_id: &str,
    before: &ModelDescriptor,
    after: &ModelDescriptor,
    diff: &mut CatalogDiff,
) {
    let (ib, ob) = before
        .cost
        .as_ref()
        .map(|c| (Some(c.input), Some(c.output)))
        .unwrap_or((None, None));
    let (ia, oa) = after
        .cost
        .as_ref()
        .map(|c| (Some(c.input), Some(c.output)))
        .unwrap_or((None, None));
    if ib != ia || ob != oa {
        diff.price_deltas.push(PriceDelta {
            provider_id: provider_id.to_string(),
            model_id: before.id.clone(),
            input_before: ib,
            input_after: ia,
            output_before: ob,
            output_after: oa,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ModelCapabilities, ModelCost, ModelLimit, ModelModalities};

    /// Build a minimal models.dev-shaped payload for two providers.
    fn upstream_fixture() -> ModelsDevPayload {
        let raw = r#"{
            "zai": {
                "id": "zai", "env": ["ZHIPU_API_KEY"], "api": "https://api.z.ai/api/paas/v4",
                "npm": "@ai-sdk/zai", "doc": "https://docs.z.ai",
                "models": {
                    "glm-5": {
                        "id": "glm-5", "name": "GLM-5", "family": "glm",
                        "reasoning": true, "tool_call": true, "temperature": true,
                        "limit": {"context": 204800, "output": 131072},
                        "cost": {"input": 1, "output": 3.2, "cache_read": 0.2, "cache_write": 0}
                    },
                    "glm-5.1": {
                        "id": "glm-5.1", "name": "GLM-5.1",
                        "tool_call": true, "temperature": true,
                        "limit": {"context": 204800, "output": 131072},
                        "cost": {"input": 2, "output": 6}
                    }
                }
            },
            "native": {
                "id": "native", "models": {}
            }
        }"#;
        serde_json::from_str(raw).unwrap()
    }

    fn catalog_glm5(cost_input: f64, cost_output: f64) -> ServiceDescriptor {
        ServiceDescriptor {
            id: "zai".to_string(),
            display_name: "Z.AI".to_string(),
            description: "Z.AI".to_string(),
            category: "international".to_string(),
            family: "zai".to_string(),
            auth_mode: "secret".to_string(),
            key_var: "ZAI_API_KEY".to_string(),
            literal_auth_token: String::new(),
            base_url: "https://z.ai".to_string(),
            default_model: "glm-5".to_string(),
            model_tiers: std::collections::HashMap::new(),
            model_choices: vec![],
            test_url: "https://z.ai".to_string(),
            setup: vec![],
            usage: vec![],
            api_base_url: Some("https://existing.example/v1".to_string()),
            npm_package: None,
            doc_url: None,
            models: vec![ModelDescriptor {
                id: "glm-5".to_string(),
                name: "GLM-5 (stale)".to_string(),
                family: None,
                release_date: None,
                cost: Some(ModelCost {
                    input: cost_input,
                    output: cost_output,
                    cache_read: None,
                    cache_write: None,
                }),
                limit: Some(ModelLimit {
                    context: 128_000,
                    output: 4_096,
                }),
                modalities: None,
                capabilities: Some(ModelCapabilities {
                    attachment: false,
                    reasoning: false,
                    temperature: true,
                    tool_call: true,
                    streaming: true,
                }),
                knowledge: None,
            }],
        }
    }

    #[test]
    fn test_price_delta_detected_and_applied() {
        let current = vec![catalog_glm5(0.5, 0.5)];
        let (merged, diff) = merge_catalog(&current, &upstream_fixture()).unwrap();
        let zai = &merged[0];
        let glm5 = zai.models.iter().find(|m| m.id == "glm-5").unwrap();
        // models.dev pricing won.
        assert_eq!(glm5.cost.as_ref().unwrap().input, 1.0);
        assert_eq!(glm5.cost.as_ref().unwrap().output, 3.2);
        // Delta recorded.
        assert_eq!(diff.price_deltas.len(), 1);
        let d = &diff.price_deltas[0];
        assert_eq!(d.model_id, "glm-5");
        assert_eq!(d.input_before, Some(0.5));
        assert_eq!(d.input_after, Some(1.0));
        assert_eq!(d.output_before, Some(0.5));
        assert_eq!(d.output_after, Some(3.2));
    }

    #[test]
    fn test_provider_service_fields_preserved() {
        let current = vec![catalog_glm5(0.5, 0.5)];
        let (merged, _) = merge_catalog(&current, &upstream_fixture()).unwrap();
        let zai = &merged[0];
        assert_eq!(zai.key_var, "ZAI_API_KEY"); // catalog wins
        assert_eq!(zai.auth_mode, "secret");
        assert_eq!(zai.default_model, "glm-5");
        // api_base_url non-empty in catalog → NOT overwritten by models.dev.
        assert_eq!(
            zai.api_base_url.as_deref(),
            Some("https://existing.example/v1")
        );
        // npm/doc empty in catalog → filled from models.dev.
        assert_eq!(zai.npm_package.as_deref(), Some("@ai-sdk/zai"));
        assert_eq!(zai.doc_url.as_deref(), Some("https://docs.z.ai"));
    }

    #[test]
    fn test_api_base_filled_when_empty() {
        let mut current = catalog_glm5(0.5, 0.5);
        current.api_base_url = None; // empty → should be filled
        let (merged, _) = merge_catalog(&[current], &upstream_fixture()).unwrap();
        assert_eq!(
            merged[0].api_base_url.as_deref(),
            Some("https://api.z.ai/api/paas/v4")
        );
    }

    #[test]
    fn test_new_model_added() {
        let current = vec![catalog_glm5(0.5, 0.5)];
        let (merged, diff) = merge_catalog(&current, &upstream_fixture()).unwrap();
        // glm-5.1 is in models.dev but not the catalog fixture → added.
        assert!(merged[0].models.iter().any(|m| m.id == "glm-5.1"));
        assert!(diff.models_added.iter().any(|(_, id)| id == "glm-5.1"));
    }

    #[test]
    fn test_catalog_only_model_preserved() {
        let mut current = catalog_glm5(0.5, 0.5);
        // Inject a catalog-only model not present in models.dev zai.
        current.models.push(ModelDescriptor {
            id: "glm-custom-curate".to_string(),
            name: "Curated".to_string(),
            family: None,
            release_date: None,
            cost: None,
            limit: None,
            modalities: Some(ModelModalities {
                input: vec!["text".to_string()],
                output: vec!["text".to_string()],
            }),
            capabilities: None,
            knowledge: None,
        });
        let (merged, diff) = merge_catalog(&[current], &upstream_fixture()).unwrap();
        assert!(merged[0].models.iter().any(|m| m.id == "glm-custom-curate"));
        assert!(
            diff.models_removed
                .iter()
                .any(|(p, id)| p == "zai" && id == "glm-custom-curate")
        );
    }

    #[test]
    fn test_manual_provider_untouched() {
        // `native` is Manual (not in models.dev's mapped set) → kept as-is.
        let current = vec![ServiceDescriptor {
            id: "native".to_string(),
            models: vec![ModelDescriptor {
                id: "claude-haiku-3-5".to_string(),
                name: "Claude".to_string(),
                family: None,
                release_date: None,
                cost: Some(ModelCost {
                    input: 0.8,
                    output: 4.0,
                    cache_read: None,
                    cache_write: None,
                }),
                limit: None,
                modalities: None,
                capabilities: None,
                knowledge: None,
            }],
            ..catalog_glm5(0.5, 0.5)
        }];
        let (merged, diff) = merge_catalog(&current, &upstream_fixture()).unwrap();
        assert_eq!(diff.providers_manual, 1);
        assert_eq!(diff.providers_synced, 0);
        // native's model untouched (pricing not refreshed).
        let native = &merged[0];
        assert_eq!(native.models[0].cost.as_ref().unwrap().input, 0.8);
    }

    #[test]
    fn test_round_trip_envelope() {
        let providers = vec![catalog_glm5(1.0, 3.0)];
        let json = serialize_catalog(&providers).unwrap();
        let parsed = parse_catalog(&json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "zai");
    }

    #[test]
    fn test_is_clean_when_nothing_changes() {
        // Build a "current" catalog that already matches models.dev exactly.
        let upstream = upstream_fixture();
        let glm5 = upstream.provider_models("zai");
        let current = vec![ServiceDescriptor {
            id: "zai".to_string(),
            models: glm5,
            ..catalog_glm5(1.0, 3.2)
        }];
        let (_, diff) = merge_catalog(&current, &upstream).unwrap();
        // No new models added (glm-5.1 is in current? no — current only has
        // whatever provider_models returned, which is BOTH glm-5 and glm-5.1).
        assert!(diff.models_added.is_empty());
        assert!(diff.price_deltas.is_empty());
        assert!(diff.is_clean());
    }
}
