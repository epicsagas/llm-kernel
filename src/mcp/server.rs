//! MCP server core — tool registration, initialization, and dispatch logic.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use async_trait::async_trait;

use crate::mcp::auth::BearerAuth;
use crate::mcp::schema::{PromptDescription, ResourceDescription, ToolDescription};

/// MCP protocol versions this server understands, newest first.
///
/// During `initialize` the server echoes the client's requested version when it
/// appears here, otherwise it falls back to [`LATEST_PROTOCOL_VERSION`].
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// The newest MCP protocol version this server implements.
pub const LATEST_PROTOCOL_VERSION: &str = "2025-06-18";

/// Handler function type for MCP tool calls (synchronous).
pub type Handler =
    Box<dyn Fn(serde_json::Value) -> crate::error::Result<serde_json::Value> + Send + Sync>;

/// Async tool-handler trait — the async counterpart to the synchronous [`Handler`].
///
/// Object-safe via `async_trait`, so an [`McpServer`] can store
/// `Arc<dyn AsyncToolHandler>` and await it from an async transport
/// (e.g. the HTTP/SSE transport).
#[async_trait]
pub trait AsyncToolHandler: Send + Sync {
    /// Invoke the handler with the tool call parameters.
    async fn call(&self, params: serde_json::Value) -> crate::error::Result<serde_json::Value>;
}

/// Adapts an async closure `Fn(Value) -> Future<Output = Result<Value>>` into an
/// [`AsyncToolHandler`], so [`McpServer::set_async_handler`] accepts a plain
/// async closure.
struct AsyncHandlerFn<F>(F);

#[async_trait]
impl<F, Fut> AsyncToolHandler for AsyncHandlerFn<F>
where
    F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = crate::error::Result<serde_json::Value>> + Send,
{
    async fn call(&self, params: serde_json::Value) -> crate::error::Result<serde_json::Value> {
        (self.0)(params).await
    }
}

/// An MCP server that manages tools, resources, prompts, and dispatches calls.
pub struct McpServer {
    server_name: String,
    server_version: String,
    tools: Vec<ToolDescription>,
    resources: Vec<ResourceDescription>,
    prompts: Vec<PromptDescription>,
    handlers: HashMap<String, Handler>,
    async_handlers: HashMap<String, Arc<dyn AsyncToolHandler>>,
    resource_handlers: HashMap<String, Handler>,
    prompt_handlers: HashMap<String, Handler>,
    auth: Option<BearerAuth>,
}

