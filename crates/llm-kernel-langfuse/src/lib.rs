//! Langfuse observability adapter for [`llm_kernel`].
//!
//! Thin [`LLMClientMiddleware`] that ships each `complete()` call to a
//! Langfuse server as a `generation-create` ingestion event. The kernel
//! stays vendor-free; this crate is the only place that knows Langfuse
//! exists.
//!
//! # Example
//!
//! ```no_run
//! use llm_kernel::llm::{LLMClient, LLMRequest, MiddlewareClient, OpenAIClient};
//! use llm_kernel_langfuse::{LangfuseConfig, LangfuseMiddleware};
//!
//! # async fn demo() -> llm_kernel::error::Result<()> {
//! let config = LangfuseConfig::from_env().expect("LANGFUSE_PUBLIC_KEY / LANGFUSE_SECRET_KEY set");
//! let client = OpenAIClient::from_key("gpt-4o", "sk-...")?;
//! let observed = MiddlewareClient::new(client, LangfuseMiddleware::new(config));
//! let response = observed
//!     .complete(LLMRequest::builder().user_message("hello").build())
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Scope
//!
//! - Only the non-streaming `complete()` path emits events —
//!   `MiddlewareClient::stream_complete` does not fire middleware hooks
//!   (kernel limitation).
//! - One HTTP POST per call, sent inline from the hook with a 5s timeout.
//!   // ponytail: per-call POST; switch to batched background flush when volume matters
//! - Latency is not reported: hooks are `&self` and requests carry no id,
//!   so correlating `on_request` start times with responses would need
//!   keyed mutable state.
//! - Ingestion failures are logged via `tracing::warn!` and never
//!   propagate to the LLM call.

use std::time::Duration;

use async_trait::async_trait;
use llm_kernel::error::KernelError;
use llm_kernel::llm::{LLMClientMiddleware, LLMRequest, LLMResponse};
use serde::Serialize;
use serde_json::json;

/// Connection and capture settings for a Langfuse server.
///
/// Manual [`Debug`] keeps `secret_key` out of logs (kernel convention —
/// see `OpenAIClient`, which does not derive `Debug` for the same reason).
#[derive(Clone)]
pub struct LangfuseConfig {
    /// Base URL of the Langfuse instance. Use `https://` — a plain-`http`
    /// host sends keys and prompts in cleartext.
    pub host: String,
    /// Langfuse public API key.
    pub public_key: String,
    /// Langfuse secret API key.
    pub secret_key: String,
    /// Include prompts (`input`) and completions (`output`) in events.
    /// Set to `false` for PII-sensitive deployments — events then carry
    /// only model, usage, and outcome metadata.
    pub capture_io: bool,
    /// Attach a session id to every event for conversation grouping.
    pub session_id: Option<String>,
}

impl std::fmt::Debug for LangfuseConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LangfuseConfig")
            .field("host", &self.host)
            .field("public_key", &self.public_key)
            .field("secret_key", &"<REDACTED>")
            .field("capture_io", &self.capture_io)
            .field("session_id", &self.session_id)
            .finish()
    }
}

impl LangfuseConfig {
    /// Cloud-hosted default config with the given keys.
    pub fn new(public_key: impl Into<String>, secret_key: impl Into<String>) -> Self {
        Self {
            host: "https://cloud.langfuse.com".to_string(),
            public_key: public_key.into(),
            secret_key: secret_key.into(),
            capture_io: true,
            session_id: None,
        }
    }

    /// Build a config from `LANGFUSE_PUBLIC_KEY` / `LANGFUSE_SECRET_KEY`
    /// (plus optional `LANGFUSE_HOST`). Returns `None` when either key is
    /// missing or empty.
    pub fn from_env() -> Option<Self> {
        let public_key = std::env::var("LANGFUSE_PUBLIC_KEY").ok()?;
        let secret_key = std::env::var("LANGFUSE_SECRET_KEY").ok()?;
        if public_key.is_empty() || secret_key.is_empty() {
            return None;
        }
        let mut config = Self::new(public_key, secret_key);
        if let Ok(host) = std::env::var("LANGFUSE_HOST")
            && !host.is_empty()
        {
            config.host = host;
        }
        Some(config)
    }
}

/// [`LLMClientMiddleware`] that reports completions to Langfuse.
pub struct LangfuseMiddleware {
    config: LangfuseConfig,
    http: reqwest::Client,
}

impl LangfuseMiddleware {
    /// Create a middleware. The HTTP client uses a 5s timeout so a slow
    /// Langfuse endpoint cannot stall the LLM call beyond that.
    pub fn new(config: LangfuseConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        Self { config, http }
    }

