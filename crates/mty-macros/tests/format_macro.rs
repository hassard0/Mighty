//! Integration tests for the `format!` builtin macro.
//!
//! v0.24 Track B shipped the conversion sigils and named-arg
//! passthrough. v0.25 Track D extends that with width/precision/align/
//! fill/sign/alternate-form layout flags. The two suites live
//! side-by-side here — the v0.24 baseline tests must keep passing,
//! and the v0.25 tests cover the new spec arms.

use mty_macros::stdlib::format::{
    arg_is_string_literal, decode_string_literal, expand_format_call, parse_template, Alignment,
    ConvKind, FormatExpandError, FormatPiece, FormatSpec,
};

// --- v0.24 baseline ---------------------------------------------------------

#[test]
fn format_empty_string() {
    let out = expand_format_call(&["\"\""]).expect("empty template expands");
    assert_eq!(out, "\"\"");
}

#[test]
fn format_single_arg_default() {
    let out = expand_format_call(&["\"hi {}\"", "n"]).expect("single-arg expands");
    assert!(out.contains("\"hi \""), "literal missing: {out}");
    assert!(out.contains("(n).to_str()"), "to_str call missing: {out}");
    assert!(out.contains('+'), "concat operator missing: {out}");
}

#[test]
fn format_multiple_args() {
    let out = expand_format_call(&["\"{} + {} = {}\"", "1", "2", "3"]).expect("3-arg expands");
    assert!(out.contains("(1).to_str()"), "got: {out}");
    assert!(out.contains("(2).to_str()"), "got: {out}");
    assert!(out.contains("(3).to_str()"), "got: {out}");
    assert!(out.contains("\" + \""), "got: {out}");
    assert!(out.contains("\" = \""), "got: {out}");
}

#[test]
fn format_hex_lowercase() {
    let out = expand_format_call(&["\"{:x}\"", "255"]).expect("hex expands");
    assert!(out.contains("(255).to_hex_str()"), "got: {out}");
}

#[test]
fn format_hex_uppercase() {
    let out = expand_format_call(&["\"{:X}\"", "255"]).expect("HEX expands");
    assert!(out.contains("(255).to_hex_upper_str()"), "got: {out}");
}

#[test]
fn format_named_arg_passthrough() {
    let out = expand_format_call(&["\"score: {score}\""]).expect("named expands");
    assert!(
        out.contains("(score).to_str()"),
        "named-arg call missing: {out}"
    );
    assert!(out.contains("\"score: \""), "literal missing: {out}");
}

#[test]
fn format_literal_braces() {
    let out = expand_format_call(&["\"{{}}\""]).expect("literal braces expand");
    assert!(out.contains("\"{}\""), "literal braces missing: {out}");

    let pieces = parse_template("{{}}").unwrap();
    assert_eq!(pieces, vec![FormatPiece::Literal("{}".into())]);
}

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

#[test]
fn format_first_arg_must_be_literal() {
    let e = expand_format_call(&["my_template", "x"]).unwrap_err();
    assert!(matches!(e, FormatExpandError::NotAStringLiteral));

    let e2 = expand_format_call(&["\"a\" + \"b\"", "x"]).unwrap_err();
    assert!(matches!(e2, FormatExpandError::NotAStringLiteral));
}

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

#[test]
fn format_debug_spec() {
    let out = expand_format_call(&["\"v={:?}\"", "val"]).expect("debug expands");
    assert!(out.contains("(val).to_debug_str()"), "got: {out}");
}

#[test]
fn format_bare_x_is_named_not_hex() {
    let out = expand_format_call(&["\"{x}\""]).expect("bare {x} expands");
    assert!(out.contains("(x).to_str()"), "got: {out}");
    assert!(
        !out.contains("to_hex_str"),
        "must not interpret as hex: {out}"
    );
}

#[test]
fn format_template_runtime_concat_smoke() {
    let out =
        expand_format_call(&["\"cell {x},{y} = {color:x}\""]).expect("named-only template expands");
    assert!(out.contains("(x).to_str()"), "got: {out}");
    assert!(out.contains("(y).to_str()"), "got: {out}");
    assert!(out.contains("(color).to_hex_str()"), "got: {out}");
    assert!(out.contains("\"cell \""), "got: {out}");
    assert!(out.contains("\",\""), "got: {out}");
    assert!(out.contains("\" = \""), "got: {out}");
}

// --- v0.25 Track D extensions ----------------------------------------------

#[test]
fn format_width_basic() {
    let pieces = parse_template("{:5}").unwrap();
    match &pieces[0] {
        FormatPiece::Positional { spec } => {
            assert_eq!(spec.width, Some(5));
            assert_eq!(spec.kind, ConvKind::Display);
            assert!(!spec.zero_pad);
            assert!(spec.align.is_none());
        }
        other => panic!("expected Positional, got {other:?}"),
    }
    let out = expand_format_call(&["\"{:5}\"", "n"]).unwrap();
    assert!(out.contains("pad_str(5"), "got: {out}");
}

