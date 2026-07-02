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

    /// Dispatch a request, checking the `Authorization` header if auth is configured.
    ///
    /// Returns a JSON-RPC `-32001` error response when auth fails.
    /// Pass `None` for `auth_header` when no header is present (stdio transport).
    pub fn dispatch_authenticated(
        &self,
        request: &str,
        auth_header: Option<&str>,
    ) -> Option<String> {
        let provided = auth_header.unwrap_or("");
        if !self.server.check_auth(provided) {
            // Echo back the request id (string or number) when we can parse one,
            // else a null id per JSON-RPC.
            let id = serde_json::from_str::<serde_json::Value>(request.trim())
                .ok()
                .and_then(|v| v.get("id").cloned())
                .unwrap_or(serde_json::Value::Null);
            return Some(self.error_response(id, -32001, "Unauthorized"));
        }
        self.dispatch(request)
    }

    /// Dispatch a JSON-RPC request (single or batch) and return the response.
    pub fn dispatch(&self, request: &str) -> Option<String> {
        let trimmed = request.trim();
        if trimmed.starts_with('[') {
            // Batch request
            let reqs: Vec<serde_json::Value> = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(e) => {
                    return Some(self.error_response(
                        serde_json::Value::Null,
                        -32700,
                        &format!("Parse error: {e}"),
                    ));
                }
            };
            let responses: Vec<String> = reqs
                .iter()
                .filter_map(|req| self.dispatch_single(req))
                .collect();
            if responses.is_empty() {
                None
            } else {
                Some(format!("[{}]", responses.join(",")))
            }
        } else {
            let req: serde_json::Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(e) => {
                    return Some(self.error_response(
                        serde_json::Value::Null,
                        -32700,
                        &format!("Parse error: {e}"),
                    ));
                }
            };
            self.dispatch_single(&req)
        }
    }

    /// Dispatch a single pre-parsed JSON-RPC request.
    fn dispatch_single(&self, req: &serde_json::Value) -> Option<String> {
        // Notifications (the `id` member is absent) don't get responses. This is
        // distinct from a null id, which is a request that must be answered.
        req.get("id")?;
        // Preserve the id verbatim (JSON-RPC ids may be a string or a number).
        let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");

        let result = match method {
            "initialize" => {
                let requested = req
                    .get("params")
                    .and_then(|p| p.get("protocolVersion"))
                    .and_then(|v| v.as_str());
                Ok(self.server.initialize_response(requested))
            }
            "ping" => Ok(serde_json::json!({})),
            "tools/list" => Ok(serde_json::json!({
                "tools": self.server.tools()
            })),
            "resources/list" => Ok(serde_json::json!({
                "resources": self.server.resources()
            })),
            "resources/templates/list" => Ok(serde_json::json!({
                "resourceTemplates": []
            })),
            "prompts/list" => Ok(serde_json::json!({
                "prompts": self.server.prompts()
            })),
            "prompts/get" => self.handle_prompt_get(req),
            "tools/call" => return Some(self.handle_tool_call(&id, req)),
            "resources/read" => self.handle_resource_read(req),
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

    /// Handle `tools/call`. Returns a full JSON-RPC response string.
    ///
    /// An **unknown tool** is a protocol error (`-32602`, invalid params). A
    /// tool that runs and **fails** is reported in-band as a successful result
    /// with `isError: true`, per the MCP spec — so the model sees the error and
    /// can adapt rather than the whole request failing at the transport layer.
    fn handle_tool_call(&self, id: &serde_json::Value, req: &serde_json::Value) -> String {
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

        if !self.server.has_tool(tool_name) {
            return self.error_response(id.clone(), -32602, &format!("Unknown tool: {tool_name}"));
        }

        match self.server.call_tool(tool_name, params) {
            Ok(result) => self.success_response(
                id.clone(),
                serde_json::json!({
                    "content": [{ "type": "text", "text": result.to_string() }],
                    "isError": false
                }),
            ),
            Err(e) => self.success_response(
                id.clone(),
                serde_json::json!({
                    "content": [{ "type": "text", "text": e.to_string() }],
                    "isError": true
                }),
            ),
        }
    }

    /// Handle `prompts/get`: render a registered prompt with the given
    /// arguments. An unknown prompt is an invalid-params error.
    fn handle_prompt_get(
        &self,
        req: &serde_json::Value,
    ) -> std::result::Result<serde_json::Value, (i32, String)> {
        let name = req
            .get("params")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("");
        let args = req
            .get("params")
            .and_then(|p| p.get("arguments"))
            .cloned()
            .unwrap_or(serde_json::json!({}));
        self.server
            .get_prompt(name, args)
            .map_err(|e| (-32602, e.to_string()))
    }

    fn handle_resource_read(
        &self,
        req: &serde_json::Value,
    ) -> std::result::Result<serde_json::Value, (i32, String)> {
        let uri = req
            .get("params")
            .and_then(|p| p.get("uri"))
            .and_then(|u| u.as_str())
            .unwrap_or("");
        self.server
            .read_resource(uri, serde_json::json!({}))
            .map(|content| {
                serde_json::json!({
                    "contents": [{
                        "uri": uri,
                        "text": content.to_string()
                    }]
                })
            })
            .map_err(|e| (-32603, e.to_string()))
    }

    fn success_response(&self, id: serde_json::Value, result: serde_json::Value) -> String {
        serde_json::to_string(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }))
        .unwrap_or_default()
    }

    fn error_response(&self, id: serde_json::Value, code: i32, message: &str) -> String {
        serde_json::to_string(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": code,
                "message": message,
            }
        }))
        .unwrap_or_default()
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
        let resp = dispatcher
            .dispatch(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(parsed["result"]["serverInfo"]["name"], "test-server");
    }

    #[test]
    fn dispatch_tools_list() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let resp = dispatcher
            .dispatch(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
            .unwrap();
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
        let resp = dispatcher
            .dispatch(r#"{"jsonrpc":"2.0","id":4,"method":"nonexistent"}"#)
            .unwrap();
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
    fn dispatch_unknown_tool_is_invalid_params() {
        // An unknown tool is a protocol error (-32602), not an in-band failure.
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let req = r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"missing","arguments":{}}}"#;
        let resp = dispatcher.dispatch(req).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(parsed["error"]["code"], -32602);
    }

    #[test]
    fn dispatch_ping_returns_empty_result() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let resp = dispatcher
            .dispatch(r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#)
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(parsed["id"], 9);
        assert!(parsed["result"].is_object());
        assert_eq!(parsed["result"].as_object().unwrap().len(), 0);
    }

    #[test]
    fn dispatch_preserves_string_id() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let resp = dispatcher
            .dispatch(r#"{"jsonrpc":"2.0","id":"req-abc","method":"tools/list"}"#)
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(parsed["id"], "req-abc");
    }

    #[test]
    fn initialize_echoes_client_protocol_version() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let resp = dispatcher
            .dispatch(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#,
            )
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(parsed["result"]["protocolVersion"], "2024-11-05");
    }

    #[test]
    fn tool_execution_error_reported_in_band() {
        // A registered tool whose handler fails → result with isError: true,
        // NOT a JSON-RPC error object.
        let mut server = McpServer::new("t", "1.0");
        server.register_tool(ToolDescription {
            name: "boom".into(),
            description: "always fails".into(),
            input_schema: serde_json::json!({"type": "object"}),
        });
        server.set_handler("boom", |_| {
            Err(crate::error::KernelError::Config("kaboom".into()))
        });
        let dispatcher = JsonRpcDispatcher::new(&server);
        let req = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"boom","arguments":{}}}"#;
        let resp = dispatcher.dispatch(req).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(
            parsed.get("error").is_none(),
            "should not be a protocol error"
        );
        assert_eq!(parsed["result"]["isError"], true);
        assert!(
            parsed["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("kaboom")
        );
    }

    #[test]
    fn dispatch_prompts_list_and_get() {
        let mut server = McpServer::new("t", "1.0");
        server.register_prompt(crate::mcp::schema::PromptDescription {
            name: "greet".into(),
            description: Some("Greet".into()),
            arguments: Vec::new(),
        });
        server.set_prompt_handler("greet", |_| {
            Ok(serde_json::json!({
                "messages": [{ "role": "user", "content": { "type": "text", "text": "hi" } }]
            }))
        });
        let dispatcher = JsonRpcDispatcher::new(&server);

        let list = dispatcher
            .dispatch(r#"{"jsonrpc":"2.0","id":1,"method":"prompts/list"}"#)
            .unwrap();
        let list: serde_json::Value = serde_json::from_str(&list).unwrap();
        assert_eq!(list["result"]["prompts"][0]["name"], "greet");

        let got = dispatcher
            .dispatch(r#"{"jsonrpc":"2.0","id":2,"method":"prompts/get","params":{"name":"greet","arguments":{}}}"#)
            .unwrap();
        let got: serde_json::Value = serde_json::from_str(&got).unwrap();
        assert_eq!(got["result"]["messages"][0]["content"]["text"], "hi");
    }

    #[test]
    fn dispatch_resource_templates_list_is_empty() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let resp = dispatcher
            .dispatch(r#"{"jsonrpc":"2.0","id":1,"method":"resources/templates/list"}"#)
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(parsed["result"]["resourceTemplates"].is_array());
    }

    #[test]
    fn notification_without_id_gets_no_response() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        // `notifications/initialized` is a notification (no id) → no response.
        assert!(
            dispatcher
                .dispatch(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .is_none()
        );
    }

    #[test]
    fn dispatch_batch_request() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let batch = r#"[
            {"jsonrpc":"2.0","id":1,"method":"initialize","params":{}},
            {"jsonrpc":"2.0","id":2,"method":"tools/list"}
        ]"#;
        let resp = dispatcher.dispatch(batch).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        // First response: initialize
        assert_eq!(arr[0]["result"]["serverInfo"]["name"], "test-server");
        // Second response: tools/list
        assert_eq!(arr[1]["result"]["tools"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn dispatch_batch_with_error() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let batch = r#"[
            {"jsonrpc":"2.0","id":1,"method":"initialize","params":{}},
            {"jsonrpc":"2.0","id":2,"method":"nonexistent"}
        ]"#;
        let resp = dispatcher.dispatch(batch).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr[0]["result"]["serverInfo"]["name"], "test-server");
        assert_eq!(arr[1]["error"]["code"], -32601);
    }

    #[test]
    fn dispatch_authenticated_passes_with_no_auth_configured() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let resp = dispatcher.dispatch_authenticated(req, None).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(parsed["result"]["serverInfo"].is_object());
    }

    #[test]
    fn dispatch_authenticated_rejects_missing_token() {
        let server = McpServer::new("secured", "1.0").with_bearer_auth("secret-token");
        let dispatcher = JsonRpcDispatcher::new(&server);
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let resp = dispatcher.dispatch_authenticated(req, None).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(parsed["error"]["code"], -32001);
    }

    #[test]
    fn dispatch_authenticated_rejects_wrong_token() {
        let server = McpServer::new("secured", "1.0").with_bearer_auth("correct-token");
        let dispatcher = JsonRpcDispatcher::new(&server);
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let resp = dispatcher
            .dispatch_authenticated(req, Some("Bearer wrong-token"))
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(parsed["error"]["code"], -32001);
    }

    #[test]
    fn dispatch_authenticated_passes_with_correct_token() {
        let server = McpServer::new("secured", "1.0").with_bearer_auth("correct-token");
        let dispatcher = JsonRpcDispatcher::new(&server);
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let resp = dispatcher
            .dispatch_authenticated(req, Some("Bearer correct-token"))
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(parsed["result"]["serverInfo"].is_object());
    }
}
