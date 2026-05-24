//! v0.3 (A54) — overlapping field borrows must error.

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
fn two_mut_on_same_field_err() {
    let src = "
        struct Pair { a: String, b: String, }
        fn f() {
          let mut s = Pair { a: String(\"x\"), b: String(\"y\") }
          let r1 = &mut s.a
          let r2 = &mut s.a
          use_mut(r1)
          use_mut(r2)
        }
        extern {
          fn use_mut(m: &mut String)
        }
    ";
    let d = check(src);
    assert!(
        has_code(&d, "MT3006"),
        "two &mut on same field → MT3006, got {:?}",
        d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn mut_then_shared_on_same_field_err() {
    let src = "
        struct Pair { a: String, b: String, }
        fn f() {
          let mut s = Pair { a: String(\"x\"), b: String(\"y\") }
          let m = &mut s.a
          let r = &s.a
          use_mut(m)
          use_ref(r)
        }
        extern {
          fn use_ref(r: &String)
          fn use_mut(m: &mut String)
        }
    ";
    let d = check(src);
    assert!(
        has_code(&d, "MT3005"),
        "&mut then & on same field → MT3005, got {:?}",
        d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}
