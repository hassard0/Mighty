//! v0.37 T6 — typeck of `extern c fn ... (...)` variadic declarations.
//!
//! Pre-v0.37 the parser didn't recognise `...` at all, so `extern c {
//! fn printf(fmt: *U8, ...) -> I32 }` lost the trailing arguments and
//! `printf("%d", 1, 2)` either MT2005'd or silently dropped the extras
//! at codegen. v0.37 T6 adds the variadic token, a `HirFn.is_variadic`
//! flag, plumbs it onto `FnDef.is_variadic`, and teaches `synth_call`
//! to relax the exact-arity check for variadic callees.
//!
//! These tests pin the typeck behaviour:
//!   1. Calling a variadic extern with the fixed prefix arity → ok
//!   2. Calling with extra args → ok (was MT2005 before T6)
//!   3. Calling with FEWER than the fixed prefix → still MT2005

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source};
use mty_types::check_package;

fn errors(src: &str) -> Vec<String> {
    let parsed = parse_source(src.into(), "extern_variadic_typeck.mty".into());
    let (pkg, mut diags) = lower(&parsed);
    let any_lower_err = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !any_lower_err {
        let mut td = check_package(&pkg);
        diags.append(&mut td);
    }
    diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .map(|d| d.code.as_str().to_string())
        .collect()
}

/// Variadic extern called with exactly the fixed prefix (`fmt` only)
/// should typecheck — no MT2005.
#[test]
fn variadic_fixed_arity_only_ok() {
    let src = r#"
extern c {
  fn printf(fmt: *U8, ...) -> I32
}

fn main() {
  let _ = printf(raw_ptr(0))
}
"#;
    let codes = errors(src);
    assert!(
        !codes.iter().any(|c| c == "MT2005"),
        "expected no MT2005 for fixed-arity variadic call, got {codes:?}"
    );
}

/// Variadic extern called with extras → still no MT2005 (the whole
/// point of variadic). Pre-T6 this would have been MT2005 because
/// `synth_call` checked `params.len() != args.len()`.
#[test]
fn variadic_extra_args_ok() {
    let src = r#"
extern c {
  fn printf(fmt: *U8, ...) -> I32
}

fn main() {
  let _ = printf(raw_ptr(0), 1, 2, 3)
}
"#;
    let codes = errors(src);
    assert!(
        !codes.iter().any(|c| c == "MT2005"),
        "expected no MT2005 for variadic call with extras, got {codes:?}"
    );
}

/// Variadic does NOT relax the minimum. Calling `printf()` with no
/// fixed args is still MT2005.
#[test]
fn variadic_below_fixed_arity_still_errors() {
    let src = r#"
extern c {
  fn printf(fmt: *U8, ...) -> I32
}

fn main() {
  let _ = printf()
}
"#;
    let codes = errors(src);
    assert!(
        codes.iter().any(|c| c == "MT2005"),
        "expected MT2005 for below-fixed-arity variadic call, got {codes:?}"
    );
}

/// Non-variadic extern still gets the strict exact-arity check. We
/// shouldn't have accidentally relaxed all extern call checks.
#[test]
fn non_variadic_extern_arity_still_strict() {
    let src = r#"
extern c {
  fn strlen(s: *U8) -> USize
}

fn main() {
  let _ = strlen(raw_ptr(0), 99)
}
"#;
    let codes = errors(src);
    assert!(
        codes.iter().any(|c| c == "MT2005"),
        "expected MT2005 for over-arity non-variadic call, got {codes:?}"
    );
}
