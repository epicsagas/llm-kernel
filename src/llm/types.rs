//! Core types for the LLM client module.
#![deny(missing_docs)]

use std::fmt;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

/// Role of a message sender in a chat conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    /// System-level instruction message.
    System,
    /// User input message.
    User,
    /// Assistant response message.
    Assistant,
    /// Tool/function result message.
    Tool,
}

impl fmt::Display for MessageRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::System => write!(f, "system"),
            Self::User => write!(f, "user"),
            Self::Assistant => write!(f, "assistant"),
            Self::Tool => write!(f, "tool"),
        }
    }
}

/// A single content part in a multimodal chat message.
///
/// Supports text, image URLs, and base64-encoded images.
/// Single-text messages serialize as a plain string for backward compatibility
/// with OpenAI and Anthropic APIs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    /// Plain text content.
    Text {
        /// The text string.
        text: String,
    },
    /// Image specified by URL.
    ImageUrl {
        /// URL pointing to the image.
        url: String,
    },
    /// Image specified as base64-encoded data.
    ImageBase64 {
        /// MIME type (e.g. `"image/png"`).
        media_type: String,
        /// Base64-encoded image data.
        data: String,
    },
}

impl ContentPart {
    /// Create a text content part.
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text { text: s.into() }
    }

    /// Create an image URL content part.
    pub fn image_url(url: impl Into<String>) -> Self {
        Self::ImageUrl { url: url.into() }
    }

    /// Extract text content, if this is a text part.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            _ => None,
        }
    }
}

/// Serde helper: serialize `Vec<ContentPart>` as a plain string when there's
/// a single text entry, or as an array otherwise.
mod content_vec_serde {
    use super::ContentPart;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(parts: &[ContentPart], s: S) -> Result<S::Ok, S::Error> {
        if parts.len() == 1
            && let ContentPart::Text { text } = &parts[0]
        {
            return s.serialize_str(text);
        }
        parts.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<ContentPart>, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum StringOrParts {
            S(String),
            P(Vec<ContentPart>),
        }
        match StringOrParts::deserialize(d)? {
            StringOrParts::S(s) => Ok(vec![ContentPart::text(s)]),
            StringOrParts::P(v) => Ok(v),
        }
    }
}

/// A single message in a chat conversation.
///
/// Implements [`Default`] for forward-compatible struct-update syntax.
/// Prefer the `ChatMessage::system` / `::user` / `::assistant` / `::tool`
/// constructors for clarity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Role of the message sender.
    pub role: MessageRole,
    /// Content parts (text, images). Serializes as a plain string when
    /// containing a single text part for backward compatibility.
    #[serde(with = "content_vec_serde")]
    pub content: Vec<ContentPart>,
}

impl Default for ChatMessage {
    fn default() -> Self {
        Self {
            role: MessageRole::User,
            content: Vec::new(),
        }
    }
}

impl ChatMessage {
    /// Create a system message with text content.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: vec![ContentPart::text(content)],
        }
    }

    /// Create a user message with text content.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: vec![ContentPart::text(content)],
        }
    }

    /// Create an assistant message with text content.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: vec![ContentPart::text(content)],
        }
    }

    /// Create a tool result message.
    pub fn tool(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Tool,
            content: vec![ContentPart::text(content)],
        }
    }

    /// Create a user message with multimodal content parts.
    pub fn user_multimodal(parts: Vec<ContentPart>) -> Self {
        Self {
            role: MessageRole::User,
            content: parts,
        }
    }

    /// Extract all text from this message's content parts.
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|p| p.as_text())
            .collect::<Vec<_>>()
            .join("")
    }
}

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

/// Desired output format for the LLM response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseFormat {
    /// Plain text response (default).
    Text,
    /// JSON object response.
    Json,
    /// JSON response conforming to the given schema.
    JsonSchema {
        /// JSON Schema the response must satisfy.
        schema: serde_json::Value,
    },
}

