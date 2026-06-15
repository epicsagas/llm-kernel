//! Provider-id mapping between the embedded catalog and the
//! [models.dev](https://models.dev) upstream catalog.
//!
//! `resolve` answers "which models.dev provider key holds the canonical model
//! list for a given catalog provider id". This drives the catalog sync tooling
//! ([`crate::provider::sync`]) and runtime enrichment of discovery results
//! ([`crate::discovery`]).

/// How a catalog provider id maps to a models.dev provider key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mapping {
    /// The catalog id is the exact models.dev provider key.
    Exact,
    /// The catalog id maps to a different models.dev provider key.
    Aliased(&'static str),
    /// The catalog id has no models.dev counterpart — keep its models as-is.
    Manual,
}

impl Mapping {
    /// Return the models.dev provider key this mapping resolves to, or `None`
    /// for [`Mapping::Manual`].
    #[must_use]
    pub fn upstream_key(&self) -> Option<&'static str> {
        match self {
            Mapping::Exact => None,
            Mapping::Aliased(k) => Some(k),
            Mapping::Manual => None,
        }
    }
}

/// Resolve a catalog provider id to its models.dev counterpart.
///
/// Returns [`Mapping::Exact`] when the id matches a models.dev key directly,
/// [`Mapping::Aliased`] with the upstream key when an alias is known, and
/// [`Mapping::Manual`] for catalog providers absent from models.dev (local
/// engines and curated passthroughs such as `native`). Callers should keep
/// [`Mapping::Manual`] providers untouched during sync.
///
/// When adding a provider to `catalog.json`, decide its mapping here so the
/// table stays in sync — otherwise it silently resolves to [`Mapping::Manual`].
#[must_use]
pub fn resolve(catalog_id: &str) -> Mapping {
    match catalog_id {
        // Exact matches (catalog id == models.dev provider key).
        "openai" | "zai" | "minimax" | "minimax-cn" | "deepseek" | "alibaba" | "alibaba-cn"
        | "lmstudio" => Mapping::Exact,
        // Aliased matches (catalog id → different models.dev provider key).
        "gemini" => Mapping::Aliased("google"),
        "zai-cn" => Mapping::Aliased("zai"),
        "zai-coding" | "zai-cn-coding" => Mapping::Aliased("zai-coding-plan"),
        "kimi" => Mapping::Aliased("kimi-for-coding"),
        "moonshot" => Mapping::Aliased("moonshotai"),
        "alibaba-us" => Mapping::Aliased("alibaba"),
        // Everything else (native, ve, mimo, ollama, llamacpp, unknowns) is manual.
        _ => Mapping::Manual,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXACT: &[&str] = &[
        "openai",
        "zai",
        "minimax",
        "minimax-cn",
        "deepseek",
        "alibaba",
        "alibaba-cn",
        "lmstudio",
    ];
    const MANUAL: &[&str] = &["native", "ve", "mimo", "ollama", "llamacpp"];

    #[test]
    fn test_exact_matches() {
        for &id in EXACT {
            assert_eq!(resolve(id), Mapping::Exact, "{id} should be Exact");
        }
    }

    #[test]
    fn test_aliased_matches() {
        assert_eq!(resolve("gemini"), Mapping::Aliased("google"));
        assert_eq!(resolve("zai-cn"), Mapping::Aliased("zai"));
        assert_eq!(resolve("zai-coding"), Mapping::Aliased("zai-coding-plan"));
        assert_eq!(
            resolve("zai-cn-coding"),
            Mapping::Aliased("zai-coding-plan")
        );
        assert_eq!(resolve("kimi"), Mapping::Aliased("kimi-for-coding"));
        assert_eq!(resolve("moonshot"), Mapping::Aliased("moonshotai"));
        assert_eq!(resolve("alibaba-us"), Mapping::Aliased("alibaba"));
    }

    #[test]
    fn test_manual_matches() {
        for &id in MANUAL {
            assert_eq!(resolve(id), Mapping::Manual, "{id} should be Manual");
        }
        assert_eq!(resolve("totally-unknown"), Mapping::Manual);
    }

    #[test]
    fn test_upstream_key_helper() {
        assert_eq!(resolve("openai").upstream_key(), None); // Exact → use the id itself
        assert_eq!(resolve("gemini").upstream_key(), Some("google"));
        assert_eq!(resolve("ollama").upstream_key(), None); // Manual → no upstream
    }

    #[test]
    fn test_every_embedded_provider_is_classified() {
        // Guard against drift: every provider currently in catalog.json must
        // be explicitly classified as Exact, Aliased, or Manual. If a new
        // provider is added without a mapping decision, this test surfaces it
        // by showing an unexpected Manual for a provider that should sync.
        let ids = crate::provider::ProviderIndex::embedded().ids();
        let aliased: &[&str] = &[
            "gemini",
            "zai-cn",
            "zai-coding",
            "zai-cn-coding",
            "kimi",
            "moonshot",
            "alibaba-us",
        ];
        for id in &ids {
            let m = resolve(id);
            let is_exact = EXACT.contains(&id.as_str());
            let is_aliased = aliased.contains(&id.as_str());
            let is_manual = MANUAL.contains(&id.as_str());
            assert!(
                match m {
                    Mapping::Exact => is_exact,
                    Mapping::Aliased(_) => is_aliased,
                    Mapping::Manual => is_manual,
                },
                "provider {id} resolved to {m:?} but is not in any known bucket — \
                 add a mapping decision in resolve()"
            );
        }
    }
}
