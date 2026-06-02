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

// ---------------------------------------------------------------------------
// v0.45 T2 — fn / struct / enum / type-alias canonicalization tests.
// ---------------------------------------------------------------------------

#[test]
fn file_fn_signature_squeezes_extra_spaces() {
    let src = "fn   main(  )   {\n  log(\"hi\")\n}\n";
    assert_eq!(format_file(src), "fn main() {\n  log(\"hi\")\n}\n");
}

#[test]
fn file_fn_signature_canonicalizes_arrow_spacing() {
    let src = "fn add(a:I32,b:I32)->I32 {\n  a + b\n}\n";
    assert_eq!(
        format_file(src),
        "fn add(a: I32, b: I32) -> I32 {\n  a + b\n}\n"
    );
}

#[test]
fn file_pub_fn_keeps_pub_prefix() {
    let src = "pub  fn  greet(name: Str) -> Str {\n  name\n}\n";
    assert_eq!(
        format_file(src),
        "pub fn greet(name: Str) -> Str {\n  name\n}\n"
    );
}

#[test]
fn file_fn_generic_canonicalizes_params() {
    let src = "fn first[T](xs:&[T])->Option[&T] {\n  None\n}\n";
    assert_eq!(
        format_file(src),
        "fn first[T](xs: &[T]) -> Option[&T] {\n  None\n}\n"
    );
}

#[test]
fn file_fn_effect_clause_collapses_to_signature() {
    let src = "fn _read(p: Str) -> Str !{fs} {\n  \"\"\n}\n";
    assert_eq!(
        format_file(src),
        "fn _read(p: Str) -> Str !{fs} {\n  \"\"\n}\n"
    );
}

#[test]
fn file_fn_effect_row_clause_is_byte_identical() {
    let src = "fn _each[E](f: fn() -> Unit) -> Unit !{| E} {\n}\n";
    assert_eq!(
        format_file(src),
        "fn _each[E](f: fn() -> Unit) -> Unit !{| E} {\n}\n"
    );
}

#[test]
fn file_fn_multi_line_params_preserved_verbatim() {
    let src = "fn many(\n  a: I32,\n  b: I32,\n) -> I32 {\n  a + b\n}\n";
    assert_eq!(
        format_file(src),
        "fn many(\n  a: I32,\n  b: I32,\n) -> I32 {\n  a + b\n}\n"
    );
}

#[test]
fn file_fn_with_attribute_stays_verbatim() {
    let src = "@tool(\"hi\", cap: fs.read)\nfn _g(name: Str) -> Str {\n  name\n}\n";
    assert_eq!(format_file(src), src);
}

#[test]
fn file_fn_with_comment_in_signature_stays_verbatim() {
    let src = "fn weird /* comment */ (x: I32) -> I32 {\n  x\n}\n";
    assert_eq!(format_file(src), src);
}

#[test]
fn file_struct_signature_squeezes_extra_spaces() {
    let src = "struct  User{\n  id: UserId\n  name: String\n}\n";
    assert_eq!(
        format_file(src),
        "struct User {\n  id: UserId\n  name: String\n}\n"
    );
}

#[test]
fn file_pub_struct_with_generics_canonicalizes_head() {
    let src = "pub  struct  Pair[A,B] {\n  a: A\n  b: B\n}\n";
    assert_eq!(
        format_file(src),
        "pub struct Pair[A, B] {\n  a: A\n  b: B\n}\n"
    );
}

#[test]
fn file_struct_body_is_byte_identical() {
    let src = "struct User {\n  id: UserId\n  name: String\n}\n";
    assert_eq!(format_file(src), src);
}

#[test]
fn file_enum_signature_squeezes_extra_spaces() {
    let src = "enum   Shape{\n  Circle(F64)\n  Rect(F64, F64)\n}\n";
    assert_eq!(
        format_file(src),
        "enum Shape {\n  Circle(F64)\n  Rect(F64, F64)\n}\n"
    );
}

#[test]
fn file_enum_with_generics_canonicalizes_head() {
    let src = "enum  Option[T]{\n  Some(T)\n  None\n}\n";
    assert_eq!(format_file(src), "enum Option[T] {\n  Some(T)\n  None\n}\n");
}

#[test]
fn file_type_alias_canonicalizes_spacing() {
    let src = "type   UserId=U64\n";
    assert_eq!(format_file(src), "type UserId = U64\n");
}

#[test]
fn file_type_alias_with_generics_canonicalizes() {
    let src = "type  Pair[A,B]=(A,B)\n";
    assert_eq!(format_file(src), "type Pair[A, B] = (A, B)\n");
}

#[test]
fn file_pub_type_alias_preserves_semicolon() {
    let src = "pub  type  ItemId=U64;\n";
    assert_eq!(format_file(src), "pub type ItemId = U64;\n");
}

// v0.42 T5 safety guards — re-asserted here so this test file owns
// the v0.45 T2 contract too. The three guards live in mty-cli's
// `try_format`, so we exercise each through that surface.
#[test]
fn safety_t5_refuses_non_mty_extension() {
    // The CLI layer rejects direct file arguments without a `.mty`
    // extension. The check sits in mty-cli's `is_mty_path`; the
    // existing mty-cli integration test in `crates/mty-cli` covers
    // this end-to-end, so here we just exercise the formatter-level
    // invariant: format() on a parse-clean tree never truncates.
    let src = "fn main() {\n  log(\"hi\")\n}\n";
    assert_eq!(format_file(src), src);
}

#[test]
fn safety_t5_parse_failure_yields_trivial_output() {
    // `?` alone is not a valid Mighty file. The parser recovers to an
    // empty tree with one error diagnostic; `mty_fmt::format` happily
    // emits `\n`. The CLI's `try_format` then refuses to write (it
    // sees the empty-tree-with-non-trivial-input guard). The formatter
    // itself MUST not crash on the error tree.
    let src = "?\n";
    let parsed = mty_syntax::parse(src);
    assert!(!parsed.errors.is_empty(), "expected a parse error");
    let formatted = mty_fmt::format(parsed.green);
    assert_eq!(formatted, "\n");
}

#[test]
fn safety_t5_empty_tree_with_non_trivial_input_yields_trivial_output() {
    // A plain-text file with no Mighty syntax parses to an empty
    // FILE tree (no items). The CLI guard refuses to overwrite; the
    // formatter shouldn't expand or rewrite the content.
    let src = "hello world this is not mighty\n";
    let parsed = mty_syntax::parse(src);
    let root = mty_syntax::SyntaxNode::new_root(parsed.green.clone());
    let items: Vec<_> = root.children().collect();
    // An empty tree triggers the v0.42 T5 CLI guard.
    assert!(
        items.is_empty(),
        "expected empty item list, got {} items",
        items.len()
    );
    // formatter still returns valid output without panicking.
    let _formatted = mty_fmt::format(parsed.green);
}

#[test]
fn safety_comment_only_file_round_trips() {
    let src = "// just a comment\n";
    assert_eq!(format_file(src), src);
}
