//! Async trait abstraction over model discovery sources.
//!
//! Provides a [`DiscoverySource`] trait so callers can fetch model listings from
//! heterogeneous backends (e.g. [models.dev](https://github.com/anomalyco/models.dev))
//! behind a single async interface.

#[cfg(feature = "discovery-async")]
mod inner {
    use async_trait::async_trait;
    use std::time::Duration;

    /// Async source of discoverable models.
    #[async_trait]
    pub trait DiscoverySource: Send + Sync {
        /// Human-readable source name.
        fn name(&self) -> &'static str;
        /// Discover available models from this source.
        async fn discover(&self) -> anyhow::Result<Vec<crate::discovery::ModelEntry>>;
    }

    /// Async [`DiscoverySource`] backed by a models.dev-style catalog API.
    pub struct ModelsDevSource {
        /// Base URL the catalog is served from (e.g. `https://models.dev`).
        base_url: String,
    }

    impl ModelsDevSource {
        /// Build a source pointing at the default models.dev catalog.
        pub fn new() -> Self {
            Self {
                base_url: "https://models.dev".to_string(),
            }
        }

        /// Build a source with a custom base URL (handy for tests or a self-hosted
        /// catalog).
        ///
        /// **Trust boundary:** the base URL is used verbatim — there is no
        /// scheme or host allowlist, so this constructor must only receive
        /// admin-configured values, never input derived from untrusted data
        /// (a caller-controlled URL could be directed at internal services).
        /// Responses are size-capped, but treat the URL itself as a trust
        /// boundary.
        pub fn with_base_url(base_url: impl Into<String>) -> Self {
            Self {
                base_url: base_url.into(),
            }
        }
    }

    impl Default for ModelsDevSource {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait]
    impl DiscoverySource for ModelsDevSource {
        fn name(&self) -> &'static str {
            "models.dev"
        }

        async fn discover(&self) -> anyhow::Result<Vec<crate::discovery::ModelEntry>> {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                // Do not follow redirects: the base URL is a trusted catalog
                // endpoint, and a 3xx should surface as an error rather than be
                // silently chased to an unexpected host.
                .redirect(reqwest::redirect::Policy::none())
                .build()?;
            let url = format!("{}/api.json", self.base_url.trim_end_matches('/'));
            let response = client.get(&url).send().await?;
            // Bound the response so a malformed or hostile endpoint cannot
            // drive unbounded memory allocation during deserialization.
            const MAX_BYTES: usize = 64 * 1024 * 1024; // 64 MiB
            let bytes = response.bytes().await?;
            if bytes.len() > MAX_BYTES {
                anyhow::bail!("discovery response exceeded {MAX_BYTES} bytes");
            }
            let payload: crate::discovery::ModelsDevPayload = serde_json::from_slice(&bytes)?;
            Ok(payload.models)
        }
    }
}

#[cfg(feature = "discovery-async")]
pub use inner::{DiscoverySource, ModelsDevSource};

#[cfg(all(test, feature = "discovery-async"))]
mod tests {
    use super::*;
    use crate::discovery::{ModelEntry, ModelsDevPayload};

    /// In-memory source used purely to exercise the trait without network access.
    struct StaticSource(Vec<ModelEntry>);

    #[async_trait::async_trait]
    impl DiscoverySource for StaticSource {
        fn name(&self) -> &'static str {
            "static"
        }

        async fn discover(&self) -> anyhow::Result<Vec<ModelEntry>> {
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn test_static_source_returns_models_and_name() {
        let entries = vec![
            ModelEntry {
                id: "anthropic/claude-3-5-sonnet".to_string(),
                name: "Claude 3.5 Sonnet".to_string(),
                provider_id: "anthropic".to_string(),
                limits: None,
            },
            ModelEntry {
                id: "openai/gpt-4o".to_string(),
                name: "GPT-4o".to_string(),
                provider_id: "openai".to_string(),
                limits: None,
            },
        ];
        let source = StaticSource(entries.clone());
        assert_eq!(source.name(), "static");
        let discovered = source.discover().await.unwrap();
        assert_eq!(discovered.len(), entries.len());
        assert_eq!(discovered[0].id, "anthropic/claude-3-5-sonnet");
        assert_eq!(discovered[1].id, "openai/gpt-4o");
    }

    #[test]
    fn test_parse_payload_via_models_dev_type() {
        let raw = r#"{
            "models": [
                {
                    "id": "anthropic/claude-3-5-sonnet",
                    "name": "Claude 3.5 Sonnet",
                    "provider_id": "anthropic",
                    "limits": {
                        "context": 200000,
                        "input": 200000,
                        "output": 8192
                    }
                }
            ]
        }"#;
        let payload: ModelsDevPayload = serde_json::from_str(raw).unwrap();
        assert_eq!(payload.models.len(), 1);
        assert_eq!(payload.models[0].id, "anthropic/claude-3-5-sonnet");
    }
}
