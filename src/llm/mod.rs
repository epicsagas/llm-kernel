//! Async LLM client with OpenAI and Anthropic backends.
//!
//! The [`LLMClient`] trait provides a unified interface for chat completion
//! and SSE streaming. Implementations: [`OpenAIClient`], [`AnthropicClient`].
//!
//! Resilience composes from focused decorators that all implement
//! [`LLMClient`]: [`RetryClient`] (rate-limit / 5xx backoff), [`RouterClient`]
//! (cost-aware routing + cross-provider fallback), [`MiddlewareClient`]
//! (request/response observation), and [`CacheClient`] (response cache). See
//! [`router`] for the composition table and a worked stack example.
//!
//! The [`json_extract`] module handles extracting structured JSON from
//! raw LLM text output (code fences, raw JSON, etc.).
//!
//! Requires the `client-async` feature.

/// Response cache wrapper for [`LLMClient`] over a [`KvStore`].
///
/// [`KvStore`]: crate::store::KvStore
#[cfg(feature = "cache")]
pub mod cache;
/// Async LLM client implementations (OpenAI, Anthropic).
#[cfg(feature = "client-async")]
pub mod client;
/// Conversation history with token-budget-aware truncation.
#[cfg(feature = "tokens")]
pub mod history;
/// JSON extraction from raw LLM text output.
pub mod json_extract;
/// Middleware hooks for [`LLMClient`] request/response lifecycle.
#[cfg(feature = "client-async")]
pub mod middleware;
/// Prompt template rendering.
pub mod prompt;
/// Exponential backoff retry wrapper for [`LLMClient`].
#[cfg(feature = "client-async")]
pub mod retry;
/// Cost-aware routing and cross-provider fallback chain for [`LLMClient`].
#[cfg(feature = "client-async")]
pub mod router;
/// Prompt templates with variable substitution and few-shot examples.
pub mod template;
/// Tool/function calling types.
pub mod tool;
/// Core LLM request/response types.
pub mod types;

#[cfg(feature = "cache")]
pub use cache::CacheClient;
#[cfg(feature = "client-async")]
pub use client::{AnthropicClient, LLMClient, OpenAIClient};
#[cfg(feature = "tokens")]
pub use history::ConversationHistory;
pub use json_extract::{JsonExtractor, extract_json, parse_json};
#[cfg(feature = "client-async")]
pub use middleware::{LLMClientMiddleware, MiddlewareClient, NoopMiddleware};
pub use prompt::render_prompt;
#[cfg(feature = "client-async")]
pub use retry::{RetryClient, RetryConfig};
#[cfg(feature = "client-async")]
pub use router::{Backend, RouterClient, RoutingStrategy};
pub use template::PromptTemplate;
pub use tool::{ToolCall, ToolDefinition, ToolResult};
#[cfg(feature = "client-async")]
pub use types::LLMStream;
pub use types::{
    ChatMessage, ContentPart, LLMRequest, LLMRequestBuilder, LLMResponse, MessageRole, ModelConfig,
    ResponseFormat, StreamEvent, TokenUsage,
};
