//! Exponential backoff retry wrapper for [`LLMClient`].
//!
//! Wraps any [`LLMClient`] implementation with configurable retry logic.
//! Retries on rate-limit (429) and server errors (5xx) with exponential
//! backoff and jitter to avoid thundering-herd behavior.
//!
//! Streaming is **not** retried — the caller receives the raw stream from
//! the inner client. Streaming retry is complex and left for a future version.
//!
//! # Example
//!
//! ```ignore
//! use llm_kernel::llm::{LLMClient, OpenAIClient, RetryClient, RetryConfig};
//! use std::time::Duration;
//!
//! let client = OpenAIClient::from_key("gpt-4o", "sk-...");
//! let retry = RetryClient::new(client, RetryConfig {
//!     max_retries: 3,
//!     base_delay: Duration::from_secs(1),
//! });
//! let response = retry.complete(request).await?;
//! ```

use std::time::Duration;

use async_trait::async_trait;

use crate::error::{KernelError, Result};
use crate::llm::client::LLMClient;
use crate::llm::types::{LLMRequest, LLMResponse, LLMStream};

/// Configuration for retry behavior.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts (0 = no retry).
    pub max_retries: u32,
    /// Base delay for exponential backoff.
    pub base_delay: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_secs(1),
        }
    }
}

/// An [`LLMClient`] wrapper that retries on transient errors.
///
/// Retries are attempted for:
/// - **429 Rate Limited** — uses the `retry-after` header value as the initial delay.
/// - **5xx Server Errors** — exponential backoff starting from `base_delay`.
///
/// All other errors (4xx, network timeouts, deserialization failures) are
/// returned immediately without retry.
pub struct RetryClient<C> {
    inner: C,
    config: RetryConfig,
}

impl<C> RetryClient<C> {
    /// Wrap an [`LLMClient`] with retry behavior.
    pub fn new(inner: C, config: RetryConfig) -> Self {
        Self { inner, config }
    }

    /// Access the underlying client.
    pub fn inner(&self) -> &C {
        &self.inner
    }
}

/// Determine whether an error is retryable and extract the suggested delay.
///
/// Returns `None` for non-retryable errors.
fn retry_delay(err: &KernelError, attempt: u32, base_delay: Duration) -> Option<Duration> {
    match err {
        KernelError::RateLimited(secs) => {
            // Use the server's retry-after hint, but still apply backoff scaling
            let server_delay = Duration::from_secs(*secs);
            Some(std::cmp::max(
                server_delay,
                backoff_with_jitter(attempt, base_delay),
            ))
        }
        KernelError::LlmApi(msg) if is_server_error(msg) => {
            Some(backoff_with_jitter(attempt, base_delay))
        }
        _ => None,
    }
}

/// Check if an `LlmApi` error string indicates a server error (5xx).
fn is_server_error(msg: &str) -> bool {
    // Error format from client.rs: "HTTP {status}: {text}"
    msg.as_bytes().get(5).is_some_and(|&b| b == b'5') && msg.starts_with("HTTP ")
}

/// Compute exponential backoff with jitter.
///
/// Delay = `base_delay * 2^attempt`, capped at 60 seconds, with a ±50% jitter
/// using a deterministic hash of the attempt number (no external RNG dependency).
fn backoff_with_jitter(attempt: u32, base_delay: Duration) -> Duration {
    let base_ms = base_delay.as_millis() as u64;
    // Exponential: base * 2^attempt, capped at 60s
    let exp_ms = base_ms.saturating_mul(1u64 << attempt.min(6));
    let capped_ms = exp_ms.min(60_000);
    // Jitter: ±50% using Knuth's multiplicative hash for pseudo-randomness
    let jitter_hash = (attempt as u64).wrapping_mul(2654435769) % 1000;
    let jittered_ms = capped_ms * (500 + jitter_hash) / 1000;
    Duration::from_millis(jittered_ms)
}

#[async_trait]
impl<C: LLMClient> LLMClient for RetryClient<C> {
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse> {
        let mut last_err = None;

        for attempt in 0..=self.config.max_retries {
            match self.inner.complete(request.clone()).await {
                Ok(response) => return Ok(response),
                Err(err) => {
                    // Don't retry if we've exhausted attempts
                    if attempt >= self.config.max_retries {
                        return Err(err);
                    }

                    // Check if error is retryable
                    let Some(delay) = retry_delay(&err, attempt, self.config.base_delay) else {
                        return Err(err);
                    };

                    last_err = Some(err);
                    tokio::time::sleep(delay).await;
                }
            }
        }

        // Should be unreachable, but just in case
        Err(last_err.unwrap())
    }

    fn model_name(&self) -> &str {
        self.inner.model_name()
    }

