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

// ---- v0.42 L20: paren juxtaposition is NOT a call of a non-callable ----
//
// Background: `(half - ((half / 2) * 2)) == 1` and `(a + b)(c)` used to
// parse as `expr1 APPLIED TO expr2`, surfacing a confusing MT2008
// "value of type `{integer}` is not callable" diagnostic. The fix in
// `try_postfix` only treats a following `(` as a CALL_EXPR when the
// preceding primary is a callable shape (path / call / field / index /
// lambda / method, or parens around any of those).

#[test]
fn l20_paren_arith_chord_keeps_parsing_as_compare() {
    // The original L20 bug report. Parses cleanly into a top-level
    // BINARY_EXPR for `==`; no CALL_EXPR anywhere.
    use mty_syntax::{parser::parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("(half - ((half / 2) * 2)) == 1");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(
        !root
            .descendants()
            .any(|n| n.kind() == SyntaxKind::CALL_EXPR),
        "L20 regression: arith chord must not parse as a call"
    );
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::BINARY_EXPR));
}

#[test]
fn l20_paren_arith_call_is_now_a_parse_error() {
    // `(a + b)(c)` used to parse as `(a+b) APPLIED TO (c)`. With the L20
    // fix, the `(c)` is no longer consumed as a CALL_EXPR and a parse
    // error is emitted at the second `(`.
    use mty_syntax::{parser::parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("(a + b)(c)");
    assert!(
        !r.errors.is_empty(),
        "L20: expected a parse error for `(a + b)(c)`, got none"
    );
    let root = SyntaxNode::new_root(r.green);
    assert!(
        !root
            .descendants()
            .any(|n| n.kind() == SyntaxKind::CALL_EXPR),
        "L20: `(a + b)(c)` must not parse as CALL_EXPR"
    );
}

#[test]
fn l20_paren_around_path_still_calls() {
    // `(f)(x)` — `f` wrapped in parens is still a callable path, so the
    // following `(x)` must parse as a CALL_EXPR.
    use mty_syntax::{parser::parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("(f)(x)");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(
        root.descendants()
            .any(|n| n.kind() == SyntaxKind::CALL_EXPR),
        "(f)(x) must still parse as CALL_EXPR"
    );
}

#[test]
fn l20_plain_call_still_works() {
    use mty_syntax::{parser::parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("f(x)");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::CALL_EXPR));
}

#[test]
fn l20_chained_calls_still_work() {
    // `g()()` — a call whose result is itself called. The inner `g()` is
    // a CALL_EXPR (callable result), so the outer `()` must also parse.
    use mty_syntax::{parser::parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("g()()");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    let calls = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::CALL_EXPR)
        .count();
    assert_eq!(calls, 2, "expected two nested CALL_EXPR nodes for g()()");
}

#[test]
fn l20_closure_call_still_works() {
    // `(fn() { 1 })()` — `fn() { ... }` is a LAMBDA_EXPR, which is
    // callable; the following `()` must parse as a CALL_EXPR.
    use mty_syntax::{parser::parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("(fn() { 1 })()");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(
        root.descendants()
            .any(|n| n.kind() == SyntaxKind::CALL_EXPR),
        "closure-call must parse as CALL_EXPR"
    );
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::LAMBDA_EXPR));
}

#[test]
fn l20_method_call_still_works() {
    // Mighty's parser treats `obj.method(x)` as a CALL_EXPR over a
    // two-segment PATH (`obj.method`), not as METHOD_CALL_EXPR; the
    // important property for L20 is just that it still parses cleanly.
    use mty_syntax::{parser::parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("obj.method(x)");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(
        root.descendants()
            .any(|n| n.kind() == SyntaxKind::CALL_EXPR),
        "obj.method(x) must still parse as a call"
    );
}

#[test]
fn l20_method_call_via_postfix_still_works() {
    // The `METHOD_CALL_EXPR` shape only kicks in when there's an
    // intervening expression boundary — e.g. on the result of an index
    // or call. Guard it explicitly so the postfix DOT path doesn't
    // regress.
    use mty_syntax::{parser::parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("xs.map(square)");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::CALL_EXPR));
}

#[test]
fn l20_indexed_callable_still_works() {
    // `arr[0](x)` — the indexed element may be a closure, so the
    // following `(x)` is allowed.
    use mty_syntax::{parser::parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("arr[0](x)");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::INDEX_EXPR));
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::CALL_EXPR));
}

#[test]
fn l46_unary_not_consumes_call_operand() {
    // L46: `!pred(1)` should parse as `!(pred(1))`, not as `(!pred)(1)`.
    use mty_syntax::{parser::parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("!pred(1)");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::UNARY_EXPR));
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::CALL_EXPR));
}

#[test]
fn l46_unary_not_call_operand_stops_before_binary() {
    use mty_syntax::{parser::parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("!pred(1) && ready");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::BINARY_EXPR));
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::CALL_EXPR));
}

#[test]
fn l20_tuple_literal_not_a_callee() {
    // `(a, b)(c)` is a tuple literal applied to `(c)` — must NOT be a
    // call.
    use mty_syntax::{parser::parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("(a, b)(c)");
    assert!(
        !r.errors.is_empty(),
        "L20: tuple literal must not be treated as callable"
    );
    let root = SyntaxNode::new_root(r.green);
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::TUPLE_EXPR));
}

#[test]
fn l20_unit_literal_not_a_callee() {
    // `()(x)` — `()` is the unit/empty tuple, definitely not callable.
    use mty_syntax::{parser::parse_expr, SyntaxKind};
    let r = parse_expr("()(x)");
    assert!(
        !r.errors.is_empty(),
        "L20: () must not be treated as callable"
    );
    let root = mty_syntax::SyntaxNode::new_root(r.green);
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::TUPLE_EXPR));
}

#[test]
fn l20_unary_not_callable() {
    // `(-f)(x)` — a negated value is arithmetic, not callable.
    use mty_syntax::{parser::parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("(-f)(x)");
    assert!(
        !r.errors.is_empty(),
        "L20: `(-f)(x)` must surface a parse error"
    );
    let root = SyntaxNode::new_root(r.green);
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::UNARY_EXPR));
}
