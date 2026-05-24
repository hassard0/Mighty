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

#[test]
fn empty_fn_clean() {
    let diags = check("fn f() {}");
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .collect();
    assert!(errors.is_empty(), "{:?}", errors);
}

#[test]
fn copy_primitive_reuses_freely() {
    let src = "
        fn f() {
          let a = 1
          let b = a
          let c = a
          let d = b + c
        }
    ";
    let diags = check(src);
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "primitives are Copy, no errors: {:?}",
        errors
    );
}

#[test]
fn move_then_use_errs() {
    let src = "
        fn f() {
          let a = String(\"x\")
          let b = move a
          use_owned(a)
        }
        extern { fn use_owned(s: String) }
    ";
    let diags = check(src);
    assert!(
        diags
            .iter()
            .any(|d| d.code.as_str() == "SD3001" && matches!(d.severity, Severity::Error)),
        "expected SD3001 use_after_move, got {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn shared_borrows_coexist() {
    let src = "
        fn f() {
          let a = String(\"x\")
          let r1 = &a
          let r2 = &a
          use_ref(r1)
          use_ref(r2)
        }
        extern { fn use_ref(r: &String) }
    ";
    let diags = check(src);
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .collect();
    assert!(errors.is_empty(), "shared+shared OK: {:?}", errors);
}
