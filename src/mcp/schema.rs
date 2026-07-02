//! Tool, resource, and prompt schema definitions for MCP.
//!
//! Field names follow the Model Context Protocol wire format (camelCase for
//! `inputSchema` / `mimeType`), so the serialized JSON is what MCP clients
//! expect from `tools/list`, `resources/list`, and `prompts/list`.

use serde::{Deserialize, Serialize};

/// Describes an MCP tool that an AI agent can invoke.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescription {
    /// The tool name (unique within the server).
    pub name: String,
    /// Human-readable description of what the tool does.
    pub description: String,
    /// JSON Schema describing the tool's input parameters.
    ///
    /// Serialized as `inputSchema` per the MCP wire format.
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

/// Describes an MCP resource that an AI agent can read.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceDescription {
    /// The resource URI (e.g. "docs://project/README.md").
    pub uri: String,
    /// Human-readable name.
    pub name: String,
    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// MIME type (e.g. "text/markdown").
    ///
    /// Serialized as `mimeType` per the MCP wire format.
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// A single argument accepted by an MCP prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptArgument {
    /// Argument name.
    pub name: String,
    /// Human-readable description of the argument.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether the argument must be supplied.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
}

/// Describes an MCP prompt (a reusable, parameterized message template) that a
/// client can list via `prompts/list` and render via `prompts/get`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptDescription {
    /// The prompt name (unique within the server).
    pub name: String,
    /// Human-readable description of what the prompt is for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Arguments the prompt accepts (used to fill the template).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<PromptArgument>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_description_serializes_camelcase_input_schema() {
        let tool = ToolDescription {
            name: "search".into(),
            description: "Search documents".into(),
            input_schema: serde_json::json!({"type": "object"}),
        };
        let json = serde_json::to_value(&tool).unwrap();
        assert!(json.get("inputSchema").is_some(), "expected camelCase key");
        assert!(json.get("input_schema").is_none());
    }

    #[test]
    fn resource_description_serializes_camelcase_mime_type() {
        let res = ResourceDescription {
            uri: "docs://readme".into(),
            name: "README".into(),
            description: Some("Project readme".into()),
            mime_type: Some("text/markdown".into()),
        };
        let json = serde_json::to_value(&res).unwrap();
        assert_eq!(json["mimeType"], "text/markdown");
        assert!(json.get("mime_type").is_none());
    }

    #[test]
    fn resource_description_roundtrip() {
        let res = ResourceDescription {
            uri: "docs://readme".into(),
            name: "README".into(),
            description: Some("Project readme".into()),
            mime_type: Some("text/markdown".into()),
        };
        let json = serde_json::to_string(&res).unwrap();
        let back: ResourceDescription = serde_json::from_str(&json).unwrap();
        assert_eq!(back.uri, "docs://readme");
        assert_eq!(back.mime_type.as_deref(), Some("text/markdown"));
    }

    #[test]
    fn prompt_description_serializes() {
        let prompt = PromptDescription {
            name: "summarize".into(),
            description: Some("Summarize a document".into()),
            arguments: vec![PromptArgument {
                name: "text".into(),
                description: Some("The text to summarize".into()),
                required: true,
            }],
        };
        let json = serde_json::to_value(&prompt).unwrap();
        assert_eq!(json["name"], "summarize");
        assert_eq!(json["arguments"][0]["name"], "text");
        assert_eq!(json["arguments"][0]["required"], true);
    }

    #[test]
    fn prompt_argument_omits_false_required() {
        let arg = PromptArgument {
            name: "opt".into(),
            description: None,
            required: false,
        };
        let json = serde_json::to_value(&arg).unwrap();
        assert!(json.get("required").is_none(), "false required is omitted");
        assert!(json.get("description").is_none());
    }
}
