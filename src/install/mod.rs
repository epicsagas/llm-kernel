//! AI tool installation wizard.
//!
//! Generates MCP configuration snippets for popular AI coding tools.
//! Each tool has its own config file format, env var syntax, and binary path.
//!
//! ```
//! use llm_kernel::install::{AgentKind, McpConfig, generate_mcp_config};
//!
//! let config = McpConfig {
//!     server_name: "my-server".into(),
//!     command: "my-server".into(),
//!     args: vec!["serve".into()],
//!     env: vec![("MY_API_KEY".into(), "${MY_API_KEY}".into())],
//! };
//!
//! let json = generate_mcp_config(&AgentKind::ClaudeDesktop, &config);
//! assert!(json.contains("my-server"));
//! ```

pub mod wizard;

pub use wizard::{generate_mcp_config, AgentKind, McpConfig};

