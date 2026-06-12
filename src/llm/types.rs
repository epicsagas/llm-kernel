//! Core types for the LLM client module.
#![deny(missing_docs)]

use std::pin::Pin;

use serde::{Deserialize, Serialize};

/// Configuration for a specific LLM model and provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Provider name (e.g. `"openai"`, `"anthropic"`).
    pub provider: String,
    /// Model identifier (e.g. `"gpt-4o"`, `"claude-sonnet-4-6"`).
    pub model: String,
    /// Environment variable name holding the API key.
    pub api_key_env: String,
    /// Optional base URL override for the provider API.
    pub base_url: Option<String>,
    /// Sampling temperature (0.0–2.0).
    pub temperature: f32,
    /// Maximum tokens to generate in the response.
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

/// A chat completion request to an LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMRequest {
    /// Optional system prompt prepended to the conversation.
    pub system: Option<String>,
    /// Ordered list of chat messages forming the conversation.
    pub messages: Vec<ChatMessage>,
    /// Sampling temperature (0.0–2.0).
    pub temperature: f32,
    /// Maximum tokens to generate. `None` uses the provider default.
    pub max_tokens: Option<u32>,
    /// Model override for this request. `None` uses the client default.
    pub model: Option<String>,
}

impl LLMRequest {
    /// Convert into OpenAI-format messages, consuming the request.
    ///
    /// Prepends a system message if `self.system` is set.
    pub fn into_openai_messages(self) -> Vec<(String, String)> {
        let mut out = Vec::with_capacity(self.messages.len() + 1);
        if let Some(system) = self.system {
            out.push(("system".into(), system));
        }
        for msg in self.messages {
            out.push((msg.role, msg.content));
        }
        out
    }

    /// Convert into Anthropic-format messages, consuming the request.
    ///
    /// Returns only user/assistant messages (system is handled separately by Anthropic API).
    pub fn into_anthropic_messages(self) -> Vec<(String, String)> {
        self.messages
            .into_iter()
            .map(|m| (m.role, m.content))
            .collect()
    }
}

/// A single message in a chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Role of the message sender (e.g. `"system"`, `"user"`, `"assistant"`).
    pub role: String,
    /// Text content of the message.
    pub content: String,
}

impl ChatMessage {
    /// Create a system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }

    /// Create a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }

    /// Create an assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
        }
    }
}

/// A chat completion response from an LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMResponse {
    /// Generated text content.
    pub content: String,
    /// Model that produced this response.
    pub model: String,
    /// Token usage statistics.
    pub usage: TokenUsage,
    /// Reason the generation stopped (e.g. `"stop"`, `"length"`, `"tool_calls"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    /// Provider-assigned response ID (useful for logging and deduplication).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Unix timestamp (seconds) when the response was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<u64>,
}

/// Token usage statistics from an LLM response.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Number of tokens in the prompt.
    pub prompt_tokens: u32,
    /// Number of tokens in the completion.
    pub completion_tokens: u32,
    /// Total tokens (prompt + completion).
    pub total_tokens: u32,
}

/// A single event in an LLM streaming response.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Partial text content arrived.
    Delta {
        /// The partial text chunk.
        content: String,
    },
    /// Final token usage statistics.
    Usage(TokenUsage),
    /// Stream has ended.
    Done,
}

/// Type alias for a boxed streaming response.
#[cfg(feature = "client-async")]
pub type LLMStream =
    Pin<Box<dyn futures_core::Stream<Item = crate::error::Result<StreamEvent>> + Send>>;
