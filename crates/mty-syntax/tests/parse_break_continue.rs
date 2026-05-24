//! v0.5 — `break` / `continue` parse as BREAK_EXPR / CONTINUE_EXPR.

use mty_syntax::{parser::parse_expr, SyntaxKind, SyntaxNode};

fn first_kind(src: &str) -> SyntaxKind {
    let r = parse_expr(src);
    let node = SyntaxNode::new_root(r.green);
    let file = node.children().next().expect("FILE has at least one child");
    file.kind()
}

#[test]
fn parses_bare_break() {
    assert_eq!(first_kind("break"), SyntaxKind::BREAK_EXPR);
}

#[test]
fn parses_break_with_value() {
    let r = parse_expr("break 42");
    let node = SyntaxNode::new_root(r.green);
    let file = node.children().next().unwrap();
    assert_eq!(file.kind(), SyntaxKind::BREAK_EXPR);
    // value child is a LITERAL_EXPR
    let lit = file
        .children()
        .find(|c| c.kind() == SyntaxKind::LITERAL_EXPR);
    assert!(lit.is_some(), "break value should lower to LITERAL_EXPR");
}

#[test]
fn parses_continue() {
    assert_eq!(first_kind("continue"), SyntaxKind::CONTINUE_EXPR);
}

#[test]
fn break_inside_loop_block() {
    // `loop { break }` should parse cleanly.
    let r = parse_expr("loop { break }");
    let node = SyntaxNode::new_root(r.green);
    let file = node.children().next().unwrap();
    assert_eq!(file.kind(), SyntaxKind::LOOP_EXPR);
    assert!(
        r.errors.is_empty(),
        "no parse errors expected: {:?}",
        r.errors
    );
    // Find the BREAK_EXPR somewhere inside.
    let found_break = file
        .descendants()
        .any(|d| d.kind() == SyntaxKind::BREAK_EXPR);
    assert!(found_break, "loop body should contain BREAK_EXPR");
}

#[test]
fn continue_inside_for() {
    let r = parse_expr("for x in xs { continue }");
    let node = SyntaxNode::new_root(r.green);
    let file = node.children().next().unwrap();
    assert_eq!(file.kind(), SyntaxKind::FOR_EXPR);
    assert!(r.errors.is_empty(), "no parse errors: {:?}", r.errors);
    let found_cont = file
        .descendants()
        .any(|d| d.kind() == SyntaxKind::CONTINUE_EXPR);
    assert!(found_cont, "for body should contain CONTINUE_EXPR");
}

#[test]
fn break_with_value_in_loop() {
    // The classic `let x = loop { if cond { break 42 } }` pattern.
    let r = parse_expr("loop { if true { break 42 } }");
    let node = SyntaxNode::new_root(r.green);
    let file = node.children().next().unwrap();
    assert_eq!(file.kind(), SyntaxKind::LOOP_EXPR);
    assert!(r.errors.is_empty(), "no parse errors: {:?}", r.errors);
    let break_expr = file
        .descendants()
        .find(|d| d.kind() == SyntaxKind::BREAK_EXPR)
        .expect("BREAK_EXPR present");
    let has_lit = break_expr
        .children()
        .any(|c| c.kind() == SyntaxKind::LITERAL_EXPR);
    assert!(has_lit, "break with value carries a literal child");
}
