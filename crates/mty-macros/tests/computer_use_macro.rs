//! Integration tests for the `@computer_use` attribute macro.
//!
//! v0.30 Track C. Mirrors `tool_macro.rs` in shape: verifies the
//! attribute parser + agent-shaped synth-fn emitter against a small
//! corpus of representative inputs.

use mty_macros::{
    expand_builtin_agent_attribute, expand_computer_use_attribute,
    parse_computer_use_attribute_args, render_spec_json, ComputerUseAttributeArgs,
    ComputerUseMacroError, ParsedAgent,
};

fn agent(name: &str, is_agent: bool) -> ParsedAgent {
    ParsedAgent {
        name: name.into(),
        source: format!("agent {name} {{ on Run(t: Str) -> Str {{ t }} }}"),
        is_agent,
    }
}

#[test]
fn expand_via_agent_dispatch_helper_succeeds() {
    let out = expand_builtin_agent_attribute(
        "computer_use",
        &[
            "width: 1280",
            "height: 800",
            "cap: computer.screen + computer.input",
        ],
        &agent("BrowserOperator", true),
    )
    .expect("computer_use is a recognised agent attr")
    .expect("expansion succeeds");
    assert!(out.spec_json.contains("\"agent\":\"BrowserOperator\""));
    assert!(out.spec_json.contains("\"width\":1280"));
    assert!(out.spec_json.contains("\"height\":800"));
    assert!(out
        .spec_json
        .contains("\"capability\":\"computer.screen+computer.input\""));
}

#[test]
fn agent_dispatch_returns_none_for_unknown_attr() {
    let out = expand_builtin_agent_attribute("frobnicate", &[], &agent("X", true));
    assert!(out.is_none());
}

#[test]
fn agent_dispatch_returns_none_for_tool_attr_via_agent_path() {
    // `tool` is not an agent-shaped attribute; the agent dispatch
    // helper should not pick it up. (The fn-shaped helper does.)
    let out = expand_builtin_agent_attribute("tool", &[], &agent("X", true));
    assert!(out.is_none());
}

#[test]
fn missing_cap_returns_mt6017() {
    let err = expand_computer_use_attribute(&["width: 1280"], &agent("X", true)).unwrap_err();
    assert!(matches!(err, ComputerUseMacroError::MissingCap));
    assert_eq!(err.code(), 6017);
}

#[test]
fn malformed_cap_returns_mt6018() {
    let err = parse_computer_use_attribute_args(&["cap: 123bad"]).unwrap_err();
    assert!(matches!(err, ComputerUseMacroError::MalformedCap { .. }));
    assert_eq!(err.code(), 6018);
}

#[test]
fn zero_width_returns_mt6019() {
    let err = parse_computer_use_attribute_args(&["width: 0", "cap: computer.screen"]).unwrap_err();
    assert_eq!(err.code(), 6019);
}

#[test]
fn non_agent_returns_mt6020() {
    let err =
        expand_computer_use_attribute(&["cap: computer.screen"], &agent("X", false)).unwrap_err();
    assert!(matches!(err, ComputerUseMacroError::NotAnAgent { .. }));
    assert_eq!(err.code(), 6020);
}

#[test]
fn spec_json_round_trips_via_serde() {
    let args = ComputerUseAttributeArgs {
        width: 640,
        height: 480,
        capability: "computer.screen".into(),
        model: Some("claude-opus-4-7".into()),
    };
    let s = render_spec_json(&args, "Foo");
    // The macro emits hand-rolled JSON; verify it parses as valid
    // JSON via serde_json so any escape regression is caught here.
    let v: serde_json::Value = serde_json::from_str(&s).expect("valid JSON");
    assert_eq!(v["agent"], "Foo");
    assert_eq!(v["width"], 640);
    assert_eq!(v["height"], 480);
    assert_eq!(v["capability"], "computer.screen");
    assert_eq!(v["model"], "claude-opus-4-7");
}
