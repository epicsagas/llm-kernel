use std::time::Duration;

use async_trait::async_trait;

use crate::error::{KernelError, Result};
use crate::llm::tool::{ToolCall, ToolDefinition};
use crate::llm::types::{
    LLMRequest, LLMResponse, LLMStream, ModelConfig, ReasoningConfig, ReasoningEffort,
    ResponseFormat, StreamEvent, TokenUsage, Verbosity,
};

/// Best-effort redaction of an HTTP error response body before it lands in a
/// [`KernelError::Http`]. Some API gateways/proxies echo the request
/// `Authorization` header inside error bodies; without this, a caller that logs
/// the error leaks the API key. Full pattern masking when the `safety` feature
/// is enabled; otherwise the body is passed through unchanged (the masking
/// regex is an opt-in dependency).
fn redact_http_body(body: &str) -> String {
    #[cfg(feature = "safety")]
    {
        crate::safety::sanitize::mask_secrets(body)
    }
    #[cfg(not(feature = "safety"))]
    {
        body.to_string()
    }
}

/// Convert kernel [`ToolDefinition`]s into OpenAI `tools` (`type: "function"`).
fn openai_tools(tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                }
            })
        })
        .collect()
}

/// Map a [`ResponseFormat`] to OpenAI's `response_format` object, or `None` for
/// the provider default (plain text).
fn openai_response_format(rf: &ResponseFormat) -> Option<serde_json::Value> {
    match rf {
        ResponseFormat::Text => None,
        ResponseFormat::Json => Some(serde_json::json!({ "type": "json_object" })),
        ResponseFormat::JsonSchema { schema } => Some(serde_json::json!({
            "type": "json_schema",
            "json_schema": { "name": "response", "schema": schema, "strict": true }
        })),
    }
}

/// Convert kernel [`ToolDefinition`]s into Anthropic `tools` (with `input_schema`).
fn anthropic_tools(tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.input_schema,
            })
        })
        .collect()
}

/// Map a [`ResponseFormat`] to Anthropic's `output_config`. Only
/// [`ResponseFormat::JsonSchema`] has a native equivalent; `Json` (schemaless)
/// and `Text` return `None`.
fn anthropic_output_config(rf: &ResponseFormat) -> Option<serde_json::Value> {
    match rf {
        ResponseFormat::JsonSchema { schema } => Some(serde_json::json!({
            "format": { "type": "json_schema", "schema": schema }
        })),
        ResponseFormat::Json | ResponseFormat::Text => None,
    }
}

/// Map kernel [`ReasoningConfig`] to the wire keys the OpenAI-compatible path
/// forwards: `effort` becomes the official top-level `reasoning_effort`
/// parameter; `enabled` (OpenRouter extension) and `summary` (official
/// Responses-API `reasoning.summary`) share one `reasoning` object. Each key
/// is emitted only when set, so a `None` config (or an unset knob) adds
/// nothing to the request body.
fn openai_reasoning(
    cfg: Option<&ReasoningConfig>,
) -> (Option<ReasoningEffort>, Option<serde_json::Value>) {
    match cfg {
        None => (None, None),
        Some(c) => {
            let mut obj = serde_json::Map::new();
            if let Some(enabled) = c.enabled {
                obj.insert("enabled".into(), enabled.into());
            }
            if let Some(summary) = c.summary {
                obj.insert(
                    "summary".into(),
                    serde_json::to_value(summary).expect("ReasoningSummary is a plain enum"),
                );
            }
            (
                c.effort,
                if obj.is_empty() {
                    None
                } else {
                    Some(serde_json::Value::Object(obj))
                },
            )
        }
    }
}

/// Serialize the outgoing body and merge [`LLMRequest::extra_body`] keys into
/// it (last-write-wins), for official spec parameters or provider extensions
/// the kernel does not model natively.
fn openai_body_with_extra(
    body: &OpenAIChatRequest,
    extra: Option<&serde_json::Map<String, serde_json::Value>>,
) -> serde_json::Value {
    let mut v = serde_json::to_value(body).expect("OpenAIChatRequest serializes");
    if let Some(extra) = extra
        && let serde_json::Value::Object(map) = &mut v
    {
        for (k, val) in extra {
            map.insert(k.clone(), val.clone());
        }
    }
    v
}

/// Build a `reqwest::Client` with connect and total timeouts.
fn http_client() -> Result<reqwest::Client> {
    crate::tls::ensure_tls_provider();
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| KernelError::Config(format!("Failed to build HTTP client: {}", e)))
}

/// Check for HTTP 429 rate-limit response and extract `retry-after` header.
fn check_rate_limit(resp: &reqwest::Response) -> Result<()> {
    if resp.status().as_u16() == 429 {
        let retry = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(60);
        return Err(KernelError::RateLimited(retry));
    }
    Ok(())
}

/// Unified async interface for LLM chat completion and streaming.
#[async_trait]
pub trait LLMClient: Send + Sync {
    /// Send a chat completion request and return the full response.
    ///
    /// **Reasoning-model answer promotion (non-streaming only):** when a provider
    /// returns the final answer in `reasoning_content` and leaves `content` empty
    /// (notably GLM-4.7), `complete` promotes the reasoning into `content` so
    /// downstream consumers transparently receive the answer. The original
    /// reasoning is preserved in [`LLMResponse::reasoning`].
    ///
    /// This promotion is **not** applied by [`stream_complete`](Self::stream_complete),
    /// which surfaces reasoning as separate [`StreamEvent::ReasoningDelta`] events —
    /// see that variant's docs for the streaming accumulation contract.
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse>;
    /// Return the model name this client is configured to use.
    fn model_name(&self) -> &str;

    /// Stream a chat completion, yielding events as they arrive.
    ///
    /// Does **not** promote reasoning into a final `content` — see
    /// [`StreamEvent::ReasoningDelta`] for why streaming consumers of
    /// reasoning-only models (GLM-4.7) must accumulate both `ReasoningDelta` and
    /// `Delta` to reconstruct the answer.
    async fn stream_complete(&self, request: LLMRequest) -> Result<LLMStream>;
}

/// Async LLM client for the OpenAI chat completions API.
pub struct OpenAIClient {
    api_key: String,
    model: String,
    base_url: String,
    client: reqwest::Client,
}

impl OpenAIClient {
    /// Create a new client using credentials from the environment variable in `config`.
    pub fn new(config: &ModelConfig) -> Result<Self> {
        let api_key = std::env::var(&config.api_key_env).map_err(|_| {
            KernelError::Config(format!(
                "Environment variable {} not set",
                config.api_key_env
            ))
        })?;
        Ok(Self {
            api_key,
            model: config.model.clone(),
            base_url: config
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".into()),
            client: http_client()?,
        })
    }

