//! Integration tests for the `format!` builtin macro (v0.24 Track B).
//!
//! These tests exercise the parser + expander pair directly rather
//! than driving the full HIR preprocessor — the HIR-level wiring is
//! covered by `crates/mty-hir/src/lower/macros.rs`'s in-module tests
//! and the `tests/conformance/macros/format_basic` fixture (run by
//! `conformance_full`).
//!
//! ## Coverage summary (per v0.24 plan)
//!
//! 1. `format_empty_string`              — `format!("")` → `""`
//! 2. `format_single_arg_default`        — `format!("hi {}", n)` uses `to_str`
//! 3. `format_multiple_args`             — `format!("{} + {} = {}", 1, 2, 3)`
//! 4. `format_hex_lowercase`             — `format!("{:x}", 255)` uses `to_hex_str`
//! 5. `format_hex_uppercase`             — `format!("{:X}", 255)` uses `to_hex_upper_str`
//! 6. `format_named_arg_passthrough`     — `format!("score: {score}")`
//! 7. `format_literal_braces`            — `format!("{{}}")` → `{}`
//! 8. `format_unknown_spec_clear_error`  — `format!("{:05}", n)` → UnsupportedSpec
//! 9. `format_arity_mismatch_diagnostic` — `format!("{} {}", 1)` → NotEnoughArgs
//! 10. `format_first_arg_must_be_literal` — `format!(x, 1)` rejected
//! 11. `format_named_arg_with_hex_spec`  — `format!("{n:x}")`
//! 12. `format_debug_spec`               — `format!("{:?}", v)`
//! 13. `format_bare_x_is_named_not_hex`  — `format!("{x}")` is named-arg
//! 14. `format_template_runtime_concat_smoke` — sanity check the snippet shape

use mty_macros::stdlib::format::{
    arg_is_string_literal, decode_string_literal, expand_format_call, parse_template, ConvKind,
    FormatExpandError, FormatPiece,
};

// --- 1. empty template ------------------------------------------------------

#[test]
fn format_empty_string() {
    let out = expand_format_call(&["\"\""]).expect("empty template expands");
    assert_eq!(out, "\"\"");
}

// --- 2. single positional ---------------------------------------------------

#[test]
fn format_single_arg_default() {
    let out = expand_format_call(&["\"hi {}\"", "n"]).expect("single-arg expands");
    // The expansion folds `"hi "` and `(n).to_str()` into a `+` chain
    // anchored by an empty string head.
    assert!(out.contains("\"hi \""), "literal missing: {out}");
    assert!(out.contains("(n).to_str()"), "to_str call missing: {out}");
    assert!(out.contains('+'), "concat operator missing: {out}");
}

// --- 3. multiple positional args -------------------------------------------

#[test]
fn format_multiple_args() {
    let out = expand_format_call(&["\"{} + {} = {}\"", "1", "2", "3"]).expect("3-arg expands");
    assert!(out.contains("(1).to_str()"), "got: {out}");
    assert!(out.contains("(2).to_str()"), "got: {out}");
    assert!(out.contains("(3).to_str()"), "got: {out}");
    assert!(out.contains("\" + \""), "got: {out}");
    assert!(out.contains("\" = \""), "got: {out}");
}

// --- 4. hex lowercase via spec ----------------------------------------------

#[test]
fn format_hex_lowercase() {
    let out = expand_format_call(&["\"{:x}\"", "255"]).expect("hex expands");
    assert!(out.contains("(255).to_hex_str()"), "got: {out}");
}

// --- 5. hex uppercase via spec ----------------------------------------------

#[test]
fn format_hex_uppercase() {
    let out = expand_format_call(&["\"{:X}\"", "255"]).expect("HEX expands");
    assert!(out.contains("(255).to_hex_upper_str()"), "got: {out}");
}

// --- 6. named-arg passthrough -----------------------------------------------

#[test]
fn format_named_arg_passthrough() {
    let out = expand_format_call(&["\"score: {score}\""]).expect("named expands");
    assert!(
        out.contains("(score).to_str()"),
        "named-arg call missing: {out}"
    );
    assert!(out.contains("\"score: \""), "literal missing: {out}");
}

// --- 7. literal braces ------------------------------------------------------

#[test]
fn format_literal_braces() {
    // `format!("{{}}")` should yield the literal string `{}`.
    let out = expand_format_call(&["\"{{}}\""]).expect("literal braces expand");
    // The expansion contains a Mighty string literal `"{}"`.
    assert!(out.contains("\"{}\""), "literal braces missing: {out}");

    // Direct parse check too.
    let pieces = parse_template("{{}}").unwrap();
    assert_eq!(pieces, vec![FormatPiece::Literal("{}".into())]);
}

// --- 8. unsupported spec: clear error ---------------------------------------

#[test]
fn format_unknown_spec_clear_error() {
    let e = expand_format_call(&["\"{:05}\"", "n"]).unwrap_err();
    match e {
        FormatExpandError::UnsupportedSpec { ref spec, .. } => {
            assert_eq!(spec, "05", "spec text preserved");
        }
        other => panic!("expected UnsupportedSpec, got {other:?}"),
    }
    // Message is human-readable.
    let msg = e.to_string();
    assert!(msg.contains("v0.24"), "msg: {msg}");
    assert!(
        msg.contains("not supported") || msg.contains("v0.25"),
        "msg: {msg}"
    );
}

// --- 9. arity mismatch ------------------------------------------------------

