use std::time::Duration;

use async_trait::async_trait;

use crate::error::{KernelError, Result};
use crate::llm::tool::{ToolCall, ToolDefinition};
use crate::llm::types::{
    LLMRequest, LLMResponse, LLMStream, ModelConfig, ResponseFormat, StreamEvent, TokenUsage,
};

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

/// Build a `reqwest::Client` with connect and total timeouts.
fn http_client() -> Result<reqwest::Client> {
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
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse>;
    /// Return the model name this client is configured to use.
    fn model_name(&self) -> &str;

    /// Stream a chat completion, yielding events as they arrive.
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
    pub fn from_key(model: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: "https://api.openai.com/v1".into(),
            client: http_client().unwrap_or_default(),
        }
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
#[derive(serde::Deserialize)]
struct OpenAIRespMessage {
    #[serde(default)]
    content: Option<String>,
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

#[derive(serde::Deserialize)]
struct OpenAIUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
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
        };

        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
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
                message: text,
            });
        }

        let chat_resp: OpenAIChatResponse = resp
            .json()
            .await
            .map_err(|e| KernelError::LlmApi(e.to_string()))?;

        let id = chat_resp.id;
        let created = chat_resp.created;
        let first = chat_resp.choices.into_iter().next();
        let finish_reason = first.as_ref().and_then(|c| c.finish_reason.clone());
        let (content, tool_calls) = match first {
            Some(c) => {
                let content = c.message.content.unwrap_or_default();
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
                (content, calls)
            }
            None => (String::new(), Vec::new()),
        };

        let usage = chat_resp.usage.map(|u| TokenUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        });

        Ok(LLMResponse {
            content,
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
        };

        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
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
                message: text,
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

/// Parse an OpenAI streaming JSON chunk into a StreamEvent.
fn parse_openai_sse(data: &str) -> Option<StreamEvent> {
    let v: serde_json::Value = serde_json::from_str(data).ok()?;

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
            let text = v.get("delta")?.get("text")?.as_str()?;
            if !text.is_empty() {
                return Some(StreamEvent::Delta {
                    content: text.to_string(),
                });
            }
            None
        }
        "message_delta" => {
            let usage = v.get("usage").and_then(|u| {
                Some(TokenUsage {
                    prompt_tokens: 0,
                    completion_tokens: u.get("output_tokens")?.as_u64()? as u32,
                    total_tokens: 0,
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
    pub fn from_key(model: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: "https://api.anthropic.com/v1".into(),
            client: http_client().unwrap_or_default(),
        }
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
/// carry `id`/`name`/`input`.
#[derive(serde::Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: Option<String>,
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
                message: text,
            });
        }

        let chat_resp: AnthropicResponse = resp
            .json()
            .await
            .map_err(|e| KernelError::LlmApi(e.to_string()))?;

        let mut content = String::new();
        let mut tool_calls = Vec::new();
        for block in chat_resp.content {
            match block.block_type.as_str() {
                "text" => {
                    if let Some(t) = block.text {
                        content.push_str(&t);
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
            model: chat_resp.model,
            usage: TokenUsage {
                prompt_tokens: chat_resp.usage.input_tokens,
                completion_tokens: chat_resp.usage.output_tokens,
                total_tokens: chat_resp.usage.input_tokens + chat_resp.usage.output_tokens,
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
                message: text,
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
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["tools"][0]["function"]["name"], "get_weather");
        assert_eq!(json["response_format"]["type"], "json_object");
        // Omitted when None (backward-compatible request shape).
        assert!(json.get("max_tokens").is_none());
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
}