    /// Create a new client with an explicit API key, using the default OpenAI base URL.
    ///
    /// Returns a [`KernelError::Config`] if the HTTP client (with its connect /
    /// total timeouts) cannot be built, rather than silently falling back to a
    /// timeout-less `reqwest::Client::default()`.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use llm_kernel::llm::OpenAIClient;
    /// let client = OpenAIClient::from_key("gpt-4o-mini", "sk-...")?;
    /// # Ok::<(), llm_kernel::error::KernelError>(())
    /// ```
    pub fn from_key(model: impl Into<String>, api_key: impl Into<String>) -> Result<Self> {
        Ok(Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: "https://api.openai.com/v1".into(),
            client: http_client()?,
        })
    }

    /// Create from an explicit key and a shared `reqwest::Client`.
    ///
    /// Prefer this over [`from_key`](Self::from_key) when constructing multiple
    /// clients in a hot path — the shared client reuses the underlying TCP
    /// connection pool.
    pub fn from_key_with_client(
        model: impl Into<String>,
        api_key: impl Into<String>,
        client: reqwest::Client,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: "https://api.openai.com/v1".into(),
            client,
        }
    }

    /// Create from an explicit key, a custom base URL, and a shared `reqwest::Client`.
    ///
    /// Use this for OpenAI-compatible providers that are not the default OpenAI
    /// endpoint (DeepSeek, Groq, Ollama, LM Studio, custom gateways, …) when you
    /// already hold the key in memory and want to reuse a shared connection pool.
    pub fn from_key_with_base_url(
        model: impl Into<String>,
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        client: reqwest::Client,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: base_url.into(),
            client,
        }
    }
}

#[derive(serde::Serialize)]
struct OpenAIChatRequest {
    model: String,
    messages: Vec<OpenAIChatMessage>,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<serde_json::Value>,
    /// Official OpenAI `reasoning_effort` parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<ReasoningEffort>,
    /// OpenRouter extension / Responses-style `reasoning` object.
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<serde_json::Value>,
    /// Official OpenAI `verbosity` parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    verbosity: Option<Verbosity>,
}

#[derive(serde::Serialize)]
struct OpenAIChatMessage {
    role: String,
    content: String,
}

#[derive(serde::Deserialize)]
struct OpenAIChatResponse {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    created: Option<u64>,
    choices: Vec<OpenAIChoice>,
    model: String,
    usage: Option<OpenAIUsage>,
}

#[derive(serde::Deserialize)]
struct OpenAIChoice {
    message: OpenAIRespMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

/// Response-side assistant message. `content` is `null` on tool-call turns, so
/// it is optional and defaults to empty.
///
/// Reasoning models (GLM-4.5+/z.ai, OpenAI o1) emit their chain-of-thought in
/// `reasoning_content` and leave `content` null — see ADR below. DeepSeek-R1 uses
/// the field name `reasoning`, covered via serde alias.
#[derive(serde::Deserialize)]
struct OpenAIRespMessage {
    #[serde(default)]
    content: Option<String>,
    /// Reasoning model's chain-of-thought. Aliased from `reasoning` (DeepSeek-R1).
    #[serde(default, alias = "reasoning")]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<OpenAIToolCall>,
}

#[derive(serde::Deserialize)]
struct OpenAIToolCall {
    id: String,
    function: OpenAIFunctionCall,
}

#[derive(serde::Deserialize)]
struct OpenAIFunctionCall {
    name: String,
    #[serde(default)]
    arguments: String,
}

/// Token usage reported by OpenAI-compatible providers.
///
/// All fields are `#[serde(default)]`: some providers (and partial/error
/// responses) omit `usage` fields, and a missing field should not cause the
/// whole response to fail parsing — `total_tokens` can be recomputed downstream.
#[derive(serde::Deserialize)]
struct OpenAIUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    total_tokens: u32,
    /// OpenAI o1 / GLM-4.7 expose reasoning token counts here.
    #[serde(default)]
    completion_tokens_details: Option<OpenAICompletionTokensDetails>,
}

/// Nested under `usage.completion_tokens_details` for reasoning models.
#[derive(serde::Deserialize)]
struct OpenAICompletionTokensDetails {
    #[serde(default)]
    reasoning_tokens: Option<u32>,
}

#[async_trait]
impl LLMClient for OpenAIClient {
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse> {
        let model = request.model.clone().unwrap_or_else(|| self.model.clone());
        let temperature = request.temperature;
        let max_tokens = request.max_tokens;
        let tools = request
            .tools
            .as_deref()
            .map(openai_tools)
            .filter(|t| !t.is_empty());
        let response_format = request
            .response_format
            .as_ref()
            .and_then(openai_response_format);
        let (reasoning_effort, reasoning) = openai_reasoning(request.reasoning.as_ref());
        let verbosity = request.verbosity;
        let extra_body = request.extra_body.clone();
        let messages: Vec<_> = request
            .into_openai_messages()
            .into_iter()
            .map(|(role, content)| OpenAIChatMessage { role, content })
            .collect();

        let body = OpenAIChatRequest {
            model,
            messages,
            temperature,
            max_tokens,
            stream: false,
            tools,
            response_format,
            reasoning_effort,
            reasoning,
            verbosity,
        };

        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&openai_body_with_extra(&body, extra_body.as_ref()))
            .send()
            .await
            .map_err(|e| KernelError::LlmApi(e.to_string()))?;

        check_rate_limit(&resp)?;