#[test]
fn format_zero_pad() {
    let pieces = parse_template("{:05}").unwrap();
    match &pieces[0] {
        FormatPiece::Positional { spec } => {
            assert_eq!(spec.width, Some(5));
            assert!(spec.zero_pad);
            assert_eq!(spec.fill, '0');
            assert_eq!(spec.align, Some(Alignment::Right));
        }
        other => panic!("expected Positional, got {other:?}"),
    }
    let out = expand_format_call(&["\"{:05}\"", "n"]).unwrap();
    assert!(out.contains("pad_str(5"), "got: {out}");
    assert!(out.contains("'0'"), "got: {out}");
}

#[test]
fn format_align_left() {
    let pieces = parse_template("{:<5}").unwrap();
    match &pieces[0] {
        FormatPiece::Positional { spec } => {
            assert_eq!(spec.align, Some(Alignment::Left));
            assert_eq!(spec.width, Some(5));
        }
        other => panic!("expected Positional, got {other:?}"),
    }
    let out = expand_format_call(&["\"{:<5}\"", "n"]).unwrap();
    assert!(out.contains("\"left\""), "got: {out}");
}

#[test]
fn format_align_right() {
    let pieces = parse_template("{:>5}").unwrap();
    match &pieces[0] {
        FormatPiece::Positional { spec } => {
            assert_eq!(spec.align, Some(Alignment::Right));
            assert_eq!(spec.width, Some(5));
        }
        other => panic!("expected Positional, got {other:?}"),
    }
    let out = expand_format_call(&["\"{:>5}\"", "n"]).unwrap();
    assert!(out.contains("\"right\""), "got: {out}");
}

#[test]
fn format_align_center() {
    let pieces = parse_template("{:^5}").unwrap();
    match &pieces[0] {
        FormatPiece::Positional { spec } => {
            assert_eq!(spec.align, Some(Alignment::Center));
            assert_eq!(spec.width, Some(5));
        }
        other => panic!("expected Positional, got {other:?}"),
    }
    let out = expand_format_call(&["\"{:^5}\"", "n"]).unwrap();
    assert!(out.contains("\"center\""), "got: {out}");
}

#[test]
fn format_fill_char() {
    let pieces = parse_template("{:*<5}").unwrap();
    match &pieces[0] {
        FormatPiece::Positional { spec } => {
            assert_eq!(spec.fill, '*');
            assert_eq!(spec.align, Some(Alignment::Left));
            assert_eq!(spec.width, Some(5));
        }
        other => panic!("expected Positional, got {other:?}"),
    }
    let out = expand_format_call(&["\"{:*<5}\"", "n"]).unwrap();
    assert!(out.contains("'*'"), "got: {out}");
}

#[test]
fn format_precision() {
    let pieces = parse_template("{:.3}").unwrap();
    match &pieces[0] {
        FormatPiece::Positional { spec } => {
            assert_eq!(spec.precision, Some(3));
        }
        other => panic!("expected Positional, got {other:?}"),
    }
    let out = expand_format_call(&["\"{:.3}\"", "pi"]).unwrap();
    assert!(out.contains("to_str_spec"), "got: {out}");
    assert!(out.contains(", 3)"), "got: {out}");
}

#[test]
fn format_sign_positive() {
    let pieces = parse_template("{:+}").unwrap();
    match &pieces[0] {
        FormatPiece::Positional { spec } => {
            assert!(spec.sign_plus);
        }
        other => panic!("expected Positional, got {other:?}"),
    }
    let out = expand_format_call(&["\"{:+}\"", "n"]).unwrap();
    assert!(out.contains("to_str_spec(true"), "got: {out}");
}

#[test]
fn format_alt_hex() {
    let pieces = parse_template("{:#x}").unwrap();
    match &pieces[0] {
        FormatPiece::Positional { spec } => {
            assert!(spec.alternate);
            assert_eq!(spec.kind, ConvKind::HexLower);
        }
        other => panic!("expected Positional, got {other:?}"),
    }
    let out = expand_format_call(&["\"{:#x}\"", "255"]).unwrap();
    assert!(out.contains("to_hex_str_spec"), "got: {out}");
    assert!(out.contains("false, true"), "got: {out}");
}

#[test]
fn format_alt_binary() {
    let pieces = parse_template("{:#b}").unwrap();
    match &pieces[0] {
        FormatPiece::Positional { spec } => {
            assert!(spec.alternate);
            assert_eq!(spec.kind, ConvKind::Binary);
        }
        other => panic!("expected Positional, got {other:?}"),
    }
    let out = expand_format_call(&["\"{:#b}\"", "5"]).unwrap();
    assert!(out.contains("to_bin_str_spec"), "got: {out}");
}

