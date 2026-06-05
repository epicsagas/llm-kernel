//! MCP (Model Context Protocol) server framework.
//!
//! Provides a JSON-RPC 2.0 dispatch layer for building MCP servers in Rust.
//! Supports both stdio and HTTP transports.
//!
//! ## Quick start
//!
//! ```ignore
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
pub use server::{Handler, McpServer};
pub use transport::JsonRpcDispatcher;