    async fn send(&self, body: GenerationBody) {
        let batch = IngestionBatch {
            batch: vec![BatchEvent {
                id: uuid::Uuid::new_v4().to_string(),
                r#type: "generation-create",
                timestamp: rfc3339_now(),
                body,
            }],
        };
        let url = format!(
            "{}/api/public/ingestion",
            self.config.host.trim_end_matches('/')
        );
        let request = self
            .http
            .post(&url)
            .basic_auth(&self.config.public_key, Some(&self.config.secret_key))
            .json(&batch);
        match request.send().await {
            Ok(response) if response.status().is_success() => {}
            Ok(response) => {
                tracing::warn!(status = %response.status(), "langfuse ingestion rejected event");
            }
            Err(error) => {
                tracing::warn!(%error, "langfuse ingestion request failed");
            }
        }
    }
}

#[async_trait]
impl LLMClientMiddleware for LangfuseMiddleware {
    async fn on_response(&self, request: &LLMRequest, response: &LLMResponse) {
        self.send(success_body(request, response, &self.config))
            .await;
    }

    async fn on_error(&self, request: &LLMRequest, error: &KernelError) {
        self.send(error_body(request, error, &self.config)).await;
    }
}

fn rfc3339_now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn success_body(
    request: &LLMRequest,
    response: &LLMResponse,
    config: &LangfuseConfig,
) -> GenerationBody {
    GenerationBody {
        id: uuid::Uuid::new_v4().to_string(),
        trace_id: uuid::Uuid::new_v4().to_string(),
        name: "llm-kernel.complete",
        model: response.model.clone(),
        input: capture(config, || serde_json::to_value(request).unwrap_or_default()),
        output: capture(config, || {
            json!({
                "content": response.content,
                "reasoning": response.reasoning,
                "toolCalls": response.tool_calls,
            })
        }),
        usage: Some(UsagePayload {
            prompt_tokens: response.usage.prompt_tokens,
            completion_tokens: response.usage.completion_tokens,
            total_tokens: response.usage.total_tokens,
            reasoning_tokens: response.usage.reasoning_tokens,
        }),
        level: "DEFAULT",
        status_message: None,
        session_id: config.session_id.clone(),
        metadata: json!({
            "success": true,
            "finishReason": response.finish_reason,
        }),
    }
}

fn error_body(
    request: &LLMRequest,
    error: &KernelError,
    config: &LangfuseConfig,
) -> GenerationBody {
    GenerationBody {
        id: uuid::Uuid::new_v4().to_string(),
        trace_id: uuid::Uuid::new_v4().to_string(),
        name: "llm-kernel.complete",
        model: request
            .model
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        input: capture(config, || serde_json::to_value(request).unwrap_or_default()),
        output: None,
        usage: None,
        level: "ERROR",
        // Provider error bodies can echo request credentials back (see the
        // kernel's own `redact_http_body` rationale) — mask before shipping.
        status_message: Some(llm_kernel::safety::mask_secrets(&error.to_string())),
        session_id: config.session_id.clone(),
        metadata: json!({
            "success": false,
            "finishReason": null,
        }),
    }
}

fn capture(
    config: &LangfuseConfig,
    value: impl FnOnce() -> serde_json::Value,
) -> Option<serde_json::Value> {
    if config.capture_io {
        Some(value())
    } else {
        None
    }
}

#[derive(Serialize)]
struct IngestionBatch {
    batch: Vec<BatchEvent>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchEvent {
    id: String,
    r#type: &'static str,
    timestamp: String,
    body: GenerationBody,
}

/// A Langfuse `generation-create` body. `traceId` is always fresh; Langfuse
/// auto-creates the trace on ingestion.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationBody {
    id: String,
    trace_id: String,
    name: &'static str,
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    input: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<UsagePayload>,
    level: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    status_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    metadata: serde_json::Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UsagePayload {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_tokens: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm_kernel::llm::{LLMClient, LLMStream, MiddlewareClient, TokenUsage};

    fn config(capture_io: bool) -> LangfuseConfig {
        LangfuseConfig {
            capture_io,
            ..LangfuseConfig::new("pk-test", "sk-test")
        }
    }

    fn sample_request() -> LLMRequest {
        LLMRequest::builder()
            .user_message("secret question")
            .build()
    }

    fn sample_response() -> LLMResponse {
        LLMResponse {
            content: "the answer".to_string(),
            reasoning: None,
            model: "mock-model".to_string(),
            usage: TokenUsage {
                prompt_tokens: 11,
                completion_tokens: 7,
                total_tokens: 18,
                reasoning_tokens: Some(3),
            },
            tool_calls: Vec::new(),
            finish_reason: Some("stop".to_string()),
            id: None,
            created: None,
        }
    }

    #[test]
    fn success_body_maps_usage_and_model() {
        let body = success_body(&sample_request(), &sample_response(), &config(true));
        assert_eq!(body.model, "mock-model");
        assert!(!body.trace_id.is_empty());
        assert_eq!(body.level, "DEFAULT");
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["usage"]["promptTokens"], 11);
        assert_eq!(json["usage"]["reasoningTokens"], 3);
    }

