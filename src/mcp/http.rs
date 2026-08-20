//! Streamable HTTP remote transport for MCP.
//!
//! Exposes an [`McpServer`] over HTTP: a single JSON-RPC endpoint
//! (`POST /mcp`) returning JSON responses, per the Streamable HTTP transport.
//! The server's `Authorization` (Bearer) check applies to every request, so a
//! server secured for stdio is secured identically over HTTP.
//!
//! This is a dual-era endpoint: modern (stateless, `_meta`-carrying) requests
//! are validated against the standard request headers
//! (`MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name`) and answered per the
//! `2026-07-28` revision; legacy `initialize`-handshake requests are served
//! per the negotiated legacy revision with no header requirements.
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
use axum::http::{HeaderMap, HeaderName, StatusCode, header};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde_json::Value;

use crate::mcp::McpServer;
use crate::mcp::server::{
    SUPPORTED_PROTOCOL_VERSIONS, request_protocol_version, shape_modern_result,
};

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

    /// Build the axum router with the JSON-RPC route.
    pub fn router(&self) -> axum::Router {
        axum::Router::new()
            .route("/mcp", post(rpc_handler))
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
/// JSON-RPC code for unauthorized access.
const ERR_UNAUTHORIZED: i32 = -32001;
/// MCP-spec code: request headers do not match the request body.
const ERR_HEADER_MISMATCH: i32 = -32020;
/// MCP-spec code: the requested protocol version is not supported.
const ERR_UNSUPPORTED_VERSION: i32 = -32022;

/// Methods this server implements for modern (stateless) requests. `ping`
/// and `initialize` are legacy-only (`ping` was removed in 2026-07-28).
const MODERN_METHODS: &[&str] = &[
    "server/discover",
    "tools/list",
    "tools/call",
    "resources/list",
    "resources/templates/list",
    "resources/read",
    "prompts/list",
    "prompts/get",
    "subscriptions/listen",
];

/// Dispatch a single JSON-RPC request against the server (async path).
///
/// Dual-era: a request declaring its protocol version in `params._meta` is
/// served statelessly (2026-07-28 semantics — `server/discover` answered,
/// `ping` rejected, results stamped with `resultType`); anything else follows
/// the legacy handler table. `tools/call` is awaited via
/// [`McpServer::call_tool_async`]. Notifications (no `id`) return `None`.
async fn dispatch_async(server: &McpServer, req: &Value) -> Option<Value> {
    // Notifications (no id) get no response.
    let id = req.get("id")?.clone();
    let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");

    // Era selection mirrors the stdio dispatcher: `_meta` protocol version
    // means modern, except `initialize`, which always selects legacy.
    let modern = if method == "initialize" {
        None
    } else {
        request_protocol_version(req)
    };
    if let Some(version) = modern {
        if !SUPPORTED_PROTOCOL_VERSIONS.contains(&version) {
            return Some(rpc_error(
                Some(id),
                ERR_UNSUPPORTED_VERSION,
                "Unsupported protocol version",
                Some(serde_json::json!({
                    "supported": SUPPORTED_PROTOCOL_VERSIONS,
                    "requested": version,
                })),
            ));
        }
        match method {
            "server/discover" => {
                let mut result = server.discover_response();
                shape_modern_result(version, method, &mut result);
                return Some(rpc_result(id, result));
            }
            // `ping` was removed in 2026-07-28; it is legacy-only.
            "ping" => {
                return Some(rpc_error(
                    Some(id),
                    ERR_METHOD_NOT_FOUND,
                    "Method not found: ping (removed in protocol 2026-07-28)",
                    None,
                ));
            }
            // The stream answer is produced by the handler; when dispatched
            // directly (no stream), return just the graceful-closure result.
            "subscriptions/listen" => {
                let (_, close) = server.subscription_ack_and_close(&id);
                return Some(rpc_result(id, close));
            }
            _ => {}
        }
    }

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
                .map_err(|e| (ERR_INVALID_PARAMS, e.to_string()))
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
            } else if let Err(e) = server.validate_tool_args(name, &params) {
                Err((ERR_INVALID_PARAMS, e))
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
        Ok(mut value) => {
            if let Some(version) = modern {
                shape_modern_result(version, method, &mut value);
            }
            rpc_result(id, value)
        }
        Err((code, message)) => rpc_error(Some(id), code, &message, None),
    })
}

