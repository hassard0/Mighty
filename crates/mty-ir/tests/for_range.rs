//! v0.5 — `for x in 1..5` actually iterates 4 times via the iterator
//! protocol (`__mty_iter_next`).

mod common;

use common::*;
use mty_ir::interp::RunResult;

#[test]
fn for_exclusive_range_iterates_correct_count() {
    // 1..5 should yield 1,2,3,4 — sum = 10.
    let src = r#"
        fn main() -> I32 {
            let mut sum = 0
            for i in 1..5 {
                sum = sum + i
            }
            sum
        }
    "#;
    let (res, _) = run_main(src);
    assert!(matches!(res, RunResult::Ok { exit: 10 }), "got {:?}", res);
}

#[test]
fn for_inclusive_range_iterates_correct_count() {
    // 1..=5 should yield 1,2,3,4,5 — sum = 15.
    let src = r#"
        fn main() -> I32 {
            let mut sum = 0
            for i in 1..=5 {
                sum = sum + i
            }
            sum
        }
    "#;
    let (res, _) = run_main(src);
    assert!(matches!(res, RunResult::Ok { exit: 15 }), "got {:?}", res);
}

#[test]
fn empty_range_runs_body_zero_times() {
    // 5..5 is empty; sum stays 0.
    let src = r#"
        fn main() -> I32 {
            let mut sum = 100
            for i in 5..5 {
                sum = sum + i
            }
            sum
        }
    "#;
    let (res, _) = run_main(src);
    assert!(matches!(res, RunResult::Ok { exit: 100 }), "got {:?}", res);
}

#[test]
fn break_inside_for_exits_early() {
    // Sum 0..10 but break at i == 3 → 0+1+2 = 3.
    let src = r#"
        fn main() -> I32 {
            let mut sum = 0
            for i in 0..10 {
                if i == 3 { break }
                sum = sum + i
            }
            sum
        }
    "#;
    let (res, _) = run_main(src);
    assert!(matches!(res, RunResult::Ok { exit: 3 }), "got {:?}", res);
}
