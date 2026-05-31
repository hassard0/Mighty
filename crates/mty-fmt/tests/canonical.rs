//! Unit tests for the per-node canonical printers in
//! `mty_fmt::fmt::{types, patterns, exprs}`. These printers are
//! exposed as library surface; file-level canonicalization is advancing
//! item-by-item, so most tests exercise printers directly and the
//! top-level tests pin each newly routed item shape.

use mty_fmt::doc::Doc;
use mty_fmt::printer::{pretty, Layout};
use mty_syntax::{parser, SyntaxKind, SyntaxNode};

fn render(d: Doc) -> String {
    pretty(&d, &Layout::default())
}

fn type_node(src: &str) -> SyntaxNode {
    let r = parser::parse_type(src);
    let root = SyntaxNode::new_root(r.green);
    // parse_type wraps in a FILE; descend to the first type-shaped node.
    root.descendants()
        .find(|n| {
            matches!(
                n.kind(),
                SyntaxKind::TYPE_PATH
                    | SyntaxKind::TYPE_BORROW
                    | SyntaxKind::TYPE_TUPLE
                    | SyntaxKind::TYPE_ARRAY
                    | SyntaxKind::TYPE_FN
                    | SyntaxKind::TYPE_RESULT_SUGAR
                    | SyntaxKind::TYPE_UNION
            )
        })
        .expect("expected type node")
}

fn expr_node(src: &str) -> SyntaxNode {
    let r = parser::parse_expr(src);
    let root = SyntaxNode::new_root(r.green);
    root.first_child().expect("expected expression node")
}

fn format_file(src: &str) -> String {
    mty_fmt::format(mty_syntax::parse(src).green)
}

#[test]
fn types_path_simple() {
    let t = type_node("Foo");
    assert_eq!(render(mty_fmt::fmt::types::type_expr(&t)), "Foo");
}

#[test]
fn types_path_generic() {
    let t = type_node("Map[Str, I32]");
    assert_eq!(render(mty_fmt::fmt::types::type_expr(&t)), "Map[Str, I32]");
}

#[test]
fn types_borrow_mut() {
    let t = type_node("&mut Foo");
    assert_eq!(render(mty_fmt::fmt::types::type_expr(&t)), "&mut Foo");
}

#[test]
fn types_tuple() {
    let t = type_node("(I32, Str)");
    assert_eq!(render(mty_fmt::fmt::types::type_expr(&t)), "(I32, Str)");
}

#[test]
fn types_result_sugar() {
    let t = type_node("I32!ParseErr");
    assert_eq!(render(mty_fmt::fmt::types::type_expr(&t)), "I32!ParseErr");
}

#[test]
fn types_fn() {
    let t = type_node("fn(I32, Str) -> Bool");
    assert_eq!(
        render(mty_fmt::fmt::types::type_expr(&t)),
        "fn(I32, Str) -> Bool"
    );
}

#[test]
fn exprs_arith_canonicalizes_spacing() {
    let e = expr_node("1+2*3");
    let out = render(mty_fmt::fmt::exprs::expr(&e));
    assert!(out.contains("1 + 2 * 3"), "got {:?}", out);
}

#[test]
fn exprs_method_call() {
    let e = expr_node("xs.map(square)");
    assert_eq!(render(mty_fmt::fmt::exprs::expr(&e)), "xs.map(square)");
}

#[test]
fn exprs_send() {
    let e = expr_node("logger!Info(x)");
    assert_eq!(render(mty_fmt::fmt::exprs::expr(&e)), "logger!Info(x)");
}

#[test]
fn exprs_ask_with_deadline() {
    let e = expr_node("fetcher?Page(url) @2s");
    let out = render(mty_fmt::fmt::exprs::expr(&e));
    assert_eq!(out, "fetcher?Page(url) @2s");
}

#[test]
fn exprs_turbofish_path() {
    let e = expr_node("Some::[I32](42)");
    let out = render(mty_fmt::fmt::exprs::expr(&e));
    assert_eq!(out, "Some::[I32](42)");
}

#[test]
fn exprs_keyword_method_name() {
    let e = expr_node("dom.on(\"click\", h)");
    let out = render(mty_fmt::fmt::exprs::expr(&e));
    assert_eq!(out, "dom.on(\"click\", h)");
}

#[test]
fn exprs_run() {
    let e = expr_node("run job(input)");
    let out = render(mty_fmt::fmt::exprs::expr(&e));
    assert_eq!(out, "run job(input)");
}

#[test]
fn file_const_decl_canonicalizes_spacing() {
    let src = "const   ANSWER:I32=40+2\n";
    assert_eq!(format_file(src), "const ANSWER: I32 = 40 + 2\n");
}

#[test]
fn file_pub_const_decl_preserves_semicolon() {
    let src = "pub const ITEMS:Vec[I32]=Vec.new();\n";
    assert_eq!(format_file(src), "pub const ITEMS: Vec[I32] = Vec.new();\n");
}

#[test]
fn file_const_decl_with_attached_comments_stays_verbatim() {
    let src = "\
const BG_COLOR:    U32 = 487724799_u32   // 0x1d2230ff

// Key constants
const KEY_LEFT:    U32 = 37_u32
";
    assert_eq!(
        format_file(src),
        "\
const BG_COLOR:    U32 = 487724799_u32   // 0x1d2230ff

// Key constants
const KEY_LEFT: U32 = 37_u32
"
    );
}