/// Wrap a result value in a JSON-RPC response envelope.
fn rpc_result(id: Value, result: Value) -> Value {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// Build a JSON-RPC error response envelope, with optional `error.data`.
/// A `None` id serializes as JSON `null` (error bodies for rejected requests
/// may carry no id).
fn rpc_error(id: Option<Value>, code: i32, message: &str, data: Option<Value>) -> Value {
    let mut error = serde_json::json!({ "code": code, "message": message });
    if let Some(d) = data {
        error["data"] = d;
    }
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": error })
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

/// Reject cross-origin browser requests (MCP spec: servers MUST validate
/// `Origin` to prevent DNS rebinding). A page on any website can POST to a
/// loopback MCP server; without this, that page executes tools.
///
/// Non-browser clients send no `Origin` and are unaffected.
fn origin_allowed(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) else {
        return true; // no Origin — not a browser-initiated request
    };
    if origin == "null" {
        return false;
    }
    // Only loopback origins may drive a local MCP server. Parse the host
    // bracket-aware: an IPv6 origin is `http://[::1]:3000`, where a naive
    // `split(':').next()` yields "[" and rejects a legitimate loopback.
    origin
        .split_once("://")
        .map(|(_, host_port)| {
            if let Some(rest) = host_port.strip_prefix('[') {
                rest.split(']').next().unwrap_or("") // "::1" (no brackets)
            } else {
                host_port.split(':').next().unwrap_or("")
            }
        })
        .is_some_and(|host| host == "localhost" || host == "127.0.0.1" || host == "::1")
}

fn forbidden_response(id: Option<Value>) -> Json<Value> {
    Json(serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": ERR_UNAUTHORIZED, "message": "Forbidden origin" }
    }))
}

fn unauthorized_response(id: Option<Value>) -> Json<Value> {
    Json(serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": ERR_UNAUTHORIZED, "message": "Unauthorized" }
    }))
}

/// Dispatch a single request or a JSON-RPC batch (array). Returns `None` only
/// when nothing needs answering (all notifications).
async fn dispatch_any(server: &McpServer, req: &Value) -> Option<Value> {
    let Some(batch) = req.as_array() else {
        return dispatch_async(server, req).await;
    };
    let mut out = Vec::with_capacity(batch.len());
    for item in batch {
        if let Some(resp) = dispatch_async(server, item).await {
            out.push(resp);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(Value::Array(out))
    }
}

/// Decode an `Mcp-Name` (or `Mcp-Param-*`) header value: values that are not
/// header-safe are carried Base64-encoded with the `=?base64?…?=` sentinel
/// (Streamable HTTP "Value Encoding"); plain values pass through unchanged.
fn decode_header_value(raw: &str) -> String {
    if let Some(encoded) = raw
        .strip_prefix("=?base64?")
        .and_then(|s| s.strip_suffix("?="))
    {
        use base64::Engine as _;
        if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded)
            && let Ok(text) = String::from_utf8(decoded)
        {
            return text;
        }
    }
    raw.to_string()
}

