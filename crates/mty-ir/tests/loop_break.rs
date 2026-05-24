//! v0.5 — `loop { … break }` terminates and yields the break value.

mod common;

use common::*;
use mty_ir::interp::RunResult;

#[test]
fn loop_with_unconditional_break_returns_unit() {
    let src = r#"
        fn main() {
            loop { break }
        }
    "#;
    let (res, _) = run_main(src);
    assert!(matches!(res, RunResult::Ok { .. }), "got {:?}", res);
}

#[test]
fn loop_with_conditional_break_terminates() {
    // `let mut n = 0; loop { if n >= 3 { break } n = n + 1 }`. Without
    // break wired up to a real exit target, this would spin until
    // BudgetExceeded — the v0.4 baseline.
    let src = r#"
        fn main() {
            let mut n = 0
            loop {
                if n >= 3 { break }
                n = n + 1
            }
        }
    "#;
    let (res, _) = run_main(src);
    assert!(
        matches!(res, RunResult::Ok { .. }),
        "loop should terminate via break, got {:?}",
        res
    );
}

#[test]
fn loop_with_break_value() {
    // `let x = loop { break 42 }` evaluates to 42; main returns it as
    // its exit code so we can assert on `RunResult::Ok { exit: 42 }`.
    let src = r#"
        fn main() -> I32 {
            let x = loop { break 42 }
            x
        }
    "#;
    let (res, _) = run_main(src);
    assert!(matches!(res, RunResult::Ok { exit: 42 }), "got {:?}", res);
}

#[test]
fn while_with_explicit_break_terminates() {
    let src = r#"
        fn main() {
            let mut n = 0
            while true {
                n = n + 1
                if n >= 5 { break }
            }
        }
    "#;
    let (res, _) = run_main(src);
    assert!(matches!(res, RunResult::Ok { .. }), "got {:?}", res);
}

#[test]
fn nested_break_only_exits_innermost() {
    // The outer loop terminates via its own break, after the inner
    // loop has broken `n` times.
    let src = r#"
        fn main() -> I32 {
            let mut outer = 0
            let mut total = 0
            loop {
                if outer >= 3 { break }
                let mut inner = 0
                loop {
                    if inner >= 2 { break }
                    total = total + 1
                    inner = inner + 1
                }
                outer = outer + 1
            }
            total
        }
    "#;
    let (res, _) = run_main(src);
    assert!(matches!(res, RunResult::Ok { exit: 6 }), "got {:?}", res);
}
