//! v0.5 — `continue` re-enters the loop header without executing the
//! rest of the body.

mod common;

use common::*;
use mty_ir::interp::RunResult;

#[test]
fn continue_skips_remaining_body() {
    // Count even numbers in 0..6 via continue.
    let src = r#"
        fn main() -> I32 {
            let mut sum = 0
            let mut n = 0
            while n < 6 {
                n = n + 1
                if n % 2 == 1 { continue }
                sum = sum + n
            }
            sum
        }
    "#;
    // 0..6 with break on odd: even values are 2,4,6 → sum = 12.
    let (res, _) = run_main(src);
    assert!(matches!(res, RunResult::Ok { exit: 12 }), "got {:?}", res);
}

#[test]
fn continue_in_for_loop() {
    // Skip 2 in 0..5, sum the rest: 0+1+3+4 = 8.
    let src = r#"
        fn main() -> I32 {
            let mut sum = 0
            for i in 0..5 {
                if i == 2 { continue }
                sum = sum + i
            }
            sum
        }
    "#;
    let (res, _) = run_main(src);
    assert!(matches!(res, RunResult::Ok { exit: 8 }), "got {:?}", res);
}