/// A chat completion request to an LLM provider.
///
/// This struct implements [`Default`] so callers can use struct-update syntax
/// to stay forward-compatible with future field additions:
///
/// ```rust,ignore
/// let req = LLMRequest {
///     system: Some("...".into()),
///     messages: vec![ChatMessage::user("hi")],
///     ..LLMRequest::default()
/// };
/// ```
///
/// New fields added to `LLMRequest` in future non-breaking releases are
/// absorbed by `..LLMRequest::default()` and will not break such call sites
/// (unlike full struct literals, which must enumerate every field). For the
/// fluent equivalent, see [`LLMRequest::builder`].
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
    /// Desired response format. `None` uses the provider default.
    ///
    /// Forwarded to the provider by [`OpenAIClient`](crate::llm::OpenAIClient)
    /// (OpenAI `response_format`) and, for [`ResponseFormat::JsonSchema`], by
    /// [`AnthropicClient`](crate::llm::AnthropicClient) (Anthropic
    /// `output_config.format`). [`ResponseFormat::Json`] without a schema has no
    /// native Anthropic equivalent and is a no-op there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    /// Tool definitions available to the model for this request.
    ///
    /// Forwarded to both OpenAI (`tools` with `type: "function"`) and Anthropic
    /// (`tools` with `input_schema`). Any tool calls the model makes are returned
    /// in [`LLMResponse::tool_calls`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<crate::llm::ToolDefinition>>,
}

impl Default for LLMRequest {
    fn default() -> Self {
        Self {
            system: None,
            messages: Vec::new(),
            // Matches `LLMRequestBuilder::build()`'s `unwrap_or(0.7)` so the two
            // construction paths agree. Keep these coupled.
            temperature: 0.7,
            max_tokens: None,
            model: None,
            response_format: None,
            tools: None,
        }
    }
}

impl LLMRequest {
    /// Create a new builder for constructing an `LLMRequest`.
    pub fn builder() -> LLMRequestBuilder {
        LLMRequestBuilder::default()
    }

    /// Convert into OpenAI-format messages, consuming the request.
    ///
    /// Prepends a system message if `self.system` is set.
    pub(crate) fn into_openai_messages(self) -> Vec<(String, String)> {
        let mut out = Vec::with_capacity(self.messages.len() + 1);
        if let Some(system) = self.system {
            out.push(("system".into(), system));
        }
        for msg in self.messages {
            out.push((msg.role.to_string(), msg.text_content()));
        }
        out
    }

    /// Convert into Anthropic-format messages, consuming the request.
    ///
    /// Returns only user/assistant messages (system is handled separately by Anthropic API).
    pub(crate) fn into_anthropic_messages(self) -> Vec<(String, String)> {
        self.messages
            .into_iter()
            .map(|m| (m.role.to_string(), m.text_content()))
            .collect()
    }
}

/// Builder for constructing `LLMRequest` instances with a fluent API.
///
/// # Example
///
/// ```no_run
/// use llm_kernel::llm::LLMRequest;
///
/// let request = LLMRequest::builder()
///     .system("You are concise.")
///     .user_message("Summarise Rust ownership in one line.")
///     .temperature(0.0)
///     .build();
/// ```
#[derive(Debug, Clone, Default)]
pub struct LLMRequestBuilder {
    system: Option<String>,
    messages: Vec<ChatMessage>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    model: Option<String>,
    response_format: Option<ResponseFormat>,
    tools: Option<Vec<crate::llm::ToolDefinition>>,
}

impl LLMRequestBuilder {
    /// Set the system prompt.
    pub fn system(mut self, prompt: impl Into<String>) -> Self {
        self.system = Some(prompt.into());
        self
    }

    /// Append a user message.
    pub fn user_message(mut self, content: impl Into<String>) -> Self {
        self.messages.push(ChatMessage::user(content));
        self
    }

    /// Append an assistant message.
    pub fn assistant_message(mut self, content: impl Into<String>) -> Self {
        self.messages.push(ChatMessage::assistant(content));
        self
    }

    /// Append a raw `ChatMessage`.
    pub fn message(mut self, msg: ChatMessage) -> Self {
        self.messages.push(msg);
        self
    }

    /// Replace the message list with the provided messages.
    ///
    /// Convenience for callers that already hold a `Vec<ChatMessage>` (e.g. a
    /// pre-built conversation), avoiding repeated `.message()` calls.
    pub fn messages(mut self, messages: Vec<ChatMessage>) -> Self {
        self.messages = messages;
        self
    }

