//! Langfuse observability adapter for [`llm_kernel`].
//!
//! Thin [`LLMClientMiddleware`] that reports each non-streaming
//! `complete()` call to Langfuse as a `generation` span, exported via
//! OpenTelemetry OTLP/JSON to `{host}/api/public/otel/v1/traces` — the
//! ingestion path Langfuse recommends for languages without an official
//! SDK (the legacy `POST /api/public/ingestion` API is deprecated).
//!
//! The kernel stays vendor-free; this crate is the only place that knows
//! Langfuse exists.
//!
//! # Example
//!
//! ```no_run
//! use llm_kernel::llm::{LLMClient, LLMRequest, MiddlewareClient, OpenAIClient};
//! use llm_kernel_langfuse::{LangfuseConfig, LangfuseMiddleware};
//!
//! # async fn demo() -> llm_kernel::error::Result<()> {
//! let config = LangfuseConfig::from_env().expect("LANGFUSE_PUBLIC_KEY / LANGFUSE_SECRET_KEY set");
//! let middleware = LangfuseMiddleware::new(config);
//! let client = OpenAIClient::from_key("gpt-4o", "sk-...")?;
//! let observed = MiddlewareClient::new(client, middleware.clone());
//! let response = observed
//!     .complete(LLMRequest::builder().user_message("hello").build())
//!     .await?;
//! // Flush pending spans on shutdown (SIGTERM) — the last batch is lost
//! // otherwise. opentelemetry 0.32 has no global shutdown helper.
//! middleware.shutdown();
//! # Ok(())
//! # }
//! ```
//!
//! # Design notes
//!
//! - Spans are queued to a [`BatchSpanProcessor`]-backed provider that
//!   exports from a dedicated thread — the LLM call path never performs
//!   network I/O in the hooks. The exporter therefore uses the *blocking*
//!   reqwest client: the batch thread drives export via `block_on`, and an
//!   async client panics inside it (traces then vanish silently).
//! - The `x-langfuse-ingestion-version: 4` header is always sent —
//!   without it Langfuse delays directly-ingested OTel data by up to
//!   10 minutes.
//! - [`LangfuseConfig::capture_io`] gates only the
//!   `langfuse.observation.input`/`.output` attributes. Model, usage,
//!   level, and metadata are always reported so cost and failure
//!   dashboards stay alive with the kill switch on.
//! - Only the non-streaming `complete()` path emits events —
//!   `MiddlewareClient::stream_complete` does not fire middleware hooks
//!   (kernel limitation).
//! - Per-call observability: `LLMRequest::observability` (kernel 0.31+)
//!   drives the span name (`name`), parent trace (`traceparent`, W3C),
//!   session (`session_id`, overriding the config default), tags, and
//!   metadata; the `elapsed` hook parameter sets real span timing.
//! - Error `status_message` is masked via
//!   [`llm_kernel::safety::mask_secrets`] — provider error bodies can echo
//!   request credentials back.
//!
//! [`BatchSpanProcessor`]: opentelemetry_sdk::trace::BatchSpanProcessor

use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use llm_kernel::error::KernelError;
use llm_kernel::llm::{LLMClientMiddleware, LLMRequest, LLMResponse};
use opentelemetry::KeyValue;
use opentelemetry::propagation::TextMapPropagator;
use opentelemetry::trace::{Span, SpanBuilder, TracerProvider};
use opentelemetry_otlp::{Protocol, SpanExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;

/// Observation name reported to Langfuse (verb-first, model-free — model is
/// a separate generation attribute and must not appear in the name).
const OBSERVATION_NAME: &str = "llm-kernel.complete";

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
    /// Include prompts (`input`) and completions (`output`) in spans.
    /// Set to `false` for PII-sensitive deployments — model, usage, level,
    /// and metadata are still reported.
    pub capture_io: bool,
    /// Attach a session id to every span for conversation grouping.
    pub session_id: Option<String>,
    /// Deployment environment (`langfuse.environment`, e.g. `production`).
    /// Set it — without it local/CI traces pollute production dashboards.
    pub environment: Option<String>,
    /// Release/version tag (`langfuse.release`).
    pub release: Option<String>,
    /// Static tags attached to every trace (`langfuse.trace.tags`).
    pub tags: Vec<String>,
}

