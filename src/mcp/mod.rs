//! MCP (Model Context Protocol) server framework.
//!
//! Provides a JSON-RPC 2.0 dispatch layer for building MCP servers in Rust.
//! Supports both stdio and HTTP transports.
//!
//! ## Quick start
//!
//! ```no_run
//! use llm_kernel::mcp::{McpServer, ToolDescription, JsonRpcDispatcher};
//!
//! let mut server = McpServer::new("my-server", "1.0.0");
//!
//! server.register_tool(ToolDescription {
//!     name: "greet".into(),
//!     description: "Say hello".into(),
//!     input_schema: serde_json::json!({
//!         "type": "object",
//!         "properties": {
//!             "name": { "type": "string" }
//!         }
//!     }),
//! });
//!
//! server.set_handler("greet", |params| {
//!     Ok(serde_json::json!({ "greeting": "Hello!" }))
//! });
//! ```

pub mod auth;
pub mod schema;
pub mod server;
pub mod transport;

pub use auth::BearerAuth;
pub use schema::{ResourceDescription, ToolDescription};
pub use server::{AsyncToolHandler, Handler, McpServer};
pub use transport::JsonRpcDispatcher;

/// HTTP/SSE remote transport for MCP (axum + tokio).
#[cfg(feature = "mcp-http")]
pub mod http;
#[cfg(feature = "mcp-http")]
pub use http::{HttpTransport, serve};

/// MCP notification types for server-initiated messages.
#[derive(Debug, Clone)]
pub enum McpNotification {
    /// The list of available tools has changed.
    ToolsListChanged,
    /// The list of available resources has changed.
    ResourcesListChanged,
    /// Progress notification for a long-running operation.
    Progress {
        /// Opaque token identifying the in-progress operation.
        progress_token: String,
        /// Current progress value.
        progress: u64,
        /// Total expected value, if known.
        total: Option<u64>,
    },
}

impl McpServer {
    /// Format a notification as a JSON-RPC message string.
    pub fn format_notification(&self, notification: McpNotification) -> String {
        let method = match &notification {
            McpNotification::ToolsListChanged => "notifications/tools/list_changed",
            McpNotification::ResourcesListChanged => "notifications/resources/list_changed",
            McpNotification::Progress { .. } => "notifications/progress",
        };
        let mut params = serde_json::json!({});
        if let McpNotification::Progress {
            progress_token,
            progress,
            total,
        } = &notification
        {
            params["progressToken"] = serde_json::json!(progress_token);
            params["progress"] = serde_json::json!(progress);
            if let Some(t) = total {
                params["total"] = serde_json::json!(t);
            }
        }
        serde_json::to_string(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
        .unwrap_or_default()
    }
}
