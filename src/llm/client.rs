use async_trait::async_trait;

use crate::error::{KernelError, Result};
use crate::llm::types::{LLMRequest, LLMResponse, ModelConfig, TokenUsage};

#[async_trait]
pub trait LLMClient: Send + Sync {
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse>;
    fn model_name(&self) -> &str;
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
}
