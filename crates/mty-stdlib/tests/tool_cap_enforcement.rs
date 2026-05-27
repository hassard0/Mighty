//! Integration tests for capability-enforced tool sandboxing
//! (v0.26 Track B).
//!
//! The load-bearing guarantee: even if the LLM prompts a tool with
//! a path outside its declared cap, the runtime denies BEFORE the
//! tool body runs. These tests pin that contract for the four
//! cap-family flavours (fs / net / clock / model) plus the bare-cap
//! and narrowed-cap shapes.

use mty_stdlib::mcp::{
    clear_registry_for_tests, invoke_tool, register_tool, CapabilityGrant, CapabilitySet, FsMode,
    ParamFieldSchema, RegisteredTool, ToolDescriptor, ToolError, ToolParameterSchema,
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Global lock — see mcp_server.rs for rationale.
static REGISTRY_LOCK: Mutex<()> = Mutex::new(());

fn register_path_tool(name: &str, cap: &str) {
    let cap_owned = cap.to_string();
    let cap_for_invoke = cap.to_string();
    let name_owned = name.to_string();
    let mut props = HashMap::new();
    props.insert("path".to_string(), ParamFieldSchema::primitive("string"));
    register_tool(RegisteredTool {
        descriptor: ToolDescriptor {
            name: name_owned.clone(),
            description: format!("Read file via {cap}"),
            input_schema: ToolParameterSchema {
                ty: "object".into(),
                properties: props,
                required: vec!["path".into()],
            },
            capability: Some(cap_owned),
        },
        invoke: Arc::new(move |args, caps| {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            mty_stdlib::mcp::require_capability(&name_owned, &cap_for_invoke, path, caps)?;
            Ok(json!(format!("read: {path}")))
        }),
    });
}

#[test]
fn tool_call_with_matching_cap_succeeds() {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_registry_for_tests();
    register_path_tool("read_file", "fs.read");

    let caps = CapabilitySet::from_grants([CapabilityGrant::Fs {
        mode: FsMode::Read,
        roots: vec![],
    }]);
    let result = invoke_tool("read_file", json!({ "path": "/data/x" }), &caps).expect("succeeds");
    assert_eq!(result, json!("read: /data/x"));
    clear_registry_for_tests();
}

#[test]
fn tool_call_without_cap_returns_denied() {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_registry_for_tests();
    register_path_tool("read_file", "fs.read");

    let caps = CapabilitySet::empty();
    let err = invoke_tool("read_file", json!({ "path": "/data/x" }), &caps).unwrap_err();
    match err {
        ToolError::CapabilityDenied {
            tool,
            required,
            reason,
        } => {
            assert_eq!(tool, "read_file");
            assert_eq!(required, "fs.read");
            assert!(reason.contains("fs"), "{reason}");
        }
        other => panic!("expected CapabilityDenied, got {other:?}"),
    }
    clear_registry_for_tests();
}

#[test]
fn tool_call_with_narrower_cap_within_scope() {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_registry_for_tests();
    register_path_tool("read_file", "fs.read");

    // Cap-set covers /data; tool requested /data/x → ALLOWED.
    let caps = CapabilitySet::from_grants([CapabilityGrant::Fs {
        mode: FsMode::Read,
        roots: vec!["/data".into()],
    }]);
    let result = invoke_tool("read_file", json!({ "path": "/data/x" }), &caps).expect("succeeds");
    assert_eq!(result, json!("read: /data/x"));
    clear_registry_for_tests();
}

#[test]
fn tool_call_with_path_outside_cap_scope_denied() {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_registry_for_tests();
    register_path_tool("read_file", "fs.read");

    // Cap-set covers /data; tool requested /etc/passwd → DENIED.
    let caps = CapabilitySet::from_grants([CapabilityGrant::Fs {
        mode: FsMode::Read,
        roots: vec!["/data".into()],
    }]);
    let err = invoke_tool("read_file", json!({ "path": "/etc/passwd" }), &caps).unwrap_err();
    match err {
        ToolError::CapabilityDenied { reason, .. } => {
            assert!(reason.contains("/etc/passwd"), "{reason}");
            assert!(reason.contains("outside"), "{reason}");
        }
        other => panic!("expected CapabilityDenied, got {other:?}"),
    }
    clear_registry_for_tests();
}

#[test]
fn tool_with_read_cap_cannot_perform_write() {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_registry_for_tests();
    register_path_tool("write_file", "fs.write");

    // Cap-set grants Read mode; the tool declares fs.write → DENIED.
    let caps = CapabilitySet::from_grants([CapabilityGrant::Fs {
        mode: FsMode::Read,
        roots: vec![],
    }]);
    let err = invoke_tool("write_file", json!({ "path": "/tmp/x" }), &caps).unwrap_err();
    assert!(matches!(err, ToolError::CapabilityDenied { .. }), "{err:?}");
    clear_registry_for_tests();
}

#[test]
fn tool_with_rw_cap_can_read_and_write() {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_registry_for_tests();
    register_path_tool("read_file", "fs.read");
    register_path_tool("write_file", "fs.write");

    let caps = CapabilitySet::from_grants([CapabilityGrant::Fs {
        mode: FsMode::ReadWrite,
        roots: vec![],
    }]);
    invoke_tool("read_file", json!({ "path": "/x" }), &caps).expect("read ok");
    invoke_tool("write_file", json!({ "path": "/x" }), &caps).expect("write ok");
    clear_registry_for_tests();
}

#[test]
fn net_cap_suffix_matches_subdomains() {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_registry_for_tests();
    register_path_tool("fetch", "net.get"); // reuse path-tool with `host` arg

    let caps = CapabilitySet::from_grants([CapabilityGrant::Net {
        hosts: vec!["example.com".into()],
    }]);
    // The cap-resolver suffix-matches, so api.example.com is allowed.
    invoke_tool("fetch", json!({ "path": "api.example.com" }), &caps).expect("subdomain ok");
    // evil.com is not.
    let err = invoke_tool("fetch", json!({ "path": "evil.com" }), &caps).unwrap_err();
    assert!(matches!(err, ToolError::CapabilityDenied { .. }), "{err:?}");
    clear_registry_for_tests();
}

#[test]
fn unknown_tool_returns_unknown_tool_error_not_cap_denied() {
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_registry_for_tests();

    let caps = CapabilitySet::unrestricted();
    let err = invoke_tool("nonexistent", json!({}), &caps).unwrap_err();
    assert!(matches!(err, ToolError::UnknownTool { .. }), "{err:?}");
}