/// Validate the standard Streamable HTTP request headers of a *modern*
/// request: `MCP-Protocol-Version` must be present and match the body's
/// `_meta` version, the version must be one this server supports, `Mcp-Method`
/// must match the body method, and `Mcp-Name` must match `params.name` /
/// `params.uri` on the methods that carry one. Failures return the HTTP
/// status plus JSON-RPC error body the spec prescribes.
fn validate_modern_headers(
    req: &Value,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<Value>)> {
    let id = req.get("id").cloned();
    let reject = |status: StatusCode, code: i32, message: &str, data: Option<Value>| {
        Err((status, Json(rpc_error(id, code, message, data))))
    };
    let version = request_protocol_version(req).expect("caller checked for _meta version");
    let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");

    if !SUPPORTED_PROTOCOL_VERSIONS.contains(&version) {
        return reject(
            StatusCode::BAD_REQUEST,
            ERR_UNSUPPORTED_VERSION,
            "Unsupported protocol version",
            Some(serde_json::json!({
                "supported": SUPPORTED_PROTOCOL_VERSIONS,
                "requested": version,
            })),
        );
    }
    match headers
        .get("mcp-protocol-version")
        .and_then(|v| v.to_str().ok())
    {
        None => {
            return reject(
                StatusCode::BAD_REQUEST,
                ERR_HEADER_MISMATCH,
                "Missing required MCP-Protocol-Version header",
                None,
            );
        }
        Some(h) if h != version => {
            return reject(
                StatusCode::BAD_REQUEST,
                ERR_HEADER_MISMATCH,
                "MCP-Protocol-Version header does not match the body _meta version",
                None,
            );
        }
        _ => {}
    }
    match headers.get("mcp-method").and_then(|v| v.to_str().ok()) {
        None => {
            return reject(
                StatusCode::BAD_REQUEST,
                ERR_HEADER_MISMATCH,
                "Missing required Mcp-Method header",
                None,
            );
        }
        Some(h) if h != method => {
            return reject(
                StatusCode::BAD_REQUEST,
                ERR_HEADER_MISMATCH,
                &format!("Mcp-Method header '{h}' does not match body method '{method}'"),
                None,
            );
        }
        _ => {}
    }
    // `Mcp-Name` mirrors params.name / params.uri on the methods that carry
    // one. When the body has no value there, the client MUST omit the header
    // and the server MUST NOT expect it.
    let name_source = match method {
        "tools/call" | "prompts/get" => req.pointer("/params/name"),
        "resources/read" => req.pointer("/params/uri"),
        _ => None,
    };
    if let Some(source) = name_source
        && !source.is_null()
    {
        match headers.get("mcp-name").and_then(|v| v.to_str().ok()) {
            None => {
                return reject(
                    StatusCode::BAD_REQUEST,
                    ERR_HEADER_MISMATCH,
                    "Missing required Mcp-Name header",
                    None,
                );
            }
            Some(h) if decode_header_value(h) != source.as_str().unwrap_or("") => {
                return reject(
                    StatusCode::BAD_REQUEST,
                    ERR_HEADER_MISMATCH,
                    "Mcp-Name header does not match the body value",
                    None,
                );
            }
            _ => {}
        }
    }
    Ok(())
}

/// The `subscriptions/listen` answer: an SSE response stream carrying the
/// acknowledgment notification, then the graceful-closure result that ends
/// the subscription (this server agrees to no notification types and closes
/// immediately). `X-Accel-Buffering: no` asks reverse proxies not to hold
/// the stream back.
fn subscription_response(server: &McpServer, req: &Value) -> Response {
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let (ack, close) = server.subscription_ack_and_close(&id);
    let events = vec![
        Ok::<_, Infallible>(Event::default().data(ack.to_string())),
        Ok(Event::default().data(rpc_result(id, close).to_string())),
    ];
    (
        [(HeaderName::from_static("x-accel-buffering"), "no")],
        Sse::new(tokio_stream::iter(events)),
    )
        .into_response()
}

