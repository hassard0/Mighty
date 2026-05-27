use insta::assert_snapshot;

fn dump_expr(src: &str) -> String {
    let r = mty_syntax::parser::parse_expr(src);
    let node = mty_syntax::SyntaxNode::new_root(r.green);
    format!("{:#?}\nerrors: {:?}", node, r.errors)
}

#[test]
fn s_let_simple() {
    assert_snapshot!(dump_expr("{ let x = 1; x }"));
}

#[test]
fn s_let_typed() {
    assert_snapshot!(dump_expr("{ let x: I32 = 1; x }"));
}

#[test]
fn s_let_struct_pat() {
    assert_snapshot!(dump_expr("{ let User { id, name } = u; id }"));
}

#[test]
fn s_if_else_if() {
    assert_snapshot!(dump_expr("if a { 1 } else if b { 2 } else { 3 }"));
}

#[test]
fn s_match() {
    assert_snapshot!(dump_expr(
        "match res { Ok(v) => v, Err(e) => return Err(e) }"
    ));
}

#[test]
fn s_match_guard() {
    assert_snapshot!(dump_expr("match n { x if x > 0 => x, _ => 0 }"));
}

#[test]
fn s_for_in() {
    assert_snapshot!(dump_expr("for item in items { process(item)? }"));
}

#[test]
fn s_while() {
    assert_snapshot!(dump_expr("while ready() { step() }"));
}

#[test]
fn s_loop() {
    assert_snapshot!(dump_expr("loop { tick() }"));
}

#[test]
fn s_nested_block() {
    assert_snapshot!(dump_expr("{ { let x = 1 } { let y = 2 } }"));
}

// ---- slice-2: if let ----

#[test]
fn parse_if_let_some() {
    use mty_syntax::{parse, SyntaxKind, SyntaxNode};
    let src = "fn f() { if let Some(x) = opt { x } else { 0 } }";
    let r = parse(src);
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    let if_node = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::IF_EXPR)
        .expect("IF_EXPR");
    let has_let = if_node
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|t| t.kind() == SyntaxKind::LET_KW);
    assert!(has_let, "if let should carry LET_KW token");
}

#[test]
fn parse_if_let_ok_no_else() {
    use mty_syntax::parse;
    let src = "fn f() { if let Ok(n) = parse_int(s) { use_n(n) } }";
    let r = parse(src);
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
}

#[test]
fn parse_if_let_in_agent_handler() {
    use mty_syntax::parse;
    // From example 19: handler body uses if-let on a cache lookup.
    let src = "
agent A {
  on M(q) {
    if let Some(hit) = cache.get(q) {
      return Ok(hit)
    }
  }
}";
    let r = parse(src);
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
}

// ---- v0.29 Track D: while let ----

#[test]
fn parse_while_let_some() {
    use mty_syntax::{parse, SyntaxKind, SyntaxNode};
    let src = "fn f() { while let Some(x) = iter.next() { use_x(x) } }";
    let r = parse(src);
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    let while_node = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::WHILE_EXPR)
        .expect("WHILE_EXPR");
    let has_let = while_node
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|t| t.kind() == SyntaxKind::LET_KW);
    assert!(has_let, "while let should carry LET_KW token");
}

#[test]
fn parse_while_let_no_else_branch() {
    use mty_syntax::parse;
    // `while let` has no else arm, unlike `if let`.
    let src = "fn f() { while let Some(c) = resp.stream().next() { handle(c) } }";
    let r = parse(src);
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
}

#[test]
fn parse_plain_while_still_parses() {
    use mty_syntax::{parse, SyntaxKind, SyntaxNode};
    // Regression: adding the let arm must not break the plain shape.
    let src = "fn f() { while ready() { step() } }";
    let r = parse(src);
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    let while_node = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::WHILE_EXPR)
        .expect("WHILE_EXPR");
    let has_let = while_node
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|t| t.kind() == SyntaxKind::LET_KW);
    assert!(!has_let, "plain while must not carry LET_KW");
}
