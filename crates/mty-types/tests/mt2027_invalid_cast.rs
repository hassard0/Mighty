//! v0.37 T2 (MT2027 emit-site): `expr as Ty` is only valid for a fixed
//! set of scalar conversions (int↔int, int↔float, float↔float,
//! bool→int, char↔int). Casts that don't have a defined scalar
//! conversion path are rejected here so they don't silently fall
//! through to `IrTy::Error` in the back-end.

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source};
use mty_types::check_package;

fn diag_codes(src: &str) -> Vec<String> {
    let parsed = parse_source(src.into(), "mt2027.mty".into());
    let (pkg, mut diags) = lower(&parsed);
    let any_lower_err = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !any_lower_err {
        diags.extend(check_package(&pkg));
    }
    diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .map(|d| d.code.as_str())
        .collect()
}

// ─────────────────────────────────────────────────────────────
// Accepted scalar conversions — must NOT emit MT2027.
// ─────────────────────────────────────────────────────────────

#[test]
fn u8_to_i64_widening_is_accepted() {
    // The motivating case from v0.36 T1 / v0.37 T2.
    let src = "fn f(x: U8) -> I64 { x as I64 }";
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"MT2027".to_string()),
        "U8→I64 widening must not emit MT2027, got {:?}",
        codes
    );
}

#[test]
fn i64_to_u8_narrowing_is_accepted() {
    // Truncating cast: well-defined per spec §5.4 even though it loses
    // information.
    let src = "fn f(x: I64) -> U8 { x as U8 }";
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"MT2027".to_string()),
        "I64→U8 narrowing must not emit MT2027, got {:?}",
        codes
    );
}

#[test]
fn f32_to_f64_widening_is_accepted() {
    let src = "fn f(x: F32) -> F64 { x as F64 }";
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"MT2027".to_string()),
        "F32→F64 must not emit MT2027, got {:?}",
        codes
    );
}

#[test]
fn f64_to_i32_truncation_is_accepted() {
    let src = "fn f(x: F64) -> I32 { x as I32 }";
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"MT2027".to_string()),
        "F64→I32 truncation must not emit MT2027, got {:?}",
        codes
    );
}

// ─────────────────────────────────────────────────────────────
// Rejected casts — must emit MT2027.
// ─────────────────────────────────────────────────────────────

#[test]
fn bool_to_str_emits_mt2027() {
    let src = "fn f(x: Bool) -> Str { x as Str }";
    let codes = diag_codes(src);
    assert!(
        codes.contains(&"MT2027".to_string()),
        "Bool→Str cast must emit MT2027, got {:?}",
        codes
    );
}

#[test]
fn str_to_i32_emits_mt2027() {
    let src = "fn f(x: Str) -> I32 { x as I32 }";
    let codes = diag_codes(src);
    assert!(
        codes.contains(&"MT2027".to_string()),
        "Str→I32 cast must emit MT2027, got {:?}",
        codes
    );
}
