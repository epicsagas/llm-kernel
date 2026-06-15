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
        /// **Trust boundary (SSRF):** the base URL is used verbatim. There is
        /// no scheme or host allowlist and no private-address/loopback
        /// blocking, so this constructor must only receive admin-configured
        /// values — never input derived from untrusted data. Redirects are
        /// disabled and the response body is size-capped, but a caller-chosen
        /// URL can still be pointed directly at an internal service (e.g. a
        /// cloud metadata endpoint), so treat the URL itself as the trust
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
            // Surface non-success HTTP as a clear error before any body is
            // read, so a 4xx/5xx error page is not misread as malformed JSON.
            let mut response = client.get(&url).send().await?.error_for_status()?;
            // Bound the response so a malformed or hostile endpoint cannot
            // drive unbounded memory allocation. Two layers:
            //   1. Fast-reject via Content-Length when the server advertises it.
            //   2. Stream the body with a hard cap, stopping the instant it is
            //      crossed — robust against a missing or understated length.
            const MAX_BYTES: usize = 64 * 1024 * 1024; // 64 MiB
            if let Some(len) = response.content_length()
                && (len as usize) > MAX_BYTES
            {
                anyhow::bail!("discovery response advertised {len} bytes (cap {MAX_BYTES})");
            }
            let body = read_capped_body(&mut response, MAX_BYTES).await?;
            let payload: crate::discovery::ModelsDevPayload = serde_json::from_slice(&body)?;
            Ok(payload.entries())
        }
    }

    /// Reads the response body incrementally, erroring the moment its length
    /// crosses `max_bytes`.
    ///
    /// Reading via [`reqwest::Response::chunk`] (rather than `Response::bytes`)
    /// keeps peak memory bounded even when `Content-Length` is absent or
    /// understates the true body: we stop as soon as the cap is exceeded,
    /// before handing the bytes to the deserializer.
    async fn read_capped_body(
        response: &mut reqwest::Response,
        max_bytes: usize,
    ) -> anyhow::Result<Vec<u8>> {
        let mut buf: Vec<u8> = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if buf.len() + chunk.len() > max_bytes {
                anyhow::bail!("discovery response exceeded {max_bytes} bytes while streaming");
            }
            buf.extend_from_slice(&chunk);
        }
        Ok(buf)
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
                ..Default::default()
            },
            ModelEntry {
                id: "openai/gpt-4o".to_string(),
                name: "GPT-4o".to_string(),
                provider_id: "openai".to_string(),
                ..Default::default()
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
    fn test_parse_real_payload_via_models_dev_type() {
        // Real models.dev shape: provider-keyed map with nested model objects.
        let raw = r#"{
            "anthropic": {
                "id": "anthropic",
                "env": ["ANTHROPIC_API_KEY"],
                "models": {
                    "claude-opus-4-5": {
                        "id": "claude-opus-4-5",
                        "name": "Claude Opus 4.5",
                        "tool_call": true,
                        "temperature": true,
                        "limit": {"context": 200000, "output": 64000},
                        "cost": {"input": 5, "output": 25}
                    }
                }
            }
        }"#;
        let payload: ModelsDevPayload = serde_json::from_str(raw).unwrap();
        let entries = payload.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "claude-opus-4-5");
        assert_eq!(entries[0].provider_id, "anthropic");
    }
}