impl std::fmt::Debug for LangfuseConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LangfuseConfig")
            .field("host", &self.host)
            .field("public_key", &self.public_key)
            .field("secret_key", &"<REDACTED>")
            .field("capture_io", &self.capture_io)
            .field("session_id", &self.session_id)
            .field("environment", &self.environment)
            .field("release", &self.release)
            .field("tags", &self.tags)
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
            environment: None,
            release: None,
            tags: Vec::new(),
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
        if let Ok(environment) = std::env::var("LANGFUSE_TRACING_ENVIRONMENT")
            && !environment.is_empty()
        {
            config.environment = Some(environment);
        }
        if let Ok(release) = std::env::var("LANGFUSE_RELEASE")
            && !release.is_empty()
        {
            config.release = Some(release);
        }
        Some(config)
    }
}

/// [`LLMClientMiddleware`] that reports completions to Langfuse via OTLP.
///
/// Build **one per application** and `clone()` it into every
/// [`MiddlewareClient`](llm_kernel::llm::MiddlewareClient) (fallback
/// chains, per-model clients): clones share a single batch exporter and
/// thread. Calling `new` per client spawns one exporter thread each.
/// Call [`shutdown`](Self::shutdown) exactly once before process exit to
/// flush the last batch.
#[derive(Clone)]
pub struct LangfuseMiddleware {
    config: LangfuseConfig,
    provider: SdkTracerProvider,
}

impl LangfuseMiddleware {
    /// Build the tracer provider and exporter. Panics only when the
    /// exporter cannot be constructed (invalid configuration) — this is a
    /// startup-time failure, not a per-call one.
    pub fn new(config: LangfuseConfig) -> Self {
        let credentials = {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{}", config.public_key, config.secret_key))
        };
        let exporter = SpanExporter::builder()
            .with_http()
            .with_protocol(Protocol::HttpJson)
            .with_endpoint(format!(
                "{}/api/public/otel/v1/traces",
                config.host.trim_end_matches('/')
            ))
            .with_headers(
                [
                    ("Authorization".to_string(), format!("Basic {credentials}")),
                    // Without this header Langfuse delays direct OTel
                    // ingestion by up to 10 minutes.
                    ("x-langfuse-ingestion-version".to_string(), "4".to_string()),
                ]
                .into_iter()
                .collect(),
            )
            .build()
            .expect("langfuse OTLP exporter construction failed");

        let provider = SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .with_resource(
                Resource::builder()
                    .with_service_name("llm-kernel-langfuse")
                    .build(),
            )
            .build();
        Self { config, provider }
    }

    /// Flush and shut down the exporter. Call once on SIGTERM/before exit —
    /// the final batch is silently dropped otherwise. Repeated calls are
    /// no-ops.
    pub fn shutdown(&self) {
        if let Err(error) = self.provider.shutdown() {
            tracing::warn!(%error, "langfuse exporter shutdown failed");
        }
    }

    /// Emit one span. Real wall-clock timing comes from the middleware
    /// `elapsed` measurement (start = now - elapsed, end = now); the span
    /// nests under the caller's trace when `observability.traceparent`
    /// carries a valid W3C trace context.
    fn emit(&self, request: &LLMRequest, attributes: Vec<KeyValue>, elapsed: Duration) {
        let observability = request.observability.as_ref();
        let name = observability
            .and_then(|context| context.name.as_deref())
            .unwrap_or(OBSERVATION_NAME);
        let end = std::time::SystemTime::now();
        let start = end.checked_sub(elapsed).unwrap_or(end);
        let parent_context = observability
            .and_then(|context| context.traceparent.as_deref())
            .map(|traceparent| {
                let mut carrier = HashMap::new();
                carrier.insert("traceparent".to_string(), traceparent.to_string());
                TraceContextPropagator::new().extract(&carrier)
            })
            .unwrap_or_default();
        let mut span = SpanBuilder::from_name(name.to_string())
            .with_start_time(start)
            .with_attributes(attributes)
            .start_with_context(
                &self.provider.tracer("llm-kernel-langfuse"),
                &parent_context,
            );
        span.end_with_timestamp(end);
    }
}

#[async_trait]
impl LLMClientMiddleware for LangfuseMiddleware {
    async fn on_response(&self, request: &LLMRequest, response: &LLMResponse, elapsed: Duration) {
        self.emit(
            request,
            generation_attributes(request, response, &self.config),
            elapsed,
        );
    }

    async fn on_error(&self, request: &LLMRequest, error: &KernelError, elapsed: Duration) {
        self.emit(
            request,
            error_attributes(request, error, &self.config),
            elapsed,
        );
    }
}

