//! MCP config generation for AI coding tools.

use serde::{Deserialize, Serialize};

/// Supported AI agent tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentKind {
    /// Claude Desktop (claude.ai desktop app).
    ClaudeDesktop,
    /// Cursor IDE.
    Cursor,
    /// GitHub Copilot CLI / VS Code extension.
    Copilot,
    /// OpenCode terminal.
    OpenCode,
    /// Cline VS Code extension.
    Cline,
}

impl AgentKind {
    /// All supported agent kinds.
    pub fn all() -> &'static [AgentKind] {
        &[
            AgentKind::ClaudeDesktop,
            AgentKind::Cursor,
            AgentKind::Copilot,
            AgentKind::OpenCode,
            AgentKind::Cline,
        ]
    }

    /// Human-readable display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::ClaudeDesktop => "Claude Desktop",
            Self::Cursor => "Cursor",
            Self::Copilot => "GitHub Copilot",
            Self::OpenCode => "OpenCode",
            Self::Cline => "Cline",
        }
    }

    /// Config file path (relative to home directory).
    pub fn config_path(&self) -> &'static str {
        match self {
            Self::ClaudeDesktop => ".claude.json",
            Self::Cursor => ".cursor/mcp.json",
            Self::Copilot => ".copilot/mcp.json",
            Self::OpenCode => ".opencode.json",
            Self::Cline => ".cline/mcp.json",
        }
    }
}

/// MCP server connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    /// The name shown in the AI tool's MCP server list.
    pub server_name: String,
    /// The command to start the MCP server.
    pub command: String,
    /// Arguments to pass to the command.
    pub args: Vec<String>,
    /// Environment variables to set (name, value pairs).
    /// Values may contain `${VAR}` references.
    pub env: Vec<(String, String)>,
}

/// Generate an MCP configuration snippet for the given agent.
///
/// Returns a JSON string that can be written to the agent's config file
/// or appended to an existing config.
pub fn generate_mcp_config(agent: &AgentKind, config: &McpConfig) -> String {
    match agent {
        AgentKind::ClaudeDesktop => generate_claude_desktop(config),
        AgentKind::Cursor | AgentKind::Copilot => generate_vscode_style(config),
        AgentKind::OpenCode => generate_opencode_style(config),
        AgentKind::Cline => generate_cline_style(config),
    }
}

/// Generate the `mcpServers` JSON block (common format).
fn mcp_server_entry(config: &McpConfig) -> serde_json::Value {
    let mut server = serde_json::json!({
        "command": config.command,
        "args": config.args,
    });

    if !config.env.is_empty() {
        let env_obj: serde_json::Map<String, serde_json::Value> = config
            .env
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        server.as_object_mut().unwrap().insert("env".into(), serde_json::Value::Object(env_obj));
    }

    server
}

fn make_mcp_json(server_name: &str, entry: serde_json::Value) -> String {
    let mut map = serde_json::Map::new();
    map.insert(server_name.to_string(), entry);
    let json = serde_json::json!({ "mcpServers": map });
    serde_json::to_string_pretty(&json).unwrap_or_default()
}

fn generate_claude_desktop(config: &McpConfig) -> String {
    let entry = mcp_server_entry(config);
    make_mcp_json(&config.server_name, entry)
}

fn generate_vscode_style(config: &McpConfig) -> String {
    let entry = mcp_server_entry(config);
    make_mcp_json(&config.server_name, entry)
}

fn generate_opencode_style(config: &McpConfig) -> String {
    let entry = mcp_server_entry(config);
    make_mcp_json(&config.server_name, entry)
}

fn generate_cline_style(config: &McpConfig) -> String {
    let entry = mcp_server_entry(config);
    make_mcp_json(&config.server_name, entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> McpConfig {
        McpConfig {
            server_name: "test-server".into(),
            command: "test-server".into(),
            args: vec!["--stdio".into()],
            env: vec![("TEST_API_KEY".into(), "${TEST_API_KEY}".into())],
        }
    }

    #[test]
    fn all_agents_covered() {
        assert_eq!(AgentKind::all().len(), 5);
        for agent in AgentKind::all() {
            let config = test_config();
            let json = generate_mcp_config(agent, &config);
            assert!(json.contains("test-server"), "missing server name for {:?}", agent);
            assert!(json.contains("TEST_API_KEY"), "missing env for {:?}", agent);
        }
    }

    #[test]
    fn claude_desktop_format() {
        let json = generate_mcp_config(&AgentKind::ClaudeDesktop, &test_config());
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["mcpServers"]["test-server"].is_object());
        assert_eq!(parsed["mcpServers"]["test-server"]["command"], "test-server");
    }

    #[test]
    fn no_env_when_empty() {
        let config = McpConfig {
            server_name: "bare".into(),
            command: "bare".into(),
            args: vec![],
            env: vec![],
        };
        let json = generate_mcp_config(&AgentKind::Cursor, &config);
        assert!(!json.contains("env"), "env should be absent when empty");
    }

    #[test]
    fn agent_display_names() {
        assert_eq!(AgentKind::ClaudeDesktop.display_name(), "Claude Desktop");
        assert_eq!(AgentKind::Cursor.display_name(), "Cursor");
    }
}
