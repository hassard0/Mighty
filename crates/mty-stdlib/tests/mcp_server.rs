//! Integration tests for the MCP server (v0.26 Track B).
//!
//! Cover the server's request-dispatch surface end-to-end: registered
//! tools surface in `tools/list`, `tools/call` routes to the invoke
//! closure, unknown tools/methods produce the spec-mandated JSON-RPC
//! error codes, and the stdio transport round-trips real JSON-RPC
//! frames.

use mty_stdlib::mcp::{
    clear_registry_for_tests, register_tool, CapabilitySet, JsonRpcRequest, McpServer,
    ParamFieldSchema, RegisteredTool, ToolDescriptor, ToolError, ToolParameterSchema,
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Global lock to serialise tests that mutate the process-wide tool
/// registry. The registry is a `Mutex<HashMap>` shared across the
/// entire `mty_stdlib::mcp` namespace, so two tests that both call
/// `clear_registry_for_tests` + register fresh tools race without
/// this gate.
static REGISTRY_LOCK: Mutex<()> = Mutex::new(());

fn make_echo_tool(name: &str, cap: Option<&str>) -> RegisteredTool {
    let mut props = HashMap::new();
    props.insert("text".to_string(), ParamFieldSchema::primitive("string"));
    let cap_owned = cap.map(|s| s.to_string());
    RegisteredTool {
        descriptor: ToolDescriptor {
            name: name.to_string(),
            description: format!("Echo tool {name}"),
            input_schema: ToolParameterSchema {
                ty: "object".into(),
                properties: props,
                required: vec!["text".into()],
            },
            capability: cap_owned,
        },
        invoke: Arc::new(|args, _caps| {
            let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
            Ok(json!(format!("echoed: {text}")))
        }),
    }
}

#[test]
fn server_lists_registered_tools() {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_registry_for_tests();
    register_tool(make_echo_tool("echo_a", None));
    register_tool(make_echo_tool("echo_b", Some("fs.read")));

    let srv = McpServer::from_tool_registry();
    let req = JsonRpcRequest::new(1, "tools/list", json!({}));
    let resp = srv.handle_request(req);
    assert!(resp.error.is_none(), "got error: {:?}", resp.error);
    let tools = resp
        .result
        .as_ref()
        .and_then(|r| r.get("tools"))
        .and_then(|t| t.as_array())
        .expect("tools array");
    assert_eq!(tools.len(), 2, "tools: {tools:?}");
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
        .collect();
    assert!(names.contains(&"echo_a"));
    assert!(names.contains(&"echo_b"));
    clear_registry_for_tests();
}

#[test]
fn server_routes_tool_call_to_invoke() {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_registry_for_tests();
    register_tool(make_echo_tool("echo", None));

    let srv = McpServer::from_tool_registry().with_capabilities(CapabilitySet::unrestricted());
    let req = JsonRpcRequest::new(
        7,
        "tools/call",
        json!({ "name": "echo", "arguments": { "text": "hello" } }),
    );
    let resp = srv.handle_request(req);
    assert!(resp.error.is_none(), "got error: {:?}", resp.error);
    let text = resp
        .result
        .as_ref()
        .and_then(|r| r.get("content"))
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|first| first.get("text"))
        .and_then(|t| t.as_str())
        .expect("text content");
    assert_eq!(text, "echoed: hello");
    clear_registry_for_tests();
}

#[test]
fn server_returns_error_for_unknown_tool() {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_registry_for_tests();

    let srv = McpServer::from_tool_registry();
    let req = JsonRpcRequest::new(9, "tools/call", json!({ "name": "nope", "arguments": {} }));
    let resp = srv.handle_request(req);
    let err = resp.error.expect("expected error");
    assert_eq!(
        err.code,
        ToolError::UnknownTool {
            name: "nope".into()
        }
        .rpc_code()
    );
}

