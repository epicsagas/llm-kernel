//! Async LLM client with OpenAI and Anthropic backends.
//!
//! The [`LLMClient`] trait provides a unified interface for chat completion
//! and SSE streaming. Implementations: [`OpenAIClient`], [`AnthropicClient`].
//!
//! The [`json_extract`] module handles extracting structured JSON from
//! raw LLM text output (code fences, raw JSON, etc.).
//!
//! Requires the `client-async` feature.

#[cfg(feature = "client-async")]
pub mod client;
pub mod json_extract;
pub mod prompt;
pub mod types;

#[cfg(feature = "client-async")]
pub use client::{AnthropicClient, LLMClient, OpenAIClient};
pub use json_extract::{extract_json, parse_json, JsonExtractor};
pub use prompt::render_prompt;
#[cfg(feature = "client-async")]
pub use types::LLMStream;
pub use types::{ChatMessage, LLMRequest, LLMResponse, ModelConfig, StreamEvent, TokenUsage};
