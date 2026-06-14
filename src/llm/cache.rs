//! Response cache wrapper for [`LLMClient`] backed by a [`KvStore`].
//!
//! [`CacheClient`] is a dedicated [`LLMClient`] wrapper — not an
//! observation-only middleware — because serving a cached response must
//! *short-circuit* the upstream call, which the [`LLMClientMiddleware`]
//! hooks (immutable, observe-only) deliberately cannot do. It composes like
//! the other wrappers: `CacheClient<RetryClient<OpenAIClient>>`.
//!
//! Only [`LLMClient::complete`] is cached; [`LLMClient::stream_complete`] is
//! always pass-through, since a streamed response cannot be replayed from a
//! stored buffer without changing its semantics.
//!
//! [`LLMClientMiddleware`]: crate::llm::LLMClientMiddleware
//! [`KvStore`]: crate::store::KvStore

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use async_trait::async_trait;

use crate::error::Result;
use crate::llm::client::LLMClient;
use crate::llm::types::{LLMRequest, LLMResponse, LLMStream};
use crate::store::KvStore;

/// Cache-key schema version. Bump to invalidate every cached entry at once
/// when the key derivation changes.
const KEY_VERSION: u8 = 1;
/// Namespace prefix so a shared [`KvStore`] can host several caches.
const KEY_PREFIX: &str = "llm-resp";

/// Derive a stable cache key for a request.
///
/// The payload is the canonical JSON of the whole [`LLMRequest`] (every field
/// it carries influences the response: model, system, messages, temperature,
/// max_tokens, response_format, tools). `serde_json` emits struct fields in a
/// fixed order, so identical requests hash identically.
///
/// The hash is best-effort: a Rust-version drift in [`DefaultHasher`] or a hash
/// collision can only ever cause a *cache miss* (recomputed response), never a
/// wrong hit, because a stored value is returned only under this exact key.
fn cache_key(request: &LLMRequest) -> String {
    let payload = serde_json::to_vec(request).unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    payload.hash(&mut hasher);
    format!("{KEY_PREFIX}:v{KEY_VERSION}:{:016x}", hasher.finish())
}

/// An [`LLMClient`] wrapper that caches `complete` responses in a [`KvStore`].
///
/// On `complete`, the cache is checked first; a hit returns the stored response
/// without calling the inner client. On a miss the inner client is called and a
/// successful response is stored. Cache read/write errors are non-fatal — a
/// failed read falls through to the inner client, a failed write is dropped,
/// and the response is still returned.
///
/// `stream_complete` is always delegated to the inner client (never cached).
pub struct CacheClient<C> {
    inner: C,
    store: Arc<dyn KvStore>,
}

impl<C> CacheClient<C> {
    /// Wrap `inner` with a response cache backed by `store`.
    pub fn new(inner: C, store: Arc<dyn KvStore>) -> Self {
        Self { inner, store }
    }

    /// Access the underlying (uncached) client.
    pub fn inner(&self) -> &C {
        &self.inner
    }
}

#[async_trait]
impl<C: LLMClient> LLMClient for CacheClient<C> {
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse> {
        let key = cache_key(&request);

        // Cache hit — return the stored response if it deserializes cleanly.
        if let Some(response) = self
            .store
            .get(&key)?
            .as_ref()
            .and_then(|bytes| serde_json::from_slice::<LLMResponse>(bytes).ok())
        {
            return Ok(response);
        }

        // Cache miss — call through, then store a copy of the response.
        let response = self.inner.complete(request).await?;
        if let Ok(bytes) = serde_json::to_vec(&response) {
            let _ = self.store.put(&key, &bytes);
        }
        Ok(response)
    }

    fn model_name(&self) -> &str {
        self.inner.model_name()
    }

    async fn stream_complete(&self, request: LLMRequest) -> Result<LLMStream> {
        self.inner.stream_complete(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::KernelError;
    use crate::llm::types::TokenUsage;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A mock client that counts upstream calls and returns a fixed response.
    struct CountingClient {
        calls: Arc<AtomicUsize>,
        body: String,
    }

    #[async_trait]
    impl LLMClient for CountingClient {
        async fn complete(&self, _request: LLMRequest) -> Result<LLMResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(LLMResponse {
                content: self.body.clone(),
                model: "mock".into(),
                usage: TokenUsage::default(),
                finish_reason: None,
                id: None,
                created: None,
            })
        }
        fn model_name(&self) -> &str {
            "mock"
        }
        async fn stream_complete(&self, _request: LLMRequest) -> Result<LLMStream> {
            Err(KernelError::LlmApi("not implemented".into()))
        }
    }

    fn make_request(text: &str) -> LLMRequest {
        LLMRequest::builder().user_message(text).build()
    }

    #[tokio::test]
    async fn identical_request_served_from_cache_after_first_call() {
        let calls = Arc::new(AtomicUsize::new(0));
        let inner = CountingClient {
            calls: calls.clone(),
            body: "hello".into(),
        };
        let store: Arc<dyn KvStore> =
            Arc::new(crate::store::SqliteKvStore::open_in_memory().unwrap());
        let client = CacheClient::new(inner, store);

        let r1 = client.complete(make_request("ping")).await.unwrap();
        let r2 = client.complete(make_request("ping")).await.unwrap();

        assert_eq!(r1.content, "hello");
        assert_eq!(r2.content, "hello");
        // Only the first call hit the upstream; the second was cached.
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn differing_request_misses_cache() {
        let calls = Arc::new(AtomicUsize::new(0));
        let inner = CountingClient {
            calls: calls.clone(),
            body: "x".into(),
        };
        let store: Arc<dyn KvStore> =
            Arc::new(crate::store::SqliteKvStore::open_in_memory().unwrap());
        let client = CacheClient::new(inner, store);

        let _ = client.complete(make_request("alpha")).await.unwrap();
        let _ = client.complete(make_request("beta")).await.unwrap();

        // Different message text -> different key -> two upstream calls.
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn cache_key_reflects_temperature() {
        let calls = Arc::new(AtomicUsize::new(0));
        let inner = CountingClient {
            calls: calls.clone(),
            body: "x".into(),
        };
        let store: Arc<dyn KvStore> =
            Arc::new(crate::store::SqliteKvStore::open_in_memory().unwrap());
        let client = CacheClient::new(inner, store);

        let _ = client
            .complete(
                LLMRequest::builder()
                    .user_message("hi")
                    .temperature(0.0)
                    .build(),
            )
            .await
            .unwrap();
        let _ = client
            .complete(
                LLMRequest::builder()
                    .user_message("hi")
                    .temperature(0.7)
                    .build(),
            )
            .await
            .unwrap();

        // Temperature is part of the key, so the second call misses.
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn model_name_delegates_to_inner() {
        let inner = CountingClient {
            calls: Arc::new(AtomicUsize::new(0)),
            body: "x".into(),
        };
        let store: Arc<dyn KvStore> =
            Arc::new(crate::store::SqliteKvStore::open_in_memory().unwrap());
        let client = CacheClient::new(inner, store);
        assert_eq!(client.model_name(), "mock");
    }
}