        let status = resp.status();

        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(KernelError::Http {
                status: status.as_u16(),
                message: redact_http_body(&text),
            });
        }

        let text = resp
            .text()
            .await
            .map_err(|e| KernelError::LlmApi(e.to_string()))?;
        let chat_resp: OpenAIChatResponse = from_json_lenient(&text)
            .map_err(|e| KernelError::LlmApi(format!("error decoding response body: {e}")))?;

        let id = chat_resp.id;
        let created = chat_resp.created;
        let first = chat_resp.choices.into_iter().next();
        let finish_reason = first.as_ref().and_then(|c| c.finish_reason.clone());
        let (content, reasoning, tool_calls) = match first {
            Some(c) => {
                let raw_content = c.message.content.unwrap_or_default();
                let reasoning = c.message.reasoning_content;
                let content = promote_reasoning_into_content(raw_content, reasoning.as_deref());
                let calls = c
                    .message
                    .tool_calls
                    .into_iter()
                    .map(|tc| ToolCall {
                        id: tc.id,
                        name: tc.function.name,
                        arguments: tc.function.arguments,
                    })
                    .collect();
                (content, reasoning, calls)
            }
            None => (String::new(), None, Vec::new()),
        };

        let usage = chat_resp.usage.map(|u| TokenUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
            reasoning_tokens: u.completion_tokens_details.and_then(|d| d.reasoning_tokens),
        });

        Ok(LLMResponse {
            content,
            reasoning,
            model: chat_resp.model,
            usage: usage.unwrap_or_default(),
            tool_calls,
            finish_reason,
            id,
            created,
        })
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    async fn stream_complete(&self, request: LLMRequest) -> Result<LLMStream> {
        let model = request.model.clone().unwrap_or_else(|| self.model.clone());
        let temperature = request.temperature;
        let max_tokens = request.max_tokens;
        let (reasoning_effort, reasoning) = openai_reasoning(request.reasoning.as_ref());
        let verbosity = request.verbosity;
        let extra_body = request.extra_body.clone();
        let messages: Vec<_> = request
            .into_openai_messages()
            .into_iter()
            .map(|(role, content)| OpenAIChatMessage { role, content })
            .collect();

        let body = OpenAIChatRequest {
            model,
            messages,
            temperature,
            max_tokens,
            stream: true,
            // Streaming is text-only here: the SSE parser emits text deltas and
            // does not reassemble streamed tool-call fragments.
            tools: None,
            response_format: None,
            reasoning_effort,
            reasoning,
            verbosity,
        };

        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&openai_body_with_extra(&body, extra_body.as_ref()))
            .send()
            .await
            .map_err(|e| KernelError::LlmApi(e.to_string()))?;

        check_rate_limit(&resp)?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(KernelError::Http {
                status: status.as_u16(),
                message: redact_http_body(&text),
            });
        }

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<StreamEvent>>(16);

        tokio::spawn(async move {
            let mut stream = std::pin::pin!(resp.bytes_stream());
            let mut buffer: Vec<u8> = Vec::new();

            use tokio_stream::StreamExt;

            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = tx.send(Err(KernelError::LlmApi(e.to_string()))).await;
                        return;
                    }
                };

                for line in drain_sse_lines(&mut buffer, &chunk) {
                    if let Some(data) = parse_sse_line(&line)
                        && let Some(event) = parse_openai_sse(data)
                    {
                        let is_done = matches!(event, StreamEvent::Done);
                        if tx.send(Ok(event)).await.is_err() || is_done {
                            return;
                        }
                    }
                }
            }
            let _ = tx.send(Ok(StreamEvent::Done)).await;
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }
}

/// Extract the data payload from an SSE `data: ...` line.
/// Returns `None` for non-data lines and for `data: [DONE]`.
fn parse_sse_line(line: &str) -> Option<&str> {
    line.strip_prefix("data: ").filter(|d| *d != "[DONE]")
}

/// Append a raw network chunk to `buffer` and drain every complete,
/// newline-terminated line, decoded as UTF-8.
///
/// Decoding is deferred until a line's bytes are fully buffered. A single
/// codepoint can straddle two network chunks, and decoding each chunk eagerly
/// with [`String::from_utf8_lossy`] would replace the split bytes with `U+FFFD`
/// — corrupting e.g. CJK or emoji deltas. Because `\n` (`0x0A`) is never a UTF-8
/// lead or continuation byte, splitting on it can't cut a codepoint, so every
/// drained line is a whole number of codepoints and decodes losslessly.
fn drain_sse_lines(buffer: &mut Vec<u8>, chunk: &[u8]) -> Vec<String> {
    buffer.extend_from_slice(chunk);
    let mut lines = Vec::new();
    while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
        let line: Vec<u8> = buffer.drain(..=pos).collect();
        lines.push(String::from_utf8_lossy(&line).trim_end().to_string());
    }
    lines
}

/// Decide the `content` exposed to consumers for a reasoning-model response.
///
/// # Two provider behaviors, one rule
///
/// - **GLM-4.7 (non-standard):** leaves `content` empty/null and returns the
///   final answer inside `reasoning_content`. Promoting reasoning into `content`
///   is required so downstream `json_extract` finds the JSON.
/// - **Standard reasoning models (OpenAI o1, DeepSeek-R1):** put the final
///   answer in `content` and the chain-of-thought in `reasoning_content`. When
///   `content` is genuinely present it is preserved unchanged.
///
/// # Caveat — empty `content` on a standard model
///
/// The promotion triggers on *any* empty `content`. If a standard model ever
/// returns an empty `content` together with reasoning (e.g. the model produced
/// only chain-of-thought and no final answer, or a transport/parse glitch), this
/// rule would surface the chain-of-thought as the answer. That is an acceptable
/// trade-off: the alternative is an empty answer, which fails every downstream
/// consumer the same way. The original reasoning is always preserved verbatim in
/// [`LLMResponse::reasoning`], so callers can detect this case and reject it.
fn promote_reasoning_into_content(raw_content: String, reasoning: Option<&str>) -> String {
    if raw_content.is_empty() {
        reasoning.unwrap_or_default().to_string()
    } else {
        raw_content
    }
}

