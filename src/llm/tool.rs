//! Tool/function calling types for LLM APIs.
//!
//! Provides the core types for defining tools, making tool calls, and returning
//! results — compatible with OpenAI and Anthropic function calling APIs.

use serde::{Deserialize, Serialize};

/// Definition of a tool/function that an LLM can invoke.
///
/// Describes the tool's name, purpose, and expected input schema to the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Name of the tool (e.g. `"get_weather"`).
    pub name: String,
    /// Human-readable description of what the tool does.
    pub description: String,
    /// JSON Schema object describing the tool's input parameters.
    pub input_schema: serde_json::Value,
}

/// A tool call requested by the LLM during generation.
///
/// Contains the tool name and serialized arguments as returned by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider-assigned ID for this tool call (used to match results).
    pub id: String,
    /// Name of the tool to invoke.
    pub name: String,
    /// JSON-encoded arguments for the tool call.
    pub arguments: String,
}

/// Result of executing a tool call, to be sent back to the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// ID of the `ToolCall` this result corresponds to.
    pub tool_call_id: String,
    /// Content returned by the tool execution.
    pub content: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_definition_roundtrip() {
        let def = ToolDefinition {
            name: "get_weather".into(),
            description: "Get current weather".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "location": { "type": "string" }
                },
                "required": ["location"]
            }),
        };
        let json = serde_json::to_string(&def).unwrap();
        let back: ToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "get_weather");
        assert_eq!(back.description, "Get current weather");
    }

    #[test]
    fn tool_call_roundtrip() {
        let call = ToolCall {
            id: "call_abc123".into(),
            name: "search".into(),
            arguments: r#"{"query": "rust"}"#.into(),
        };
        let json = serde_json::to_string(&call).unwrap();
        let back: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "call_abc123");
        assert_eq!(back.name, "search");
        assert_eq!(back.arguments, r#"{"query": "rust"}"#);
    }

    #[test]
    fn tool_result_roundtrip() {
        let result = ToolResult {
            tool_call_id: "call_abc123".into(),
            content: "Found 3 results".into(),
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: ToolResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tool_call_id, "call_abc123");
        assert_eq!(back.content, "Found 3 results");
    }
}
