//! `std.mcp` — Model Context Protocol server + client + tool registry
//! (v0.26 Track B).
//!
//! This module ships three layers that together make Mighty "the
//! standard language for agents":
//!
//! 1. **Tool registry.** Process-wide `__TOOL_REGISTRY` mapping
//!    tool-name → [`RegisteredTool`]. The `@tool` attribute macro (in
//!    `mty_macros::stdlib::tool`) generates per-fn registration calls
//!    that populate this map at module-init time. Downstream code can
//!    also register tools imperatively via [`register_tool`].
//!
//! 2. **Capability-enforced sandbox.** Every tool declares the
//!    capabilities it needs (`cap: fs.read`, `cap: net.get("api.x")`,
//!    …). The runtime checks the active [`CapabilitySet`] BEFORE
//!    invoking the tool body — if the cap is missing or the requested
//!    resource is outside the granted scope, the call short-circuits
//!    with [`ToolError::CapabilityDenied`] (never reaches the body).
//!    This guarantees the LLM cannot escalate by prompting; the
//!    enforcement lives in the runtime.
//!
//! 3. **MCP transport.** [`McpServer`] auto-exposes the registry over
//!    stdio (canonical MCP) or HTTP (JSON-RPC body); [`McpClient`]
//!    speaks the same protocol to other MCP-compliant servers. The
//!    JSON-RPC shape (`tools/list`, `tools/call`, `initialize`)
//!    matches the upstream MCP spec at
//!    <https://modelcontextprotocol.io/specification>.
//!
//! ## Surface example
//!
//! ```ignore
//! @tool("Read a file from disk", cap: fs.read)
//! fn read_file(path: String) -> Result[String, FsError] !{fs} {
//!   std.fs.read_to_string(path)
//! }
//!
//! // Auto-expose every @tool-annotated fn:
//! fn main() {
//!   let server = McpServer::from_tool_registry();
//!   server.serve_stdio().await.unwrap();
//! }
//! ```

pub mod client;
pub mod sandbox;
pub mod server;
pub mod transport;

pub use client::{connect_stdio, McpClient};
pub use sandbox::{
    current_default_capability_set, install_default_capability_set, with_default_capability_set,
    CapabilityGrant, CapabilitySet, FsMode,
};
pub use server::{McpServer, ServerInfo};
pub use transport::{StdioTransport, Transport};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// JSON-RPC 2.0 protocol version constant. The MCP spec pins JSON-RPC
/// 2.0 as the wire format.
pub const JSONRPC_VERSION: &str = "2.0";

/// MCP protocol version this implementation advertises during the
/// `initialize` handshake. Matches the upstream spec at
/// <https://modelcontextprotocol.io/specification/2024-11-05>.
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// JSON Schema (a subset) describing one tool's parameter shape. The
/// `@tool` macro auto-generates this from the Mighty parameter types.
/// Mirrors the OpenAI / Anthropic tool-schema convention so the same
/// descriptor can be handed to any LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolParameterSchema {
    /// The top-level JSON type — always `"object"` for MCP tools.
    #[serde(rename = "type")]
    pub ty: String,
    /// Field-name → field-schema map.
    pub properties: HashMap<String, ParamFieldSchema>,
    /// Names of fields that MUST be present.
    pub required: Vec<String>,
}

impl ToolParameterSchema {
    /// Construct an empty object schema (no parameters).
    pub fn empty() -> Self {
        Self {
            ty: "object".to_string(),
            properties: HashMap::new(),
            required: Vec::new(),
        }
    }
}

/// JSON Schema for one parameter field. The `@tool` macro renders
/// Mighty types into this shape:
///
/// - `String`        → `{ "type": "string" }`
/// - `I32`/`U64`/…   → `{ "type": "integer" }`
/// - `F32`/`F64`     → `{ "type": "number" }`
/// - `Bool`          → `{ "type": "boolean" }`
/// - `Vec[T]`        → `{ "type": "array", "items": <T-schema> }`
/// - `Option[T]`     → `<T-schema>` with the field omitted from `required`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParamFieldSchema {
    #[serde(rename = "type")]
    pub ty: String,
    /// Human description (carried forward from the Mighty doc-comment
    /// when present). Optional.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// For `array` shapes: the inner-element schema (boxed to keep the
    /// outer struct sized).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<ParamFieldSchema>>,
}

impl ParamFieldSchema {
    /// Construct a primitive (non-array) field schema with no
    /// description.
    pub fn primitive(ty: &str) -> Self {
        Self {
            ty: ty.to_string(),
            description: None,
            items: None,
        }
    }

    /// Construct an array field schema with the given element type.
    pub fn array_of(item: ParamFieldSchema) -> Self {
        Self {
            ty: "array".to_string(),
            description: None,
            items: Some(Box::new(item)),
        }
    }
}

