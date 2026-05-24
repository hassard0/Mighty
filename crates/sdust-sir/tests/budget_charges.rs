//! v0.5 dogfood Gap-4 — CPU step + memory budget auto-charging in the
//! SIR interpreter. The step budget was already enforced in v0.4; v0.5
//! adds a paired memory ceiling that trips as a typed
//! `RunResult::MemBudgetExceeded` and an `SD5009` trap message.

mod common;

use common::*;
use sdust_sir::interp::{run::run_fn_with_resource_budget, RunResult};

#[test]
fn step_budget_trips_a_tight_loop() {
    // A `loop { n = n + 1 }` with no break runs until the step budget
    // is exhausted (v0.4 baseline; preserved in v0.5).
    let src = r#"
        fn main() -> I32 {
            let mut n = 0
            loop { n = n + 1 }
            n
        }
    "#;
    let (_pkg, _typed, prog) = compile(src);
    let mut host = TestHost::default();
    // 100 steps is enough for entry+a few iterations, far below where
    // the loop would terminate, so we expect BudgetExceeded.
    let res = sdust_sir::interp::run::run_fn_with_budget(&prog, "main", vec![], &mut host, 100);
    assert!(
        matches!(res, Err(RunResult::BudgetExceeded)),
        "expected BudgetExceeded, got {res:?}"
    );
}

#[test]
fn mem_budget_trips_when_array_alloc_exceeds_cap() {
    // The interp charges ~24 B + payload bytes per ArrayInit. A tiny
    // cap (32 B) should be tripped by a 4-element array (24 + 4*16).
    let src = r#"
        fn main() -> I32 {
            let xs = [1, 2, 3, 4]
            xs.len() as I32
        }
    "#;
    let (_pkg, _typed, prog) = compile(src);
    let mut host = TestHost::default();
    let res = run_fn_with_resource_budget(&prog, "main", vec![], &mut host, 100_000, 32);
    assert!(
        matches!(res, Err(RunResult::Trap { code: "SD5009", .. }))
            || matches!(res, Err(RunResult::MemBudgetExceeded { .. })),
        "expected SD5009 mem trap, got {res:?}"
    );
}

#[test]
fn mem_budget_does_not_trip_when_cap_is_generous() {
    let src = r#"
        fn main() -> I32 {
            let xs = [1, 2, 3, 4]
            xs.len() as I32
        }
    "#;
    let (_pkg, _typed, prog) = compile(src);
    let mut host = TestHost::default();
    // 1 MiB is wildly more than 4 ints; should run cleanly.
    let res = run_fn_with_resource_budget(&prog, "main", vec![], &mut host, 100_000, 1024 * 1024);
    assert!(res.is_ok(), "expected Ok, got {res:?}");
}

#[test]
fn zero_mem_budget_means_unlimited() {
    let src = r#"
        fn main() -> I32 {
            let xs = [1, 2, 3, 4]
            xs.len() as I32
        }
    "#;
    let (_pkg, _typed, prog) = compile(src);
    let mut host = TestHost::default();
    // mem_budget = 0 is the legacy "no cap" sentinel.
    let res = run_fn_with_resource_budget(&prog, "main", vec![], &mut host, 100_000, 0);
    assert!(res.is_ok(), "expected Ok, got {res:?}");
}

#[test]
fn mem_budget_exceeded_exit_code_is_four() {
    let r = RunResult::MemBudgetExceeded {
        used: 1024,
        limit: 512,
    };
    assert_eq!(r.exit_code(), 4);
}