impl McpServer {
    /// Create a new MCP server with no authentication.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            server_name: name.into(),
            server_version: version.into(),
            tools: Vec::new(),
            resources: Vec::new(),
            prompts: Vec::new(),
            handlers: HashMap::new(),
            async_handlers: HashMap::new(),
            resource_handlers: HashMap::new(),
            prompt_handlers: HashMap::new(),
            auth: None,
        }
    }

    /// Require bearer token authentication for all requests.
    pub fn with_bearer_auth(mut self, token: impl Into<String>) -> Self {
        self.auth = Some(BearerAuth::new(token));
        self
    }

    /// Generate and attach a random bearer token.
    ///
    /// Returns the generated token so the caller can distribute it.
    pub fn with_generated_auth(mut self) -> (Self, String) {
        let bearer = BearerAuth::generate();
        let token = bearer.token().to_string();
        self.auth = Some(bearer);
        (self, token)
    }

    /// Validate an `Authorization` header value. Always returns `true` when no auth is configured.
    pub fn check_auth(&self, authorization_header: &str) -> bool {
        match &self.auth {
            None => true,
            Some(bearer) => bearer.validate(authorization_header),
        }
    }

    /// Returns `true` if bearer authentication is enabled on this server.
    pub fn auth_enabled(&self) -> bool {
        self.auth.is_some()
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
        handler: impl Fn(serde_json::Value) -> crate::error::Result<serde_json::Value>
        + Send
        + Sync
        + 'static,
    ) {
        self.handlers
            .insert(tool_name.to_string(), Box::new(handler));
    }

    /// Register an async handler for a tool by name.
    ///
    /// `handler` is a closure returning a future (typically `async move { … }`).
    /// Async handlers take precedence over sync handlers registered with
    /// [`Self::set_handler`] when resolved via [`Self::call_tool_async`].
    pub fn set_async_handler<F, Fut>(&mut self, tool_name: &str, handler: F)
    where
        F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = crate::error::Result<serde_json::Value>> + Send,
    {
        self.async_handlers
            .insert(tool_name.to_string(), Arc::new(AsyncHandlerFn(handler)));
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

    /// Set the handler for a resource by URI.
    pub fn set_resource_handler(
        &mut self,
        uri: &str,
        handler: impl Fn(serde_json::Value) -> crate::error::Result<serde_json::Value>
        + Send
        + Sync
        + 'static,
    ) {
        self.resource_handlers
            .insert(uri.to_string(), Box::new(handler));
    }

    /// Read a resource by URI with the given parameters.
    pub fn read_resource(
        &self,
        uri: &str,
        params: serde_json::Value,
    ) -> crate::error::Result<serde_json::Value> {
        let handler = self
            .resource_handlers
            .get(uri)
            .ok_or_else(|| crate::error::KernelError::Config(format!("unknown resource: {uri}")))?;
        handler(params)
    }

    /// Register a prompt with the server.
    pub fn register_prompt(&mut self, prompt: PromptDescription) {
        self.prompts.push(prompt);
    }

    /// Set the handler for a prompt by name.
    ///
    /// The handler receives the `prompts/get` arguments object and returns the
    /// result value — typically `{ "description": ..., "messages": [...] }`.
    pub fn set_prompt_handler(
        &mut self,
        prompt_name: &str,
        handler: impl Fn(serde_json::Value) -> crate::error::Result<serde_json::Value>
        + Send
        + Sync
        + 'static,
    ) {
        self.prompt_handlers
            .insert(prompt_name.to_string(), Box::new(handler));
    }

    /// List all registered prompts.
    pub fn prompts(&self) -> &[PromptDescription] {
        &self.prompts
    }

    /// Render a prompt by name with the given arguments.
    pub fn get_prompt(
        &self,
        name: &str,
        params: serde_json::Value,
    ) -> crate::error::Result<serde_json::Value> {
        let handler = self
            .prompt_handlers
            .get(name)
            .ok_or_else(|| crate::error::KernelError::Config(format!("unknown prompt: {name}")))?;
        handler(params)
    }

    /// Whether a tool with `name` is registered (has a sync or async handler).
    ///
    /// Lets a transport distinguish an *unknown tool* (a protocol-level invalid
    /// params error) from a tool that ran and *failed* (reported in-band with
    /// `isError: true`).
    pub fn has_tool(&self, name: &str) -> bool {
        self.handlers.contains_key(name) || self.async_handlers.contains_key(name)
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

    /// Call a tool by name, awaiting an async handler if one is registered and
    /// otherwise falling back to the synchronous handler. Errors if the tool is
    /// unknown. This is the entry point used by async transports (e.g. HTTP/SSE).
    pub async fn call_tool_async(
        &self,
        name: &str,
        params: serde_json::Value,
    ) -> crate::error::Result<serde_json::Value> {
        if let Some(handler) = self.async_handlers.get(name) {
            return handler.call(params).await;
        }
        if let Some(handler) = self.handlers.get(name) {
            return handler(params);
        }
        Err(crate::error::KernelError::Config(format!(
            "unknown tool: {name}"
        )))
    }

    /// Resolve the protocol version to report in `initialize`.
    ///
    /// Echoes `requested` when it is one of [`SUPPORTED_PROTOCOL_VERSIONS`];
    /// otherwise returns [`LATEST_PROTOCOL_VERSION`] (per the MCP spec, the
    /// server proposes its own latest when it cannot honor the client's).
    pub fn negotiate_protocol_version(&self, requested: Option<&str>) -> &'static str {
        match requested {
            Some(v) => SUPPORTED_PROTOCOL_VERSIONS
                .iter()
                .find(|&&s| s == v)
                .copied()
                .unwrap_or(LATEST_PROTOCOL_VERSION),
            None => LATEST_PROTOCOL_VERSION,
        }
    }

    /// Build the `initialize` response, negotiating the protocol version against
    /// the client's requested version.
    ///
    /// The advertised capabilities reflect what the server actually supports:
    /// `tools` and `resources` are always present; `prompts` is included only
    /// when at least one prompt is registered.
    pub fn initialize_response(&self, requested_version: Option<&str>) -> serde_json::Value {
        let mut capabilities = serde_json::json!({
            "tools": { "listChanged": false },
            "resources": { "subscribe": false, "listChanged": false },
        });
        if !self.prompts.is_empty() {
            capabilities["prompts"] = serde_json::json!({ "listChanged": false });
        }
        serde_json::json!({
            "protocolVersion": self.negotiate_protocol_version(requested_version),
            "capabilities": capabilities,
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

        let result = server
            .call_tool("echo", serde_json::json!({"msg": "hi"}))
            .unwrap();
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
        let resp = server.initialize_response(None);
        assert_eq!(resp["serverInfo"]["name"], "my-server");
        assert_eq!(resp["protocolVersion"], LATEST_PROTOCOL_VERSION);
        // No prompts registered → no prompts capability advertised.
        assert!(resp["capabilities"].get("prompts").is_none());
    }

    #[test]
    fn initialize_negotiates_supported_version() {
        let server = McpServer::new("s", "1.0");
        // A supported version the client asked for is echoed back.
        assert_eq!(
            server.initialize_response(Some("2024-11-05"))["protocolVersion"],
            "2024-11-05"
        );
        // An unsupported version falls back to the server's latest.
        assert_eq!(
            server.initialize_response(Some("1999-01-01"))["protocolVersion"],
            LATEST_PROTOCOL_VERSION
        );
    }

    #[test]
    fn initialize_advertises_prompts_when_registered() {
        let mut server = McpServer::new("s", "1.0");
        server.register_prompt(PromptDescription {
            name: "greet".into(),
            description: None,
            arguments: Vec::new(),
        });
        let resp = server.initialize_response(None);
        assert!(resp["capabilities"]["prompts"].is_object());
    }

    #[test]
    fn register_and_get_prompt() {
        let mut server = McpServer::new("s", "1.0");
        server.register_prompt(PromptDescription {
            name: "greet".into(),
            description: Some("Greet someone".into()),
            arguments: vec![crate::mcp::schema::PromptArgument {
                name: "name".into(),
                description: None,
                required: true,
            }],
        });
        server.set_prompt_handler("greet", |params| {
            let who = params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("world");
            Ok(serde_json::json!({
                "messages": [{
                    "role": "user",
                    "content": { "type": "text", "text": format!("Hello, {who}!") }
                }]
            }))
        });
        assert_eq!(server.prompts().len(), 1);
        let result = server
            .get_prompt("greet", serde_json::json!({ "name": "Ada" }))
            .unwrap();
        assert_eq!(result["messages"][0]["content"]["text"], "Hello, Ada!");
    }

    #[test]
    fn unknown_prompt_returns_error() {
        let server = McpServer::new("s", "1.0");
        assert!(server.get_prompt("missing", serde_json::json!({})).is_err());
    }

    #[test]
    fn has_tool_reports_registration() {
        let mut server = McpServer::new("s", "1.0");
        server.set_handler("echo", |p| Ok(p));
        assert!(server.has_tool("echo"));
        assert!(!server.has_tool("nope"));
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

    #[test]
    fn read_resource() {
        let mut server = McpServer::new("test", "0.1.0");
        server.register_resource(ResourceDescription {
            uri: "docs://readme".into(),
            name: "README".into(),
            description: Some("Project readme".into()),
            mime_type: Some("text/markdown".into()),
        });
        server.set_resource_handler("docs://readme", |_params| {
            Ok(serde_json::json!("# Hello World"))
        });

        let result = server
            .read_resource("docs://readme", serde_json::json!({}))
            .unwrap();
        assert_eq!(result, serde_json::json!("# Hello World"));
    }

    #[test]
    fn unknown_resource_returns_error() {
        let server = McpServer::new("test", "0.1.0");
        let result = server.read_resource("missing://uri", serde_json::json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn no_auth_by_default() {
        let server = McpServer::new("test", "0.1.0");
        assert!(!server.auth_enabled());
        assert!(server.check_auth(""));
        assert!(server.check_auth("Bearer whatever"));
    }

    #[test]
    fn with_bearer_auth_validates_correctly() {
        let server = McpServer::new("test", "0.1.0").with_bearer_auth("my-token");
        assert!(server.auth_enabled());
        assert!(server.check_auth("Bearer my-token"));
        assert!(!server.check_auth("Bearer wrong"));
        assert!(!server.check_auth(""));
    }

    #[test]
    fn with_generated_auth_returns_token() {
        let (server, token) = McpServer::new("test", "0.1.0").with_generated_auth();
        assert!(server.auth_enabled());
        assert_eq!(token.len(), 32);
        assert!(server.check_auth(&format!("Bearer {token}")));
        assert!(!server.check_auth("Bearer bad"));
    }

    /// AC3: an async-registered tool resolves via `call_tool_async` and is awaited.
    #[tokio::test]
    async fn async_handler_is_awaited() {
        let mut server = McpServer::new("test", "0.1.0");
        server.register_tool(ToolDescription {
            name: "async-echo".into(),
            description: "Echo input asynchronously".into(),
            input_schema: serde_json::json!({"type": "object"}),
        });
        server.set_async_handler("async-echo", |params| async move { Ok(params) });

        let result = server
            .call_tool_async("async-echo", serde_json::json!({"msg": "hi"}))
            .await
            .unwrap();
        assert_eq!(result["msg"], "hi");
    }

    /// AC3: `call_tool_async` falls back to a sync handler when no async one is set.
    #[tokio::test]
    async fn async_dispatch_falls_back_to_sync() {
        let mut server = McpServer::new("test", "0.1.0");
        server.register_tool(ToolDescription {
            name: "sync-echo".into(),
            description: "Echo input synchronously".into(),
            input_schema: serde_json::json!({"type": "object"}),
        });
        server.set_handler("sync-echo", |params| Ok(params));

        let result = server
            .call_tool_async("sync-echo", serde_json::json!({"x": 1}))
            .await
            .unwrap();
        assert_eq!(result["x"], 1);
    }

    #[tokio::test]
    async fn call_tool_async_unknown_tool_errors() {
        let server = McpServer::new("test", "0.1.0");
        assert!(
            server
                .call_tool_async("missing", serde_json::json!(null))
                .await
                .is_err()
        );
    }
}