/// One tool's metadata as it travels over the MCP wire and is handed
/// to LLM providers. The `@tool` macro generates a
/// `__tool_descriptor_<NAME>()` fn that returns this value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolDescriptor {
    /// Tool name as the LLM sees it. Matches the Mighty fn name.
    pub name: String,
    /// Human description, the first arg of `@tool(...)`. Shown to the
    /// LLM as the tool's purpose.
    pub description: String,
    /// JSON Schema for the parameters object.
    #[serde(rename = "inputSchema")]
    pub input_schema: ToolParameterSchema,
    /// Capability hint surfaced in the descriptor (informational —
    /// real enforcement is in [`sandbox`]). The `@tool` macro encodes
    /// the cap as a dotted path (`fs.read`, `net.get`, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability: Option<String>,
}

/// One tool's runtime entry: descriptor + invoke closure. The invoke
/// closure receives a `serde_json::Value` (the args object) and
/// returns a `Result<Value, ToolError>`. The closure MUST run the cap
/// check before touching any host resource — the `@tool` macro
/// generates this wrapper automatically.
pub struct RegisteredTool {
    pub descriptor: ToolDescriptor,
    pub invoke: ToolInvokeFn,
}

/// Type alias for the invoke closure stored in the registry.
pub type ToolInvokeFn = std::sync::Arc<
    dyn Fn(serde_json::Value, &CapabilitySet) -> Result<serde_json::Value, ToolError> + Send + Sync,
>;

impl std::fmt::Debug for RegisteredTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisteredTool")
            .field("descriptor", &self.descriptor)
            .field("invoke", &"<fn>")
            .finish()
    }
}

/// Errors a tool invocation can return. The MCP wire encodes these as
/// JSON-RPC error objects so the LLM client sees them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolError {
    /// The tool name is not in the registry.
    #[error("unknown tool: {name}")]
    UnknownTool { name: String },
    /// Arguments don't match the declared input schema (missing
    /// required field, wrong type, …).
    #[error("invalid arguments for `{tool}`: {detail}")]
    InvalidArguments { tool: String, detail: String },
    /// The active capability set does not grant what the tool
    /// declared, OR the requested resource is outside the granted
    /// scope. This is the LOAD-BEARING guarantee — the LLM cannot
    /// escalate by prompting; this fires before the tool body runs.
    #[error("capability denied for `{tool}`: tool needs `{required}`, but {reason}")]
    CapabilityDenied {
        tool: String,
        required: String,
        reason: String,
    },
    /// The tool body itself returned an error. The string is the
    /// host's rendered error.
    #[error("tool `{tool}` failed: {detail}")]
    ToolFailed { tool: String, detail: String },
    /// Transport-layer errors (malformed JSON, disconnected stdio, …).
    #[error("mcp protocol error: {detail}")]
    Protocol { detail: String },
}

impl ToolError {
    /// JSON-RPC error code for this variant. We use the MCP-spec range
    /// (-32000..-32099 for server errors) plus the standard -32600
    /// invalid-request / -32601 method-not-found codes.
    pub fn rpc_code(&self) -> i32 {
        match self {
            ToolError::UnknownTool { .. } => -32601,
            ToolError::InvalidArguments { .. } => -32602,
            ToolError::CapabilityDenied { .. } => -32001,
            ToolError::ToolFailed { .. } => -32000,
            ToolError::Protocol { .. } => -32700,
        }
    }
}

/// Generic MCP-layer error type (used by transports, server,
/// client). Wraps [`ToolError`] plus io.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("tool: {0}")]
    Tool(#[from] ToolError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("transport: {0}")]
    Transport(String),
}

// ---------------------------------------------------------------------------
// JSON-RPC 2.0 wire types
// ---------------------------------------------------------------------------

/// A JSON-RPC 2.0 request. The MCP spec layers method names like
/// `tools/list`, `tools/call`, `initialize` on top.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    /// `null` for notifications.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    pub fn new(id: i64, method: &str, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: Some(serde_json::Value::from(id)),
            method: method.to_string(),
            params: Some(params),
        }
    }
}

/// A JSON-RPC 2.0 response. Exactly one of `result` / `error` is
/// populated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn failure(id: serde_json::Value, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcError {
    pub fn from_tool_error(err: &ToolError) -> Self {
        Self {
            code: err.rpc_code(),
            message: err.to_string(),
            data: serde_json::to_value(err).ok(),
        }
    }
}

// ---------------------------------------------------------------------------
// Process-wide tool registry
// ---------------------------------------------------------------------------

static TOOL_REGISTRY: OnceLock<Mutex<HashMap<String, RegisteredTool>>> = OnceLock::new();

fn registry_slot() -> &'static Mutex<HashMap<String, RegisteredTool>> {
    TOOL_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a tool in the process-wide registry. Called either by the
/// `@tool` macro's auto-generated `__tool_register_<NAME>()` fn at
/// module-init time, or by downstream code that wants to register
/// imperatively (e.g. tests).
///
/// If the same name is registered twice, the second registration
/// overwrites the first. Tools are keyed by their `descriptor.name`
/// — collisions are the caller's responsibility to avoid.
pub fn register_tool(tool: RegisteredTool) {
    let mut map = registry_slot().lock().expect("TOOL_REGISTRY poisoned");
    map.insert(tool.descriptor.name.clone(), tool);
}

