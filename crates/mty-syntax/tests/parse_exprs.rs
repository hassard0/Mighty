use insta::assert_snapshot;

fn dump(src: &str) -> String {
    let r = mty_syntax::parser::parse_expr(src);
    let node = mty_syntax::SyntaxNode::new_root(r.green);
    format!("{:#?}\nerrors: {:?}", node, r.errors)
}

#[test]
fn e_arith() {
    assert_snapshot!(dump("1 + 2 * 3"));
}
#[test]
fn e_compare_and() {
    assert_snapshot!(dump("a == b && c != d"));
}
#[test]
fn e_field_chain() {
    assert_snapshot!(dump("f(x).y[0]"));
}
#[test]
fn e_index() {
    assert_snapshot!(dump("arr[i + 1]"));
}
#[test]
fn e_method_call() {
    assert_snapshot!(dump("xs.map(square)"));
}
#[test]
fn e_propagate() {
    assert_snapshot!(dump("foo()?"));
}
#[test]
fn e_ask_deadline() {
    assert_snapshot!(dump("obj?Msg(x) @2s"));
}
#[test]
fn e_send() {
    assert_snapshot!(dump("logger!Info(\"started\")"));
}
#[test]
fn e_move() {
    assert_snapshot!(dump("move x"));
}
#[test]
fn e_borrow_mut() {
    assert_snapshot!(dump("&mut buf"));
}
#[test]
fn e_call_some() {
    assert_snapshot!(dump("Some(x)"));
}
#[test]
fn e_struct_lit() {
    assert_snapshot!(dump("User { id, name }"));
}
#[test]
fn e_map_lit() {
    assert_snapshot!(dump("{ a: 1, b: 2 }"));
}
#[test]
fn e_arena_short() {
    assert_snapshot!(dump("arena turn: lower(parse(tokenize(input))?)"));
}
#[test]
fn e_neg_unary() {
    assert_snapshot!(dump("-x + 1"));
}
#[test]
fn e_chain_send_ask() {
    assert_snapshot!(dump("logger!Info(\"x\"); fetcher?Page(url) @2s?"));
}

// ---- slice-2 additions ----

#[test]
fn parse_lambda_nullary() {
    use mty_syntax::{parser::parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("fn() { 0 }");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(
        root.descendants()
            .any(|n| n.kind() == SyntaxKind::LAMBDA_EXPR),
        "expected LAMBDA_EXPR"
    );
}

#[test]
fn parse_lambda_with_params_and_ret() {
    use mty_syntax::{parser::parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("fn(x: I32, y) -> I32 { x + y }");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::LAMBDA_EXPR));
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::FN_PARAM_LIST));
    assert!(root.descendants().any(|n| n.kind() == SyntaxKind::RET_TYPE));
}

#[test]
fn parse_lambda_in_arg_position() {
    use mty_syntax::{parser::parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("dom.listen(\"click\", fn() { c!Click() })");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::LAMBDA_EXPR));
}

#[test]
fn parse_run_expr_in_block() {
    use mty_syntax::{parse, SyntaxKind, SyntaxNode};
    let src = "fn f() { run job(input) }";
    let r = parse(src);
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(root.descendants().any(|n| n.kind() == SyntaxKind::RUN_EXPR));
}

#[test]
fn parse_turbofish_method_call() {
    use mty_syntax::{parser::parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("Map::[Str, Json].new()");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::GENERIC_ARG_LIST));
}

#[test]
fn parse_turbofish_constructor() {
    use mty_syntax::parser::parse_expr;
    let r = parse_expr("Some::[I32](42)");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
}

#[test]
fn parse_turbofish_struct_literal() {
    use mty_syntax::{parser::parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("Map::[Str, Json]{}");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::STRUCT_EXPR));
}

#[test]
fn parse_method_with_keyword_name() {
    use mty_syntax::{parse, SyntaxKind, SyntaxNode};
    let r = parse("fn f() { dom.on(\"click\", h) }");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::METHOD_CALL_EXPR));
}

#[test]
fn parse_field_with_keyword_name() {
    use mty_syntax::parse;
    let r = parse("fn f() { let _ = x.match }");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
}

#[test]
fn parse_run_expr_with_propagate() {
    use mty_syntax::{parse, SyntaxKind, SyntaxNode};
    let src = "fn f() { run job(input)? }";
    let r = parse(src);
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(root.descendants().any(|n| n.kind() == SyntaxKind::RUN_EXPR));
}
