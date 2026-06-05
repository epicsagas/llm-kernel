use async_trait::async_trait;

use crate::error::{KernelError, Result};
use crate::llm::types::{LLMRequest, LLMResponse, LLMStream, ModelConfig, StreamEvent, TokenUsage};

#[async_trait]
pub trait LLMClient: Send + Sync {
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse>;
    fn model_name(&self) -> &str;

    /// Stream a chat completion, yielding events as they arrive.
    async fn stream_complete(&self, request: LLMRequest) -> Result<LLMStream>;
}

pub struct OpenAIClient {
    api_key: String,
    model: String,
    base_url: String,
    client: reqwest::Client,
}

impl OpenAIClient {
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
            client: reqwest::Client::new(),
        })
    }

    pub fn from_key(model: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: "https://api.openai.com/v1".into(),
            client: reqwest::Client::new(),
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
}

#[derive(serde::Serialize, serde::Deserialize)]
struct OpenAIChatMessage {
    role: String,
    content: String,
}

#[derive(serde::Deserialize)]
struct OpenAIChatResponse {
    choices: Vec<OpenAIChoice>,
    model: String,
    usage: Option<OpenAIUsage>,
}

#[derive(serde::Deserialize)]
struct OpenAIChoice {
    message: OpenAIChatMessage,
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
        let mut messages = Vec::new();
        if let Some(system) = &request.system {
            messages.push(OpenAIChatMessage {
                role: "system".into(),
                content: system.clone(),
            });
        }
        for msg in &request.messages {
            messages.push(OpenAIChatMessage {
                role: msg.role.clone(),
                content: msg.content.clone(),
            });
        }

        let body = OpenAIChatRequest {
            model: request.model.unwrap_or_else(|| self.model.clone()),
            messages,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream: false,
        };

        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| KernelError::LlmApi(e.to_string()))?;

        let status = resp.status();
        if status.as_u16() == 429 {
            let retry = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
                .unwrap_or(60);
            return Err(KernelError::RateLimited(retry));
        }

        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(KernelError::LlmApi(format!("HTTP {}: {}", status, text)));
        }

        let chat_resp: OpenAIChatResponse = resp
            .json()
            .await
            .map_err(|e| KernelError::LlmApi(e.to_string()))?;

        let content = chat_resp
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();

        let usage = chat_resp.usage.map(|u| TokenUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        });

        Ok(LLMResponse {
            content,
            model: chat_resp.model,
            usage: usage.unwrap_or_default(),
        })
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    async fn stream_complete(&self, request: LLMRequest) -> Result<LLMStream> {
        let mut messages = Vec::new();
        if let Some(system) = &request.system {
            messages.push(OpenAIChatMessage {
                role: "system".into(),
                content: system.clone(),
            });
        }
        for msg in &request.messages {
            messages.push(OpenAIChatMessage {
                role: msg.role.clone(),
                content: msg.content.clone(),
            });
        }

        let body = OpenAIChatRequest {
            model: request.model.unwrap_or_else(|| self.model.clone()),
            messages,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream: true,
        };

        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| KernelError::LlmApi(e.to_string()))?;

        let status = resp.status();
        if status.as_u16() == 429 {
            let retry = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
                .unwrap_or(60);
            return Err(KernelError::RateLimited(retry));
        }
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(KernelError::LlmApi(format!("HTTP {}: {}", status, text)));
        }

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<StreamEvent>>(16);

        tokio::spawn(async move {
            let mut stream = std::pin::pin!(resp.bytes_stream());
            let mut buffer = String::new();

            use tokio_stream::StreamExt;

            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = tx.send(Err(KernelError::LlmApi(e.to_string()))).await;
                        return;
                    }
                };
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(pos) = buffer.find('\n') {
                    let line = buffer[..pos].trim_end().to_string();
                    buffer = buffer[pos + 1..].to_string();

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

pub struct AnthropicClient {
    api_key: String,
    model: String,
    base_url: String,
    client: reqwest::Client,
}

impl AnthropicClient {
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
            client: reqwest::Client::new(),
        })
    }

    pub fn from_key(model: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: "https://api.anthropic.com/v1".into(),
            client: reqwest::Client::new(),
        }
    }
}

#[derive(serde::Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(serde::Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
    model: String,
    usage: AnthropicUsage,
}

#[derive(serde::Deserialize)]
struct AnthropicContentBlock {
    text: Option<String>,
}

#[derive(serde::Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[async_trait]
impl LLMClient for AnthropicClient {
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse> {
        let messages: Vec<AnthropicMessage> = request
            .messages
            .into_iter()
            .map(|m| AnthropicMessage {
                role: m.role,
                content: m.content,
            })
            .collect();

        let body = AnthropicRequest {
            model: request.model.unwrap_or_else(|| self.model.clone()),
            max_tokens: request.max_tokens.unwrap_or(4096),
            system: request.system,
            messages,
            stream: false,
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

        let status = resp.status();
        if status.as_u16() == 429 {
            let retry = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
                .unwrap_or(60);
            return Err(KernelError::RateLimited(retry));
        }

        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(KernelError::LlmApi(format!("HTTP {}: {}", status, text)));
        }

        let chat_resp: AnthropicResponse = resp
            .json()
            .await
            .map_err(|e| KernelError::LlmApi(e.to_string()))?;

        let content = chat_resp
            .content
            .into_iter()
            .filter_map(|b| b.text)
            .collect::<Vec<_>>()
            .join("");

        Ok(LLMResponse {
            content,
            model: chat_resp.model,
            usage: TokenUsage {
                prompt_tokens: chat_resp.usage.input_tokens,
                completion_tokens: chat_resp.usage.output_tokens,
                total_tokens: chat_resp.usage.input_tokens + chat_resp.usage.output_tokens,
            },
        })
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    async fn stream_complete(&self, request: LLMRequest) -> Result<LLMStream> {
        let messages: Vec<AnthropicMessage> = request
            .messages
            .iter()
            .map(|m| AnthropicMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        let body = AnthropicRequest {
            model: request.model.clone().unwrap_or_else(|| self.model.clone()),
            max_tokens: request.max_tokens.unwrap_or(4096),
            system: request.system.clone(),
            messages,
            stream: true,
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

        let status = resp.status();
        if status.as_u16() == 429 {
            let retry = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
                .unwrap_or(60);
            return Err(KernelError::RateLimited(retry));
        }
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(KernelError::LlmApi(format!("HTTP {}: {}", status, text)));
        }

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<StreamEvent>>(16);

        tokio::spawn(async move {
            let mut stream = std::pin::pin!(resp.bytes_stream());
            let mut buffer = String::new();
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
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(pos) = buffer.find('\n') {
                    let line = buffer[..pos].trim_end().to_string();
                    buffer = buffer[pos + 1..].to_string();

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
}
