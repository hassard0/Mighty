//! v0.29 Track C end-to-end: a full Mighty program that uses a non-Unit
//! bang-send return type must round-trip parse → lower → type-check →
//! SIR-lower without errors. Pre-v0.29 the typed bang-send result was
//! dropped as Unit, so the `let r: Str = panel ! Review(...)` line below
//! would have tripped a Unit / Str mismatch at the type-check stage.

use mty_diagnostics::Severity;
use mty_driver::{lower, lower_to_sir, parse_source, type_check};

#[test]
fn typed_bang_send_str_reply_round_trips_end_to_end() {
    let src = r#"
        protocol Reviewer { Review(s: Str) -> Str }
        agent Panel: Reviewer {
          on Review(snippet) -> snippet
        }
        fn main() {
          let panel = spawn Panel()
          let report: Str = panel ! Review("eval(user_input)")
          log(report)
        }
    "#;
    let parsed = parse_source(src.into(), "bang_send_e2e.mty".into());
    let (pkg, mut diags) = lower(&parsed);
    let lower_errors: Vec<_> = diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .cloned()
        .collect();
    assert!(
        lower_errors.is_empty(),
        "lowering should be clean, got: {:?}",
        lower_errors
            .iter()
            .map(|d| format!("{}: {}", d.code.as_str(), d.primary.message))
            .collect::<Vec<_>>()
    );
    diags.extend(type_check(&pkg));
    let type_errors: Vec<_> = diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .cloned()
        .collect();
    assert!(
        type_errors.is_empty(),
        "type-check should be clean (typed bang-send return-type lowering); got: {:?}",
        type_errors
            .iter()
            .map(|d| format!("{}: {}", d.code.as_str(), d.primary.message))
            .collect::<Vec<_>>()
    );
    // SIR lowering must also accept the program — pin that we haven't
    // broken downstream passes that consume the typed-package output.
    let (_prog, sir_diags) = lower_to_sir(&pkg);
    let sir_errors: Vec<_> = sir_diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .cloned()
        .collect();
    assert!(
        sir_errors.is_empty(),
        "SIR lowering should be clean, got: {:?}",
        sir_errors
            .iter()
            .map(|d| format!("{}: {}", d.code.as_str(), d.primary.message))
            .collect::<Vec<_>>()
    );
}

#[test]
fn typed_bang_send_without_let_annotation_threads_through_format() {
    // The demo-08 simplification target: `let r = panel ! Review(s);
    // log(format!("{}", r))` no longer needs the `format!` wrap because
    // the bang-send result already types as Str. Test that the bare
    // `log(panel ! Review(...))` shape works directly.
    let src = r#"
        protocol Reviewer { Review(s: Str) -> Str }
        agent Panel: Reviewer {
          on Review(snippet) -> snippet
        }
        fn main() {
          let panel = spawn Panel()
          log(panel ! Review("snippet"))
        }
    "#;
    let parsed = parse_source(src.into(), "bang_send_log_direct.mty".into());
    let (pkg, mut diags) = lower(&parsed);
    diags.extend(type_check(&pkg));
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .cloned()
        .collect();
    assert!(
        errors.is_empty(),
        "bang-send → log(...) should be clean without format! wrap; got: {:?}",
        errors
            .iter()
            .map(|d| format!("{}: {}", d.code.as_str(), d.primary.message))
            .collect::<Vec<_>>()
    );
}