    #[test]
    fn capture_off_hides_prompts_and_output() {
        let body = success_body(&sample_request(), &sample_response(), &config(false));
        let json = serde_json::to_value(&body).unwrap();
        assert!(json.get("input").is_none());
        assert!(json.get("output").is_none());
        // usage still reported
        assert_eq!(json["usage"]["totalTokens"], 18);
    }

    #[test]
    fn capture_on_includes_prompt_text() {
        let body = success_body(&sample_request(), &sample_response(), &config(true));
        let json = serde_json::to_value(&body).unwrap();
        assert!(json["input"].to_string().contains("secret question"));
        assert_eq!(json["output"]["content"], "the answer");
    }

    #[test]
    fn error_body_marks_level_and_message() {
        let error = KernelError::LlmApi("boom".to_string());
        let body = error_body(&sample_request(), &error, &config(true));
        assert_eq!(body.level, "ERROR");
        assert_eq!(body.status_message.as_deref(), Some("LLM API error: boom"));
        assert_eq!(body.model, "unknown");
        assert!(body.usage.is_none());
    }

    #[test]
    fn error_body_masks_secrets_echoed_by_gateway() {
        // Regression: gateways can echo the request Authorization header in
        // error bodies; the status message must not leak it to Langfuse.
        let error = KernelError::Http {
            status: 401,
            message: "echoed Authorization: Bearer sk-live-abcdef123".to_string(),
        };
        let body = error_body(&sample_request(), &error, &config(false));
        let message = body.status_message.expect("message present");
        assert!(!message.contains("sk-live-abcdef123"), "leaked: {message}");
        assert!(message.contains("****"));
    }

    #[test]
    fn session_id_propagates() {
        let mut cfg = config(true);
        cfg.session_id = Some("s-1".to_string());
        let body = success_body(&sample_request(), &sample_response(), &cfg);
        assert_eq!(body.session_id.as_deref(), Some("s-1"));
    }

    // --- integration: wiremock -------------------------------------------

    struct MockOk;
    struct MockErr;

    #[async_trait]
    impl LLMClient for MockOk {
        async fn complete(&self, _request: LLMRequest) -> llm_kernel::error::Result<LLMResponse> {
            Ok(sample_response())
        }
        fn model_name(&self) -> &str {
            "mock-model"
        }
        async fn stream_complete(
            &self,
            _request: LLMRequest,
        ) -> llm_kernel::error::Result<LLMStream> {
            Ok(Box::pin(tokio_stream::iter(Vec::new())))
        }
    }

    #[async_trait]
    impl LLMClient for MockErr {
        async fn complete(&self, _request: LLMRequest) -> llm_kernel::error::Result<LLMResponse> {
            Err(KernelError::LlmApi("boom".to_string()))
        }
        fn model_name(&self) -> &str {
            "mock-model"
        }
        async fn stream_complete(
            &self,
            _request: LLMRequest,
        ) -> llm_kernel::error::Result<LLMStream> {
            Ok(Box::pin(tokio_stream::iter(Vec::new())))
        }
    }

    #[tokio::test]
    async fn complete_posts_one_generation_event() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let mut cfg = config(true);
        cfg.host = server.uri();
        let client = MiddlewareClient::new(MockOk, LangfuseMiddleware::new(cfg));
        client.complete(sample_request()).await.unwrap();

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let post = &requests[0];
        assert!(post.url.as_str().ends_with("/api/public/ingestion"));
        // base64("pk-test:sk-test"), precomputed — this crate carries no base64 dep
        assert_eq!(
            post.headers
                .get("authorization")
                .expect("authorization header")
                .to_str()
                .unwrap(),
            "Basic cGstdGVzdDpzay10ZXN0"
        );

        let body: serde_json::Value = serde_json::from_slice(&post.body).unwrap();
        assert_eq!(body["batch"].as_array().map(Vec::len), Some(1));
        let event = &body["batch"][0];
        assert_eq!(event["type"], "generation-create");
        let body_obj = &event["body"];
        assert_eq!(body_obj["model"], "mock-model");
        assert_eq!(body_obj["usage"]["promptTokens"], 11);
        assert!(body_obj["traceId"].as_str().is_some_and(|t| !t.is_empty()));
        assert_eq!(body_obj["output"]["content"], "the answer");
    }

    #[tokio::test]
    async fn error_path_posts_error_event() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let mut cfg = config(true);
        cfg.host = server.uri();
        let client = MiddlewareClient::new(MockErr, LangfuseMiddleware::new(cfg));
        let result = client.complete(sample_request()).await;
        assert!(result.is_err());

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body["batch"][0]["body"]["level"], "ERROR");
        assert_eq!(
            body["batch"][0]["body"]["statusMessage"],
            "LLM API error: boom"
        );
    }

    #[tokio::test]
    async fn ingestion_failure_does_not_break_the_call() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let mut cfg = config(true);
        cfg.host = server.uri();
        let client = MiddlewareClient::new(MockOk, LangfuseMiddleware::new(cfg));
        let response = client.complete(sample_request()).await;
        assert!(response.is_ok());
    }
}
