//! v0.29 Track D: `while let pat = scrutinee { body }` typeck.
//!
//! These tests pin down the source-level streaming surface introduced
//! by Track D: the parser accepts the construct, HIR lowers it to
//! `HirExpr::WhileLet`, and the type checker
//!
//!   1. synthesises the scrutinee's type,
//!   2. checks the pattern against that type so its bindings flow
//!      into the body's local scope, and
//!   3. types the whole expression as `Unit` (matching plain `while`).
//!
//! The motivating shape is
//!
//! ```ignore
//! while let Some(chunk) = resp.stream().next() {
//!   handle(chunk)
//! }
//! ```
//!
//! which is what `examples/30_stream_consume.mty` exercises.

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source};
use mty_types::check_package;

fn errors(src: &str) -> Vec<String> {
    let parsed = parse_source(src.into(), "while_let_typeck.mty".into());
    let (pkg, mut diags) = lower(&parsed);
    let any_lower_err = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !any_lower_err {
        diags.extend(check_package(&pkg));
    }
    diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .map(|d| format!("{}: {}", d.code.as_str(), d.primary.message))
        .collect()
}

#[test]
fn while_let_some_option_typechecks() {
    // Canonical shape: `Option[T]` scrutinee + `Some(binding)` pattern.
    // The pattern binding `x` is `I32`, and the body uses it.
    let src = "
        fn pull() -> Option[I32] { None }
        fn drain() {
          while let Some(x) = pull() {
            let _y: I32 = x
          }
        }
    ";
    let errs = errors(src);
    assert!(errs.is_empty(), "expected no typeck errors, got {:?}", errs);
}

#[test]
fn while_let_pattern_typechecks_against_scrutinee() {
    // The pattern's typed against the scrutinee, so a clashing
    // type-annotated binding inside the body would propagate
    // through. We use a no-error body referring to the binding —
    // demonstrates that the binding is in fact reachable from
    // inside the loop (mirrors the `if let` slice-2 typing rule).
    let src = "
        fn pull() -> Option[I32] { None }
        fn drain() -> I32 {
          let mut total: I32 = 0
          while let Some(x) = pull() {
            total = total + x
          }
          total
        }
    ";
    let errs = errors(src);
    assert!(
        errs.is_empty(),
        "expected no typeck errors when binding flows into body, got {:?}",
        errs
    );
}

#[test]
fn while_let_in_main_with_break_clean() {
    // End-to-end: `while let` body uses `break`, which routes
    // through the loop frame the IR lowering installs.
    let src = "
        fn produce() -> Option[I32] { Some(1) }
        fn main() {
          while let Some(_x) = produce() {
            break
          }
        }
    ";
    let errs = errors(src);
    assert!(
        errs.is_empty(),
        "expected no typeck errors with break inside while-let, got {:?}",
        errs
    );
}

#[test]
fn plain_while_still_typechecks() {
    // Regression: the dual-shape WHILE_EXPR lowering must not break
    // the original `while cond { body }` shape.
    let src = "
        fn ready() -> Bool { false }
        fn step() {}
        fn drain() {
          while ready() {
            step()
          }
        }
    ";
    let errs = errors(src);
    assert!(
        errs.is_empty(),
        "plain while must still typecheck cleanly, got {:?}",
        errs
    );
}
