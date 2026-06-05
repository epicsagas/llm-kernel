//! Async LLM client with OpenAI and Anthropic backends.
//!
//! The [`LLMClient`] trait provides a unified interface for chat completion
//! and SSE streaming. Implementations: [`OpenAIClient`], [`AnthropicClient`].
//!
//! Requires the `client-async` feature.

#[cfg(feature = "client-async")]
pub mod client;
pub mod prompt;
pub mod types;

#[cfg(feature = "client-async")]
pub use client::{AnthropicClient, LLMClient, OpenAIClient};
pub use prompt::render_prompt;
#[cfg(feature = "client-async")]
pub use types::LLMStream;
pub use types::{ChatMessage, LLMRequest, LLMResponse, ModelConfig, StreamEvent, TokenUsage};
