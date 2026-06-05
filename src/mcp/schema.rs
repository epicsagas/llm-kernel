//! Tool and resource schema definitions for MCP.

use serde::{Deserialize, Serialize};

/// Describes an MCP tool that an AI agent can invoke.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescription {
    /// The tool name (unique within the server).
    pub name: String,
    /// Human-readable description of what the tool does.
    pub description: String,
    /// JSON Schema describing the tool's input parameters.
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_description_serializes() {
        let tool = ToolDescription {
            name: "search".into(),
            description: "Search documents".into(),
            input_schema: serde_json::json!({"type": "object"}),
        };
        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("\"search\""));
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
    }
}
