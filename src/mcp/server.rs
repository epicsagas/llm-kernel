//! MCP server core — tool registration, initialization, and dispatch logic.

use std::collections::HashMap;

use crate::mcp::schema::{ResourceDescription, ToolDescription};

/// Handler function type for MCP tool calls.
pub type Handler = Box<dyn Fn(serde_json::Value) -> crate::error::Result<serde_json::Value> + Send + Sync>;

/// An MCP server that manages tools, resources, and dispatches calls.
pub struct McpServer {
    server_name: String,
    server_version: String,
    tools: Vec<ToolDescription>,
    resources: Vec<ResourceDescription>,
    handlers: HashMap<String, Handler>,
}

impl McpServer {
    /// Create a new MCP server.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            server_name: name.into(),
            server_version: version.into(),
            tools: Vec::new(),
            resources: Vec::new(),
            handlers: HashMap::new(),
        }
    }

    /// Register a tool with the server.
    pub fn register_tool(&mut self, tool: ToolDescription) {
        self.tools.push(tool);
    }

    /// Register a resource with the server.
    pub fn register_resource(&mut self, resource: ResourceDescription) {
        self.resources.push(resource);
    }

    /// Set the handler for a tool by name.
    pub fn set_handler(
        &mut self,
        tool_name: &str,
        handler: impl Fn(serde_json::Value) -> crate::error::Result<serde_json::Value> + Send + Sync + 'static,
    ) {
        self.handlers.insert(tool_name.to_string(), Box::new(handler));
    }

    /// Get the server name.
    pub fn name(&self) -> &str {
        &self.server_name
    }

    /// Get the server version.
    pub fn version(&self) -> &str {
        &self.server_version
    }

    /// List all registered tools.
    pub fn tools(&self) -> &[ToolDescription] {
        &self.tools
    }

    /// List all registered resources.
    pub fn resources(&self) -> &[ResourceDescription] {
        &self.resources
    }

    /// Call a tool by name with the given parameters.
    pub fn call_tool(
        &self,
        name: &str,
        params: serde_json::Value,
    ) -> crate::error::Result<serde_json::Value> {
        let handler = self
            .handlers
            .get(name)
            .ok_or_else(|| crate::error::KernelError::Config(format!("unknown tool: {name}")))?;
        handler(params)
    }

    /// Build the `initialize` response.
    pub fn initialize_response(&self) -> serde_json::Value {
        serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": { "listChanged": false },
                "resources": { "subscribe": false, "listChanged": false },
            },
            "serverInfo": {
                "name": self.server_name,
                "version": self.server_version,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_call_tool() {
        let mut server = McpServer::new("test", "0.1.0");
        server.register_tool(ToolDescription {
            name: "echo".into(),
            description: "Echo input".into(),
            input_schema: serde_json::json!({"type": "object"}),
        });
        server.set_handler("echo", |params| Ok(params));

        let result = server.call_tool("echo", serde_json::json!({"msg": "hi"})).unwrap();
        assert_eq!(result["msg"], "hi");
    }

    #[test]
    fn unknown_tool_returns_error() {
        let server = McpServer::new("test", "0.1.0");
        let result = server.call_tool("missing", serde_json::json!(null));
        assert!(result.is_err());
    }

    #[test]
    fn initialize_response_shape() {
        let server = McpServer::new("my-server", "2.0.0");
        let resp = server.initialize_response();
        assert_eq!(resp["serverInfo"]["name"], "my-server");
        assert_eq!(resp["protocolVersion"], "2024-11-05");
    }

    #[test]
    fn list_tools() {
        let mut server = McpServer::new("test", "0.1.0");
        server.register_tool(ToolDescription {
            name: "a".into(),
            description: "Tool A".into(),
            input_schema: serde_json::json!({}),
        });
        server.register_tool(ToolDescription {
            name: "b".into(),
            description: "Tool B".into(),
            input_schema: serde_json::json!({}),
        });
        assert_eq!(server.tools().len(), 2);
    }
}
