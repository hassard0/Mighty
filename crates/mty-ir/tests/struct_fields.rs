//! v0.41 T1 — Struct field reads must return the value of the named
//! field, not field 0. Regression coverage for L15 from the Mighty IDE
//! dogfooding lessons doc.
//!
//! Pre-fix all of these returned the first field's value because both
//! the HIR `Field { receiver, name }` lowering and the
//! multi-segment-path projection in `crates/mty-ir/src/lower/exprs.rs`
//! looked up the field index via the **stdlib** whitelist only and
//! fell back to 0 for user struct field names. The field index carried
//! by `Rvalue::FieldRead` collapsed to 0 for every non-first field.

mod common;

use common::*;
use mty_ir::interp::RunResult;

#[test]
fn struct_field_read_second_field() {
    // Two-field struct. `p.y` must return 22, not 11.
    let src = r#"
        struct Point { x: I32, y: I32 }
        fn main() -> I32 {
            let p = Point { x: 11, y: 22 }
            p.y
        }
    "#;
    let (res, _) = run_main(src);
    assert!(matches!(res, RunResult::Ok { exit: 22 }), "got {:?}", res);
}

#[test]
fn struct_field_read_first_field_still_works() {
    let src = r#"
        struct Point { x: I32, y: I32 }
        fn main() -> I32 {
            let p = Point { x: 11, y: 22 }
            p.x
        }
    "#;
    let (res, _) = run_main(src);
    assert!(matches!(res, RunResult::Ok { exit: 11 }), "got {:?}", res);
}

#[test]
fn struct_field_read_three_fields() {
    // Three-field struct exercising fields 0, 1, and 2.
    let src = r#"
        struct T3 { a: I32, b: I32, c: I32 }
        fn main() -> I32 {
            let t = T3 { a: 10, b: 20, c: 30 }
            t.a + t.b + t.c
        }
    "#;
    let (res, _) = run_main(src);
    assert!(matches!(res, RunResult::Ok { exit: 60 }), "got {:?}", res);
}

#[test]
fn struct_field_read_last_of_three() {
    let src = r#"
        struct T3 { a: I32, b: I32, c: I32 }
        fn main() -> I32 {
            let t = T3 { a: 10, b: 20, c: 30 }
            t.c
        }
    "#;
    let (res, _) = run_main(src);
    assert!(matches!(res, RunResult::Ok { exit: 30 }), "got {:?}", res);
}

#[test]
fn struct_field_read_in_expression_context() {
    // Field read used as a call argument.
    let src = r#"
        struct Pair { a: I32, b: I32 }
        fn double(n: I32) -> I32 { n + n }
        fn main() -> I32 {
            let p = Pair { a: 7, b: 9 }
            double(p.b)
        }
    "#;
    let (res, _) = run_main(src);
    assert!(matches!(res, RunResult::Ok { exit: 18 }), "got {:?}", res);
}

#[test]
fn struct_field_read_nested() {
    // outer.inner.x — chained field read through a nested struct.
    let src = r#"
        struct Inner { x: I32, y: I32 }
        struct Outer { tag: I32, inner: Inner }
        fn main() -> I32 {
            let o = Outer { tag: 1, inner: Inner { x: 100, y: 200 } }
            o.inner.y
        }
    "#;
    let (res, _) = run_main(src);
    assert!(matches!(res, RunResult::Ok { exit: 200 }), "got {:?}", res);
}

#[test]
fn struct_field_read_after_mutation() {
    // Build, mutate, then read the same field.
    let src = r#"
        struct Pt { x: I32, y: I32 }
        fn main() -> I32 {
            let mut p = Pt { x: 1, y: 2 }
            p.y = 42
            p.y
        }
    "#;
    let (res, _) = run_main(src);
    assert!(matches!(res, RunResult::Ok { exit: 42 }), "got {:?}", res);
}

#[test]
fn struct_field_read_first_still_correct_after_other_mutated() {
    // Mutating `y` must not corrupt `x`.
    let src = r#"
        struct Pt { x: I32, y: I32 }
        fn main() -> I32 {
            let mut p = Pt { x: 11, y: 22 }
            p.y = 99
            p.x
        }
    "#;
    let (res, _) = run_main(src);
    assert!(matches!(res, RunResult::Ok { exit: 11 }), "got {:?}", res);
}

#[test]
fn struct_field_read_in_return_position() {
    let src = r#"
        struct R { a: I32, b: I32, c: I32 }
        fn pick() -> I32 {
            let r = R { a: 1, b: 2, c: 3 }
            return r.b
        }
        fn main() -> I32 { pick() }
    "#;
    let (res, _) = run_main(src);
    assert!(matches!(res, RunResult::Ok { exit: 2 }), "got {:?}", res);
}

#[test]
fn struct_field_read_mixed_types() {
    // Mixed types — the second field is an integer; reading it must
    // not return the first (string) field's value.
    let src = r#"
        struct Mixed { name: Str, count: I32 }
        fn main() -> I32 {
            let m = Mixed { name: "alice", count: 42 }
            m.count
        }
    "#;
    let (res, _) = run_main(src);
    assert!(matches!(res, RunResult::Ok { exit: 42 }), "got {:?}", res);
}
