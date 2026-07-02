//! HTTP/SSE remote transport for MCP.
//!
//! Exposes an [`McpServer`] over HTTP: a JSON-RPC endpoint (`POST /mcp`) and an
//! SSE endpoint (`POST /mcp/sse`) that streams the response as a server-sent
//! event. Both reuse the server's `Authorization` (Bearer) check, so a server
//! secured for stdio is secured identically over HTTP.
//!
//! The transport holds the server behind an `Arc` (shared across request
//! tasks) and dispatches `tools/call` via [`McpServer::call_tool_async`], so
//! async handlers work transparently over HTTP.
//!
//! Requires the `mcp-http` feature (axum + tokio).

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::post;
use serde_json::Value;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::mcp::McpServer;

/// Shared MCP server state for the HTTP transport.
#[derive(Clone)]
pub struct HttpTransport {
    server: Arc<McpServer>,
}

impl HttpTransport {
    /// Wrap a shared MCP server for HTTP serving.
    pub fn new(server: Arc<McpServer>) -> Self {
        Self { server }
    }

    /// Build the axum router with JSON-RPC and SSE routes.
    pub fn router(&self) -> axum::Router {
        axum::Router::new()
            .route("/mcp", post(rpc_handler))
            .route("/mcp/sse", post(sse_handler))
            .with_state(self.clone())
    }
}

/// Run the MCP HTTP transport on `addr` until the server is stopped.
pub async fn serve(server: Arc<McpServer>, addr: SocketAddr) -> std::io::Result<()> {
    let transport = HttpTransport::new(server);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, transport.router()).await?;
    Ok(())
}

/// JSON-RPC code for "method not found".
const ERR_METHOD_NOT_FOUND: i32 = -32601;
/// JSON-RPC code for invalid params (unknown tool / prompt / resource).
const ERR_INVALID_PARAMS: i32 = -32602;
/// JSON-RPC code for a tool-execution / internal error.
const ERR_INTERNAL: i32 = -32603;
/// JSON-RPC code for unauthorized access.
const ERR_UNAUTHORIZED: i32 = -32001;

/// Dispatch a single JSON-RPC request against the server (async path).
///
/// `tools/call` is awaited via [`McpServer::call_tool_async`]; `initialize`,
/// `ping`, `tools/list`, `resources/list`, `resources/templates/list`,
/// `prompts/list`, `prompts/get`, and `resources/read` are handled
/// synchronously. Notifications (no `id`) return `None`.
async fn dispatch_async(server: &McpServer, req: &Value) -> Option<Value> {
    // Notifications (no id) get no response.
    let id = req.get("id")?.clone();
    let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");

    let result: Result<Value, (i32, String)> = match method {
        "initialize" => {
            let requested = req
                .pointer("/params/protocolVersion")
                .and_then(|v| v.as_str());
            Ok(server.initialize_response(requested))
        }
        "ping" => Ok(serde_json::json!({})),
        "tools/list" => Ok(serde_json::json!({ "tools": server.tools() })),
        "resources/list" => Ok(serde_json::json!({ "resources": server.resources() })),
        "resources/templates/list" => Ok(serde_json::json!({ "resourceTemplates": [] })),
        "prompts/list" => Ok(serde_json::json!({ "prompts": server.prompts() })),
        "prompts/get" => {
            let name = req
                .pointer("/params/name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args = req
                .pointer("/params/arguments")
                .cloned()
                .unwrap_or(serde_json::json!({}));
            server
                .get_prompt(name, args)
                .map_err(|e| (ERR_INVALID_PARAMS, e.to_string()))
        }
        "resources/read" => {
            let uri = req
                .pointer("/params/uri")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            server
                .read_resource(uri, serde_json::json!({}))
                .map(|content| {
                    serde_json::json!({
                        "contents": [{ "uri": uri, "text": content.to_string() }]
                    })
                })
                .map_err(|e| (ERR_INTERNAL, e.to_string()))
        }
        "tools/call" => {
            let name = req
                .pointer("/params/name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let params = req
                .pointer("/params/arguments")
                .cloned()
                .unwrap_or(serde_json::json!(null));
            if !server.has_tool(name) {
                Err((ERR_INVALID_PARAMS, format!("Unknown tool: {name}")))
            } else {
                // Execution failures are reported in-band with isError: true.
                match server.call_tool_async(name, params).await {
                    Ok(r) => Ok(serde_json::json!({
                        "content": [{ "type": "text", "text": r.to_string() }],
                        "isError": false
                    })),
                    Err(e) => Ok(serde_json::json!({
                        "content": [{ "type": "text", "text": e.to_string() }],
                        "isError": true
                    })),
                }
            }
        }
        _ => Err((ERR_METHOD_NOT_FOUND, format!("Method not found: {method}"))),
    };

    Some(match result {
        Ok(value) => serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": value }),
        Err((code, message)) => serde_json::json!({
            "jsonrpc": "2.0", "id": id,
            "error": { "code": code, "message": message }
        }),
    })
}

/// Extract and validate the `Authorization` header. Returns `true` if the
/// request may proceed.
fn authorized(server: &McpServer, headers: &HeaderMap) -> bool {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    server.check_auth(auth)
}

fn unauthorized_response(id: Option<Value>) -> Json<Value> {
    Json(serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": ERR_UNAUTHORIZED, "message": "Unauthorized" }
    }))
}