/// Attributes for a successful generation. `capture_io` gates only
/// input/output — model/usage/level/metadata always ship so cost and
/// failure dashboards survive the kill switch.
fn generation_attributes(
    request: &LLMRequest,
    response: &LLMResponse,
    config: &LangfuseConfig,
) -> Vec<KeyValue> {
    let name = observation_name(request);
    let mut attributes = vec![
        KeyValue::new("langfuse.observation.type", "generation"),
        KeyValue::new("langfuse.trace.name", name.to_string()),
        KeyValue::new("langfuse.observation.model.name", response.model.clone()),
        KeyValue::new(
            "langfuse.observation.usage_details",
            serde_json::json!({
                "input": response.usage.prompt_tokens,
                "output": response.usage.completion_tokens,
                "total": response.usage.total_tokens,
            })
            .to_string(),
        ),
        KeyValue::new(
            "langfuse.observation.model.parameters",
            serde_json::json!({
                "temperature": request.temperature,
                "max_tokens": request.max_tokens,
            })
            .to_string(),
        ),
        KeyValue::new("langfuse.observation.level", "DEFAULT"),
        KeyValue::new("langfuse.observation.metadata.success", true),
    ];
    if config.capture_io {
        attributes.push(KeyValue::new(
            "langfuse.observation.input",
            serde_json::to_string(&conversation_input(request)).unwrap_or_default(),
        ));
        attributes.push(KeyValue::new(
            "langfuse.observation.output",
            rendered_output(response),
        ));
    }
    if let Some(reason) = &response.finish_reason {
        attributes.push(KeyValue::new(
            "langfuse.observation.metadata.finishReason",
            reason.clone(),
        ));
    }
    if let Some(reasoning) = response.usage.reasoning_tokens {
        attributes.push(KeyValue::new(
            "langfuse.observation.metadata.reasoningTokens",
            i64::from(reasoning),
        ));
    }
    push_session(&mut attributes, config, request);
    attributes
}

/// Attributes for a failed generation.
fn error_attributes(
    request: &LLMRequest,
    error: &KernelError,
    config: &LangfuseConfig,
) -> Vec<KeyValue> {
    let name = observation_name(request);
    let mut attributes = vec![
        KeyValue::new("langfuse.observation.type", "generation"),
        KeyValue::new("langfuse.trace.name", name.to_string()),
        KeyValue::new(
            "langfuse.observation.model.name",
            request
                .model
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
        ),
        KeyValue::new("langfuse.observation.level", "ERROR"),
        // Provider error bodies can echo request credentials back (see the
        // kernel's own `redact_http_body` rationale) — mask before shipping.
        KeyValue::new(
            "langfuse.observation.status_message",
            llm_kernel::safety::mask_secrets(&error.to_string()),
        ),
        KeyValue::new("langfuse.observation.metadata.success", false),
    ];
    if config.capture_io {
        attributes.push(KeyValue::new(
            "langfuse.observation.input",
            serde_json::to_string(&conversation_input(request)).unwrap_or_default(),
        ));
    }
    push_session(&mut attributes, config, request);
    attributes
}

/// Render the request as a flat OpenAI-format message list (system
/// first) so Langfuse draws a role-labeled conversation instead of a raw
/// JSON blob.
fn conversation_input(request: &LLMRequest) -> Vec<serde_json::Value> {
    let mut messages = Vec::with_capacity(request.messages.len() + 1);
    if let Some(system) = &request.system {
        messages.push(serde_json::json!({"role": "system", "content": system}));
    }
    for message in &request.messages {
        messages.push(serde_json::json!({
            "role": message.role,
            "content": message.text_content(),
        }));
    }
    messages
}

/// Render the response as plain content, or an assistant message with
/// tool calls when present — matching what Langfuse renders as a
/// conversation turn. Returns the attribute value: a bare string stays
/// unencoded (double-encoding would store `"서울"` with quotes, and a
/// JSON-returning model's object would render as an escaped string
/// instead of parsing), only the tool-call form is serialized.
fn rendered_output(response: &LLMResponse) -> String {
    if response.tool_calls.is_empty() {
        return response.content.clone();
    }
    serde_json::json!({
        "role": "assistant",
        "content": response.content,
        "tool_calls": response
            .tool_calls
            .iter()
            .map(|call| {
                serde_json::json!({
                    "id": call.id,
                    "name": call.name,
                    "arguments": call.arguments,
                })
            })
            .collect::<Vec<_>>(),
    })
    .to_string()
}

