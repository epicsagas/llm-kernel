//! Cost-aware routing and fallback chain for [`LLMClient`].
//!
//! [`RouterClient`] holds an ordered set of [`Backend`]s and routes each
//! request according to a [`RoutingStrategy`]. On error it falls through to
//! the next backend. This is the orchestration layer for cross-provider /
//! cross-model resilience.
//!
//! # Separation of concerns
//!
//! The `llm` module composes resilience from focused decorators rather than
//! folding every behavior into one client:
//!
//! | Concern | Decorator |
//! |---------|-----------|
//! | rate-limit (429) / 5xx backoff | [`RetryClient`](crate::llm::retry::RetryClient) |
//! | routing / cross-provider fallback | [`RouterClient`] |
//! | request/response observation | [`MiddlewareClient`](crate::llm::middleware::MiddlewareClient) |
//! | response caching | [`CacheClient`](crate::llm::cache::CacheClient) |
//!
//! Because every decorator implements [`LLMClient`], they compose freely. A
//! resilient, observed, cached multi-backend stack reads inside-out:
//!
//! ```ignore
//! use llm_kernel::llm::{
//!     CacheClient, LLMClient, MiddlewareClient, NoopMiddleware, OpenAIClient,
//!     RetryClient, RouterClient, RoutingStrategy, Backend, RetryConfig,
//! };
//!
//! let cheap = Backend::new(OpenAIClient::from_key("gpt-4o-mini", "sk-...")?, Some((0.15, 0.60)));
//! let powerful = Backend::new(OpenAIClient::from_key("gpt-4o", "sk-...")?, Some((2.50, 10.00)));
//!
//! // Route across backends; retry each transiently; observe; cache.
//! let stack = CacheClient::new(
//!     MiddlewareClient::new(
//!         RouterClient::new(
//!             vec![cheap, powerful]
//!                 .into_iter()
//!                 .map(|b| Backend { client: RetryClient::new(b.client, RetryConfig::default()), ..b })
//!                 .collect(),
//!             RoutingStrategy::LowestCost,
//!         )?,
//!         NoopMiddleware,
//!     ),
//!     store,
//! );
//! ```
//!
//! Streaming cannot fall through mid-stream, so [`RouterClient::stream_complete`]
//! delegates to the preferred backend only — wrap individual backends in
//! [`RetryClient`](crate::llm::retry::RetryClient) for transient resilience.

use std::cmp::Ordering;

use async_trait::async_trait;

use crate::error::{KernelError, Result};
use crate::llm::client::LLMClient;
use crate::llm::types::{LLMRequest, LLMResponse, LLMStream};

/// A single backend in a [`RouterClient`] chain.
///
/// `cost_per_1m` is the `(input, output)` price per 1,000,000 tokens in USD,
/// used by [`RoutingStrategy::LowestCost`] to order backends. `priority`
/// (ascending) is used by [`RoutingStrategy::Fallback`]. Ties in either keep
/// insertion order (stable sort).
pub struct Backend<C: LLMClient> {
    /// The underlying client.
    pub client: C,
    /// `(input_per_1m, output_per_1m)` in USD. `None` = unknown → sorts last.
    pub cost_per_1m: Option<(f64, f64)>,
    /// Lower = tried first. Defaults to `0`.
    pub priority: usize,
    label: Option<String>,
}

impl<C: LLMClient> Backend<C> {
    /// Create a backend with no cost metadata and default priority.
    pub fn new(client: C, cost_per_1m: Option<(f64, f64)>) -> Self {
        Self {
            client,
            cost_per_1m,
            priority: 0,
            label: None,
        }
    }

    /// Set the fallback priority (lower = tried first).
    pub fn with_priority(mut self, priority: usize) -> Self {
        self.priority = priority;
        self
    }

    /// Override the label used for this backend (otherwise the underlying
    /// client's [`LLMClient::model_name`] is used).
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    fn name(&self) -> &str {
        self.label
            .as_deref()
            .unwrap_or_else(|| self.client.model_name())
    }