#[test]
fn format_alt_octal() {
    let pieces = parse_template("{:#o}").unwrap();
    match &pieces[0] {
        FormatPiece::Positional { spec } => {
            assert!(spec.alternate);
            assert_eq!(spec.kind, ConvKind::Octal);
        }
        other => panic!("expected Positional, got {other:?}"),
    }
    let out = expand_format_call(&["\"{:#o}\"", "8"]).unwrap();
    assert!(out.contains("to_oct_str_spec"), "got: {out}");
}

#[test]
fn format_combined_spec() {
    let pieces = parse_template("{:#05x}").unwrap();
    match &pieces[0] {
        FormatPiece::Positional { spec } => {
            assert!(spec.alternate);
            assert!(spec.zero_pad);
            assert_eq!(spec.width, Some(5));
            assert_eq!(spec.kind, ConvKind::HexLower);
        }
        other => panic!("expected Positional, got {other:?}"),
    }
    let out = expand_format_call(&["\"{:#05x}\"", "0xff"]).unwrap();
    assert!(out.contains("to_hex_str_spec"), "got: {out}");
    assert!(out.contains("pad_str(5"), "got: {out}");
}

#[test]
fn format_named_arg_with_width() {
    let pieces = parse_template("{n:5}").unwrap();
    match &pieces[0] {
        FormatPiece::Named { ident, spec } => {
            assert_eq!(ident, "n");
            assert_eq!(spec.width, Some(5));
        }
        other => panic!("expected Named, got {other:?}"),
    }
    let out = expand_format_call(&["\"{n:5}\""]).unwrap();
    assert!(out.contains("pad_str(5"), "got: {out}");
    assert!(out.contains("(n)"), "got: {out}");
}

#[test]
fn format_indexed_positional_deferred() {
    let e = expand_format_call(&["\"{0}\"", "x"]).unwrap_err();
    assert!(
        matches!(e, FormatExpandError::UnsupportedSpec { .. }),
        "got: {e:?}"
    );
}

#[test]
fn format_dynamic_width_deferred() {
    let e = expand_format_call(&["\"{:1$}\"", "x", "5"]).unwrap_err();
    assert!(
        matches!(e, FormatExpandError::UnsupportedSpec { .. }),
        "got: {e:?}"
    );
}

#[test]
fn format_bin_no_prefix() {
    let pieces = parse_template("{:b}").unwrap();
    match &pieces[0] {
        FormatPiece::Positional { spec } => {
            assert_eq!(spec.kind, ConvKind::Binary);
            assert!(!spec.alternate);
        }
        other => panic!("expected Positional, got {other:?}"),
    }
    let out = expand_format_call(&["\"{:b}\"", "5"]).unwrap();
    assert!(out.contains("(5).to_bin_str()"), "got: {out}");
}

#[test]
fn format_oct_no_prefix() {
    let pieces = parse_template("{:o}").unwrap();
    match &pieces[0] {
        FormatPiece::Positional { spec } => {
            assert_eq!(spec.kind, ConvKind::Octal);
            assert!(!spec.alternate);
        }
        other => panic!("expected Positional, got {other:?}"),
    }
    let out = expand_format_call(&["\"{:o}\"", "8"]).unwrap();
    assert!(out.contains("(8).to_oct_str()"), "got: {out}");
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
    assert_eq!(ConvKind::Display.method(), "to_str");
    assert_eq!(ConvKind::HexLower.method(), "to_hex_str");
    assert_eq!(ConvKind::HexUpper.method(), "to_hex_upper_str");
    assert_eq!(ConvKind::Debug.method(), "to_debug_str");
    assert_eq!(ConvKind::Binary.method(), "to_bin_str");
    assert_eq!(ConvKind::Octal.method(), "to_oct_str");
}

#[test]
fn alignment_runtime_strings_stable() {
    assert_eq!(Alignment::Left.as_runtime_str(), "left");
    assert_eq!(Alignment::Right.as_runtime_str(), "right");
    assert_eq!(Alignment::Center.as_runtime_str(), "center");
}

#[test]
fn format_spec_display_is_bare() {
    let s = FormatSpec::display();
    assert!(s.is_bare_conversion());
    assert_eq!(s.kind, ConvKind::Display);
}

#[test]
fn arg_is_string_literal_helpers() {
    assert!(arg_is_string_literal("\"plain\""));
    assert!(arg_is_string_literal("  \"padded\"  "));
    assert!(!arg_is_string_literal("42"));
    assert!(!arg_is_string_literal("ident"));
    assert!(!arg_is_string_literal("\"a\" + \"b\""));
}
