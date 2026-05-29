//! v0.37 T2 — `expr as Ty` cast surface.
//!
//! Pre-v0.37 the parser slotted `AS_KW` into `infix_bp` but its loop
//! always emitted a `BINARY_EXPR` and called `expr_bp` for the RHS,
//! so `1 as I64` parsed as a binary op with a type-path RHS — which
//! `lower_bin_op` then mistook for `BinOp::Add`. T2 special-cases the
//! `AS_KW` arm of the binary loop to emit a real `CAST_EXPR` whose
//! RHS is parsed by the `type_expr` production.
//!
//! These tests pin the syntactic shape (no snapshots — direct
//! descendant assertions so a future renaming of nested nodes
//! doesn't churn the suite).

use mty_syntax::{parse, parser::parse_expr, SyntaxKind, SyntaxNode};

fn expr_root(src: &str) -> SyntaxNode {
    let r = parse_expr(src);
    assert!(
        r.errors.is_empty(),
        "unexpected parse errors for `{}`: {:?}",
        src,
        r.errors
    );
    SyntaxNode::new_root(r.green)
}

fn has_kind(n: &SyntaxNode, k: SyntaxKind) -> bool {
    n.descendants().any(|d| d.kind() == k)
}

fn count_kind(n: &SyntaxNode, k: SyntaxKind) -> usize {
    n.descendants().filter(|d| d.kind() == k).count()
}

#[test]
fn cast_simple_emits_cast_expr() {
    let n = expr_root("x as I64");
    assert!(
        has_kind(&n, SyntaxKind::CAST_EXPR),
        "expected CAST_EXPR, tree:\n{:#?}",
        n
    );
    // Must NOT degrade into BINARY_EXPR — that was the v0.36 bug.
    assert!(
        !has_kind(&n, SyntaxKind::BINARY_EXPR),
        "unexpected BINARY_EXPR in `x as I64`, tree:\n{:#?}",
        n
    );
}

#[test]
fn cast_in_arithmetic_higher_than_add() {
    // `as` binds tighter than `+`, so this should parse as
    // `(x as I64) + 1`: a BINARY_EXPR whose LHS is a CAST_EXPR.
    let n = expr_root("x as I64 + 1");
    assert!(has_kind(&n, SyntaxKind::CAST_EXPR));
    assert!(has_kind(&n, SyntaxKind::BINARY_EXPR));
    // Exactly one cast and exactly one binary op.
    assert_eq!(count_kind(&n, SyntaxKind::CAST_EXPR), 1);
    assert_eq!(count_kind(&n, SyntaxKind::BINARY_EXPR), 1);
}

#[test]
fn cast_left_assoc_chain() {
    // `a as U8 as I64` — left-associative chain → two CAST_EXPR nodes
    // (the outer wraps the inner). No BINARY_EXPR.
    let n = expr_root("a as U8 as I64");
    assert_eq!(count_kind(&n, SyntaxKind::CAST_EXPR), 2);
    assert!(!has_kind(&n, SyntaxKind::BINARY_EXPR));
}

#[test]
fn cast_in_fn_arg() {
    let r = parse("fn f() { g(x as I64) }");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let n = SyntaxNode::new_root(r.green);
    assert!(has_kind(&n, SyntaxKind::CAST_EXPR));
    assert!(has_kind(&n, SyntaxKind::CALL_EXPR));
}

#[test]
fn cast_in_let_binding() {
    let r = parse("fn f() { let n: I64 = x as I64 }");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let n = SyntaxNode::new_root(r.green);
    assert!(has_kind(&n, SyntaxKind::CAST_EXPR));
}

#[test]
fn cast_in_if_condition() {
    let r = parse("fn f() { if (x as I64) > 0 { } }");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let n = SyntaxNode::new_root(r.green);
    assert!(has_kind(&n, SyntaxKind::CAST_EXPR));
    assert!(has_kind(&n, SyntaxKind::IF_EXPR));
}

#[test]
fn cast_with_float_target() {
    let n = expr_root("n as F64");
    assert!(has_kind(&n, SyntaxKind::CAST_EXPR));
    assert!(!has_kind(&n, SyntaxKind::BINARY_EXPR));
}

#[test]
fn cast_method_call_lhs() {
    // `obj.size() as I64` — the postfix call binds tighter than `as`.
    // The current parser flattens `recv.name(args)` into a CALL_EXPR
    // over a path with a `.`-segment (see `parse_exprs__e_method_call`
    // snapshot), so we assert on CALL_EXPR rather than METHOD_CALL_EXPR.
    let n = expr_root("obj.size() as I64");
    assert!(has_kind(&n, SyntaxKind::CAST_EXPR));
    assert!(has_kind(&n, SyntaxKind::CALL_EXPR));
}

#[test]
fn cast_precedence_below_multiply() {
    // `as` is tighter than `*`, so `2 * x as I64` parses as
    // `2 * (x as I64)`.
    let n = expr_root("2 * x as I64");
    assert!(has_kind(&n, SyntaxKind::CAST_EXPR));
    assert!(has_kind(&n, SyntaxKind::BINARY_EXPR));
}

#[test]
fn cast_no_silent_binary_expr_on_known_breakage() {
    // The exact shape that v0.36 T1 called out: `let x: I64 = y as I64`.
    // Prior to T2 this synthesised a BINARY_EXPR whose lower-bin-op
    // fell through to `BinOp::Add`.
    let r = parse("fn f() { let x: I64 = y as I64 }");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let n = SyntaxNode::new_root(r.green);
    assert!(has_kind(&n, SyntaxKind::CAST_EXPR));
    // The only BINARY_EXPR we'd accept here is none — the let-init RHS
    // is purely the cast.
    assert_eq!(count_kind(&n, SyntaxKind::BINARY_EXPR), 0);
}

#[test]
fn cast_missing_type_emits_error() {
    // `x as` with no type term must error rather than silently parse.
    let r = parse_expr("x as");
    assert!(
        !r.errors.is_empty(),
        "expected at least one parse error for `x as`"
    );
}