    /// Average of input+output per-1m cost. `None` and non-finite (NaN) values
    /// normalize to `f64::MAX` so they sort last under [`RoutingStrategy::LowestCost`].
    fn cost_rank(&self) -> f64 {
        let raw = self
            .cost_per_1m
            .map(|(input, output)| (input + output) / 2.0)
            .unwrap_or(f64::MAX);
        if raw.is_nan() { f64::MAX } else { raw }
    }
}

/// How a [`RouterClient`] orders and falls through its backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RoutingStrategy {
    /// Try backends in ascending `priority` order; on error fall through.
    /// Ties keep insertion order.
    #[default]
    Fallback,
    /// Try backends in ascending cost order; on error fall through to the next
    /// cheapest. Backends with unknown (or non-finite) cost sort last.
    LowestCost,
}

/// Compute the try-order of `backends` under `strategy`. Stable: ties keep
/// insertion order.
fn compute_order<C: LLMClient>(backends: &[Backend<C>], strategy: RoutingStrategy) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..backends.len()).collect();
    match strategy {
        RoutingStrategy::Fallback => idx.sort_by_key(|&i| backends[i].priority),
        RoutingStrategy::LowestCost => idx.sort_by(|&a, &b| {
            backends[a]
                .cost_rank()
                .partial_cmp(&backends[b].cost_rank())
                .unwrap_or(Ordering::Equal)
        }),
    }
    idx
}

/// A [`LLMClient`] that routes requests across multiple backends with fallback.
///
/// Construct with [`RouterClient::new`]. Each `complete` tries the backends in
/// the order dictated by the [`RoutingStrategy`]; the first success is returned
/// and any remaining backends are skipped. If every backend errors, the last
/// error is returned.
///
/// Fall-through applies **uniformly to every error variant** (including
/// non-retryable 4xx such as 400/401). This is intended for cross-provider /
/// cross-model resilience, but note the consequence: a misconfigured request
/// (e.g. an invalid 400) will be retried against *every* backend. Wrap each
/// backend in a [`RetryClient`](crate::llm::retry::RetryClient) to absorb
/// transient errors (rate-limit, 5xx) before the router treats them as
/// failures and moves on.
pub struct RouterClient<C: LLMClient> {
    backends: Vec<Backend<C>>,
    strategy: RoutingStrategy,
    /// Precomputed try-order; immutable after [`RouterClient::new`].
    order: Vec<usize>,
}

impl<C: LLMClient> RouterClient<C> {
    /// Create a router over `backends`.
    ///
    /// Returns [`KernelError::Config`] if `backends` is empty.
    pub fn new(backends: Vec<Backend<C>>, strategy: RoutingStrategy) -> Result<Self> {
        if backends.is_empty() {
            return Err(KernelError::Config(
                "RouterClient requires at least one backend".into(),
            ));
        }
        let order = compute_order(&backends, strategy);
        Ok(Self {
            backends,
            strategy,
            order,
        })
    }

    /// Backend indices in try-order (computed once at construction).
    fn order(&self) -> &[usize] {
        &self.order
    }

    /// The backends in insertion order.
    pub fn backends(&self) -> &[Backend<C>] {
        &self.backends
    }

    /// The active routing strategy.
    pub fn strategy(&self) -> RoutingStrategy {
        self.strategy
    }
}

#[async_trait]
impl<C: LLMClient> LLMClient for RouterClient<C> {
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse> {
        let mut last_err: Option<KernelError> = None;
        for &i in self.order() {
            match self.backends[i].client.complete(request.clone()).await {
                Ok(response) => return Ok(response),
                Err(err) => last_err = Some(err),
            }
        }
        Err(last_err
            .unwrap_or_else(|| KernelError::Config("RouterClient: no backends tried".into())))
    }