/// Snapshot the names of every tool currently in the registry.
pub fn registered_tool_names() -> Vec<String> {
    let map = registry_slot().lock().expect("TOOL_REGISTRY poisoned");
    let mut names: Vec<String> = map.keys().cloned().collect();
    names.sort();
    names
}

/// Snapshot the descriptors of every registered tool. Sorted by name
/// for determinism (so the `tools/list` MCP response is stable).
pub fn registered_descriptors() -> Vec<ToolDescriptor> {
    let map = registry_slot().lock().expect("TOOL_REGISTRY poisoned");
    let mut descriptors: Vec<ToolDescriptor> = map.values().map(|t| t.descriptor.clone()).collect();
    descriptors.sort_by(|a, b| a.name.cmp(&b.name));
    descriptors
}

/// Invoke a registered tool by name, passing a cap-set to enforce the
/// declared capability. Returns the tool's JSON result, or a
/// [`ToolError`] if the name is unknown or the cap check fails.
pub fn invoke_tool(
    name: &str,
    args: serde_json::Value,
    caps: &CapabilitySet,
) -> Result<serde_json::Value, ToolError> {
    let invoke = {
        let map = registry_slot().lock().expect("TOOL_REGISTRY poisoned");
        match map.get(name) {
            Some(tool) => tool.invoke.clone(),
            None => {
                return Err(ToolError::UnknownTool {
                    name: name.to_string(),
                })
            }
        }
    };
    invoke(args, caps)
}

/// Drop every tool from the registry. Test-only — production code
/// should NEVER call this. Tests that need isolated registries must
/// also serialize on a global mutex (the registry is process-wide).
pub fn clear_registry_for_tests() {
    let mut map = registry_slot().lock().expect("TOOL_REGISTRY poisoned");
    map.clear();
}

/// Register a tool from the JSON text the `@tool` macro emits.
///
/// The macro-generated `__tool_register_<NAME>()` Mighty fn calls
/// `std.mcp.register_tool_from_json(...)`; the runtime dispatcher
/// routes that into here. The invoke closure stored alongside the
/// descriptor is a thin placeholder — real native marshalling lives
/// in code-generated Rust glue, scheduled for the v0.27 codegen
/// integration. v0.26 verifies the schema + descriptor round-trip
/// only.
pub fn register_tool_from_json(descriptor_json: &str) -> Result<(), McpError> {
    let descriptor: ToolDescriptor = serde_json::from_str(descriptor_json)?;
    let name = descriptor.name.clone();
    let placeholder_name = name.clone();
    let invoke: ToolInvokeFn = std::sync::Arc::new(move |_args, _caps| {
        Err(ToolError::ToolFailed {
            tool: placeholder_name.clone(),
            detail: "v0.26 placeholder invoke — register a real impl via register_tool".to_string(),
        })
    });
    register_tool(RegisteredTool { descriptor, invoke });
    let _ = name; // suppress unused
    Ok(())
}

/// Capability-checking entry point for tool implementations. Returns
/// `Ok(())` if the active cap-set grants `required` for `resource`,
/// otherwise the standard [`ToolError::CapabilityDenied`]. The
/// `@tool` macro's invoke wrapper calls this BEFORE the user fn
/// body — that's the load-bearing guarantee that the LLM cannot
/// escalate by prompting.
pub fn require_capability(
    tool: &str,
    required: &str,
    resource: &str,
    caps: &CapabilitySet,
) -> Result<(), ToolError> {
    match caps.check(required, resource) {
        Ok(()) => Ok(()),
        Err(reason) => Err(ToolError::CapabilityDenied {
            tool: tool.to_string(),
            required: required.to_string(),
            reason,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonrpc_version_constant_is_2_0() {
        assert_eq!(JSONRPC_VERSION, "2.0");
    }

    #[test]
    fn tool_error_rpc_codes_match_spec() {
        assert_eq!(
            ToolError::UnknownTool { name: "x".into() }.rpc_code(),
            -32601
        );
        assert_eq!(
            ToolError::CapabilityDenied {
                tool: "t".into(),
                required: "fs.read".into(),
                reason: "missing".into(),
            }
            .rpc_code(),
            -32001
        );
    }

    #[test]
    fn parameter_schema_serialises_to_input_schema_field() {
        let mut props = HashMap::new();
        props.insert("path".to_string(), ParamFieldSchema::primitive("string"));
        let schema = ToolParameterSchema {
            ty: "object".into(),
            properties: props,
            required: vec!["path".into()],
        };
        let desc = ToolDescriptor {
            name: "read_file".into(),
            description: "Read a file".into(),
            input_schema: schema,
            capability: Some("fs.read".into()),
        };
        let s = serde_json::to_string(&desc).expect("serialise");
        assert!(s.contains("\"inputSchema\""), "got: {s}");
        assert!(s.contains("\"capability\""), "got: {s}");
        assert!(s.contains("\"path\""), "got: {s}");
    }
}
