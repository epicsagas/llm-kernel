//! Middleware hooks for [`LLMClient`] request/response lifecycle.
//!
//! Provides an [`LLMClientMiddleware`] trait with hooks that fire before
//! each request, after each successful response, and on each error.
//!
//! The [`MiddlewareClient`] wrapper composes with any [`LLMClient`],
//! including [`RetryClient`](crate::llm::retry::RetryClient).
//!
//! # Example
//!
//! ```ignore
//! use llm_kernel::llm::{LLMClient, MiddlewareClient, NoopMiddleware};
//!
//! let client = OpenAIClient::from_key("gpt-4o", "sk-...");
//! let middleware = NoopMiddleware;
//! let wrapped = MiddlewareClient::new(client, middleware);
//! let response = wrapped.complete(request).await?;
//! ```

use async_trait::async_trait;

use crate::error::{KernelError, Result};
use crate::llm::client::LLMClient;
use crate::llm::types::{LLMRequest, LLMResponse, LLMStream};

/// Hook trait for intercepting [`LLMClient`] request/response cycles.
///
/// All methods have default no-op implementations. Override only the hooks
/// you need. Each hook receives an immutable reference, so middleware cannot
/// mutate requests or responses — only observe.
///
/// The trait is object-safe: `Box<dyn LLMClientMiddleware>` is usable.
#[async_trait]
pub trait LLMClientMiddleware: Send + Sync {
    /// Called before each `complete` request is sent.
    ///
    /// Use for logging, metrics, or request tracing.
    async fn on_request(&self, _request: &LLMRequest) {}

    /// Called after a successful `complete` response.
    ///
    /// Use for logging, metrics, or response tracing.
    async fn on_response(&self, _request: &LLMRequest, _response: &LLMResponse) {}

    /// Called when `complete` returns an error.
    ///
    /// Use for error logging, alerting, or metrics.
    async fn on_error(&self, _request: &LLMRequest, _error: &KernelError) {}
}

/// A default no-op middleware. Useful as a type parameter default.
pub struct NoopMiddleware;

#[async_trait]
impl LLMClientMiddleware for NoopMiddleware {
    // All methods use default no-op implementations.
}

/// An [`LLMClient`] wrapper that calls [`LLMClientMiddleware`] hooks around
/// each request.
///
/// Composable: `MiddlewareClient<RetryClient<OpenAIClient>, LogMiddleware>`.
pub struct MiddlewareClient<C, M> {
    inner: C,
    middleware: M,
}

impl<C, M> MiddlewareClient<C, M> {
    /// Create a new middleware-wrapped client.
    pub fn new(inner: C, middleware: M) -> Self {
        Self { inner, middleware }
    }

    /// Access the underlying client.
    pub fn inner(&self) -> &C {
        &self.inner
    }
}

#[async_trait]
impl<C: LLMClient, M: LLMClientMiddleware> LLMClient for MiddlewareClient<C, M> {
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse> {
        self.middleware.on_request(&request).await;
        match self.inner.complete(request.clone()).await {
            Ok(response) => {
                self.middleware.on_response(&request, &response).await;
                Ok(response)
            }
            Err(err) => {
                self.middleware.on_error(&request, &err).await;
                Err(err)
            }
        }
    }

    fn model_name(&self) -> &str {
        self.inner.model_name()
    }

    async fn stream_complete(&self, request: LLMRequest) -> Result<LLMStream> {
        // Streaming does not invoke middleware hooks — the stream is opaque
        // and errors arrive asynchronously in the stream events.
        self.inner.stream_complete(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A middleware that records hook invocations.
    #[derive(Default)]
    struct RecordingMiddleware {
        on_request_called: Arc<Mutex<bool>>,
        on_response_called: Arc<Mutex<bool>>,
        on_error_called: Arc<Mutex<bool>>,
    }

    #[async_trait]
    impl LLMClientMiddleware for RecordingMiddleware {
        async fn on_request(&self, _request: &LLMRequest) {
            *self.on_request_called.lock().unwrap() = true;
        }

        async fn on_response(&self, _request: &LLMRequest, _response: &LLMResponse) {
            *self.on_response_called.lock().unwrap() = true;
        }

        async fn on_error(&self, _request: &LLMRequest, _error: &KernelError) {
            *self.on_error_called.lock().unwrap() = true;
        }
    }

    /// A mock client that returns a fixed response or error.
    struct MockClient {
        response: std::sync::Mutex<Option<Result<LLMResponse>>>,
    }

    impl MockClient {
        fn ok() -> Self {
            Self {
                response: std::sync::Mutex::new(Some(Ok(LLMResponse {
                    content: "hello".into(),
                    model: "mock".into(),
                    ..Default::default()
                }))),
            }
        }

        fn err() -> Self {
            Self {
                response: std::sync::Mutex::new(Some(Err(KernelError::LlmApi("fail".into())))),
            }
        }
    }

    #[async_trait]
    impl LLMClient for MockClient {
        async fn complete(&self, _request: LLMRequest) -> Result<LLMResponse> {
            self.response.lock().unwrap().take().unwrap()
        }

        fn model_name(&self) -> &str {
            "mock"
        }

        async fn stream_complete(&self, _request: LLMRequest) -> Result<LLMStream> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn middleware_calls_on_request_and_on_response_on_success() {
        let mid = RecordingMiddleware::default();
        let req_called = mid.on_request_called.clone();
        let res_called = mid.on_response_called.clone();

        let client = MiddlewareClient::new(MockClient::ok(), mid);
        let result = client.complete(LLMRequest::builder().build()).await;

        assert!(result.is_ok());
        assert!(*req_called.lock().unwrap());
        assert!(*res_called.lock().unwrap());
    }

    #[tokio::test]
    async fn middleware_calls_on_error_on_failure() {
        let mid = RecordingMiddleware::default();
        let err_called = mid.on_error_called.clone();

        let client = MiddlewareClient::new(MockClient::err(), mid);
        let result = client.complete(LLMRequest::builder().build()).await;

        assert!(result.is_err());
        assert!(*err_called.lock().unwrap());
    }

    #[tokio::test]
    async fn middleware_delegates_model_name() {
        let client = MiddlewareClient::new(MockClient::ok(), NoopMiddleware);
        assert_eq!(client.model_name(), "mock");
    }

    #[tokio::test]
    async fn noop_middleware_compiles_and_works() {
        let client = MiddlewareClient::new(MockClient::ok(), NoopMiddleware);
        let result = client.complete(LLMRequest::builder().build()).await;
        assert!(result.is_ok());
    }
}