/// Escape raw C0 control characters (`0x00`–`0x1F`) inside JSON string
/// literals as `\u00XX`.
///
/// Some OpenAI-compatible gateways (observed on OpenRouter) return HTTP 200
/// bodies whose `reasoning` strings contain unescaped control characters
/// (raw `\n`, `\t`, …), which strict JSON parsers reject wholesale. This
/// walk-only pass rewrites each such character to its escape and leaves
/// everything outside string literals untouched. Best effort: structurally
/// broken JSON stays broken and the original parse error is surfaced.
fn escape_unescaped_control_chars(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_string = false;
    let mut escaped = false;
    for ch in raw.chars() {
        if escaped {
            escaped = false;
            out.push(ch);
            continue;
        }
        match ch {
            '"' => {
                in_string = !in_string;
                out.push(ch);
            }
            '\\' if in_string => {
                escaped = true;
                out.push(ch);
            }
            c if in_string && (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// Parse `raw` as JSON, retrying once with control-character escaping when
/// strict parsing fails.
///
/// Valid bodies take the strict path unchanged (zero overhead beyond the
/// string read); only bodies that strict-JSON rejects get the sanitize pass.
/// If the sanitized retry also fails, the *original* error is returned so
/// callers see the real reason and not a secondary artifact.
fn from_json_lenient<T: serde::de::DeserializeOwned>(raw: &str) -> serde_json::Result<T> {
    match serde_json::from_str(raw) {
        Ok(v) => Ok(v),
        Err(err) => serde_json::from_str(&escape_unescaped_control_chars(raw)).map_err(|_| err),
    }
}

/// Parse an OpenAI streaming JSON chunk into a StreamEvent.
fn parse_openai_sse(data: &str) -> Option<StreamEvent> {
    let v: serde_json::Value = from_json_lenient(data).ok()?;

    // GLM-4.5+/o1 send reasoning and answer as separate delta chunks; check
    // reasoning_content first so it is surfaced as ReasoningDelta, not dropped.
    if let Some(rc) = v
        .get("choices")?
        .get(0)?
        .get("delta")?
        .get("reasoning_content")
        .and_then(|c| c.as_str())
        && !rc.is_empty()
    {
        return Some(StreamEvent::ReasoningDelta {
            content: rc.to_string(),
        });
    }

    // Extract delta content
    if let Some(content) = v
        .get("choices")?
        .get(0)?
        .get("delta")?
        .get("content")
        .and_then(|c| c.as_str())
        && !content.is_empty()
    {
        return Some(StreamEvent::Delta {
            content: content.to_string(),
        });
    }

    // Extract usage from the final chunk
    if let Some(usage) = v.get("usage").and_then(|u| {
        Some(TokenUsage {
            prompt_tokens: u.get("prompt_tokens")?.as_u64()? as u32,
            completion_tokens: u.get("completion_tokens")?.as_u64()? as u32,
            total_tokens: u.get("total_tokens")?.as_u64()? as u32,
            reasoning_tokens: u
                .get("completion_tokens_details")
                .and_then(|d| d.get("reasoning_tokens"))
                .and_then(|r| r.as_u64())
                .map(|n| n as u32),
        })
    }) {
        return Some(StreamEvent::Usage(usage));
    }

    // finish_reason = "stop" means done (no more content in this chunk)
    if v.get("choices")?
        .get(0)?
        .get("finish_reason")
        .and_then(|r| r.as_str())
        .is_some()
    {
        return Some(StreamEvent::Done);
    }

    None
}

/// Parse an Anthropic streaming JSON chunk into a StreamEvent.
fn parse_anthropic_sse(event_type: &str, data: &str) -> Option<StreamEvent> {
    let v: serde_json::Value = serde_json::from_str(data).ok()?;

    match event_type {
        "content_block_delta" => {
            let delta = v.get("delta")?;
            // Extended thinking deltas arrive as {"type":"thinking_delta","thinking":"..."}.
            match delta.get("type").and_then(|t| t.as_str()) {
                Some("thinking_delta") => {
                    let text = delta.get("thinking")?.as_str()?;
                    if !text.is_empty() {
                        return Some(StreamEvent::ReasoningDelta {
                            content: text.to_string(),
                        });
                    }
                    None
                }
                _ => {
                    // text_delta (default) carries {"text":"..."}.
                    let text = delta.get("text")?.as_str()?;
                    if !text.is_empty() {
                        return Some(StreamEvent::Delta {
                            content: text.to_string(),
                        });
                    }
                    None
                }
            }
        }
        "message_delta" => {
            let usage = v.get("usage").and_then(|u| {
                Some(TokenUsage {
                    prompt_tokens: 0,
                    completion_tokens: u.get("output_tokens")?.as_u64()? as u32,
                    total_tokens: 0,
                    reasoning_tokens: None,
                })
            });
            if let Some(usage) = usage {
                return Some(StreamEvent::Usage(usage));
            }
            Some(StreamEvent::Done)
        }
        "message_stop" => Some(StreamEvent::Done),
        _ => None,
    }
}

/// Async LLM client for the Anthropic Messages API.
pub struct AnthropicClient {
    api_key: String,
    model: String,
    base_url: String,
    client: reqwest::Client,
}

impl AnthropicClient {
    /// Create a new client using credentials from the environment variable in `config`.
    pub fn new(config: &ModelConfig) -> Result<Self> {
        let api_key = std::env::var(&config.api_key_env).map_err(|_| {
            KernelError::Config(format!(
                "Environment variable {} not set",
                config.api_key_env
            ))
        })?;
        Ok(Self {
            api_key,
            model: config.model.clone(),
            base_url: config
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com/v1".into()),
            client: http_client()?,
        })
    }

    /// Create a new client with an explicit API key, using the default Anthropic base URL.
    ///
    /// Returns a [`KernelError::Config`] if the HTTP client (with its connect /
    /// total timeouts) cannot be built, rather than silently falling back to a
    /// timeout-less `reqwest::Client::default()`.
    pub fn from_key(model: impl Into<String>, api_key: impl Into<String>) -> Result<Self> {
        Ok(Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: "https://api.anthropic.com/v1".into(),
            client: http_client()?,
        })
    }

    /// Create from an explicit key and a shared `reqwest::Client`.
    pub fn from_key_with_client(
        model: impl Into<String>,
        api_key: impl Into<String>,
        client: reqwest::Client,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: "https://api.anthropic.com/v1".into(),
            client,
        }
    }

    /// Create from an explicit key, a custom base URL, and a shared `reqwest::Client`.
    ///
    /// Use this for Anthropic-compatible endpoints that are not the default
    /// `api.anthropic.com` (self-hosted proxies, regional gateways, …).
    pub fn from_key_with_base_url(
        model: impl Into<String>,
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        client: reqwest::Client,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: base_url.into(),
            client,
        }
    }
}

#[derive(serde::Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_config: Option<serde_json::Value>,
}

#[derive(serde::Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(serde::Deserialize)]
struct AnthropicResponse {
    #[serde(default)]
    id: Option<String>,
    content: Vec<AnthropicContentBlock>,
    model: String,
    #[serde(default)]
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

/// A response content block. `text` blocks carry `text`; `tool_use` blocks
/// carry `id`/`name`/`input`; `thinking` blocks (extended thinking) carry `thinking`.
#[derive(serde::Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: Option<String>,
    /// Extended thinking content (`{"type":"thinking","thinking":"..."}`).
    #[serde(default)]
    thinking: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Option<serde_json::Value>,
}

