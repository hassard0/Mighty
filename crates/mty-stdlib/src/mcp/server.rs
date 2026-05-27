//! MCP server that auto-exposes the @tool registry.
//!
//! Reads JSON-RPC requests from the transport, routes them to the
//! correct handler, and writes JSON-RPC responses back. Handlers:
//!
//! - `initialize` — handshake; returns the server's name + protocol
//!   version.
//! - `tools/list` — returns every descriptor in the
//!   [`super::registered_descriptors`] snapshot.
//! - `tools/call` — looks up the named tool, runs the cap check,
//!   invokes the body, and returns the JSON result.
//! - `ping` — liveness probe; returns `{}`.
//!
//! Unknown methods return JSON-RPC error -32601 (method not found).

use super::sandbox::{current_default_capability_set, CapabilitySet};
use super::transport::{StdioTransport, Transport};
use super::{
    invoke_tool, registered_descriptors, JsonRpcError, JsonRpcRequest, JsonRpcResponse, McpError,
    ToolError, MCP_PROTOCOL_VERSION,
};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

/// MCP server config — name + version surfaced to clients during
/// the `initialize` handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

impl Default for ServerInfo {
    fn default() -> Self {
        Self {
            name: "mty-mcp-server".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
}

/// The MCP server. Holds the [`ServerInfo`] and an optional
/// per-server cap-set override (when `None`, the process-wide default
/// from [`current_default_capability_set`] is used per call).
#[derive(Debug, Default, Clone)]
pub struct McpServer {
    pub info: ServerInfo,
    /// If `Some`, every tool call uses this cap-set. If `None`,
    /// reads the process-wide default at call-time (so a driver can
    /// install fresh caps before each request).
    pub caps: Option<CapabilitySet>,
}

impl McpServer {
    /// Build a server that reads its tool list from the process-wide
    /// `@tool` registry. The `caps` field defaults to `None` so the
    /// driver's installed default cap-set applies.
    pub fn from_tool_registry() -> Self {
        Self::default()
    }

    /// Override the cap-set this server hands to every tool call.
    /// Returns the server for builder-style chaining.
    pub fn with_capabilities(mut self, caps: CapabilitySet) -> Self {
        self.caps = Some(caps);
        self
    }

    /// Override the server-info advertised during `initialize`.
    pub fn with_info(mut self, info: ServerInfo) -> Self {
        self.info = info;
        self
    }

    /// Run the server's read-route-write loop over an arbitrary
    /// transport. Returns once `read_frame` returns `Ok(None)` (clean
    /// EOF) or a fatal transport error.
    pub fn serve_on<T: Transport>(&self, mut transport: T) -> Result<(), McpError> {
        while let Some(req) = transport.read_frame()? {
            let resp = self.handle_request(req);
            transport.write_frame(&resp)?;
        }
        Ok(())
    }

    /// Convenience: run the server over the process's stdio. Blocking.
    pub fn serve_stdio_blocking(&self) -> Result<(), McpError> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let transport = StdioTransport::new(stdin.lock(), stdout.lock());
        self.serve_on(transport)
    }

    /// Run the server over the supplied [`Read`]+[`Write`] pair.
    /// Used by tests for in-memory round-trips.
    pub fn serve_io<R: Read, W: Write>(&self, reader: R, writer: W) -> Result<(), McpError> {
        let transport = StdioTransport::new(reader, writer);
        self.serve_on(transport)
    }

    /// Dispatch one request, producing the response. Pure function —
    /// no I/O.
    pub fn handle_request(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        let id = req.id.clone().unwrap_or(serde_json::Value::Null);
        match req.method.as_str() {
            "initialize" => self.handle_initialize(id),
            "ping" => JsonRpcResponse::success(id, serde_json::json!({})),
            "tools/list" => self.handle_tools_list(id),
            "tools/call" => self.handle_tools_call(id, req.params),
            other => JsonRpcResponse::failure(
                id,
                JsonRpcError {
                    code: -32601,
                    message: format!("method not found: {other}"),
                    data: None,
                },
            ),
        }
    }

    fn handle_initialize(&self, id: serde_json::Value) -> JsonRpcResponse {
        JsonRpcResponse::success(
            id,
            serde_json::json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "serverInfo": self.info,
                "capabilities": {
                    "tools": {}
                }
            }),
        )
    }

    fn handle_tools_list(&self, id: serde_json::Value) -> JsonRpcResponse {
        let tools = registered_descriptors();
        JsonRpcResponse::success(id, serde_json::json!({ "tools": tools }))
    }

    fn handle_tools_call(
        &self,
        id: serde_json::Value,
        params: Option<serde_json::Value>,
    ) -> JsonRpcResponse {
        let Some(params) = params else {
            return JsonRpcResponse::failure(
                id,
                JsonRpcError::from_tool_error(&ToolError::InvalidArguments {
                    tool: "<unknown>".into(),
                    detail: "params missing".into(),
                }),
            );
        };
        let Some(name) = params
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::to_string)
        else {
            return JsonRpcResponse::failure(
                id,
                JsonRpcError::from_tool_error(&ToolError::InvalidArguments {
                    tool: "<unknown>".into(),
                    detail: "`name` field missing".into(),
                }),
            );
        };
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::Value::Object(Default::default()));
        let caps = self
            .caps
            .clone()
            .unwrap_or_else(current_default_capability_set);
        match invoke_tool(&name, args, &caps) {
            Ok(result) => JsonRpcResponse::success(
                id,
                serde_json::json!({ "content": [{ "type": "text", "text": json_to_string(&result) }] }),
            ),
            Err(err) => JsonRpcResponse::failure(id, JsonRpcError::from_tool_error(&err)),
        }
    }
}