    /// Streaming is **not** retried. Delegates directly to the inner client.
    async fn stream_complete(&self, request: LLMRequest) -> Result<LLMStream> {
        self.inner.stream_complete(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    #[test]
    fn is_server_error_detects_5xx() {
        assert!(is_server_error("HTTP 500: internal server error"));
        assert!(is_server_error("HTTP 502: bad gateway"));
        assert!(is_server_error("HTTP 503: service unavailable"));
    }

    #[test]
    fn is_server_error_rejects_4xx() {
        assert!(!is_server_error("HTTP 400: bad request"));
        assert!(!is_server_error("HTTP 401: unauthorized"));
        assert!(!is_server_error("HTTP 429: too many requests"));
    }

    #[test]
    fn is_server_error_rejects_malformed() {
        assert!(!is_server_error("connection refused"));
        assert!(!is_server_error(""));
    }

    #[test]
    fn backoff_increases_with_attempts() {
        let base = Duration::from_secs(1);
        let d0 = backoff_with_jitter(0, base);
        let d1 = backoff_with_jitter(1, base);
        let d2 = backoff_with_jitter(2, base);

        // Each attempt roughly doubles (with jitter)
        assert!(d0.as_millis() > 0);
        assert!(d1.as_millis() > d0.as_millis() / 2);
        assert!(d2.as_millis() > d1.as_millis() / 2);
    }

    #[test]
    fn backoff_capped_at_60s() {
        let base = Duration::from_secs(10);
        let d = backoff_with_jitter(20, base);
        assert!(d <= Duration::from_secs(60));
    }

    #[test]
    fn retry_delay_rate_limited_uses_server_hint() {
        let err = KernelError::RateLimited(30);
        let delay = retry_delay(&err, 0, Duration::from_secs(1));
        assert!(delay.is_some());
        // Should be at least 30s (server hint) or backoff, whichever is larger
        assert!(delay.unwrap() >= Duration::from_secs(30));
    }

    #[test]
    fn retry_delay_server_error_returns_backoff() {
        let err = KernelError::LlmApi("HTTP 500: error".into());
        let delay = retry_delay(&err, 0, Duration::from_secs(1));
        assert!(delay.is_some());
    }

    #[test]
    fn retry_delay_client_error_returns_none() {
        let err = KernelError::LlmApi("HTTP 400: bad request".into());
        let delay = retry_delay(&err, 0, Duration::from_secs(1));
        assert!(delay.is_none());
    }

    #[test]
    fn retry_delay_config_error_returns_none() {
        let err = KernelError::Config("missing key".into());
        let delay = retry_delay(&err, 0, Duration::from_secs(1));
        assert!(delay.is_none());
    }

    /// A mock client that fails N times then succeeds.
    struct MockClient {
        fail_count: AtomicU32,
        responses: std::sync::Mutex<Vec<Result<LLMResponse>>>,
    }

    impl MockClient {
        fn new(responses: Vec<Result<LLMResponse>>) -> Self {
            Self {
                fail_count: AtomicU32::new(0),
                responses: std::sync::Mutex::new(responses),
            }
        }
    }

    #[async_trait]
    impl LLMClient for MockClient {
        async fn complete(&self, _request: LLMRequest) -> Result<LLMResponse> {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                panic!("MockClient: no more responses");
            }
            let result = responses.remove(0);
            if result.is_err() {
                self.fail_count
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            result
        }

        fn model_name(&self) -> &str {
            "mock"
        }

        async fn stream_complete(&self, _request: LLMRequest) -> Result<LLMStream> {
            unimplemented!()
        }
    }

    fn ok_response() -> Result<LLMResponse> {
        Ok(LLMResponse {
            content: "ok".into(),
            model: "mock".into(),
            usage: Default::default(),
            finish_reason: None,
            id: None,
            created: None,
        })
    }

    fn server_error() -> Result<LLMResponse> {
        Err(KernelError::LlmApi("HTTP 500: error".into()))
    }

    fn rate_limited() -> Result<LLMResponse> {
        Err(KernelError::RateLimited(0))
    }

    fn client_error() -> Result<LLMResponse> {
        Err(KernelError::LlmApi("HTTP 400: bad request".into()))
    }

    #[tokio::test]
    async fn retry_succeeds_after_transient_failures() {
        let mock = MockClient::new(vec![server_error(), server_error(), ok_response()]);
        let retry = RetryClient::new(
            mock,
            RetryConfig {
                max_retries: 3,
                base_delay: Duration::from_millis(1),
            },
        );

        let result = retry.complete(LLMRequest::builder().build()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().content, "ok");
    }

    #[tokio::test]
    async fn retry_exhausted_returns_last_error() {
        let mock = MockClient::new(vec![
            server_error(),
            server_error(),
            server_error(),
            server_error(), // 4th failure = max_retries + 1
        ]);
        let retry = RetryClient::new(
            mock,
            RetryConfig {
                max_retries: 3,
                base_delay: Duration::from_millis(1),
            },
        );

        let result = retry.complete(LLMRequest::builder().build()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn retry_does_not_retry_client_error() {
        let mock = MockClient::new(vec![client_error()]);
        let retry = RetryClient::new(
            mock,
            RetryConfig {
                max_retries: 3,
                base_delay: Duration::from_millis(1),
            },
        );

        let result = retry.complete(LLMRequest::builder().build()).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, KernelError::LlmApi(m) if m.contains("400")));
    }

    #[tokio::test]
    async fn retry_handles_rate_limit_with_backoff() {
        let mock = MockClient::new(vec![rate_limited(), ok_response()]);
        let retry = RetryClient::new(
            mock,
            RetryConfig {
                max_retries: 3,
                base_delay: Duration::from_millis(1),
            },
        );

        let result = retry.complete(LLMRequest::builder().build()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn retry_zero_retries_returns_immediately() {
        let mock = MockClient::new(vec![server_error()]);
        let retry = RetryClient::new(
            mock,
            RetryConfig {
                max_retries: 0,
                base_delay: Duration::from_millis(1),
            },
        );

        let result = retry.complete(LLMRequest::builder().build()).await;
        assert!(result.is_err());
    }

    #[test]
    fn default_config() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.base_delay, Duration::from_secs(1));
    }
}