#[derive(serde::Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[async_trait]
impl LLMClient for AnthropicClient {
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse> {
        let model = request.model.clone().unwrap_or_else(|| self.model.clone());
        let max_tokens = request.max_tokens.unwrap_or(4096);
        let temperature = request.temperature;
        let system = request.system.clone();
        let tools = request
            .tools
            .as_deref()
            .map(anthropic_tools)
            .filter(|t| !t.is_empty());
        let output_config = request
            .response_format
            .as_ref()
            .and_then(anthropic_output_config);
        let messages: Vec<AnthropicMessage> = request
            .into_anthropic_messages()
            .into_iter()
            .map(|(role, content)| AnthropicMessage { role, content })
            .collect();

        let body = AnthropicRequest {
            model,
            max_tokens,
            temperature,
            system,
            messages,
            stream: false,
            tools,
            output_config,
        };

        let resp = self
            .client
            .post(format!("{}/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| KernelError::LlmApi(e.to_string()))?;

        check_rate_limit(&resp)?;

        let status = resp.status();

        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(KernelError::Http {
                status: status.as_u16(),
                message: redact_http_body(&text),
            });
        }

        let chat_resp: AnthropicResponse = resp
            .json()
            .await
            .map_err(|e| KernelError::LlmApi(e.to_string()))?;

        let mut content = String::new();
        let mut reasoning = String::new();
        let mut tool_calls = Vec::new();
        for block in chat_resp.content {
            match block.block_type.as_str() {
                "text" => {
                    if let Some(t) = block.text {
                        content.push_str(&t);
                    }
                }
                "thinking" => {
                    if let Some(t) = block.thinking {
                        reasoning.push_str(&t);
                    }
                }
                "tool_use" => {
                    if let (Some(id), Some(name)) = (block.id, block.name) {
                        let arguments = block
                            .input
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "{}".to_string());
                        tool_calls.push(ToolCall {
                            id,
                            name,
                            arguments,
                        });
                    }
                }
                _ => {}
            }
        }

        Ok(LLMResponse {
            content,
            reasoning: if reasoning.is_empty() {
                None
            } else {
                Some(reasoning)
            },
            model: chat_resp.model,
            usage: TokenUsage {
                prompt_tokens: chat_resp.usage.input_tokens,
                completion_tokens: chat_resp.usage.output_tokens,
                total_tokens: chat_resp.usage.input_tokens + chat_resp.usage.output_tokens,
                reasoning_tokens: None,
            },
            tool_calls,
            finish_reason: chat_resp.stop_reason,
            id: chat_resp.id,
            created: None,
        })
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    async fn stream_complete(&self, request: LLMRequest) -> Result<LLMStream> {
        let model = request.model.clone().unwrap_or_else(|| self.model.clone());
        let max_tokens = request.max_tokens.unwrap_or(4096);
        let temperature = request.temperature;
        let system = request.system.clone();
        let messages: Vec<AnthropicMessage> = request
            .into_anthropic_messages()
            .into_iter()
            .map(|(role, content)| AnthropicMessage { role, content })
            .collect();

        let body = AnthropicRequest {
            model,
            max_tokens,
            temperature,
            system,
            messages,
            stream: true,
            // Streaming is text-only here: the SSE parser emits text deltas and
            // does not reassemble streamed tool-use blocks.
            tools: None,
            output_config: None,
        };

        let resp = self
            .client
            .post(format!("{}/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| KernelError::LlmApi(e.to_string()))?;

        check_rate_limit(&resp)?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(KernelError::Http {
                status: status.as_u16(),
                message: redact_http_body(&text),
            });
        }

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<StreamEvent>>(16);

        tokio::spawn(async move {
            let mut stream = std::pin::pin!(resp.bytes_stream());
            let mut buffer: Vec<u8> = Vec::new();
            let mut current_event = String::new();

            use tokio_stream::StreamExt;

            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = tx.send(Err(KernelError::LlmApi(e.to_string()))).await;
                        return;
                    }
                };

                for line in drain_sse_lines(&mut buffer, &chunk) {
                    if let Some(evt) = line.strip_prefix("event: ") {
                        current_event = evt.to_string();
                    } else if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            let _ = tx.send(Ok(StreamEvent::Done)).await;
                            return;
                        }
                        if let Some(event) = parse_anthropic_sse(&current_event, data) {
                            let is_done = matches!(event, StreamEvent::Done);
                            if tx.send(Ok(event)).await.is_err() || is_done {
                                return;
                            }
                        }
                        current_event.clear();
                    }
                }
            }
            let _ = tx.send(Ok(StreamEvent::Done)).await;
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::ReasoningSummary;

    #[test]
    fn parse_sse_line_extracts_data() {
        assert_eq!(
            parse_sse_line("data: {\"id\":\"1\"}"),
            Some("{\"id\":\"1\"}")
        );
    }

    #[test]
    fn parse_sse_line_skips_done() {
        assert_eq!(parse_sse_line("data: [DONE]"), None);
    }

    #[test]
    fn parse_sse_line_skips_non_data() {
        assert_eq!(parse_sse_line("event: ping"), None);
        assert_eq!(parse_sse_line(""), None);
    }

    #[test]
    fn drain_sse_lines_reassembles_multibyte_split_across_chunks() {
        // "data: 안녕\n" — "data: " is 6 bytes, 안/녕 are 3 bytes each.
        let full = "data: 안녕\n".as_bytes().to_vec();
        // Split at byte 7, mid-way through "안"'s 3-byte sequence.
        let (first, rest) = full.split_at(7);

        let mut buffer = Vec::new();
        // No newline yet, and the trailing bytes are a partial codepoint:
        // nothing should be emitted, and nothing should be corrupted.
        assert!(drain_sse_lines(&mut buffer, first).is_empty());

        let lines = drain_sse_lines(&mut buffer, rest);
        assert_eq!(lines, vec!["data: 안녕".to_string()]);
        // A per-chunk from_utf8_lossy would instead have produced U+FFFD here.
        assert!(!lines[0].contains('\u{FFFD}'));
    }

    #[test]
    fn drain_sse_lines_handles_multiple_lines_and_keeps_partial_tail() {
        let mut buffer = Vec::new();
        let lines = drain_sse_lines(&mut buffer, b"event: ping\r\ndata: {}\npartial");
        assert_eq!(
            lines,
            vec!["event: ping".to_string(), "data: {}".to_string()]
        );
        // The unterminated "partial" tail stays buffered for the next chunk.
        let lines = drain_sse_lines(&mut buffer, b" tail\n");
        assert_eq!(lines, vec!["partial tail".to_string()]);
    }

    #[test]
    fn openai_delta_extraction() {
        let data = r#"{"id":"chatcmpl-1","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}"#;
        let event = parse_openai_sse(data).unwrap();
        match event {
            StreamEvent::Delta { content } => assert_eq!(content, "Hello"),
            _ => panic!("expected Delta, got {:?}", event),
        }
    }

    #[test]
    fn openai_usage_extraction() {
        let data = r#"{"id":"chatcmpl-1","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#;
        let event = parse_openai_sse(data).unwrap();
        match event {
            StreamEvent::Usage(usage) => {
                assert_eq!(usage.prompt_tokens, 10);
                assert_eq!(usage.completion_tokens, 5);
                assert_eq!(usage.total_tokens, 15);
            }
            _ => panic!("expected Usage, got {:?}", event),
        }
    }

    #[test]
    fn openai_finish_reason_is_done() {
        let data =
            r#"{"id":"chatcmpl-1","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#;
        let event = parse_openai_sse(data).unwrap();
        assert!(matches!(event, StreamEvent::Done));
    }

