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
//! The cache key incorporates the **client identity** (`model_name`) so that
//! two different providers sharing one [`KvStore`] never cross-contaminate
//! an identical request. An optional TTL ([`CacheClient::with_ttl`]) expires
//! stale entries; without it entries live until removed.
//!
//! [`LLMClientMiddleware`]: crate::llm::LLMClientMiddleware
//! [`KvStore`]: crate::store::KvStore

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::llm::client::LLMClient;
use crate::llm::types::{LLMRequest, LLMResponse, LLMStream};
use crate::store::KvStore;

/// Cache-key schema version. Bump to invalidate every cached entry at once
/// when the key derivation or stored format changes.
const KEY_VERSION: u8 = 2;
/// Namespace prefix so a shared [`KvStore`] can host several caches.
const KEY_PREFIX: &str = "llm-resp";

/// A cached response paired with the wall-clock second it was stored.
#[derive(Serialize, Deserialize)]
struct CachedResponse {
    /// Unix-epoch seconds at which the entry was stored.
    stored_at_secs: u64,
    /// The cached response.
    response: LLMResponse,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Derive a stable cache key for a request under a given client identity.
///
/// The payload is the client's `model_name` (so two providers sharing a store
/// can't collide on an identical request) followed by the canonical JSON of
/// the whole [`LLMRequest`] (every field influences the response: messages,
/// temperature, max_tokens, response_format, tools). `serde_json` emits struct
/// fields in a fixed order, so identical requests hash identically.
///
/// The hash is best-effort: a Rust-version drift in [`DefaultHasher`] or a hash
/// collision can only ever cause a *cache miss* (recomputed response), never a
/// wrong hit.
fn cache_key(model_name: &str, request: &LLMRequest) -> String {
    let mut hasher = DefaultHasher::new();
    model_name.hash(&mut hasher);
    serde_json::to_vec(request)
        .unwrap_or_default()
        .hash(&mut hasher);
    format!("{KEY_PREFIX}:v{KEY_VERSION}:{:016x}", hasher.finish())
}

/// An [`LLMClient`] wrapper that caches `complete` responses in a [`KvStore`].
///
/// On `complete`, the cache is checked first; a fresh (non-expired) hit returns
/// the stored response without calling the inner client. On a miss the inner
/// client is called and a successful response is stored. Cache read/write
/// errors are non-fatal — a failed read falls through to the inner client, a
/// failed write is dropped, and the response is still returned.
///
/// `stream_complete` is always delegated to the inner client (never cached).
pub struct CacheClient<C> {
    inner: C,
    store: Arc<dyn KvStore>,
    ttl: Option<Duration>,
}

impl<C> CacheClient<C> {
    /// Wrap `inner` with a response cache backed by `store` (no expiry).
    pub fn new(inner: C, store: Arc<dyn KvStore>) -> Self {
        Self {
            inner,
            store,
            ttl: None,
        }
    }

    /// Wrap `inner` with a response cache whose entries expire after `ttl`.
    ///
    /// Expired entries are lazily evicted on read (treated as a miss). A TTL
    /// bounds staleness; to bound total size, wrap the [`KvStore`] with an
    /// evicting implementation — `CacheClient` does not enforce a size cap.
    pub fn with_ttl(inner: C, store: Arc<dyn KvStore>, ttl: Duration) -> Self {
        Self {
            inner,
            store,
            ttl: Some(ttl),
        }
    }

    /// Access the underlying (uncached) client.
    pub fn inner(&self) -> &C {
        &self.inner
    }

    /// Look up a non-expired cached response for `key`, if any.
    fn lookup(&self, key: &str) -> Option<LLMResponse> {
        let bytes = self.store.get(key).ok()??;
        let entry: CachedResponse = serde_json::from_slice(&bytes).ok()?;
        if let Some(ttl) = self.ttl {
            let age = now_secs().saturating_sub(entry.stored_at_secs);
            if age > ttl.as_secs() {
                return None;
            }
        }
        Some(entry.response)
    }

    /// Store a response under `key` (best-effort; failures are dropped).
    fn store_entry(&self, key: &str, response: &LLMResponse) {
        let entry = CachedResponse {
            stored_at_secs: now_secs(),
            response: response.clone(),
        };
        if let Ok(bytes) = serde_json::to_vec(&entry) {
            let _ = self.store.put(key, &bytes);
        }
    }
}

#[async_trait]
impl<C: LLMClient> LLMClient for CacheClient<C> {
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse> {
        let key = cache_key(self.inner.model_name(), &request);

        if let Some(response) = self.lookup(&key) {
            return Ok(response);
        }

        let response = self.inner.complete(request).await?;
        self.store_entry(&key, &response);
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
        model: &'static str,
    }

