//! Regression coverage for logical short-circuiting.
//!
//! L47 from the Mighty IDE lessons documented that `&&` / `||` evaluated
//! both operands, which breaks normal guard-then-use app code. These
//! snippets use divide-by-zero on the skipped side so eager evaluation
//! fails loudly.

mod common;

use common::*;
use mty_ir::interp::RunResult;

fn exit_of(src: &str) -> i32 {
    let (res, _) = run_main(src);
    match res {
        RunResult::Ok { exit } => exit,
        other => panic!("expected Ok, got {:?}", other),
    }
}

#[test]
fn and_skips_rhs_when_lhs_false() {
    let src = r#"
        fn main() -> I32 {
            if false && (1 / 0 == 0) { 1 } else { 7 }
        }
    "#;
    assert_eq!(exit_of(src), 7);
}

#[test]
fn or_skips_rhs_when_lhs_true() {
    let src = r#"
        fn main() -> I32 {
            if true || (1 / 0 == 0) { 9 } else { 1 }
        }
    "#;
    assert_eq!(exit_of(src), 9);
}

#[test]
fn and_evaluates_rhs_when_lhs_true() {
    let src = r#"
        fn main() -> I32 {
            if true && (6 / 2 == 3) { 11 } else { 1 }
        }
    "#;
    assert_eq!(exit_of(src), 11);
}

#[test]
fn or_evaluates_rhs_when_lhs_false() {
    let src = r#"
        fn main() -> I32 {
            if false || (8 / 2 == 4) { 13 } else { 1 }
        }
    "#;
    assert_eq!(exit_of(src), 13);
}
