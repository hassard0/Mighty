//! v0.3 (A55) — NLL last-use acceptance tests.
//!
//! Each test case is a program that the slice-4 lexical checker
//! rejected with a false positive, and which v0.3 NLL accepts because
//! the borrow's borrower binding hits its last use before the
//! conflicting later borrow.

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source};
use mty_types::check_package_typed;

fn check(src: &str) -> Vec<mty_diagnostics::Diagnostic> {
    let parsed = parse_source(src.into(), "test.sd".into());
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

fn assert_no_errors(diags: &[mty_diagnostics::Diagnostic], ctx: &str) {
    let errs: Vec<_> = diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .collect();
    assert!(
        errs.is_empty(),
        "{}: expected no errors, got {:?}",
        ctx,
        errs
    );
}

fn assert_has_code(diags: &[mty_diagnostics::Diagnostic], code: &str, ctx: &str) {
    assert!(
        diags
            .iter()
            .any(|d| d.code.as_str() == code && matches!(d.severity, Severity::Error)),
        "{}: expected error {}, got {:?}",
        ctx,
        code,
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn shared_then_mut_after_last_use() {
    let src = "
        fn f() {
          let mut a = String(\"x\")
          let r = &a
          use_ref(r)
          let m = &mut a
          use_mut(m)
        }
        extern {
          fn use_ref(r: &String)
          fn use_mut(m: &mut String)
        }
    ";
    let d = check(src);
    assert_no_errors(&d, "shared then mut after last use");
}

#[test]
fn two_consecutive_mut_after_last_use() {
    let src = "
        fn f() {
          let mut a = String(\"x\")
          let m1 = &mut a
          use_mut(m1)
          let m2 = &mut a
          use_mut(m2)
        }
        extern {
          fn use_mut(m: &mut String)
        }
    ";
    let d = check(src);
    assert_no_errors(&d, "two mut after last use");
}

#[test]
fn nll_chain_through_three_borrows() {
    // r1 ends; r2 starts; r2 ends; m starts. All disjoint in last-use.
    let src = "
        fn f() {
          let mut a = String(\"x\")
          let r1 = &a
          use_ref(r1)
          let r2 = &a
          use_ref(r2)
          let m = &mut a
          use_mut(m)
        }
        extern {
          fn use_ref(r: &String)
          fn use_mut(m: &mut String)
        }
    ";
    let d = check(src);
    assert_no_errors(&d, "NLL chain of three borrows");
}

#[test]
fn lexical_still_rejects_overlap() {
    // Under NLL, `r` is used at `use_ref(r)` which is AFTER `let m = &mut a`.
    // So the shared borrow is still live → SD3004 still fires.
    let src = "
        fn f() {
          let mut a = String(\"x\")
          let r = &a
          let m = &mut a
          use_ref(r)
          use_mut(m)
        }
        extern {
          fn use_ref(r: &String)
          fn use_mut(m: &mut String)
        }
    ";
    let d = check(src);
    assert_has_code(&d, "SD3004", "lexical overlap still rejected");
}
