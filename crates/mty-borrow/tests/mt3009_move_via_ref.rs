//! v0.3 (A56) — `move *ref` (and `let x = *ref` of a non-Copy type)
//! must emit MT3009.

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source};
use mty_types::check_package_typed;

fn check(src: &str) -> Vec<mty_diagnostics::Diagnostic> {
    let parsed = parse_source(src.into(), "test.mty".into());
    let (pkg, mut diags) = lower(&parsed);
    let any_err = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !any_err {
        let typed = check_package_typed(&pkg);
        diags.extend(typed.diagnostics.clone());
        let any_type_err = diags.iter().any(|d| matches!(d.severity, Severity::Error));
        if !any_type_err {
            diags.extend(mty_borrow::check_package(&typed, &pkg));
        }
    }
    diags
}

fn has_code(diags: &[mty_diagnostics::Diagnostic], code: &str) -> bool {
    diags
        .iter()
        .any(|d| d.code.as_str() == code && matches!(d.severity, Severity::Error))
}

#[test]
fn deref_of_string_ref_is_sd3009() {
    let src = "
        fn f() {
          let a = String(\"x\")
          let r = &a
          let x = *r
          use_owned(x)
        }
        extern { fn use_owned(s: String) }
    ";
    let d = check(src);
    assert!(
        has_code(&d, "MT3009"),
        "deref of &String → MT3009, got {:?}",
        d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn explicit_move_deref_string_is_sd3009() {
    let src = "
        fn f() {
          let a = String(\"x\")
          let r = &a
          let x = move *r
          use_owned(x)
        }
        extern { fn use_owned(s: String) }
    ";
    let d = check(src);
    assert!(
        has_code(&d, "MT3009"),
        "move *r of &String → MT3009, got {:?}",
        d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn deref_of_copy_ref_is_ok() {
    let src = "
        fn f() {
          let a = 42
          let r = &a
          let x = *r
          use_copy(x)
        }
        extern { fn use_copy(v: I32) }
    ";
    let d = check(src);
    let errs: Vec<_> = d
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .map(|d| d.code.as_str().to_string())
        .collect();
    assert!(errs.is_empty(), "deref of &I32 → ok, got {:?}", errs);
}
