//! JSON-RPC 2.0 transport and dispatch for MCP.
//!
//! Reads JSON-RPC requests from stdin (stdio transport) and routes them
//! to the appropriate [`McpServer`] handler. Writes responses to stdout.

use std::io::{self, BufRead, Write};

use crate::mcp::server::{
    McpServer, SUPPORTED_PROTOCOL_VERSIONS, request_protocol_version, shape_modern_result,
};

/// Cap on a single stdio JSON-RPC line. Reading is bounded by
/// [`read_limited_line`], so a peer that never sends `\n` can never make the
/// process buffer more than this many bytes — fatal under a
/// `panic = "abort"` release profile otherwise.
const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

/// JSON-RPC error code for a request sent before `initialize` (MCP lifecycle).
const ERR_NOT_INITIALIZED: i32 = -32002;

/// Read one line (up to and including `\n`) into `buf`, capping input at
/// `max` bytes per line.
///
/// Unlike `BufRead::lines`, memory use is bounded *before* the newline
/// arrives: once `max` bytes accumulate without one, the read fails instead of
/// the buffer growing. Returns bytes read (`0` at EOF); fails with
/// `InvalidData` on an over-long line.
fn read_limited_line<R: BufRead>(
    reader: &mut R,
    max: usize,
    buf: &mut Vec<u8>,
) -> io::Result<usize> {
    buf.clear();
    let mut total = 0usize;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(total); // EOF
        }
        let newline = available.iter().position(|&b| b == b'\n');
        let take = newline.map_or(available.len(), |i| i + 1);
        if total + take > max {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("JSON-RPC line exceeds {max} bytes"),
            ));
        }
        buf.extend_from_slice(&available[..take]);
        reader.consume(take);
        total += take;
        if newline.is_some() {
            return Ok(total);
        }
    }
}

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
    ///
    /// JSON-RPC batches were removed from the MCP spec in `2025-06-18`; they
    /// are still accepted here for clients negotiating `2024-11-05`.
    pub fn dispatch(&self, request: &str) -> Option<String> {
        let trimmed = request.trim();
        if trimmed.starts_with('[') {
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
            // Batches were removed from the MCP spec in `2025-06-18`; they are
            // tolerated for `2024-11-05` clients only. A modern request inside
            // a batch is a protocol violation and rejects the whole batch.
            if reqs.iter().any(|r| request_protocol_version(r).is_some()) {
                return Some(self.error_response(
                    serde_json::Value::Null,
                    -32600,
                    "Invalid Request: JSON-RPC batches are not supported by protocol revisions after 2024-11-05",
                ));
            }
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
        // A non-object is never a valid request — answering with silence
        // (the notification path) leaves the client waiting forever.
        if !req.is_object() {
            return Some(self.error_response(
                serde_json::Value::Null,
                -32600,
                "Invalid Request: expected a JSON object",
            ));
        }
        // Notifications (the `id` member is absent) don't get responses. This is
        // distinct from a null id, which is a request that must be answered.
        req.get("id")?;
        // Preserve the id verbatim (JSON-RPC ids may be a string or a number).
        let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");

        // Era selection (dual-era server): a request declaring its protocol
        // version in `_meta` is served statelessly; `initialize` always
        // selects legacy handshake semantics, even when it carries `_meta`.
        let modern = if method == "initialize" {
            None
        } else {
            request_protocol_version(req)
        };
        if let Some(version) = modern {
            if let Some(err) = self.check_modern_version(&id, version) {
                return Some(err);
            }
            // Methods that exist only in modern revisions (or were removed
            // there) are answered before the shared handler table.
            match method {
                "server/discover" => {
                    let mut result = self.server.discover_response();
                    shape_modern_result(version, method, &mut result);
                    return Some(self.success_response(id, result));
                }
                "subscriptions/listen" => return Some(self.subscription_close(&id)),
                // `ping` was removed in 2026-07-28; it is legacy-only.
                "ping" => {
                    return Some(self.error_response(
                        id,
                        -32601,
                        "Method not found: ping (removed in protocol 2026-07-28)",
                    ));
                }
                _ => {}
            }
        }

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
            "tools/call" => return Some(self.handle_tool_call(&id, req, modern)),
            "resources/read" => self.handle_resource_read(req),
            _ => Err((-32601, format!("Method not found: {method}"))),
        };

        match result {
            Ok(mut value) => {
                if let Some(version) = modern {
                    shape_modern_result(version, method, &mut value);
                }
                Some(self.success_response(id, value))
            }
            Err((code, message)) => Some(self.error_response(id, code, &message)),
        }
    }

    /// Version gate for modern requests: a version outside
    /// [`SUPPORTED_PROTOCOL_VERSIONS`] is rejected with `-32022`
    /// (UnsupportedProtocolVersion) listing what this server does support,
    /// so the client can retry with a mutually supported version.
    fn check_modern_version(&self, id: &serde_json::Value, requested: &str) -> Option<String> {
        if SUPPORTED_PROTOCOL_VERSIONS.contains(&requested) {
            return None;
        }
        let mut error = self.error_response(id.clone(), -32022, "Unsupported protocol version");
        // Splice a `data` member into the serialized error response.
        if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&error) {
            v["error"]["data"] = serde_json::json!({
                "supported": SUPPORTED_PROTOCOL_VERSIONS,
                "requested": requested,
            });
            error = serde_json::to_string(&v).unwrap_or(error);
        }
        Some(error)
    }

    /// Answer `subscriptions/listen` on stdio: the acknowledgment
    /// notification and the graceful-closure result, one JSON-RPC message per
    /// line (the stdio framing is newline-delimited).
    fn subscription_close(&self, id: &serde_json::Value) -> String {
        let (ack, close) = self.server.subscription_ack_and_close(id);
        format!(
            "{}\n{}",
            serde_json::to_string(&ack).unwrap_or_default(),
            self.success_response(id.clone(), close)
        )
    }

    /// Dispatch a JSON-RPC request, awaiting async tool handlers.
    ///
    /// Identical to [`JsonRpcDispatcher::dispatch`] except that `tools/call`
    /// resolves through [`McpServer::call_tool_async`], so tools registered
    /// with `set_async_handler` work. The synchronous [`Self::dispatch`]
    /// cannot invoke them — prefer this one whenever the server has any
    /// async handler.
    pub async fn dispatch_async(&self, request: &str) -> Option<String> {
        let trimmed = request.trim();
        if trimmed.starts_with('[') {
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
            let mut responses: Vec<String> = Vec::with_capacity(reqs.len());
            for req in &reqs {
                if let Some(r) = self.dispatch_single_async(req).await {
                    responses.push(r);
                }
            }
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
            self.dispatch_single_async(&req).await
        }
    }

    /// [`Self::dispatch_authenticated`] with async tool handler support.
    pub async fn dispatch_authenticated_async(
        &self,
        request: &str,
        auth_header: Option<&str>,
    ) -> Option<String> {
        if !self.server.check_auth(auth_header.unwrap_or("")) {
            let id = serde_json::from_str::<serde_json::Value>(request.trim())
                .ok()
                .and_then(|v| v.get("id").cloned())
                .unwrap_or(serde_json::Value::Null);
            return Some(self.error_response(id, -32001, "Unauthorized"));
        }
        self.dispatch_async(request).await
    }

    async fn dispatch_single_async(&self, req: &serde_json::Value) -> Option<String> {
        // Only `tools/call` can reach an async handler; everything else is
        // pure metadata and shares the synchronous path.
        if req.get("method").and_then(|v| v.as_str()) == Some("tools/call") {
            req.get("id")?;
            let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
            let modern = request_protocol_version(req);
            if let Some(version) = modern
                && let Some(err) = self.check_modern_version(&id, version)
            {
                return Some(err);
            }
            return Some(self.handle_tool_call_async(&id, req, modern).await);
        }
        self.dispatch_single(req)
    }

    /// Lifecycle gate for connection-oriented transports: the MCP spec
    /// requires clients to send no requests other than `initialize` before
    /// the server has answered initialization.
    ///
    /// Returns the `-32002` error response to send, or `None` when the
    /// request may proceed. Modern (stateless, `_meta`-carrying) requests
    /// bypass the gate — 2026-07-28 has no handshake at all. Notifications
    /// never trigger the gate, and a pre-initialization batch is likewise
    /// passed through, as batches are a `2024-11-05` compatibility feature
    /// this dispatcher accepts leniently.
    fn check_initialized(&self, parsed: &serde_json::Value, initialized: bool) -> Option<String> {
        if initialized || !parsed.is_object() {
            return None;
        }
        if request_protocol_version(parsed).is_some() {
            return None; // modern era: stateless, no handshake to await
        }
        if parsed.get("method").and_then(|v| v.as_str()) == Some("initialize") {
            return None;
        }
        // Notifications (no id) get no response — nothing to reject.
        parsed.get("id")?;
        let id = parsed.get("id").cloned().unwrap_or(serde_json::Value::Null);
        Some(self.error_response(
            id,
            ERR_NOT_INITIALIZED,
            "Server not initialized: send an initialize request first",
        ))
    }

    /// Run the stdio transport loop: read lines from stdin, dispatch, write to stdout.
    ///
    /// Enforces the MCP lifecycle: requests arriving before `initialize` are
    /// rejected with `-32002`.
    ///
    /// Tools registered with `set_async_handler` are NOT callable from this
    /// loop — use [`Self::run_stdio_async`] when the server has any.
    ///
    /// # Errors
    ///
    /// Fails immediately (`InvalidInput`) when the server has any async-only
    /// tool: every call to it would otherwise return `isError` at runtime and
    /// look like a handler bug.
    pub fn run_stdio(&self) -> io::Result<()> {
        if let Some(name) = self.server.async_only_tools().first() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "tool '{name}' has only an async handler; use run_stdio_async instead of run_stdio"
                ),
            ));
        }
        let stdin = io::stdin();
        let mut reader = io::BufReader::new(stdin.lock());
        let mut stdout = io::stdout().lock();
        let mut initialized = false;
        let mut raw = Vec::new();

        loop {
            let n = read_limited_line(&mut reader, MAX_LINE_BYTES, &mut raw)?;
            if n == 0 {
                break; // EOF
            }
            let line = std::str::from_utf8(&raw).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "JSON-RPC line is not valid UTF-8",
                )
            })?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if let Some(err) = self.check_initialized(&parsed, initialized) {
                    writeln!(stdout, "{err}")?;
                    stdout.flush()?;
                    continue;
                }
                if parsed.get("method").and_then(|m| m.as_str()) == Some("initialize") {
                    initialized = true;
                }
            }
            if let Some(response) = self.dispatch(trimmed) {
                writeln!(stdout, "{response}")?;
                stdout.flush()?;
            }
        }
        Ok(())
    }

    /// [`Self::run_stdio`] for servers with async tool handlers.
    ///
    /// Enforces the MCP lifecycle like [`Self::run_stdio`] does. Reads stdin
    /// with blocking I/O between awaits — an MCP stdio server is a dedicated
    /// process, so occupying the calling task is intended. Drive it from a
    /// runtime's blocking-friendly context (e.g. a dedicated task).
    pub async fn run_stdio_async(&self) -> io::Result<()> {
        let stdin = io::stdin();
        let mut reader = io::BufReader::new(stdin.lock());
        let mut stdout = io::stdout().lock();
        let mut initialized = false;
        let mut raw = Vec::new();

        loop {
            let n = read_limited_line(&mut reader, MAX_LINE_BYTES, &mut raw)?;
            if n == 0 {
                break; // EOF
            }
            let line = std::str::from_utf8(&raw).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "JSON-RPC line is not valid UTF-8",
                )
            })?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if let Some(err) = self.check_initialized(&parsed, initialized) {
                    writeln!(stdout, "{err}")?;
                    stdout.flush()?;
                    continue;
                }
                if parsed.get("method").and_then(|m| m.as_str()) == Some("initialize") {
                    initialized = true;
                }
            }
            if let Some(response) = self.dispatch_async(trimmed).await {
                writeln!(stdout, "{response}")?;
                stdout.flush()?;
            }
        }
        Ok(())
    }

    /// [`Self::handle_tool_call`] resolving through `call_tool_async`.
    async fn handle_tool_call_async(
        &self,
        id: &serde_json::Value,
        req: &serde_json::Value,
        modern: Option<&str>,
    ) -> String {
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
        if let Err(e) = self.server.validate_tool_args(tool_name, &params) {
            return self.error_response(id.clone(), -32602, &e);
        }

        let mut result = match self.server.call_tool_async(tool_name, params).await {
            Ok(result) => serde_json::json!({
                "content": [{ "type": "text", "text": result.to_string() }],
                "isError": false
            }),
            Err(e) => serde_json::json!({
                "content": [{ "type": "text", "text": e.to_string() }],
                "isError": true
            }),
        };
        if let Some(version) = modern {
            shape_modern_result(version, "tools/call", &mut result);
        }
        self.success_response(id.clone(), result)
    }

    /// Handle `tools/call`. Returns a full JSON-RPC response string.
    ///
    /// An **unknown tool** is a protocol error (`-32602`, invalid params). A
    /// tool that runs and **fails** is reported in-band as a successful result
    /// with `isError: true`, per the MCP spec — so the model sees the error and
    /// can adapt rather than the whole request failing at the transport layer.
    fn handle_tool_call(
        &self,
        id: &serde_json::Value,
        req: &serde_json::Value,
        modern: Option<&str>,
    ) -> String {
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
        if let Err(e) = self.server.validate_tool_args(tool_name, &params) {
            return self.error_response(id.clone(), -32602, &e);
        }

        let mut result = match self.server.call_tool(tool_name, params) {
            Ok(result) => serde_json::json!({
                "content": [{ "type": "text", "text": result.to_string() }],
                "isError": false
            }),
            Err(e) => serde_json::json!({
                "content": [{ "type": "text", "text": e.to_string() }],
                "isError": true
            }),
        };
        if let Some(version) = modern {
            shape_modern_result(version, "tools/call", &mut result);
        }
        self.success_response(id.clone(), result)
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
            .map_err(|e| (-32602, e.to_string()))
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

    /// A server whose tool is registered ONLY via `set_async_handler` — the
    /// shape real consumers use when their handlers await I/O.
    fn async_only_server() -> McpServer {
        let mut server = McpServer::new("async-server", "0.1.0");
        server.register_tool(ToolDescription {
            name: "search".into(),
            description: "async search".into(),
            input_schema: serde_json::json!({"type": "object"}),
        });
        server.set_async_handler("search", |p: serde_json::Value| async move { Ok(p) });
        server
    }

    #[tokio::test]
    async fn async_only_tool_runs_over_dispatch_async() {
        let server = async_only_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"search","arguments":{"q":"x"}}}"#;
        let resp = dispatcher.dispatch_async(req).await.expect("response");
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(parsed["result"]["isError"], false, "{parsed}");
        assert!(
            parsed["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("\"q\""),
            "{parsed}"
        );
    }

    #[tokio::test]
    async fn dispatch_async_still_serves_metadata_methods() {
        let server = async_only_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let resp = dispatcher
            .dispatch_async(r#"{"jsonrpc":"2.0","id":7,"method":"tools/list"}"#)
            .await
            .expect("response");
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(parsed["id"], 7);
        assert_eq!(parsed["result"]["tools"][0]["name"], "search");
    }

    #[tokio::test]
    async fn missing_required_argument_is_rejected_not_defaulted() {
        let mut server = McpServer::new("s", "1.0");
        server.register_tool(ToolDescription {
            name: "search".into(),
            description: "d".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {"query": {"type": "string"}},
                "required": ["query"]
            }),
        });
        server.set_async_handler("search", |p: serde_json::Value| async move { Ok(p) });
        let dispatcher = JsonRpcDispatcher::new(&server);

        // No arguments at all — the handler would see null and search for "".
        let resp = dispatcher
            .dispatch_async(
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"search"}}"#,
            )
            .await
            .expect("response");
        assert!(resp.contains("-32602"), "{resp}");
        assert!(resp.contains("query"), "{resp}");

        // Supplying it dispatches normally.
        let ok = dispatcher
            .dispatch_async(
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"search","arguments":{"query":"x"}}}"#,
            )
            .await
            .expect("response");
        let parsed: serde_json::Value = serde_json::from_str(&ok).unwrap();
        assert_eq!(parsed["result"]["isError"], false, "{ok}");
    }

    #[test]
    fn non_object_request_gets_invalid_request_not_silence() {
        let server = async_only_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let resp = dispatcher
            .dispatch("[1, 2]")
            .expect("must answer, not hang");
        assert!(resp.contains("-32600"), "{resp}");
    }

    #[test]
    fn sync_call_of_async_only_tool_says_so() {
        let server = async_only_server();
        let err = server
            .call_tool("search", serde_json::json!({}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("async"), "misleading error: {err}");
    }

    fn test_server() -> McpServer {
        let mut server = McpServer::new("test-server", "0.1.0");
        server.register_tool(ToolDescription {
            name: "echo".into(),
            description: "Echo input".into(),
            input_schema: serde_json::json!({"type": "object"}),
        });
        server.set_handler("echo", Ok);
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

    #[test]
    fn read_limited_line_bounds_memory_before_newline() {
        // A 40-byte "line" with no newline against a 10-byte cap must fail —
        // the buffer never grows past the cap waiting for `\n`.
        let mut reader = io::Cursor::new(vec![b'x'; 40]);
        let mut buf = Vec::new();
        let err = read_limited_line(&mut reader, 10, &mut buf).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData, "{err}");
        assert!(buf.len() <= 10, "buffer grew past the cap: {}", buf.len());
    }

    #[test]
    fn read_limited_line_reads_normal_lines() {
        let mut reader = io::Cursor::new(b"{\"a\":1}\n{\"b\":2}\ntrailing".to_vec());
        let mut buf = Vec::new();
        assert_eq!(read_limited_line(&mut reader, 1024, &mut buf).unwrap(), 8);
        assert_eq!(buf, b"{\"a\":1}\n");
        assert_eq!(read_limited_line(&mut reader, 1024, &mut buf).unwrap(), 8);
        assert_eq!(buf, b"{\"b\":2}\n");
        // Final unterminated line, then EOF (0).
        assert_eq!(read_limited_line(&mut reader, 1024, &mut buf).unwrap(), 8);
        assert_eq!(buf, b"trailing");
        assert_eq!(read_limited_line(&mut reader, 1024, &mut buf).unwrap(), 0);
    }

    #[test]
    fn pre_initialize_requests_are_rejected() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let parsed: serde_json::Value =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":5,"method":"tools/list"}"#).unwrap();
        let err = dispatcher
            .check_initialized(&parsed, false)
            .expect("gate must reject");
        assert!(err.contains("-32002"), "{err}");

        // initialize itself always passes …
        let init: serde_json::Value =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#).unwrap();
        assert!(dispatcher.check_initialized(&init, false).is_none());
        // … and everything passes once initialized.
        assert!(dispatcher.check_initialized(&parsed, true).is_none());

        // Notifications (no id) are never answered, so the gate ignores them.
        let notif: serde_json::Value =
            serde_json::from_str(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .unwrap();
        assert!(dispatcher.check_initialized(&notif, false).is_none());

        // Modern (stateless) requests bypass the gate: 2026-07-28 has no
        // handshake to await.
        let modern: serde_json::Value = serde_json::from_str(
            r#"{"jsonrpc":"2.0","id":9,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}"#,
        )
        .unwrap();
        assert!(dispatcher.check_initialized(&modern, false).is_none());
    }

    /// A modern (stateless, `_meta`-carrying) request, one per test constant.
    fn modern_request(method: &str, id: i64, version: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{{"_meta":{{"io.modelcontextprotocol/protocolVersion":"{version}"}}}}}}"#
        )
    }

    #[test]
    fn modern_server_discover_answers_versions_and_identity() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let resp = dispatcher
            .dispatch(&modern_request("server/discover", 1, "2026-07-28"))
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(
            parsed["result"]["supportedVersions"][0],
            crate::mcp::server::LATEST_PROTOCOL_VERSION
        );
        assert_eq!(
            parsed["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
            "test-server"
        );
        // CacheableResult: discover responses carry ttlMs + cacheScope.
        assert!(parsed["result"]["ttlMs"].is_u64(), "{parsed}");
        assert_eq!(parsed["result"]["resultType"], "complete");
    }

    #[test]
    fn modern_unsupported_version_lists_supported() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let resp = dispatcher
            .dispatch(&modern_request("tools/list", 2, "1999-01-01"))
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(parsed["error"]["code"], -32022, "{parsed}");
        assert_eq!(parsed["error"]["data"]["requested"], "1999-01-01");
        assert!(
            parsed["error"]["data"]["supported"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("2025-06-18"))
        );
    }

    #[test]
    fn modern_ping_is_method_not_found() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let resp = dispatcher
            .dispatch(&modern_request("ping", 3, "2026-07-28"))
            .unwrap();
        assert!(resp.contains("-32601"), "{resp}");
        // Legacy ping still works.
        let resp = dispatcher
            .dispatch(r#"{"jsonrpc":"2.0","id":4,"method":"ping"}"#)
            .unwrap();
        assert!(resp.contains("result"), "{resp}");
    }

    #[test]
    fn modern_results_carry_result_type_and_cache_fields() {
        let mut server = test_server();
        server.set_handler("echo", Ok);
        let dispatcher = JsonRpcDispatcher::new(&server);

        let list = dispatcher
            .dispatch(&modern_request("tools/list", 1, "2026-07-28"))
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&list).unwrap();
        assert_eq!(parsed["result"]["resultType"], "complete");
        assert!(parsed["result"]["ttlMs"].is_u64());
        assert_eq!(parsed["result"]["cacheScope"], "private");

        let call = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"echo","arguments":{},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}"#;
        let call = dispatcher.dispatch(call).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&call).unwrap();
        assert_eq!(parsed["result"]["resultType"], "complete", "{parsed}");
        assert!(
            parsed["result"].get("ttlMs").is_none(),
            "tools/call is not cacheable"
        );

        // A modern request naming a LEGACY version is served without modern
        // result shaping (the client speaks that revision's schema).
        let legacy_shaped = dispatcher
            .dispatch(&modern_request("tools/list", 3, "2025-06-18"))
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&legacy_shaped).unwrap();
        assert!(parsed["result"].get("resultType").is_none(), "{parsed}");
    }

    #[test]
    fn modern_subscriptions_listen_acks_then_closes() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let resp = dispatcher
            .dispatch(&modern_request("subscriptions/listen", 7, "2026-07-28"))
            .unwrap();
        // Two newline-delimited messages: the acknowledgment notification,
        // then the graceful-closure result response.
        let lines: Vec<&str> = resp.lines().collect();
        assert_eq!(lines.len(), 2, "{resp}");
        let ack: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(ack["method"], "notifications/subscriptions/acknowledged");
        // The agreed subset is empty: nothing was requested that we support.
        assert_eq!(ack["params"]["notifications"], serde_json::json!({}));
        assert_eq!(
            ack["params"]["_meta"]["io.modelcontextprotocol/subscriptionId"],
            7
        );
        let close: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(close["id"], 7);
        assert_eq!(close["result"]["resultType"], "complete");
        assert_eq!(
            close["result"]["_meta"]["io.modelcontextprotocol/subscriptionId"],
            7
        );
    }

    #[test]
    fn modern_request_inside_batch_rejects_the_batch() {
        let server = test_server();
        let dispatcher = JsonRpcDispatcher::new(&server);
        let batch = format!("[{}]", modern_request("tools/list", 1, "2026-07-28"));
        let resp = dispatcher.dispatch(&batch).unwrap();
        assert!(resp.contains("-32600"), "{resp}");
    }
}