    fn model_name(&self) -> &str {
        // The preferred backend is the first in routing order.
        let first = self.order().first().copied().unwrap_or(0);
        self.backends[first].name()
    }

    async fn stream_complete(&self, request: LLMRequest) -> Result<LLMStream> {
        // A stream cannot fall through once the first byte is sent, so delegate
        // to the preferred backend only. Note: errors raised *before* the
        // stream is established (connection, 403) are also NOT retried against
        // other backends here — wrap the preferred backend in `RetryClient` for
        // transient resilience, or call `complete()` when full fallback matters.
        let first = self.order().first().copied().unwrap_or(0);
        self.backends[first].client.stream_complete(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    };

    use crate::llm::types::StreamEvent;
    use tokio::sync::mpsc;
    use tokio_stream::wrappers::ReceiverStream;

    /// A mock client that returns a fixed sequence of `complete` results
    /// (drained one per call) and records `stream_complete` invocations.
    struct MockClient {
        name: &'static str,
        responses: Mutex<Vec<Result<LLMResponse>>>,
        stream_calls: Arc<AtomicU32>,
    }

    impl MockClient {
        fn new(name: &'static str, responses: Vec<Result<LLMResponse>>) -> Self {
            Self {
                name,
                responses: Mutex::new(responses),
                stream_calls: Arc::new(AtomicU32::new(0)),
            }
        }

        fn stream_call_count(&self) -> Arc<AtomicU32> {
            Arc::clone(&self.stream_calls)
        }
    }

    #[async_trait]
    impl LLMClient for MockClient {
        async fn complete(&self, _request: LLMRequest) -> Result<LLMResponse> {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                panic!("MockClient({}): no more responses queued", self.name);
            }
            responses.remove(0)
        }

        fn model_name(&self) -> &str {
            self.name
        }

        async fn stream_complete(&self, _request: LLMRequest) -> Result<LLMStream> {
            self.stream_calls.fetch_add(1, Ordering::Relaxed);
            // A single Done event is enough to exercise the delegation path.
            let (tx, rx) = mpsc::channel::<Result<StreamEvent>>(16);
            let _ = tx.send(Ok(StreamEvent::Done)).await;
            Ok(Box::pin(ReceiverStream::new(rx)))
        }
    }

    fn ok_from(name: &str) -> Result<LLMResponse> {
        Ok(LLMResponse {
            content: format!("from-{name}"),
            model: name.into(),
            ..Default::default()
        })
    }

    fn server_error() -> Result<LLMResponse> {
        Err(KernelError::Http {
            status: 500,
            message: "boom".into(),
        })
    }

    // --- complete() behavior -------------------------------------------------

    #[tokio::test]
    async fn fallback_falls_through_to_secondary_on_error() {
        let primary = Backend::new(MockClient::new("primary", vec![server_error()]), None);
        let secondary = Backend::new(
            MockClient::new("secondary", vec![ok_from("secondary")]),
            None,
        );
        let router = RouterClient::new(vec![primary, secondary], RoutingStrategy::Fallback)
            .expect("non-empty");

        let resp = router
            .complete(LLMRequest::builder().build())
            .await
            .expect("secondary succeeds");
        assert_eq!(resp.content, "from-secondary");
    }

    #[tokio::test]
    async fn fallback_respects_priority_order() {
        // Insert "secondary" first, but give it a higher priority value so the
        // router must try the (later-inserted) primary first.
        let secondary = Backend::new(MockClient::new("secondary", vec![server_error()]), None)
            .with_priority(10);
        let primary = Backend::new(MockClient::new("primary", vec![ok_from("primary")]), None)
            .with_priority(1);

        let router = RouterClient::new(vec![secondary, primary], RoutingStrategy::Fallback)
            .expect("non-empty");
        let resp = router
            .complete(LLMRequest::builder().build())
            .await
            .expect("primary succeeds");
        assert_eq!(resp.content, "from-primary");
    }