    #[async_trait]
    impl LLMClient for CountingClient {
        async fn complete(&self, _request: LLMRequest) -> Result<LLMResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(LLMResponse {
                content: self.body.clone(),
                model: self.model.to_string(),
                usage: TokenUsage::default(),
                finish_reason: None,
                id: None,
                created: None,
            })
        }
        fn model_name(&self) -> &str {
            self.model
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
            model: "mock",
        };
        let store: Arc<dyn KvStore> =
            Arc::new(crate::store::SqliteKvStore::open_in_memory().unwrap());
        let client = CacheClient::new(inner, store);

        let r1 = client.complete(make_request("ping")).await.unwrap();
        let r2 = client.complete(make_request("ping")).await.unwrap();

        assert_eq!(r1.content, "hello");
        assert_eq!(r2.content, "hello");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn differing_request_misses_cache() {
        let calls = Arc::new(AtomicUsize::new(0));
        let inner = CountingClient {
            calls: calls.clone(),
            body: "x".into(),
            model: "mock",
        };
        let store: Arc<dyn KvStore> =
            Arc::new(crate::store::SqliteKvStore::open_in_memory().unwrap());
        let client = CacheClient::new(inner, store);

        let _ = client.complete(make_request("alpha")).await.unwrap();
        let _ = client.complete(make_request("beta")).await.unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// P1: two clients with different `model_name` sharing one store must not
    /// cross-contaminate an identical request.
    #[tokio::test]
    async fn distinct_clients_do_not_share_entries() {
        let store: Arc<dyn KvStore> =
            Arc::new(crate::store::SqliteKvStore::open_in_memory().unwrap());
        let calls_a = Arc::new(AtomicUsize::new(0));
        let calls_b = Arc::new(AtomicUsize::new(0));
        let a = CacheClient::new(
            CountingClient {
                calls: calls_a.clone(),
                body: "from-a".into(),
                model: "openai",
            },
            store.clone(),
        );
        let b = CacheClient::new(
            CountingClient {
                calls: calls_b.clone(),
                body: "from-b".into(),
                model: "anthropic",
            },
            store,
        );

        let ra = a.complete(make_request("same")).await.unwrap();
        let rb = b.complete(make_request("same")).await.unwrap();

        // Different clients → different keys → both upstream, no contamination.
        assert_eq!(ra.content, "from-a");
        assert_eq!(rb.content, "from-b");
        assert_eq!(calls_a.load(Ordering::SeqCst), 1);
        assert_eq!(calls_b.load(Ordering::SeqCst), 1);
    }

    /// P2b: an entry older than the TTL is treated as a miss.
    #[tokio::test]
    async fn expired_entry_is_a_miss() {
        let store: Arc<dyn KvStore> =
            Arc::new(crate::store::SqliteKvStore::open_in_memory().unwrap());

        // Seed a stale entry under the exact key the client will compute.
        let inner = CountingClient {
            calls: Arc::new(AtomicUsize::new(0)),
            body: "fresh".into(),
            model: "mock",
        };
        let stale_key = cache_key("mock", &make_request("hi"));
        let stale = CachedResponse {
            stored_at_secs: 0, // ancient → definitely older than any TTL
            response: LLMResponse {
                content: "stale".into(),
                model: "mock".into(),
                usage: TokenUsage::default(),
                finish_reason: None,
                id: None,
                created: None,
            },
        };
        store
            .put(&stale_key, &serde_json::to_vec(&stale).unwrap())
            .unwrap();

        let calls = inner.calls.clone();
        let client = CacheClient::with_ttl(inner, store, Duration::from_secs(60));
        let r = client.complete(make_request("hi")).await.unwrap();

        // Stale entry ignored → upstream called → fresh response.
        assert_eq!(r.content, "fresh");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cache_key_reflects_temperature() {
        let calls = Arc::new(AtomicUsize::new(0));
        let inner = CountingClient {
            calls: calls.clone(),
            body: "x".into(),
            model: "mock",
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

        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn model_name_delegates_to_inner() {
        let inner = CountingClient {
            calls: Arc::new(AtomicUsize::new(0)),
            body: "x".into(),
            model: "mock",
        };
        let store: Arc<dyn KvStore> =
            Arc::new(crate::store::SqliteKvStore::open_in_memory().unwrap());
        let client = CacheClient::new(inner, store);
        assert_eq!(client.model_name(), "mock");
    }
}
