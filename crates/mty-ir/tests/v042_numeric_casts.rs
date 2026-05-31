//! v0.42 T2 — `expr as Ty` numeric casts actually convert.
//!
//! Pre-v0.42 the typeck side returned the target type but the runtime
//! and back-ends left the operand at its original SIR type / runtime
//! value, so e.g. `300_i32 as U8` evaluated as 300 rather than 44 and
//! `3.7_f64 as I32` evaluated as 0 rather than 3 (lesson L19 in
//! `mighty-language-lessons.md`).
//!
//! This file covers the interp side end-to-end; the back-end matrix
//! (cranelift / LLVM / wasm) is exercised by their own snapshot suites
//! plus the driver-level smoke test in `mty-driver`.

mod common;

use common::*;
use mty_ir::interp::RunResult;

fn exit_of(src: &str) -> i32 {
    let (res, _) = run_main(src);
    match res {
        RunResult::Ok { exit } => exit,
        other => panic!("expected Ok, got {:?}", other),
    }
}

// ── Int → Int width changes ──────────────────────────────────────────

#[test]
fn i32_to_u8_narrows_modulo_256() {
    // 300 → low 8 bits == 44.
    let src = r#"
        fn main() -> I32 {
            let big: I32 = 300
            let small: U8 = big as U8
            small as I32
        }
    "#;
    assert_eq!(exit_of(src), 44);
}

#[test]
fn u8_to_i32_zero_extends() {
    // 0xFF as I32 → 255 (zero-ext, not sign-ext).
    let src = r#"
        fn main() -> I32 {
            let b: U8 = 255
            b as I32
        }
    "#;
    assert_eq!(exit_of(src), 255);
}

#[test]
fn i64_to_i32_truncates_low_word() {
    // 0x1_0000_002C in i64 → low 32 bits == 0x2C (44).
    let src = r#"
        fn main() -> I32 {
            let wide: I64 = 4294967340
            wide as I32
        }
    "#;
    assert_eq!(exit_of(src), 44);
}

#[test]
fn i32_to_i64_sign_extends_negative() {
    // -1_i32 → -1_i64 (sextend), not 0x0000_0000_FFFF_FFFF.
    let src = r#"
        fn main() -> I32 {
            let small: I32 = -1
            let wide: I64 = small as I64
            // Wide == -1, so wide + 1 == 0.
            (wide + 1) as I32
        }
    "#;
    assert_eq!(exit_of(src), 0);
}

// ── Round-trip ───────────────────────────────────────────────────────

#[test]
fn round_trip_i32_u8_i32_preserves_low_byte() {
    // 65 → 65 → 65 (well within u8 range).
    let src = r#"
        fn main() -> I32 {
            let x: I32 = 65
            let y: U8 = x as U8
            let z: I32 = y as I32
            z
        }
    "#;
    assert_eq!(exit_of(src), 65);
}

// ── Int ↔ Float ──────────────────────────────────────────────────────

#[test]
fn i32_to_f32_then_multiply() {
    // The headline L19 case: 5 as F32 * 2.0 == 10.0, → 10.
    let src = r#"
        fn main() -> I32 {
            let i: I32 = 5
            let f: F32 = i as F32
            let g: F32 = f * 2.0_f32
            g as I32
        }
    "#;
    assert_eq!(exit_of(src), 10);
}

#[test]
fn usize_to_f32_then_multiply() {
    // Verbatim L19 probe: u as F32 * 2.0_f32 (no parens to dodge the
    // pre-v0.42 paren-after-ident parses-as-Call quirk), then back to
    // int. v0.37 T2 made `as` bind tighter than `*` so the
    // bare-expression form is well-defined; precedence-test in
    // crates/mty-syntax/tests/parse_cast.rs.
    let src = r#"
        fn main() -> I32 {
            let u: USize = 5
            let f: F32 = u as F32 * 2.0_f32
            f as I32
        }
    "#;
    assert_eq!(exit_of(src), 10);
}

#[test]
fn f64_to_i32_truncates_toward_zero() {
    let src = r#"
        fn main() -> I32 {
            let f: F64 = 3.7
            f as I32
        }
    "#;
    assert_eq!(exit_of(src), 3);
}

#[test]
fn negative_float_to_i32_truncates_toward_zero() {
    // -3.7 → -3, not -4. The shifted-up form keeps the test value
    // positive (exit codes clamp at 0 in main_exit_for_value), and the
    // intermediate `let shifted` avoids the paren-after-ident parser
    // quirk that turns `(f as I32) + 100` into a call.
    let src = r#"
        fn main() -> I32 {
            let f: F64 = -3.7
            let trunc: I32 = f as I32
            let shifted: I32 = trunc + 100
            shifted
        }
    "#;
    assert_eq!(exit_of(src), 97);
}

// ── Float → Int saturation policy ────────────────────────────────────

#[test]
fn huge_f64_to_i32_saturates_to_max() {
    // 99999999999.0 > i32::MAX → saturates to i32::MAX (= 2147483647).
    // We don't return it directly (POSIX exit codes only carry the low
    // 8 bits), so take the low byte of the saturated value:
    // i32::MAX = 0x7FFF_FFFF → 0xFF = 255. The intermediate `let lo`
    // avoids the paren-after-ident parser quirk for
    // `(n as U8) as I32`.
    //
    // The literal is spelled out long-form because Mighty's lexer at
    // v0.42 only accepts plain decimal float literals (no `1.0e20`
    // exponent shape; cf. v0.42 T2 debug test).
    let src = r#"
        fn main() -> I32 {
            let f: F64 = 99999999999.0
            let n: I32 = f as I32
            let lo: U8 = n as U8
            lo as I32
        }
    "#;
    assert_eq!(exit_of(src), 255);
}

#[test]
fn negative_inf_to_u8_saturates_to_zero() {
    // -1.0 → unsigned U8 should saturate at 0, not wrap.
    let src = r#"
        fn main() -> I32 {
            let f: F32 = -1.0_f32
            let n: U8 = f as U8
            n as I32
        }
    "#;
    assert_eq!(exit_of(src), 0);
}

// ── Float → Float ────────────────────────────────────────────────────

#[test]
fn f32_to_f64_widens() {
    // Intermediate `let prod` instead of `(b * 4.0)` — see
    // negative_float_to_i32 for the rationale on the paren-form quirk.
    let src = r#"
        fn main() -> I32 {
            let a: F32 = 1.5_f32
            let b: F64 = a as F64
            let prod: F64 = b * 4.0
            prod as I32
        }
    "#;
    assert_eq!(exit_of(src), 6);
}

#[test]
fn f64_to_f32_narrows() {
    let src = r#"
        fn main() -> I32 {
            let a: F64 = 2.5
            let b: F32 = a as F32
            let prod: F32 = b * 4.0_f32
            prod as I32
        }
    "#;
    assert_eq!(exit_of(src), 10);
}

// ── USize ↔ I64 ──────────────────────────────────────────────────────

#[test]
fn usize_to_i64_preserves_value() {
    let src = r#"
        fn main() -> I32 {
            let u: USize = 12345
            let i: I64 = u as I64
            let diff: I64 = i - 12000
            diff as I32
        }
    "#;
    assert_eq!(exit_of(src), 345);
}
