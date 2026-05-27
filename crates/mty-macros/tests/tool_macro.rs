//! Integration tests for the `@tool` attribute macro (v0.26 Track B).
//!
//! Companion suite to the in-module tests inside
//! `crates/mty-macros/src/stdlib/tool.rs`. The lib-internal tests
//! cover the granular parse/render helpers; this file exercises the
//! end-to-end expansion contract that the HIR preprocessor will rely
//! on once attribute-macro plumbing lands in v0.27.

use mty_macros::{
    expand_builtin_attribute, expand_tool_attribute, is_builtin_attribute, BUILTIN_ATTRIBUTE_NAMES,
};
use mty_macros::{ParsedFn, ParsedParam, ToolMacroError};

fn read_file_fn() -> ParsedFn {
    ParsedFn {
        name: "read_file".into(),
        params: vec![ParsedParam {
            name: "path".into(),
            mty_type: "String".into(),
        }],
        ret_ty: "Result[String, FsError]".into(),
        has_generics: false,
        source: "fn read_file(path: String) -> Result[String, FsError] !{fs} { std.fs.read_to_string(path) }".into(),
    }
}

fn many_param_fn() -> ParsedFn {
    ParsedFn {
        name: "search_web".into(),
        params: vec![
            ParsedParam {
                name: "query".into(),
                mty_type: "String".into(),
            },
            ParsedParam {
                name: "max_results".into(),
                mty_type: "Option[I32]".into(),
            },
            ParsedParam {
                name: "tags".into(),
                mty_type: "Vec[String]".into(),
            },
        ],
        ret_ty: "Result[Vec[String], NetError]".into(),
        has_generics: false,
        source: "fn search_web(...) -> ... { ... }".into(),
    }
}

#[test]
fn tool_attribute_generates_descriptor() {
    let exp = expand_tool_attribute(
        &["\"Read a file from disk\"", "cap: fs.read"],
        &read_file_fn(),
    )
    .expect("expansion ok");
    // The synthesised fns include the descriptor fn.
    let joined = exp.synthesised_decls.join("\n");
    assert!(
        joined.contains("fn __tool_descriptor_read_file()"),
        "synth: {joined}"
    );
}

#[test]
fn tool_attribute_generates_invoke() {
    let exp = expand_tool_attribute(
        &["\"Read a file from disk\"", "cap: fs.read"],
        &read_file_fn(),
    )
    .expect("expansion ok");
    let joined = exp.synthesised_decls.join("\n");
    // v0.27 Track A: the invoke fn now takes `Str` (the lexer's literal
    // string type) rather than `String` (the heap-buffer ADT) so the
    // synth fn type-checks without auto-coercion.
    assert!(
        joined.contains("fn __tool_invoke_read_file(__args: Str)"),
        "synth: {joined}"
    );
}

#[test]
fn tool_descriptor_includes_description() {
    let exp = expand_tool_attribute(&["\"Read a file from disk\""], &read_file_fn())
        .expect("expansion ok");
    assert!(
        exp.descriptor_json.contains("\"Read a file from disk\""),
        "descriptor: {}",
        exp.descriptor_json
    );
}

#[test]
fn tool_descriptor_includes_cap() {
    let exp = expand_tool_attribute(
        &["\"Read a file from disk\"", "cap: fs.read"],
        &read_file_fn(),
    )
    .expect("expansion ok");
    assert!(
        exp.descriptor_json.contains("\"capability\":\"fs.read\""),
        "descriptor: {}",
        exp.descriptor_json
    );
}

#[test]
fn tool_arity_mismatch_errors() {
    // `@tool()` with zero args → MT6012.
    let err = expand_tool_attribute(&[], &read_file_fn()).unwrap_err();
    assert!(
        matches!(err, ToolMacroError::MissingDescription),
        "got {err:?}"
    );
}

#[test]
fn tool_descriptor_input_schema_required_field_skips_optionals() {
    let exp = expand_tool_attribute(&["\"Search the web\"", "cap: net.get"], &many_param_fn())
        .expect("expansion ok");
    // `query` and `tags` are required (no Option wrapper); `max_results` is not.
    assert!(
        exp.descriptor_json
            .contains("\"required\":[\"query\",\"tags\"]"),
        "descriptor: {}",
        exp.descriptor_json
    );
}

#[test]
fn tool_descriptor_array_type_includes_items_schema() {
    let exp = expand_tool_attribute(&["\"Search the web\"", "cap: net.get"], &many_param_fn())
        .expect("expansion ok");
    // tags: Vec[String] → {"type":"array","items":{"type":"string"}}
    assert!(
        exp.descriptor_json
            .contains("\"items\":{\"type\":\"string\"}"),
        "descriptor: {}",
        exp.descriptor_json
    );
}

#[test]
fn tool_original_decl_returned_unchanged() {
    let f = read_file_fn();
    let exp = expand_tool_attribute(&["\"Read a file\""], &f).expect("expansion ok");
    assert_eq!(exp.original_decl, f.source);
}

#[test]
fn is_builtin_attribute_recognises_tool() {
    assert!(is_builtin_attribute("tool"));
    assert!(!is_builtin_attribute("nope"));
    assert!(BUILTIN_ATTRIBUTE_NAMES.contains(&"tool"));
}

#[test]
fn expand_builtin_attribute_routes_to_tool() {
    let exp = expand_builtin_attribute("tool", &["\"hi\""], &read_file_fn())
        .expect("known attribute")
        .expect("expansion ok");
    assert!(exp.descriptor_json.contains("\"hi\""));
}

#[test]
fn expand_builtin_attribute_returns_none_for_unknown() {
    let f = read_file_fn();
    assert!(expand_builtin_attribute("frobnicate", &[], &f).is_none());
}

#[test]
fn tool_descriptor_name_matches_fn_name() {
    let exp = expand_tool_attribute(&["\"x\""], &read_file_fn()).unwrap();
    assert!(exp.descriptor_json.contains("\"name\":\"read_file\""));
}

#[test]
fn tool_register_fn_synthesised() {
    let exp = expand_tool_attribute(&["\"x\"", "cap: fs.read"], &read_file_fn()).unwrap();
    let joined = exp.synthesised_decls.join("\n");
    assert!(
        joined.contains("__tool_register_read_file"),
        "synth: {joined}"
    );
    // v0.27 Track A: the register fn now stages the descriptor JSON +
    // cap text but no longer calls `std.mcp.register_tool_from_json`
    // — that call site moves back in v0.28 once `std.mcp` is in the
    // auto-prelude. The descriptor + cap content stay in the body so
    // the v0.28 wiring just re-introduces a single call line.
    assert!(
        joined.contains("let __desc"),
        "synth missing __desc: {joined}"
    );
    assert!(
        joined.contains("\"fs.read\""),
        "synth missing fs.read cap text: {joined}"
    );
}