/// Langfuse filters and aggregates per observation, so session /
/// environment / release / tags must ride every span — not just a root
/// trace.
fn push_session(attributes: &mut Vec<KeyValue>, config: &LangfuseConfig, request: &LLMRequest) {
    let observability = request.observability.as_ref();
    // Per-request session wins; config stays the default for requests
    // carrying no context.
    let session = observability
        .and_then(|context| context.session_id.as_deref())
        .or(config.session_id.as_deref());
    if let Some(session) = session {
        attributes.push(KeyValue::new("langfuse.session.id", session.to_string()));
    }
    if let Some(environment) = &config.environment {
        attributes.push(KeyValue::new("langfuse.environment", environment.clone()));
    }
    if let Some(release) = &config.release {
        attributes.push(KeyValue::new("langfuse.release", release.clone()));
    }
    let mut tags = config.tags.clone();
    if let Some(extra) = observability.map(|context| context.tags.as_slice()) {
        tags.extend(extra.iter().cloned());
    }
    if !tags.is_empty() {
        attributes.push(KeyValue::new("langfuse.trace.tags", format!("{tags:?}")));
    }
    // Prefixed metadata keys become first-class filterable fields in
    // Langfuse; unprefixed attributes land in an unfilterable catch-all.
    if let Some(context) = observability {
        for (key, value) in &context.metadata {
            attributes.push(KeyValue::new(
                format!("langfuse.observation.metadata.{key}"),
                value.clone(),
            ));
        }
    }
}