    #[tokio::test]
    async fn lowest_cost_orders_by_cost() {
        // Insert expensive first; the router must still try cheap first.
        let expensive = Backend::new(
            MockClient::new("expensive", vec![server_error(), ok_from("expensive")]),
            Some((10.0, 30.0)),
        );
        let cheap = Backend::new(
            MockClient::new("cheap", vec![ok_from("cheap")]),
            Some((0.15, 0.60)),
        );
        let router = RouterClient::new(vec![expensive, cheap], RoutingStrategy::LowestCost)
            .expect("non-empty");

        let resp = router
            .complete(LLMRequest::builder().build())
            .await
            .expect("cheap succeeds first");
        assert_eq!(resp.content, "from-cheap");
    }

    #[tokio::test]
    async fn lowest_cost_falls_through_to_next_cheapest() {
        // Cheapest errors; the next-cheapest must be tried and succeed.
        let cheap = Backend::new(
            MockClient::new("cheap", vec![server_error()]),
            Some((0.15, 0.60)),
        );
        let mid = Backend::new(
            MockClient::new("mid", vec![ok_from("mid")]),
            Some((1.0, 2.0)),
        );
        let router =
            RouterClient::new(vec![mid, cheap], RoutingStrategy::LowestCost).expect("non-empty");

        let resp = router
            .complete(LLMRequest::builder().build())
            .await
            .expect("mid succeeds after cheap fails");
        assert_eq!(resp.content, "from-mid");
    }

    #[tokio::test]
    async fn unknown_cost_sorts_last() {
        // The `unknown` backend has no responses queued: if it is tried first,
        // MockClient panics. The finite-cost `priced` backend must precede it
        // in LowestCost order, succeed, and short-circuit before `unknown`.
        let unknown = Backend::new(MockClient::new("unknown", vec![]), None);
        let priced = Backend::new(
            MockClient::new("priced", vec![ok_from("priced")]),
            Some((1.0, 2.0)),
        );
        let router = RouterClient::new(vec![unknown, priced], RoutingStrategy::LowestCost)
            .expect("non-empty");
        let resp = router
            .complete(LLMRequest::builder().build())
            .await
            .expect("priced tried first");
        assert_eq!(resp.content, "from-priced");
    }

    #[tokio::test]
    async fn nan_cost_is_treated_as_unknown() {
        // A NaN cost must not corrupt the sort; it must sort last like `None`.
        let nan_cost = Backend::new(MockClient::new("nan", vec![]), Some((f64::NAN, 1.0)));
        let priced = Backend::new(
            MockClient::new("priced", vec![ok_from("priced")]),
            Some((1.0, 2.0)),
        );
        let router = RouterClient::new(vec![nan_cost, priced], RoutingStrategy::LowestCost)
            .expect("non-empty");
        let resp = router
            .complete(LLMRequest::builder().build())
            .await
            .expect("priced precedes NaN-cost backend");
        assert_eq!(resp.content, "from-priced");
    }

    #[tokio::test]
    async fn all_backends_fail_returns_last_error() {
        let primary = Backend::new(MockClient::new("primary", vec![server_error()]), None);
        let secondary = Backend::new(
            MockClient::new("secondary", vec![Err(KernelError::LlmApi("down".into()))]),
            None,
        );
        let router = RouterClient::new(vec![primary, secondary], RoutingStrategy::Fallback)
            .expect("non-empty");

        let err = router
            .complete(LLMRequest::builder().build())
            .await
            .expect_err("both fail");
        // Last-tried backend was "secondary" → LlmApi, not the primary's Http 500.
        assert!(matches!(err, KernelError::LlmApi(_)));
    }

