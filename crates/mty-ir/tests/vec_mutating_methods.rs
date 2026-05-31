//! IDE dogfood L12: mutating Vec methods used as statements must update
//! the receiver in the interpreter, matching native Cranelift's in-place
//! Vec header behavior.

mod common;

use common::*;
use mty_ir::interp::RunResult;

fn assert_ok_exit(src: &str, expected: i32) {
    let (res, _h) = run_main(src);
    match res {
        RunResult::Ok { exit } => assert_eq!(exit, expected, "exit; src: {src}"),
        other => panic!("expected Ok, got {other:?}; src: {src}"),
    }
}

#[test]
fn vec_push_statement_updates_receiver() {
    let src = r#"
        fn main() -> I32 {
            let mut v: Vec[U8] = Vec.new()
            v.push(65_u8)
            v.push(66_u8)
            v.len() as I32
        }
    "#;
    assert_ok_exit(src, 2);
}

#[test]
fn vec_pop_statement_updates_receiver() {
    let src = r#"
        fn main() -> I32 {
            let mut v: Vec[U8] = Vec.new()
            v.push(65_u8)
            v.push(66_u8)
            v.pop()
            v.len() as I32
        }
    "#;
    assert_ok_exit(src, 1);
}

#[test]
fn vec_clear_statement_updates_receiver() {
    let src = r#"
        fn main() -> I32 {
            let mut v: Vec[U8] = Vec.new()
            v.push(65_u8)
            v.push(66_u8)
            v.clear()
            v.len() as I32
        }
    "#;
    assert_ok_exit(src, 0);
}