/// Render a JSON value as the `text` body of an MCP tool result. The
/// MCP spec returns content as an array of `{type, text}` blocks; we
/// keep a single block and stringify the value (strings stay
/// unquoted; other shapes serialise compactly).
fn json_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ParamFieldSchema, RegisteredTool, ToolDescriptor, ToolParameterSchema};
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn make_registered(name: &str, cap: Option<&str>) -> RegisteredTool {
        let cap_owned = cap.map(|s| s.to_string());
        let mut props = HashMap::new();
        props.insert("x".to_string(), ParamFieldSchema::primitive("string"));
        RegisteredTool {
            descriptor: ToolDescriptor {
                name: name.to_string(),
                description: "test tool".to_string(),
                input_schema: ToolParameterSchema {
                    ty: "object".into(),
                    properties: props,
                    required: vec!["x".into()],
                },
                capability: cap_owned.clone(),
            },
            invoke: Arc::new(move |args, _caps| {
                let x = args.get("x").and_then(|v| v.as_str()).unwrap_or("");
                Ok(json!(format!("echo: {x}")))
            }),
        }
    }

    #[test]
    fn unknown_method_returns_32601() {
        let srv = McpServer::default();
        let req = JsonRpcRequest::new(1, "frobnicate", json!({}));
        let resp = srv.handle_request(req);
        let err = resp.error.expect("expected error");
        assert_eq!(err.code, -32601);
    }

    #[test]
    fn initialize_returns_protocol_version() {
        let srv = McpServer::default();
        let req = JsonRpcRequest::new(1, "initialize", json!({}));
        let resp = srv.handle_request(req);
        let result = resp.result.expect("expected success");
        assert_eq!(
            result.get("protocolVersion").and_then(|v| v.as_str()),
            Some(MCP_PROTOCOL_VERSION),
        );
    }

    #[test]
    fn ping_returns_empty_object() {
        let srv = McpServer::default();
        let req = JsonRpcRequest::new(2, "ping", json!({}));
        let resp = srv.handle_request(req);
        assert!(resp.error.is_none());
        assert_eq!(resp.result, Some(json!({})));
    }

    #[test]
    fn tools_call_without_name_field_returns_invalid_args() {
        let srv = McpServer::default();
        let req = JsonRpcRequest::new(3, "tools/call", json!({ "arguments": {} }));
        let resp = srv.handle_request(req);
        let err = resp.error.expect("expected error");
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn json_to_string_unwraps_strings() {
        assert_eq!(json_to_string(&json!("hi")), "hi");
        assert_eq!(json_to_string(&json!(42)), "42");
        assert_eq!(json_to_string(&json!({"k": 1})), "{\"k\":1}");
    }

    #[test]
    fn make_registered_compiles_with_arc_invoke() {
        // Sanity that the test helper itself compiles and runs.
        let tool = make_registered("echo", Some("fs.read"));
        assert_eq!(tool.descriptor.name, "echo");
        assert_eq!(tool.descriptor.capability.as_deref(), Some("fs.read"));
    }
}
