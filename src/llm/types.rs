use std::pin::Pin;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub provider: String,
    pub model: String,
    pub api_key_env: String,
    pub base_url: Option<String>,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            provider: "openai".into(),
            model: "gpt-4o".into(),
            api_key_env: "OPENAI_API_KEY".into(),
            base_url: None,
            temperature: 0.7,
            max_tokens: Some(4096),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMRequest {
    pub system: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMResponse {
    pub content: String,
    pub model: String,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// A single event in an LLM streaming response.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Partial text content arrived.
    Delta { content: String },
    /// Final token usage statistics.
    Usage(TokenUsage),
    /// Stream has ended.
    Done,
}

/// Type alias for a boxed streaming response.
#[cfg(feature = "client-async")]
pub type LLMStream =
    Pin<Box<dyn futures_core::Stream<Item = crate::error::Result<StreamEvent>> + Send>>;