    #[tokio::test]
    async fn first_success_short_circuits_remaining_backends() {
        // If the primary succeeds, the secondary must never be called — it has
        // no responses queued, so a stray call would panic.
        let primary = Backend::new(MockClient::new("primary", vec![ok_from("primary")]), None);
        let secondary = Backend::new(MockClient::new("secondary", vec![]), None);
        let router = RouterClient::new(vec![primary, secondary], RoutingStrategy::Fallback)
            .expect("non-empty");

        let resp = router
            .complete(LLMRequest::builder().build())
            .await
            .expect("primary succeeds");
        assert_eq!(resp.content, "from-primary");
    }

    // --- ordering invariants --------------------------------------------------

    #[tokio::test]
    async fn empty_backends_is_rejected() {
        let result = RouterClient::<MockClient>::new(vec![], RoutingStrategy::Fallback);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn ties_keep_insertion_order() {
        // Equal priority: insertion order must win. primary (inserted first)
        // succeeds; secondary (no responses) must not be tried.
        let primary = Backend::new(MockClient::new("primary", vec![ok_from("primary")]), None)
            .with_priority(5);
        let secondary = Backend::new(MockClient::new("secondary", vec![]), None).with_priority(5);
        let router = RouterClient::new(vec![primary, secondary], RoutingStrategy::Fallback)
            .expect("non-empty");
        let resp = router
            .complete(LLMRequest::builder().build())
            .await
            .expect("insertion-order primary first");
        assert_eq!(resp.content, "from-primary");
    }

    #[tokio::test]
    async fn equal_cost_keeps_insertion_order() {
        let first = Backend::new(
            MockClient::new("first", vec![ok_from("first")]),
            Some((1.0, 1.0)),
        );
        let second = Backend::new(MockClient::new("second", vec![]), Some((1.0, 1.0)));
        let router =
            RouterClient::new(vec![first, second], RoutingStrategy::LowestCost).expect("non-empty");
        let resp = router
            .complete(LLMRequest::builder().build())
            .await
            .expect("insertion-order first");
        assert_eq!(resp.content, "from-first");
    }

    // --- model_name / labels --------------------------------------------------

    #[tokio::test]
    async fn model_name_reflects_preferred_backend() {
        let primary = Backend::new(MockClient::new("primary", vec![ok_from("primary")]), None)
            .with_priority(1);
        let secondary = Backend::new(
            MockClient::new("secondary", vec![ok_from("secondary")]),
            None,
        )
        .with_priority(5);
        let router = RouterClient::new(vec![secondary, primary], RoutingStrategy::Fallback)
            .expect("non-empty");
        assert_eq!(router.model_name(), "primary");
    }

    #[tokio::test]
    async fn with_label_overrides_model_name() {
        let backend = Backend::new(MockClient::new("gpt-4o", vec![ok_from("gpt-4o")]), None)
            .with_label("fallback-alias");
        let router =
            RouterClient::new(vec![backend], RoutingStrategy::Fallback).expect("non-empty");
        assert_eq!(router.model_name(), "fallback-alias");
    }

    // --- stream delegation ----------------------------------------------------

    #[tokio::test]
    async fn stream_delegates_to_preferred_backend_only() {
        let primary_mock = MockClient::new("primary", vec![]);
        let primary_calls = primary_mock.stream_call_count();
        let secondary_mock = MockClient::new("secondary", vec![]);
        let secondary_calls = secondary_mock.stream_call_count();

        let primary = Backend::new(primary_mock, None).with_priority(1);
        let secondary = Backend::new(secondary_mock, None).with_priority(5);
        let router = RouterClient::new(vec![secondary, primary], RoutingStrategy::Fallback)
            .expect("non-empty");

        let stream = router
            .stream_complete(LLMRequest::builder().build())
            .await
            .expect("preferred backend streams");
        drop(stream); // drop the stream; we only assert delegation.

        assert_eq!(
            primary_calls.load(Ordering::Relaxed),
            1,
            "preferred streamed once"
        );
        assert_eq!(
            secondary_calls.load(Ordering::Relaxed),
            0,
            "secondary not contacted"
        );
    }
}