    #[test]
    fn openai_empty_delta_skipped() {
        let data = r#"{"id":"chatcmpl-1","choices":[{"index":0,"delta":{"content":""},"finish_reason":null}]}"#;
        assert!(parse_openai_sse(data).is_none());
    }

    #[test]
    fn anthropic_content_block_delta() {
        let data = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        let event = parse_anthropic_sse("content_block_delta", data).unwrap();
        match event {
            StreamEvent::Delta { content } => assert_eq!(content, "Hello"),
            _ => panic!("expected Delta, got {:?}", event),
        }
    }

    #[test]
    fn anthropic_message_delta_usage() {
        let data = r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":5}}"#;
        let event = parse_anthropic_sse("message_delta", data).unwrap();
        match event {
            StreamEvent::Usage(usage) => assert_eq!(usage.completion_tokens, 5),
            _ => panic!("expected Usage, got {:?}", event),
        }
    }

    #[test]
    fn anthropic_message_stop() {
        let event = parse_anthropic_sse("message_stop", r#"{"type":"message_stop"}"#).unwrap();
        assert!(matches!(event, StreamEvent::Done));
    }

    #[test]
    fn anthropic_unknown_event_ignored() {
        assert!(parse_anthropic_sse("ping", "{}").is_none());
    }

    fn sample_tool() -> ToolDefinition {
        ToolDefinition {
            name: "get_weather".into(),
            description: "Get weather".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "location": { "type": "string" } },
                "required": ["location"]
            }),
        }
    }

    #[test]
    fn openai_tools_use_function_wrapper() {
        let out = openai_tools(&[sample_tool()]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["type"], "function");
        assert_eq!(out[0]["function"]["name"], "get_weather");
        // input_schema is forwarded verbatim as `parameters`.
        assert_eq!(out[0]["function"]["parameters"]["required"][0], "location");
    }

    #[test]
    fn openai_response_format_maps_each_variant() {
        assert!(openai_response_format(&ResponseFormat::Text).is_none());
        assert_eq!(
            openai_response_format(&ResponseFormat::Json).unwrap()["type"],
            "json_object"
        );
        let schema = serde_json::json!({"type": "object"});
        let js = openai_response_format(&ResponseFormat::JsonSchema { schema }).unwrap();
        assert_eq!(js["type"], "json_schema");
        assert_eq!(js["json_schema"]["strict"], true);
    }

    #[test]
    fn anthropic_tools_use_input_schema_key() {
        let out = anthropic_tools(&[sample_tool()]);
        assert_eq!(out[0]["name"], "get_weather");
        assert_eq!(out[0]["input_schema"]["type"], "object");
        assert!(out[0].get("function").is_none());
    }

    #[test]
    fn anthropic_output_config_only_for_json_schema() {
        assert!(anthropic_output_config(&ResponseFormat::Text).is_none());
        assert!(anthropic_output_config(&ResponseFormat::Json).is_none());
        let schema = serde_json::json!({"type": "object"});
        let cfg = anthropic_output_config(&ResponseFormat::JsonSchema { schema }).unwrap();
        assert_eq!(cfg["format"]["type"], "json_schema");
    }

    #[test]
    fn openai_request_serializes_tools_and_format() {
        let body = OpenAIChatRequest {
            model: "gpt-4o".into(),
            messages: vec![OpenAIChatMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
            temperature: 0.7,
            max_tokens: None,
            stream: false,
            tools: Some(openai_tools(&[sample_tool()])),
            response_format: Some(serde_json::json!({ "type": "json_object" })),
            reasoning_effort: None,
            reasoning: None,
            verbosity: None,
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["tools"][0]["function"]["name"], "get_weather");
        assert_eq!(json["response_format"]["type"], "json_object");
        // Omitted when None (backward-compatible request shape).
        assert!(json.get("max_tokens").is_none());
        assert!(json.get("reasoning_effort").is_none());
        assert!(json.get("reasoning").is_none());
    }

    #[test]
    fn openai_reasoning_effort_maps_to_official_param() {
        let (effort, reasoning) =
            openai_reasoning(Some(&ReasoningConfig::effort(ReasoningEffort::High)));
        assert_eq!(effort, Some(ReasoningEffort::High));
        assert!(reasoning.is_none());
        let body = OpenAIChatRequest {
            model: "gpt-5.5".into(),
            messages: vec![],
            temperature: 0.7,
            max_tokens: None,
            stream: false,
            tools: None,
            response_format: None,
            reasoning_effort: effort,
            reasoning,
            verbosity: None,
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["reasoning_effort"], "high");
        assert!(json.get("reasoning").is_none());
    }

    #[test]
    fn openai_reasoning_disabled_maps_to_openrouter_object() {
        // immune's exact need: reasoning: {"enabled": false} on OpenRouter,
        // with no reasoning_effort key and no config -> no keys at all.
        let (effort, reasoning) = openai_reasoning(Some(&ReasoningConfig::disabled()));
        assert_eq!(effort, None);
        assert_eq!(reasoning.unwrap()["enabled"], false);

        let (effort, reasoning) = openai_reasoning(None);
        assert!(effort.is_none() && reasoning.is_none());
    }

    #[test]
    fn openai_reasoning_both_knobs_serialize_independently() {
        let (effort, reasoning) = openai_reasoning(Some(&ReasoningConfig {
            enabled: Some(true),
            effort: Some(ReasoningEffort::Low),
            summary: Some(ReasoningSummary::Concise),
        }));
        let body = OpenAIChatRequest {
            model: "m".into(),
            messages: vec![],
            temperature: 0.7,
            max_tokens: None,
            stream: false,
            tools: None,
            response_format: None,
            reasoning_effort: effort,
            reasoning,
            verbosity: Some(Verbosity::High),
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["reasoning_effort"], "low");
        assert_eq!(json["reasoning"]["enabled"], true);
        assert_eq!(json["reasoning"]["summary"], "concise");
        assert_eq!(json["verbosity"], "high");
    }

    #[test]
    fn openai_reasoning_summary_only_serializes_object() {
        let (effort, reasoning) = openai_reasoning(Some(&ReasoningConfig {
            enabled: None,
            effort: None,
            summary: Some(ReasoningSummary::Detailed),
        }));
        assert!(effort.is_none());
        assert_eq!(reasoning.unwrap()["summary"], "detailed");
    }

    #[test]
    fn openai_body_merges_extra_body_last_write_wins() {
        let body = OpenAIChatRequest {
            model: "m".into(),
            messages: vec![],
            temperature: 0.7,
            max_tokens: None,
            stream: false,
            tools: None,
            response_format: None,
            reasoning_effort: None,
            reasoning: None,
            verbosity: None,
        };
        let mut extra = serde_json::Map::new();
        extra.insert("seed".into(), 7.into());
        extra.insert("temperature".into(), 0.1.into());
        let merged = openai_body_with_extra(&body, Some(&extra));
        assert_eq!(merged["seed"], 7);
        // Extra key overrides the natively forwarded one (last-write-wins).
        assert_eq!(merged["temperature"], 0.1);
        // Without extras the body is untouched. (Compare via json! so the f32
        // → f64 widening in the Number matches on both sides.)
        let plain = openai_body_with_extra(&body, None);
        assert_eq!(plain["temperature"], serde_json::json!(0.7f32));
        assert!(plain.get("seed").is_none());
    }

    #[test]
    fn verbosity_wire_values_match_openai_spec() {
        // Official enum: low, medium, high.
        for (variant, wire) in [
            (Verbosity::Low, "low"),
            (Verbosity::Medium, "medium"),
            (Verbosity::High, "high"),
        ] {
            assert_eq!(serde_json::to_value(variant).unwrap(), wire);
        }
    }

    #[test]
    fn reasoning_summary_wire_values_match_openai_spec() {
        // Official enum: auto, concise, detailed.
        for (variant, wire) in [
            (ReasoningSummary::Auto, "auto"),
            (ReasoningSummary::Concise, "concise"),
            (ReasoningSummary::Detailed, "detailed"),
        ] {
            assert_eq!(serde_json::to_value(variant).unwrap(), wire);
        }
    }

    #[test]
    fn reasoning_effort_wire_values_match_openai_spec() {
        // Official enum: none, minimal, low, medium, high, xhigh, max.
        for (variant, wire) in [
            (ReasoningEffort::None, "none"),
            (ReasoningEffort::Minimal, "minimal"),
            (ReasoningEffort::Low, "low"),
            (ReasoningEffort::Medium, "medium"),
            (ReasoningEffort::High, "high"),
            (ReasoningEffort::XHigh, "xhigh"),
            (ReasoningEffort::Max, "max"),
        ] {
            assert_eq!(serde_json::to_value(variant).unwrap(), wire);
            let back: ReasoningEffort = serde_json::from_value(wire.into()).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn lenient_decode_rescues_openrouter_control_chars() {
        // OpenRouter was observed returning 200 with raw \n / \t inside the
        // reasoning string; strict serde_json fails the whole body.
        let raw = "{\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\",\"content\":\"답변\",\"reasoning_content\":\"step 1\nstep 2\ttab\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}";
        assert!(serde_json::from_str::<OpenAIChatResponse>(raw).is_err());
        let resp: OpenAIChatResponse = from_json_lenient(raw).unwrap();
        let first = resp.choices.into_iter().next().unwrap();
        assert_eq!(first.message.content.as_deref(), Some("답변"));
        assert_eq!(
            first.message.reasoning_content.as_deref(),
            Some("step 1\nstep 2\ttab")
        );
    }

    #[test]
    fn lenient_decode_keeps_original_error_for_structural_garbage() {
        // Broken structure (not just control chars) still errors, surfacing
        // the *original* strict-parse error rather than a sanitize artifact.
        let raw = "{\"choices\": tr";
        let strict = serde_json::from_str::<OpenAIChatResponse>(raw)
            .map(|_: OpenAIChatResponse| ())
            .unwrap_err();
        let lenient = from_json_lenient::<OpenAIChatResponse>(raw)
            .map(|_: OpenAIChatResponse| ())
            .unwrap_err();
        assert_eq!(lenient.to_string(), strict.to_string());
    }

    #[test]
    fn escape_unescaped_control_chars_only_touches_string_literals() {
        // Raw newline outside strings (pretty-printed JSON) stays as-is;
        // inside strings it becomes \uXXXX; existing escapes are untouched.
        let raw = "{\n  \"a\": \"x\ny\\n\\tz\",\n  \"b\": 1\n}";
        let out = escape_unescaped_control_chars(raw);
        assert_eq!(out, "{\n  \"a\": \"x\\u000ay\\n\\tz\",\n  \"b\": 1\n}");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["a"], "x\ny\n\tz");
    }

    #[test]
    fn escape_unescaped_control_chars_handles_escaped_quotes() {
        // \" inside a string must not toggle the in-string state.
        let raw = "{\"a\":\"say \\\"hi\\\" ok\"}";
        assert_eq!(escape_unescaped_control_chars(raw), raw);
    }

    #[test]
    fn openai_sse_lenient_reasoning_delta_with_control_chars() {
        // A streamed reasoning delta carrying a raw tab must not be dropped.
        let data = "{\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"think\ttab\"},\"finish_reason\":null}]}";
        let event = parse_openai_sse(data).unwrap();
        match event {
            StreamEvent::ReasoningDelta { content } => assert_eq!(content, "think\ttab"),
            other => panic!("expected ReasoningDelta, got {other:?}"),
        }
    }

    #[test]
    fn openai_response_parses_tool_calls() {
        let raw = r#"{
            "id": "chatcmpl-1",
            "created": 1700000000,
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": { "name": "get_weather", "arguments": "{\"location\":\"Paris\"}" }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
        }"#;
        let resp: OpenAIChatResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(resp.id.as_deref(), Some("chatcmpl-1"));
        let choice = resp.choices.into_iter().next().unwrap();
        assert_eq!(choice.finish_reason.as_deref(), Some("tool_calls"));
        assert!(choice.message.content.is_none());
        assert_eq!(choice.message.tool_calls.len(), 1);
        assert_eq!(choice.message.tool_calls[0].function.name, "get_weather");
    }

    #[test]
    fn openai_response_parses_glm47_reasoning_content() {
        // GLM-4.7: content=null, reasoning_content carries the chain-of-thought + final JSON.
        let raw = r#"{
            "id":"chatcmpl-1","created":1700000000,"model":"glm-4.7",
            "choices":[{"index":0,"message":{"role":"assistant","content":null,
                "reasoning_content":"thinking... {\"rating\":\"Buy\"}"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15,
                "completion_tokens_details":{"reasoning_tokens":3}}
        }"#;
        let resp: OpenAIChatResponse = serde_json::from_str(raw).unwrap();
        let choice = resp.choices.into_iter().next().unwrap();
        assert!(choice.message.content.is_none());
        assert!(choice.message.reasoning_content.is_some());
        assert_eq!(
            resp.usage
                .unwrap()
                .completion_tokens_details
                .unwrap()
                .reasoning_tokens,
            Some(3)
        );
    }

    #[test]
    fn openai_complete_promotes_reasoning_when_content_empty() {
        // complete() must promote reasoning_content into content when content is empty,
        // so downstream json_extract finds the JSON. Drives the *production* promotion
        // helper (not a re-implementation) so the regression guard tracks real code.
        let raw = r#"{
            "id":"chatcmpl-1","created":1700000000,"model":"glm-4.7",
            "choices":[{"index":0,"message":{"role":"assistant","content":"",
                "reasoning_content":"사고 {\"rating\":\"Sell\",\"key_thesis\":\"약세\"}"},"finish_reason":"stop"}]
        }"#;
        let resp: OpenAIChatResponse = serde_json::from_str(raw).unwrap();
        let first = resp.choices.into_iter().next().unwrap();
        let raw_content = first.message.content.unwrap_or_default();
        let reasoning = first.message.reasoning_content.clone();
        let content = promote_reasoning_into_content(raw_content, reasoning.as_deref());
        assert!(
            content.contains("\"rating\":\"Sell\""),
            "promoted content must contain JSON: {content}"
        );
        assert_eq!(
            reasoning.as_deref(),
            Some("사고 {\"rating\":\"Sell\",\"key_thesis\":\"약세\"}")
        );
    }

    #[test]
    fn openai_complete_keeps_content_when_present() {
        // Non-empty content must NOT be overwritten by reasoning.
        let raw = r#"{
            "id":"chatcmpl-1","created":1700000000,"model":"gpt-4o",
            "choices":[{"index":0,"message":{"role":"assistant","content":"answer",
                "reasoning_content":"thought"},"finish_reason":"stop"}]
        }"#;
        let resp: OpenAIChatResponse = serde_json::from_str(raw).unwrap();
        let first = resp.choices.into_iter().next().unwrap();
        let raw_content = first.message.content.clone().unwrap_or_default();
        let reasoning = first.message.reasoning_content.as_deref();
        let content = promote_reasoning_into_content(raw_content.clone(), reasoning);
        assert_eq!(content, "answer");
        assert_eq!(reasoning, Some("thought"));
    }

    #[test]
    fn promote_reasoning_into_content_edge_cases() {
        // #3 caveat: empty content + reasoning surfaces the reasoning as the answer
        // (documented trade-off; original reasoning is the source of truth).
        assert_eq!(
            promote_reasoning_into_content(String::new(), Some("chain-of-thought")),
            "chain-of-thought"
        );
        // content present wins regardless of reasoning.
        assert_eq!(
            promote_reasoning_into_content("ans".into(), Some("cot")),
            "ans"
        );
        // both empty -> empty (no panic, no spurious content).
        assert_eq!(promote_reasoning_into_content(String::new(), None), "");
        assert_eq!(promote_reasoning_into_content("x".into(), None), "x");
    }

    #[test]
    fn openai_response_parses_deepseek_reasoning_alias() {
        // DeepSeek-R1 uses field name `reasoning` instead of `reasoning_content`.
        let raw = r#"{"model":"deepseek-r1","choices":[{"message":{"content":"ans","reasoning":"thought"}}]}"#;
        let resp: OpenAIChatResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(
            resp.choices[0].message.reasoning_content.as_deref(),
            Some("thought")
        );
    }

    #[test]
    fn openai_sse_reasoning_delta_extracted() {
        let data = r#"{"choices":[{"index":0,"delta":{"reasoning_content":"thinking"},"finish_reason":null}]}"#;
        let event = parse_openai_sse(data).unwrap();
        match event {
            StreamEvent::ReasoningDelta { content } => assert_eq!(content, "thinking"),
            other => panic!("expected ReasoningDelta, got {other:?}"),
        }
    }

    #[test]
    fn openai_sse_content_delta_still_works_alongside_reasoning() {
        // Separate content chunk must still produce Delta, not be swallowed.
        let data = r#"{"choices":[{"index":0,"delta":{"content":"answer"},"finish_reason":null}]}"#;
        let event = parse_openai_sse(data).unwrap();
        assert!(matches!(event, StreamEvent::Delta { .. }));
    }

    #[test]
    fn openai_sse_usage_carries_reasoning_tokens() {
        // Final streaming chunk carries choices (empty delta) + usage with reasoning_tokens.
        let data = r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3,
            "completion_tokens_details":{"reasoning_tokens":7}}}"#;
        let event = parse_openai_sse(data).unwrap();
        match event {
            StreamEvent::Usage(u) => assert_eq!(u.reasoning_tokens, Some(7)),
            other => panic!("expected Usage, got {other:?}"),
        }
    }

    #[test]
    fn anthropic_response_parses_tool_use_block() {
        let raw = r#"{
            "id": "msg_1",
            "model": "claude-sonnet-4-6",
            "stop_reason": "tool_use",
            "content": [
                { "type": "text", "text": "Let me check." },
                { "type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": { "location": "Paris" } }
            ],
            "usage": { "input_tokens": 12, "output_tokens": 8 }
        }"#;
        let resp: AnthropicResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
        assert_eq!(resp.content.len(), 2);
        assert_eq!(resp.content[0].block_type, "text");
        assert_eq!(resp.content[1].block_type, "tool_use");
        assert_eq!(resp.content[1].name.as_deref(), Some("get_weather"));
        assert_eq!(resp.content[1].input.as_ref().unwrap()["location"], "Paris");
    }

    #[test]
    fn anthropic_response_parses_thinking_block() {
        // Extended thinking block must surface in LLMResponse.reasoning via the parse loop.
        let raw = r#"{
            "id": "msg_2",
            "model": "claude-sonnet-4-6",
            "stop_reason": "end_turn",
            "content": [
                { "type": "thinking", "thinking": "step by step..." },
                { "type": "text", "text": "Final answer." }
            ],
            "usage": { "input_tokens": 5, "output_tokens": 9 }
        }"#;
        let resp: AnthropicResponse = serde_json::from_str(raw).unwrap();
        let mut reasoning = String::new();
        let mut content = String::new();
        for block in resp.content {
            match block.block_type.as_str() {
                "text" => {
                    if let Some(t) = block.text {
                        content.push_str(&t);
                    }
                }
                "thinking" => {
                    if let Some(t) = block.thinking {
                        reasoning.push_str(&t);
                    }
                }
                _ => {}
            }
        }
        assert_eq!(content, "Final answer.");
        assert_eq!(reasoning, "step by step...");
    }

    #[test]
    fn anthropic_sse_thinking_delta_extracted() {
        let data = r#"{"delta":{"type":"thinking_delta","thinking":"a thought"}}"#;
        let event = parse_anthropic_sse("content_block_delta", data).unwrap();
        match event {
            StreamEvent::ReasoningDelta { content } => assert_eq!(content, "a thought"),
            other => panic!("expected ReasoningDelta, got {other:?}"),
        }
    }
}
