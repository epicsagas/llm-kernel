//! JSON-RPC 2.0 transport and dispatch for MCP.
//!
//! Reads JSON-RPC requests from stdin (stdio transport) and routes them
//! to the appropriate [`McpServer`] handler. Writes responses to stdout.

use std::io::{self, BufRead, Write};

use crate::mcp::server::McpServer;

/// JSON-RPC 2.0 dispatcher for MCP stdio transport.
pub struct JsonRpcDispatcher<'a> {
    server: &'a McpServer,
}

impl<'a> JsonRpcDispatcher<'a> {
    /// Create a new dispatcher wrapping an MCP server.
    pub fn new(server: &'a McpServer) -> Self {
        Self { server }
    }

    /// Dispatch a single JSON-RPC request and return the response.
    pub fn dispatch(&self, request: &str) -> Option<String> {
        let req: serde_json::Value = match serde_json::from_str(request) {
            Ok(v) => v,
            Err(e) => {
                return Some(self.error_response(
                    None,
                    -32700,
                    &format!("Parse error: {e}"),
                ));
            }
        };

        let id = req.get("id").and_then(|v| v.as_i64());
        let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");

        let result = match method {
            "initialize" => Ok(self.server.initialize_response()),
            "tools/list" => Ok(serde_json::json!({
                "tools": self.server.tools()
            })),
            "resources/list" => Ok(serde_json::json!({
                "resources": self.server.resources()
            })),
            "tools/call" => self.handle_tool_call(&req),
            _ => Err((-32601, format!("Method not found: {method}"))),
        };

        match result {
            Ok(value) => Some(self.success_response(id, value)),
            Err((code, message)) => Some(self.error_response(id, code, &message)),
        }
    }

    /// Run the stdio transport loop: read lines from stdin, dispatch, write to stdout.
    pub fn run_stdio(&self) -> io::Result<()> {
        let stdin = io::stdin();
        let mut stdout = io::stdout().lock();

        for line in stdin.lock().lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(response) = self.dispatch(trimmed) {
                writeln!(stdout, "{response}")?;
                stdout.flush()?;
            }
        }
        Ok(())
    }

    fn handle_tool_call(&self, req: &serde_json::Value) -> std::result::Result<serde_json::Value, (i32, String)> {
        let tool_name = req
            .get("params")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("");

        let params = req
            .get("params")
            .and_then(|p| p.get("arguments"))
            .cloned()
            .unwrap_or(serde_json::json!(null));

        self.server
            .call_tool(tool_name, params)
            .map(|result| {
                serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": result.to_string()
                    }]
                })
            })
            .map_err(|e| (-32603, e.to_string()))
    }

    fn success_response(&self, id: Option<i64>, result: serde_json::Value) -> String {
        let mut resp = serde_json::json!({
            "jsonrpc": "2.0",
            "result": result,
        });
        if let Some(id) = id {
            resp["id"] = serde_json::json!(id);
        }
        serde_json::to_string(&resp).unwrap_or_default()
    }

    fn error_response(&self, id: Option<i64>, code: i32, message: &str) -> String {
        let mut resp = serde_json::json!({
            "jsonrpc": "2.0",
            "error": {
                "code": code,
                "message": message,
            }
        });
        if let Some(id) = id {
            resp["id"] = serde_json::json!(id);
        }
        serde_json::to_string(&resp).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::schema::ToolDescription;

    fn test_server() -> McpServer {
        let mut server = McpServer::new("test-server", "0.1.0");
        server.register_tool(ToolDescription {
            name: "echo".into(),
            description: "Echo input".into(),
            input_schema: serde_json::json!({"type": "object"}),
        });
        server.set_handler("echo", |params| Ok(params));
        server
    }

    #[test]
    fn dispatch_initialize() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let resp = dispatcher.dispatch(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(parsed["result"]["serverInfo"]["name"], "test-server");
    }

    #[test]
    fn dispatch_tools_list() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let resp = dispatcher.dispatch(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(parsed["result"]["tools"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn dispatch_tools_call() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let req = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"echo","arguments":{"msg":"hello"}}}"#;
        let resp = dispatcher.dispatch(req).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        let text = parsed["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("hello"));
    }

    #[test]
    fn dispatch_unknown_method() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let resp = dispatcher.dispatch(r#"{"jsonrpc":"2.0","id":4,"method":"nonexistent"}"#).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(parsed["error"]["code"], -32601);
    }

    #[test]
    fn dispatch_invalid_json() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let resp = dispatcher.dispatch("not json").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(parsed["error"]["code"], -32700);
    }

    #[test]
    fn dispatch_unknown_tool() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let req = r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"missing","arguments":{}}}"#;
        let resp = dispatcher.dispatch(req).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(parsed["error"]["code"], -32603);
    }
}
