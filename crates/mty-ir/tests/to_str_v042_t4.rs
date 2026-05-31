//! v0.42 T4 (L23 fix) — interp-side coverage for `to_str` on scalar
//! receivers and `String + String` concat.
//!
//! Mirror of `crates/mty-codegen-cranelift/tests/to_str_v042_t4.rs`
//! at the SIR interpreter layer: the same surface (`n.to_str()` /
//! `"a" + "b"` / `"count=" + n.to_str()`) must behave identically
//! whether the program runs through `mty run --legacy-interp` or
//! cranelift JIT (the cross-backend conformance rule from the v0.42
//! T4 brief).

mod common;

use common::*;
use mty_ir::interp::RunResult;

fn assert_ok_exit(src: &str, expected: i32) {
    let (res, _h) = run_main(src);
    match res {
        RunResult::Ok { exit } => assert_eq!(exit, expected, "exit; src: {src}"),
        other => panic!("expected Ok, got {other:?}; src: {src}"),
    }
}

#[test]
fn i32_to_str_matches_literal() {
    let src = r#"
        fn main() -> I32 {
            let n: I32 = 42
            let s = n.to_str()
            if s == "42" { 1 } else { 0 }
        }
    "#;
    assert_ok_exit(src, 1);
}

#[test]
fn negative_i32_to_str_includes_sign() {
    let src = r#"
        fn main() -> I32 {
            let n: I32 = 0 - 7
            let s = n.to_str()
            if s == "-7" { 1 } else { 0 }
        }
    "#;
    assert_ok_exit(src, 1);
}

#[test]
fn bool_to_str_renders_true_false() {
    let src = r#"
        fn main() -> I32 {
            let t = true
            let f = false
            if t.to_str() == "true" {
              if f.to_str() == "false" { 3 } else { 1 }
            } else { 0 }
        }
    "#;
    assert_ok_exit(src, 3);
}

#[test]
fn str_plus_str_concatenates() {
    let src = r#"
        fn main() -> I32 {
            let a: Str = "hello, "
            let b: Str = "world"
            let c = a + b
            if c == "hello, world" { 1 } else { 0 }
        }
    "#;
    assert_ok_exit(src, 1);
}

#[test]
fn str_plus_int_to_str_round_trip() {
    // The motivating L23 case: build a `"count=N"` line entirely in
    // Mighty without dropping to an FFI shim.
    let src = r#"
        fn main() -> I32 {
            let n: I32 = 42
            let line = "count=" + n.to_str()
            if line == "count=42" { 1 } else { 0 }
        }
    "#;
    assert_ok_exit(src, 1);
}

#[test]
fn to_string_aliases_to_str() {
    let src = r#"
        fn main() -> I32 {
            let n: I32 = 123
            let a = n.to_str()
            let b = n.to_string()
            if a == b { 1 } else { 0 }
        }
    "#;
    assert_ok_exit(src, 1);
}