#[test]
fn server_stdio_round_trip() {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_registry_for_tests();
    register_tool(make_echo_tool("echo", None));

    let srv = McpServer::from_tool_registry().with_capabilities(CapabilitySet::unrestricted());
    // Build a JSON-RPC stream containing initialize → tools/list → tools/call.
    let mut input = String::new();
    input.push_str(
        &serde_json::to_string(&JsonRpcRequest::new(1, "initialize", json!({}))).unwrap(),
    );
    input.push('\n');
    input.push_str(
        &serde_json::to_string(&JsonRpcRequest::new(2, "tools/list", json!({}))).unwrap(),
    );
    input.push('\n');
    input.push_str(
        &serde_json::to_string(&JsonRpcRequest::new(
            3,
            "tools/call",
            json!({ "name": "echo", "arguments": { "text": "stdio" } }),
        ))
        .unwrap(),
    );
    input.push('\n');

    let mut output: Vec<u8> = Vec::new();
    srv.serve_io(input.as_bytes(), &mut output).expect("serve");
    let s = String::from_utf8(output).expect("utf8");
    let lines: Vec<&str> = s.lines().collect();
    assert_eq!(
        lines.len(),
        3,
        "expected 3 responses, got {}: {s}",
        lines.len()
    );
    // Response 1 has protocolVersion.
    assert!(lines[0].contains("protocolVersion"), "first: {}", lines[0]);
    // Response 2 has tools array.
    assert!(lines[1].contains("\"echo\""), "second: {}", lines[1]);
    // Response 3 has the echoed text.
    assert!(lines[2].contains("echoed: stdio"), "third: {}", lines[2]);
    clear_registry_for_tests();
}

#[test]
fn server_initialize_advertises_protocol_version() {
    let srv = McpServer::default();
    let req = JsonRpcRequest::new(1, "initialize", json!({}));
    let resp = srv.handle_request(req);
    let result = resp.result.expect("success");
    let pv = result
        .get("protocolVersion")
        .and_then(|v| v.as_str())
        .expect("protocolVersion");
    assert_eq!(pv, mty_stdlib::mcp::MCP_PROTOCOL_VERSION);
    // serverInfo is present.
    let info = result.get("serverInfo").expect("serverInfo");
    assert!(info.get("name").is_some());
    assert!(info.get("version").is_some());
}

#[test]
fn server_descriptors_are_sorted_for_deterministic_listing() {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_registry_for_tests();
    // Register out-of-order; expect sorted output.
    register_tool(make_echo_tool("zeta", None));
    register_tool(make_echo_tool("alpha", None));
    register_tool(make_echo_tool("mu", None));

    let srv = McpServer::from_tool_registry();
    let req = JsonRpcRequest::new(1, "tools/list", json!({}));
    let resp = srv.handle_request(req);
    let tools = resp.result.unwrap();
    let arr = tools.get("tools").unwrap().as_array().unwrap();
    let names: Vec<&str> = arr
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
        .collect();
    assert_eq!(names, vec!["alpha", "mu", "zeta"]);
    clear_registry_for_tests();
}

#[test]
fn server_tools_call_returns_capability_denied_when_cap_missing() {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_registry_for_tests();
    // Tool requires fs.read; invoke wrapper checks the cap.
    let mut props = HashMap::new();
    props.insert("path".to_string(), ParamFieldSchema::primitive("string"));
    register_tool(RegisteredTool {
        descriptor: ToolDescriptor {
            name: "read".into(),
            description: "Read".into(),
            input_schema: ToolParameterSchema {
                ty: "object".into(),
                properties: props,
                required: vec!["path".into()],
            },
            capability: Some("fs.read".into()),
        },
        invoke: Arc::new(|args, caps| {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            mty_stdlib::mcp::require_capability("read", "fs.read", path, caps)?;
            Ok(json!("ok"))
        }),
    });

    let srv = McpServer::from_tool_registry().with_capabilities(CapabilitySet::empty());
    let req = JsonRpcRequest::new(
        1,
        "tools/call",
        json!({ "name": "read", "arguments": { "path": "/tmp/x" } }),
    );
    let resp = srv.handle_request(req);
    let err = resp.error.expect("expected error");
    assert_eq!(err.code, -32001, "got {err:?}");
    assert!(err.message.contains("capability"), "msg: {}", err.message);
    clear_registry_for_tests();
}