#[test]
fn format_arity_mismatch_diagnostic() {
    let e = expand_format_call(&["\"{} {}\"", "1"]).unwrap_err();
    assert!(
        matches!(
            e,
            FormatExpandError::NotEnoughArgs {
                expected: 2,
                given: 1
            }
        ),
        "got: {e:?}"
    );
    let msg = e.to_string();
    assert!(msg.contains('2'), "msg: {msg}");
    assert!(msg.contains('1'), "msg: {msg}");
}

#[test]
fn format_arity_mismatch_too_many() {
    let e = expand_format_call(&["\"{}\"", "1", "2"]).unwrap_err();
    assert!(matches!(
        e,
        FormatExpandError::TooManyArgs {
            expected: 1,
            given: 2
        }
    ));
}

// --- 10. first arg must be a string literal ---------------------------------

#[test]
fn format_first_arg_must_be_literal() {
    // Bare identifier, not a string literal.
    let e = expand_format_call(&["my_template", "x"]).unwrap_err();
    assert!(matches!(e, FormatExpandError::NotAStringLiteral));

    // Concatenation looks string-shaped but isn't a single literal.
    let e2 = expand_format_call(&["\"a\" + \"b\"", "x"]).unwrap_err();
    assert!(matches!(e2, FormatExpandError::NotAStringLiteral));
}

// --- 11. named arg with hex spec --------------------------------------------

#[test]
fn format_named_arg_with_hex_spec() {
    let out = expand_format_call(&["\"{n:x}\""]).expect("named-arg with spec expands");
    assert!(out.contains("(n).to_hex_str()"), "got: {out}");
}

#[test]
fn format_named_arg_with_upper_hex_spec() {
    let out = expand_format_call(&["\"color={c:X}\""]).expect("named:X expands");
    assert!(out.contains("(c).to_hex_upper_str()"), "got: {out}");
    assert!(out.contains("\"color=\""), "got: {out}");
}

// --- 12. debug spec ---------------------------------------------------------

#[test]
fn format_debug_spec() {
    let out = expand_format_call(&["\"v={:?}\"", "val"]).expect("debug expands");
    assert!(out.contains("(val).to_debug_str()"), "got: {out}");
}

// --- 13. bare {x} is named-arg, NOT positional hex --------------------------

#[test]
fn format_bare_x_is_named_not_hex() {
    // Rust convention: `{x}` is a named-arg passthrough referring to
    // the in-scope identifier `x`. The hex conversion sigil is `:x`.
    let out = expand_format_call(&["\"{x}\""]).expect("bare {x} expands");
    assert!(out.contains("(x).to_str()"), "got: {out}");
    assert!(
        !out.contains("to_hex_str"),
        "must not interpret as hex: {out}"
    );
}

// --- 14. snippet shape sanity ----------------------------------------------

#[test]
fn format_template_runtime_concat_smoke() {
    // The cell-coordinates example from the v0.24 spec.
    let out =
        expand_format_call(&["\"cell {x},{y} = {color:x}\""]).expect("named-only template expands");
    assert!(out.contains("(x).to_str()"), "got: {out}");
    assert!(out.contains("(y).to_str()"), "got: {out}");
    assert!(out.contains("(color).to_hex_str()"), "got: {out}");
    assert!(out.contains("\"cell \""), "got: {out}");
    assert!(out.contains("\",\""), "got: {out}");
    assert!(out.contains("\" = \""), "got: {out}");
}

// --- bonus: extra coverage -------------------------------------------------

#[test]
fn parse_template_handles_newline_literals() {
    let pieces = parse_template("line\nbreak").unwrap();
    assert_eq!(pieces, vec![FormatPiece::Literal("line\nbreak".into())]);
}

#[test]
fn unclosed_brace_position_reported() {
    let e = parse_template("hi {").unwrap_err();
    match e {
        FormatExpandError::UnclosedBrace { position } => {
            // `{` is at byte index 3 (after "hi ").
            assert_eq!(position, 3);
        }
        other => panic!("expected UnclosedBrace, got {other:?}"),
    }
}

#[test]
fn decode_string_literal_handles_escapes() {
    assert_eq!(
        decode_string_literal("\"\\n\\t\\\"\"").as_deref(),
        Some("\n\t\"")
    );
}

#[test]
fn empty_call_errors_clearly() {
    let e = expand_format_call(&[]).unwrap_err();
    assert!(matches!(e, FormatExpandError::EmptyArgList));
    let msg = e.to_string();
    assert!(msg.contains("template"), "msg: {msg}");
}

#[test]
fn conv_kind_method_names_stable() {
    // Locking in the method-name contract so the runtime side
    // (mty-stdlib::fmt + mty-ir interp) and the parser stay in sync.
    assert_eq!(ConvKind::Display.method(), "to_str");
    assert_eq!(ConvKind::HexLower.method(), "to_hex_str");
    assert_eq!(ConvKind::HexUpper.method(), "to_hex_upper_str");
    assert_eq!(ConvKind::Debug.method(), "to_debug_str");
}

#[test]
fn arg_is_string_literal_helpers() {
    assert!(arg_is_string_literal("\"plain\""));
    assert!(arg_is_string_literal("  \"padded\"  "));
    assert!(!arg_is_string_literal("42"));
    assert!(!arg_is_string_literal("ident"));
    assert!(!arg_is_string_literal("\"a\" + \"b\""));
}