async fn rpc_handler(
    State(state): State<HttpTransport>,
    headers: HeaderMap,
    Json(req): Json<Value>,
) -> impl IntoResponse {
    let id = req.get("id").cloned();
    if !authorized(&state.server, &headers) {
        return (StatusCode::UNAUTHORIZED, unauthorized_response(id));
    }
    match dispatch_async(&state.server, &req).await {
        Some(resp) => (StatusCode::OK, Json(resp)),
        // Notification — acknowledge with 204 No Content.
        None => (StatusCode::NO_CONTENT, Json(serde_json::Value::Null)),
    }
}

async fn sse_handler(
    State(state): State<HttpTransport>,
    headers: HeaderMap,
    Json(req): Json<Value>,
) -> impl IntoResponse {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let server = state.server.clone();

    // Produce the response for this request, then stream it as one SSE event.
    tokio::spawn(async move {
        let event = if !authorized(&server, &headers) {
            Event::default().event("error").data(
                serde_json::to_string(&unauthorized_response(req.get("id").cloned()).0)
                    .unwrap_or_default(),
            )
        } else if let Some(resp) = dispatch_async(&server, &req).await {
            let data = serde_json::to_string(&resp).unwrap_or_default();
            Event::default().event("message").data(data)
        } else {
            // Notification — no response event.
            Event::default().event("noop")
        };
        let _ = tx.send(Ok::<_, Infallible>(event));
    });

    Sse::new(UnboundedReceiverStream::new(rx)).keep_alive(KeepAlive::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::schema::{ResourceDescription, ToolDescription};

    fn server_with_echo() -> McpServer {
        let mut server = McpServer::new("http-test", "1.0.0");
        server.register_tool(ToolDescription {
            name: "echo".into(),
            description: "Echo".into(),
            input_schema: serde_json::json!({"type": "object"}),
        });
        server.set_async_handler("echo", |params| async move { Ok(params) });
        server
    }

    #[tokio::test]
    async fn dispatch_initialize() {
        let server = server_with_echo();
        let req = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}});
        let resp = dispatch_async(&server, &req).await.unwrap();
        assert_eq!(resp["result"]["serverInfo"]["name"], "http-test");
    }

    #[tokio::test]
    async fn dispatch_tools_call_async() {
        let server = server_with_echo();
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": { "name": "echo", "arguments": { "msg": "hello" } }
        });
        let resp = dispatch_async(&server, &req).await.unwrap();
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("hello"));
    }

    #[tokio::test]
    async fn dispatch_unknown_method() {
        let server = server_with_echo();
        let req = serde_json::json!({"jsonrpc":"2.0","id":3,"method":"nope"});
        let resp = dispatch_async(&server, &req).await.unwrap();
        assert_eq!(resp["error"]["code"], ERR_METHOD_NOT_FOUND);
    }

    /// AC2: HTTP dispatch also serves `resources/read`, not just tools.
    #[tokio::test]
    async fn dispatch_resources_read() {
        let mut server = McpServer::new("http-test", "1.0.0");
        server.register_resource(ResourceDescription {
            uri: "docs://x".into(),
            name: "X".into(),
            description: None,
            mime_type: None,
        });
        server.set_resource_handler("docs://x", |_| Ok(serde_json::json!("# body")));
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": 4, "method": "resources/read",
            "params": { "uri": "docs://x" }
        });
        let resp = dispatch_async(&server, &req).await.unwrap();
        let text = resp["result"]["contents"][0]["text"].as_str().unwrap();
        assert!(text.contains("body"));
    }

    /// AC2: a full HTTP round-trip — bind an ephemeral port, POST a tools/call,
    /// and read the JSON-RPC response off the wire.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn http_round_trip_calls_tool() {
        let server = Arc::new(server_with_echo());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        // Hand the listener to axum in a background task.
        let transport = HttpTransport::new(server);
        tokio::spawn(async move {
            let _ = axum::serve(listener, transport.router()).await;
        });

        let body = serde_json::to_string(&serde_json::json!({
            "jsonrpc": "2.0", "id": 9, "method": "tools/call",
            "params": { "name": "echo", "arguments": { "v": 42 } }
        }))
        .unwrap();
        let req = format!(
            "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );

        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).await.unwrap();
        let response = String::from_utf8_lossy(&buf);
        assert!(response.contains("200 OK"), "response: {response}");
        // The tool result is JSON-encoded inside the `text` field, so its quotes
        // are escaped on the wire — assert on the unescaped value + content shape.
        assert!(response.contains("\"content\""), "response: {response}");
        assert!(response.contains("\\\"v\\\":42"), "response: {response}");
    }
}
