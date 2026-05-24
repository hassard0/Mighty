//! v0.3 (A54) — field-level disjoint borrows are accepted.

use sdust_diagnostics::Severity;
use sdust_driver::{lower, parse_source};
use sdust_types::check_package_typed;

fn check(src: &str) -> Vec<sdust_diagnostics::Diagnostic> {
    let parsed = parse_source(src.into(), "test.sd".into());
    let (pkg, mut diags) = lower(&parsed);
    let any_err = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !any_err {
        let typed = check_package_typed(&pkg);
        diags.extend(typed.diagnostics.clone());
        let any_type_err = diags.iter().any(|d| matches!(d.severity, Severity::Error));
        if !any_type_err {
            diags.extend(sdust_borrow::check_package(&typed, &pkg));
        }
    }
    diags
}

fn errors(diags: &[sdust_diagnostics::Diagnostic]) -> Vec<String> {
    diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .map(|d| d.code.as_str().to_string())
        .collect()
}

#[test]
fn mut_and_shared_on_disjoint_fields() {
    let src = "
        struct Pair { a: String, b: String, }
        fn f() {
          let mut s = Pair { a: String(\"x\"), b: String(\"y\") }
          let ra = &mut s.a
          let rb = &s.b
          use_mut(ra)
          use_ref(rb)
        }
        extern {
          fn use_ref(r: &String)
          fn use_mut(m: &mut String)
        }
    ";
    let d = check(src);
    let errs = errors(&d);
    assert!(
        errs.is_empty(),
        "disjoint fields → no errors, got {:?}",
        errs
    );
}

#[test]
fn two_shared_on_same_field() {
    // Two shared borrows on the same field are fine.
    let src = "
        struct Pair { a: String, b: String, }
        fn f() {
          let s = Pair { a: String(\"x\"), b: String(\"y\") }
          let r1 = &s.a
          let r2 = &s.a
          use_ref(r1)
          use_ref(r2)
        }
        extern {
          fn use_ref(r: &String)
        }
    ";
    let d = check(src);
    let errs = errors(&d);
    assert!(
        errs.is_empty(),
        "shared+shared on same field → no errors, got {:?}",
        errs
    );
}