    /// Set the sampling temperature.
    pub fn temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// Set the maximum tokens to generate.
    pub fn max_tokens(mut self, tokens: u32) -> Self {
        self.max_tokens = Some(tokens);
        self
    }

    /// Set the maximum tokens to generate, or `None` to use the provider default.
    ///
    /// Convenience for callers that already hold an `Option<u32>` (e.g. a
    /// config field), avoiding a conditional chain.
    pub fn maybe_max_tokens(mut self, tokens: Option<u32>) -> Self {
        self.max_tokens = tokens;
        self
    }

    /// Override the model for this request.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the desired response format.
    pub fn response_format(mut self, format: ResponseFormat) -> Self {
        self.response_format = Some(format);
        self
    }

    /// Set the tool definitions available to the model.
    pub fn tools(mut self, tools: Vec<crate::llm::ToolDefinition>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Build the `LLMRequest`.
    pub fn build(self) -> LLMRequest {
        LLMRequest {
            system: self.system,
            messages: self.messages,
            temperature: self.temperature.unwrap_or(0.7),
            max_tokens: self.max_tokens,
            model: self.model,
            response_format: self.response_format,
            tools: self.tools,
        }
    }
}

/// A chat completion response from an LLM provider.
///
/// Implements [`Default`] for forward-compatible struct-update syntax
/// (`LLMResponse { ..LLMResponse::default() }`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LLMResponse {
    /// Generated text content.
    pub content: String,
    /// Model that produced this response.
    pub model: String,
    /// Token usage statistics.
    pub usage: TokenUsage,
    /// Tool calls the model requested this turn.
    ///
    /// Empty unless the request supplied [`LLMRequest::tools`] and the model
    /// chose to call one. Each entry carries the provider-assigned call `id`,
    /// tool `name`, and JSON-encoded `arguments`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<crate::llm::ToolCall>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_role_display() {
        assert_eq!(MessageRole::System.to_string(), "system");
        assert_eq!(MessageRole::User.to_string(), "user");
        assert_eq!(MessageRole::Assistant.to_string(), "assistant");
        assert_eq!(MessageRole::Tool.to_string(), "tool");
    }

    #[test]
    fn message_role_serde_roundtrip() {
        let json = serde_json::to_string(&MessageRole::User).unwrap();
        assert_eq!(json, "\"user\"");
        let back: MessageRole = serde_json::from_str(&json).unwrap();
        assert_eq!(back, MessageRole::User);
    }

    #[test]
    fn chat_message_constructors() {
        let sys = ChatMessage::system("instructions");
        assert_eq!(sys.role, MessageRole::System);

        let user = ChatMessage::user("hello");
        assert_eq!(user.role, MessageRole::User);

        let asst = ChatMessage::assistant("hi there");
        assert_eq!(asst.role, MessageRole::Assistant);

        let tool = ChatMessage::tool("result");
        assert_eq!(tool.role, MessageRole::Tool);
    }

    #[test]
    fn single_text_serializes_as_string() {
        let msg = ChatMessage::user("hello");
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"content\":\"hello\""), "got: {json}");
    }

    #[test]
    fn multipart_serializes_as_array() {
        let msg = ChatMessage::user_multimodal(vec![
            ContentPart::text("describe this"),
            ContentPart::image_url("https://example.com/img.png"),
        ]);
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains("\"content\":["),
            "expected array serialization, got: {json}"
        );
    }

    #[test]
    fn single_text_deserialize_from_string() {
        let json = r#"{"role":"user","content":"hello"}"#;
        let msg: ChatMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(msg.content.len(), 1);
        assert_eq!(msg.text_content(), "hello");
    }

    #[test]
    fn multipart_deserialize_from_array() {
        let json = r#"{"role":"user","content":[{"type":"text","text":"hi"},{"type":"image_url","url":"https://x.com/img.png"}]}"#;
        let msg: ChatMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.content.len(), 2);
    }

    #[test]
    fn content_part_text_helper() {
        let p = ContentPart::text("hello");
        assert_eq!(p.as_text(), Some("hello"));
    }

    #[test]
    fn response_format_json_serialization() {
        let fmt = ResponseFormat::Json;
        let json = serde_json::to_string(&fmt).unwrap();
        assert!(json.contains("\"type\":\"json\""), "got: {json}");
    }

    #[test]
    fn response_format_text_serialization() {
        let fmt = ResponseFormat::Text;
        let json = serde_json::to_string(&fmt).unwrap();
        assert!(json.contains("\"type\":\"text\""), "got: {json}");
    }

    #[test]
    fn response_format_json_schema() {
        let fmt = ResponseFormat::JsonSchema {
            schema: serde_json::json!({"type": "object"}),
        };
        let json = serde_json::to_string(&fmt).unwrap();
        assert!(json.contains("json_schema"), "got: {json}");
    }

    #[test]
    fn builder_basic() {
        let req = LLMRequest::builder()
            .system("you are helpful")
            .user_message("hello")
            .temperature(0.5)
            .build();
        assert_eq!(req.system.as_deref(), Some("you are helpful"));
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.temperature, 0.5);
    }

    #[test]
    fn builder_with_model_and_format() {
        let req = LLMRequest::builder()
            .user_message("test")
            .model("gpt-4o-mini")
            .response_format(ResponseFormat::Json)
            .max_tokens(100)
            .build();
        assert_eq!(req.model.as_deref(), Some("gpt-4o-mini"));
        assert!(matches!(req.response_format, Some(ResponseFormat::Json)));
        assert_eq!(req.max_tokens, Some(100));
    }

    #[test]
    fn builder_with_tools() {
        use crate::llm::ToolDefinition;
        let req = LLMRequest::builder()
            .user_message("what's the weather?")
            .tools(vec![ToolDefinition {
                name: "get_weather".into(),
                description: "Get weather".into(),
                input_schema: serde_json::json!({"type": "object"}),
            }])
            .build();
        assert!(req.tools.is_some());
        assert_eq!(req.tools.unwrap().len(), 1);
    }

    /// `LLMRequest::default()` and `LLMRequest::builder().build()` must agree on
    /// every field. The temperature default (0.7) in particular is duplicated
    /// between the manual `Default` impl and the builder's `unwrap_or(0.7)`;
    /// this test couples them so a future edit to one without the other is caught.
    #[test]
    fn default_matches_builder_default() {
        let from_default = LLMRequest::default();
        let from_builder = LLMRequest::builder().build();
        assert_eq!(from_default.temperature, from_builder.temperature);
        assert_eq!(from_default.temperature, 0.7);
        assert!(from_default.system.is_none());
        assert!(from_default.messages.is_empty());
        assert!(from_default.max_tokens.is_none());
        assert!(from_default.model.is_none());
        assert!(from_default.response_format.is_none());
        assert!(from_default.tools.is_none());
    }

    #[test]
    fn builder_messages_setter_replaces_list() {
        let conv = vec![ChatMessage::user("first"), ChatMessage::assistant("second")];
        let req = LLMRequest::builder().messages(conv).build();
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0].role, MessageRole::User);
        assert_eq!(req.messages[1].role, MessageRole::Assistant);
    }

    #[test]
    fn builder_maybe_max_tokens_accepts_option() {
        // Some — sets the value
        let req = LLMRequest::builder().maybe_max_tokens(Some(512)).build();
        assert_eq!(req.max_tokens, Some(512));
        // None — explicitly defers to provider default
        let req = LLMRequest::builder().maybe_max_tokens(None).build();
        assert_eq!(req.max_tokens, None);
    }

    #[test]
    fn into_openai_messages_with_system() {
        let req = LLMRequest::builder()
            .system("be helpful")
            .user_message("hi")
            .assistant_message("hello")
            .build();
        let msgs = req.into_openai_messages();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].0, "system");
        assert_eq!(msgs[1].0, "user");
        assert_eq!(msgs[2].0, "assistant");
    }

    #[test]
    fn into_anthropic_messages_excludes_system() {
        let req = LLMRequest::builder()
            .system("be helpful")
            .user_message("hi")
            .build();
        let msgs = req.into_anthropic_messages();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].0, "user");
    }

    #[test]
    fn text_content_extracts_text() {
        let msg = ChatMessage::user_multimodal(vec![
            ContentPart::text("hello "),
            ContentPart::image_url("http://x.com/i.png"),
            ContentPart::text("world"),
        ]);
        assert_eq!(msg.text_content(), "hello world");
    }
}