fn observation_name(request: &LLMRequest) -> &str {
    request
        .observability
        .as_ref()
        .and_then(|context| context.name.as_deref())
        .unwrap_or(OBSERVATION_NAME)
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

    fn attr_map(attributes: &[KeyValue]) -> std::collections::HashMap<String, String> {
        attributes
            .iter()
            .map(|kv| {
                let value = match &kv.value {
                    opentelemetry::Value::String(s) => s.to_string(),
                    other => other.to_string(),
                };
                (kv.key.as_str().to_string(), value)
            })
            .collect()
    }

    #[test]
    fn generation_attributes_map_model_and_usage() {
        let map = attr_map(&generation_attributes(
            &sample_request(),
            &sample_response(),
            &config(true),
        ));
        assert_eq!(map["langfuse.observation.type"], "generation");
        assert_eq!(map["langfuse.observation.model.name"], "mock-model");
        assert_eq!(map["langfuse.observation.level"], "DEFAULT");
        assert!(map["langfuse.observation.usage_details"].contains("\"total\":18"));
        assert_eq!(map["langfuse.observation.metadata.reasoningTokens"], "3");
        assert_eq!(map["langfuse.observation.metadata.finishReason"], "stop");
        assert!(map["langfuse.observation.input"].contains("secret question"));
        assert!(map["langfuse.observation.output"].contains("the answer"));
        // Conversation rendering, not raw dumps: flat role-labeled message
        // array in, plain content string out.
        let input: serde_json::Value =
            serde_json::from_str(&map["langfuse.observation.input"]).unwrap();
        assert!(input.is_array());
        assert_eq!(input[0]["role"], "user");
        let output = &map["langfuse.observation.output"];
        // Bare content, not double-encoded — no surrounding JSON quotes.
        assert_eq!(output, "the answer");
    }

    #[test]
    fn capture_off_hides_prompts_but_keeps_usage() {
        let map = attr_map(&generation_attributes(
            &sample_request(),
            &sample_response(),
            &config(false),
        ));
        assert!(!map.contains_key("langfuse.observation.input"));
        assert!(!map.contains_key("langfuse.observation.output"));
        assert!(map.contains_key("langfuse.observation.usage_details"));
        assert!(map.contains_key("langfuse.observation.model.name"));
    }

    #[test]
    fn error_attributes_mask_secrets_and_set_level() {
        let error = KernelError::Http {
            status: 401,
            message: "echoed Authorization: Bearer sk-live-abcdef123".to_string(),
        };
        let map = attr_map(&error_attributes(&sample_request(), &error, &config(false)));
        assert_eq!(map["langfuse.observation.level"], "ERROR");
        assert_eq!(map["langfuse.observation.model.name"], "unknown");
        let message = &map["langfuse.observation.status_message"];
        assert!(!message.contains("sk-live-abcdef123"), "leaked: {message}");
        assert!(message.contains("****"));
    }

    #[test]
    fn environment_release_tags_ride_every_span() {
        let mut cfg = config(true);
        cfg.environment = Some("production".to_string());
        cfg.release = Some("v1.2.3".to_string());
        cfg.tags = vec!["router".to_string()];
        let map = attr_map(&generation_attributes(
            &sample_request(),
            &sample_response(),
            &cfg,
        ));
        assert_eq!(map["langfuse.environment"], "production");
        assert_eq!(map["langfuse.release"], "v1.2.3");
        assert!(map["langfuse.trace.tags"].contains("router"));
        // latency_measured marker is gone — elapsed now sets real span
        // timing, no synthetic marker should pollute metadata.
        assert!(!map.contains_key("langfuse.observation.metadata.latency_measured"));
        assert!(map["langfuse.observation.model.parameters"].contains("temperature"));
    }

    #[test]
    fn session_id_rides_every_span() {
        let mut cfg = config(true);
        cfg.session_id = Some("s-1".to_string());
        let ok = attr_map(&generation_attributes(
            &sample_request(),
            &sample_response(),
            &cfg,
        ));
        let err = attr_map(&error_attributes(
            &sample_request(),
            &KernelError::LlmApi("boom".to_string()),
            &cfg,
        ));
        assert_eq!(ok["langfuse.session.id"], "s-1");
        assert_eq!(err["langfuse.session.id"], "s-1");
    }

    // --- integration: wiremock against the OTLP endpoint ---------------

    struct MockOk;
    struct MockErr;
    struct MockSlow;

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
    impl LLMClient for MockSlow {
        async fn complete(&self, _request: LLMRequest) -> llm_kernel::error::Result<LLMResponse> {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
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

    /// Flatten an OTLP/JSON export body into `key -> value` span attributes.
    fn otel_span_attributes(body: &serde_json::Value) -> std::collections::HashMap<String, String> {
        let mut out = std::collections::HashMap::new();
        for resource_spans in body["resourceSpans"].as_array().into_iter().flatten() {
            for scope_spans in resource_spans["scopeSpans"]
                .as_array()
                .into_iter()
                .flatten()
            {
                for span in scope_spans["spans"].as_array().into_iter().flatten() {
                    for attr in span["attributes"].as_array().into_iter().flatten() {
                        let key = attr["key"].as_str().unwrap_or_default().to_string();
                        let value = attr["value"]
                            .as_object()
                            .map(|v| {
                                if let Some(s) = v.get("stringValue").and_then(|x| x.as_str()) {
                                    s.to_string()
                                } else if let Some(b) = v.get("boolValue").and_then(|x| x.as_bool())
                                {
                                    b.to_string()
                                } else if let Some(i) = v.get("intValue").and_then(|x| x.as_i64()) {
                                    i.to_string()
                                } else {
                                    String::new()
                                }
                            })
                            .unwrap_or_default();
                        out.insert(key, value);
                    }
                }
            }
        }
        out
    }

    /// First span object from an OTLP/JSON export body.
    fn first_span(body: &serde_json::Value) -> &serde_json::Value {
        &body["resourceSpans"][0]["scopeSpans"][0]["spans"][0]
    }

    fn traced_request() -> LLMRequest {
        let mut metadata = std::collections::BTreeMap::new();
        metadata.insert("source".to_string(), "router".to_string());
        LLMRequest::builder()
            .user_message("hi")
            .observability(llm_kernel::llm::ObservabilityContext {
                name: Some("assess-deep-pass".to_string()),
                traceparent: Some(
                    "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_string(),
                ),
                session_id: Some("req-session".to_string()),
                tags: vec!["workload".to_string()],
                metadata,
            })
            .build()
    }

    #[tokio::test]
    async fn observability_context_drives_name_trace_session_tags() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let mut config = config(true);
        config.host = server.uri();
        config.session_id = Some("cfg-session".to_string());
        let middleware = LangfuseMiddleware::new(config);
        let client = MiddlewareClient::new(MockOk, middleware.clone());
        client.complete(traced_request()).await.unwrap();
        middleware.shutdown();

        let requests = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        let span = first_span(&body);
        // Per-call name override (not the constant).
        assert_eq!(span["name"], "assess-deep-pass");
        // Nests under the caller's trace: trace id and parent span id come
        // from the W3C traceparent.
        assert_eq!(span["traceId"], "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(span["parentSpanId"], "00f067aa0ba902b7");
        let attrs = otel_span_attributes(&body);
        assert_eq!(attrs["langfuse.trace.name"], "assess-deep-pass");
        // Request session wins over the config default.
        assert_eq!(attrs["langfuse.session.id"], "req-session");
        // Request tags merge with config tags.
        assert!(attrs["langfuse.trace.tags"].contains("workload"));
        // Prefixed request metadata becomes a first-class field.
        assert_eq!(attrs["langfuse.observation.metadata.source"], "router");
    }

    #[tokio::test]
    async fn elapsed_sets_real_span_timing() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let mut config = config(true);
        config.host = server.uri();
        let middleware = LangfuseMiddleware::new(config);
        let client = MiddlewareClient::new(MockSlow, middleware.clone());
        client.complete(sample_request()).await.unwrap();
        middleware.shutdown();

        let requests = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        let span = first_span(&body);
        let start = span["startTimeUnixNano"].as_str().unwrap_or_default();
        let end = span["endTimeUnixNano"].as_str().unwrap_or_default();
        assert!(!start.is_empty() && !end.is_empty(), "timestamps present");
        assert_ne!(start, end, "duration must reflect the ~30ms call");
    }

    #[tokio::test]
    async fn complete_exports_generation_span_via_otlp() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let mut config = config(true);
        config.host = server.uri();
        let middleware = LangfuseMiddleware::new(config);
        let client = MiddlewareClient::new(MockOk, middleware.clone());
        client.complete(sample_request()).await.unwrap();
        // Batch processor exports off-thread — force the flush.
        middleware.shutdown();

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1, "expected exactly one OTLP export");
        let post = &requests[0];
        assert!(
            post.url.as_str().contains("/api/public/otel/v1/traces"),
            "wrong endpoint: {}",
            post.url
        );
        // base64("pk-test:sk-test"), precomputed — no base64 in test path
        assert_eq!(
            post.headers
                .get("authorization")
                .expect("authorization header")
                .to_str()
                .unwrap(),
            "Basic cGstdGVzdDpzay10ZXN0"
        );
        assert_eq!(
            post.headers
                .get("x-langfuse-ingestion-version")
                .expect("ingestion version header")
                .to_str()
                .unwrap(),
            "4"
        );

        let body: serde_json::Value = serde_json::from_slice(&post.body).unwrap();
        let attrs = otel_span_attributes(&body);
        assert_eq!(attrs["langfuse.observation.type"], "generation");
        assert_eq!(attrs["langfuse.observation.model.name"], "mock-model");
        assert!(attrs["langfuse.observation.usage_details"].contains("\"total\":18"));
        assert!(
            attrs.contains_key("langfuse.observation.input"),
            "input captured by default"
        );
        assert!(
            attrs.contains_key("langfuse.observation.output"),
            "output captured by default"
        );
    }

    #[tokio::test]
    async fn capture_off_still_exports_usage_but_not_prompts() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let mut config = config(false);
        config.host = server.uri();
        let middleware = LangfuseMiddleware::new(config);
        let client = MiddlewareClient::new(MockOk, middleware.clone());
        client.complete(sample_request()).await.unwrap();
        middleware.shutdown();

        let requests = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        let attrs = otel_span_attributes(&body);
        assert!(!attrs.contains_key("langfuse.observation.input"));
        assert!(!attrs.contains_key("langfuse.observation.output"));
        assert!(attrs.contains_key("langfuse.observation.usage_details"));
        assert!(attrs.contains_key("langfuse.observation.model.name"));
    }

    #[tokio::test]
    async fn error_path_exports_error_span() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let mut config = config(true);
        config.host = server.uri();
        let middleware = LangfuseMiddleware::new(config);
        let client = MiddlewareClient::new(MockErr, middleware.clone());
        assert!(client.complete(sample_request()).await.is_err());
        middleware.shutdown();

        let requests = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        let attrs = otel_span_attributes(&body);
        assert_eq!(attrs["langfuse.observation.level"], "ERROR");
        assert!(attrs["langfuse.observation.status_message"].contains("boom"));
    }

    #[tokio::test]
    async fn ingestion_failure_does_not_break_the_call() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let mut config = config(true);
        config.host = server.uri();
        let middleware = LangfuseMiddleware::new(config);
        let client = MiddlewareClient::new(MockOk, middleware.clone());
        let response = client.complete(sample_request()).await;
        assert!(response.is_ok());
        middleware.shutdown();
    }
}