async fn rpc_handler(
    State(state): State<HttpTransport>,
    headers: HeaderMap,
    Json(req): Json<Value>,
) -> Response {
    let id = req.get("id").cloned();
    if !origin_allowed(&headers) {
        return (StatusCode::FORBIDDEN, forbidden_response(id)).into_response();
    }
    if !authorized(&state.server, &headers) {
        // RFC 6750: a 401 carries a WWW-Authenticate challenge. MCP clients
        // use it to discover how the server wants to be authenticated.
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, r#"Bearer realm="mcp""#)],
            unauthorized_response(id),
        )
            .into_response();
    }

    // Modern (stateless) requests are subject to the standard header
    // validation, the 404 unknown-method rule, and the SSE subscription
    // stream. `initialize` always selects legacy semantics; notifications
    // (no id) have no header requirements in this revision.
    let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let modern = if method == "initialize" {
        None
    } else {
        request_protocol_version(&req)
    };
    if modern.is_some() && req.get("id").is_some() {
        if let Err((status, body)) = validate_modern_headers(&req, &headers) {
            return (status, body).into_response();
        }
        if !MODERN_METHODS.contains(&method) {
            return (
                StatusCode::NOT_FOUND,
                Json(rpc_error(
                    id,
                    ERR_METHOD_NOT_FOUND,
                    &format!("Method not found: {method}"),
                    None,
                )),
            )
                .into_response();
        }
        if method == "subscriptions/listen" {
            return subscription_response(&state.server, &req);
        }
    }

    match dispatch_any(&state.server, &req).await {
        Some(resp) => (StatusCode::OK, Json(resp)).into_response(),
        // Notification-only input — the Streamable HTTP transport requires
        // 202 Accepted with no body.
        None => StatusCode::ACCEPTED.into_response(),
    }
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

    #[test]
    fn origin_validation_blocks_cross_site_browsers() {
        let mut h = HeaderMap::new();
        assert!(origin_allowed(&h), "no Origin (non-browser client) passes");
        h.insert("origin", "http://localhost:3000".parse().unwrap());
        assert!(origin_allowed(&h));
        h.insert("origin", "http://127.0.0.1:8080".parse().unwrap());
        assert!(origin_allowed(&h));
        h.insert("origin", "https://evil.example.com".parse().unwrap());
        assert!(!origin_allowed(&h), "DNS-rebinding origin must be rejected");
        h.insert("origin", "null".parse().unwrap());
        assert!(!origin_allowed(&h));
        // Suffix trickery must not pass.
        h.insert("origin", "https://localhost.evil.com".parse().unwrap());
        assert!(!origin_allowed(&h));
        // IPv6 loopback — a naive `split(':')` would see "[" and reject it.
        h.insert("origin", "http://[::1]:3000".parse().unwrap());
        assert!(origin_allowed(&h), "IPv6 loopback must pass: {h:?}");
        h.insert("origin", "http://[::1]".parse().unwrap());
        assert!(origin_allowed(&h), "IPv6 loopback (no port) must pass");
        h.insert("origin", "http://[fe80::1]:3000".parse().unwrap());
        assert!(!origin_allowed(&h), "non-loopback IPv6 must be rejected");
    }

    #[tokio::test]
    async fn batch_requests_get_a_batch_response() {
        let server = server_with_echo();
        let batch = serde_json::json!([
            {"jsonrpc":"2.0","id":1,"method":"ping"},
            {"jsonrpc":"2.0","id":2,"method":"tools/call",
             "params":{"name":"echo","arguments":{"v":1}}}
        ]);
        let resp = dispatch_any(&server, &batch)
            .await
            .expect("batch must not be dropped");
        let arr = resp.as_array().expect("array response");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], 1);
        assert_eq!(arr[1]["result"]["isError"], false);
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

    /// Wire-level auth check: a 401 must carry `WWW-Authenticate` (RFC 6750 /
    /// MCP authorization discovery), never a bare body.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unauthorized_response_carries_www_authenticate() {
        let server = Arc::new(McpServer::new("http-test", "1.0.0").with_bearer_auth("tok"));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let transport = HttpTransport::new(server);
        tokio::spawn(async move {
            let _ = axum::serve(listener, transport.router()).await;
        });

        let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
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
        assert!(
            response.contains("401 Unauthorized"),
            "response: {response}"
        );
        assert!(
            response
                .to_ascii_lowercase()
                .contains("www-authenticate: bearer"),
            "missing WWW-Authenticate challenge: {response}"
        );
    }

    /// Notification-only POST answers 202 Accepted with no body (Streamable
    /// HTTP transport requirement).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn notification_only_post_returns_202() {
        let server = Arc::new(server_with_echo());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let transport = HttpTransport::new(server);
        tokio::spawn(async move {
            let _ = axum::serve(listener, transport.router()).await;
        });

        let body = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
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
        assert!(response.contains("202 Accepted"), "response: {response}");
        let body = response.split("\r\n\r\n").nth(1).unwrap_or("");
        assert!(body.trim().is_empty(), "202 must have no body: {body}");
    }

    /// The nonstandard `POST /mcp/sse` endpoint is gone — the Streamable
    /// HTTP transport serves everything on `/mcp`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn legacy_sse_endpoint_is_gone() {
        let server = Arc::new(server_with_echo());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let transport = HttpTransport::new(server);
        tokio::spawn(async move {
            let _ = axum::serve(listener, transport.router()).await;
        });

        let body = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
        let req = format!(
            "POST /mcp/sse HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).await.unwrap();
        let response = String::from_utf8_lossy(&buf);
        assert!(response.contains("404 Not Found"), "response: {response}");
    }

    /// Bind a router on an ephemeral port and POST one raw request to it.
    /// Returns the full HTTP response (status line, headers, body).
    async fn post_raw(headers: &[(&str, &str)], body: &str) -> String {
        let server = Arc::new(server_with_echo());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let transport = HttpTransport::new(server);
        tokio::spawn(async move {
            let _ = axum::serve(listener, transport.router()).await;
        });

        let header_block: String = headers
            .iter()
            .map(|(k, v)| format!("{k}: {v}\r\n"))
            .collect();
        let req = format!(
            "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{header_block}Connection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).await.unwrap();
        String::from_utf8_lossy(&buf).into_owned()
    }

    /// A modern (stateless) request body declaring its protocol version.
    fn modern_body(method: &str, id: i64) -> String {
        serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": method,
            "params": { "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" } }
        })
        .to_string()
    }

    /// Modern requests must carry the standard headers; omitting them is a
    /// 400 with the spec's HeaderMismatch error.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn modern_request_without_headers_is_400_header_mismatch() {
        let response = post_raw(&[], &modern_body("tools/list", 1)).await;
        assert!(response.contains("400 Bad Request"), "response: {response}");
        assert!(response.contains("-32020"), "response: {response}");
    }

    /// A fully-headered modern request answers 200 with modern result
    /// shaping (`resultType`, `ttlMs`, `cacheScope`).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn modern_request_with_valid_headers_answers_shaped_result() {
        let response = post_raw(
            &[
                ("MCP-Protocol-Version", "2026-07-28"),
                ("Mcp-Method", "tools/list"),
            ],
            &modern_body("tools/list", 2),
        )
        .await;
        assert!(response.contains("200 OK"), "response: {response}");
        assert!(
            response.contains("\"resultType\":\"complete\""),
            "response: {response}"
        );
        assert!(response.contains("\"ttlMs\""), "response: {response}");
        assert!(
            response.contains("\"cacheScope\":\"private\""),
            "response: {response}"
        );
    }

    /// A mismatched MCP-Protocol-Version header is a 400 HeaderMismatch.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn modern_version_header_mismatch_is_400() {
        let response = post_raw(
            &[
                ("MCP-Protocol-Version", "2025-06-18"),
                ("Mcp-Method", "tools/list"),
            ],
            &modern_body("tools/list", 3),
        )
        .await;
        assert!(response.contains("400 Bad Request"), "response: {response}");
        assert!(response.contains("-32020"), "response: {response}");
    }

    /// An unsupported modern version is a 400 UnsupportedProtocolVersion
    /// listing the supported versions for the client to retry with.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn modern_unsupported_version_is_400_with_supported_list() {
        let body = serde_json::json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/list",
            "params": { "_meta": { "io.modelcontextprotocol/protocolVersion": "1900-01-01" } }
        })
        .to_string();
        let response = post_raw(
            &[
                ("MCP-Protocol-Version", "1900-01-01"),
                ("Mcp-Method", "tools/list"),
            ],
            &body,
        )
        .await;
        assert!(response.contains("400 Bad Request"), "response: {response}");
        assert!(response.contains("-32022"), "response: {response}");
        assert!(response.contains("\"supported\""), "response: {response}");
    }

    /// Modern unknown methods answer 404 + `-32601` (distinguishing a modern
    /// server from a legacy one that just lacks the endpoint).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn modern_unknown_method_is_404() {
        let response = post_raw(
            &[
                ("MCP-Protocol-Version", "2026-07-28"),
                ("Mcp-Method", "no/such-method"),
            ],
            &modern_body("no/such-method", 5),
        )
        .await;
        assert!(response.contains("404 Not Found"), "response: {response}");
        assert!(response.contains("-32601"), "response: {response}");
    }

    /// Modern `ping` was removed in 2026-07-28 — it is an unknown method.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn modern_ping_is_404() {
        let response = post_raw(
            &[
                ("MCP-Protocol-Version", "2026-07-28"),
                ("Mcp-Method", "ping"),
            ],
            &modern_body("ping", 6),
        )
        .await;
        assert!(response.contains("404 Not Found"), "response: {response}");
    }

    /// `tools/call` carries an `Mcp-Name` header that must match the body.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn modern_tools_call_requires_matching_mcp_name() {
        let body = serde_json::json!({
            "jsonrpc": "2.0", "id": 7, "method": "tools/call",
            "params": {
                "name": "echo",
                "arguments": { "v": 1 },
                "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" }
            }
        })
        .to_string();
        // Missing header → 400.
        let response = post_raw(
            &[
                ("MCP-Protocol-Version", "2026-07-28"),
                ("Mcp-Method", "tools/call"),
            ],
            &body,
        )
        .await;
        assert!(response.contains("400 Bad Request"), "response: {response}");
        // Matching header → 200.
        let response = post_raw(
            &[
                ("MCP-Protocol-Version", "2026-07-28"),
                ("Mcp-Method", "tools/call"),
                ("Mcp-Name", "echo"),
            ],
            &body,
        )
        .await;
        assert!(response.contains("200 OK"), "response: {response}");
        assert!(
            response.contains("\"resultType\":\"complete\""),
            "response: {response}"
        );
    }

    /// `subscriptions/listen` answers with an SSE stream: the acknowledgment
    /// notification, then the graceful-closure result, with the proxy
    /// no-buffering header.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn modern_subscriptions_listen_streams_ack_and_close() {
        let response = post_raw(
            &[
                ("MCP-Protocol-Version", "2026-07-28"),
                ("Mcp-Method", "subscriptions/listen"),
            ],
            &modern_body("subscriptions/listen", 8),
        )
        .await;
        assert!(response.contains("200 OK"), "response: {response}");
        assert!(
            response
                .to_ascii_lowercase()
                .contains("content-type: text/event-stream"),
            "response: {response}"
        );
        assert!(
            response
                .to_ascii_lowercase()
                .contains("x-accel-buffering: no"),
            "response: {response}"
        );
        assert!(
            response.contains("notifications/subscriptions/acknowledged"),
            "response: {response}"
        );
        assert!(
            response.contains("\"resultType\":\"complete\""),
            "closure result on the stream: {response}"
        );
    }

    /// Legacy (handshake-era) requests carry no `_meta` and are exempt from
    /// the modern header requirements — dual-era serving.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn legacy_request_needs_no_modern_headers() {
        let body = r#"{"jsonrpc":"2.0","id":9,"method":"tools/list"}"#;
        let response = post_raw(&[], body).await;
        assert!(response.contains("200 OK"), "response: {response}");
        // Legacy result: no modern shaping.
        assert!(
            !response.contains("resultType"),
            "legacy result must stay unshaped: {response}"
        );
    }

    /// Base64-sentinel Mcp-Name values decode before comparison.
    #[test]
    fn decode_header_value_handles_base64_sentinel() {
        // "echo" → ZWNobw==
        assert_eq!(decode_header_value("=?base64?ZWNobw==?="), "echo");
        assert_eq!(decode_header_value("echo"), "echo");
        // Malformed sentinel falls back to the raw value.
        assert_eq!(decode_header_value("=?base64?!!!?="), "=?base64?!!!?=");
    }

    /// Modern `server/discover` over the HTTP dispatcher.
    #[tokio::test]
    async fn dispatch_modern_discover() {
        let server = server_with_echo();
        let req: Value = serde_json::from_str(&modern_body("server/discover", 1)).unwrap();
        let resp = dispatch_async(&server, &req).await.unwrap();
        assert_eq!(
            resp["result"]["supportedVersions"][0],
            crate::mcp::server::LATEST_PROTOCOL_VERSION
        );
        assert_eq!(resp["result"]["resultType"], "complete");
        assert!(resp["result"]["ttlMs"].is_u64());
    }
}
