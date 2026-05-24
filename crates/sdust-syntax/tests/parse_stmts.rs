use insta::assert_snapshot;

fn dump_expr(src: &str) -> String {
    let r = sdust_syntax::parser::parse_expr(src);
    let node = sdust_syntax::SyntaxNode::new_root(r.green);
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
