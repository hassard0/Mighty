//! The canonical `assert_eq` macro expands as documented in
//! `examples/16_macro.mty`: `assert_eq(a, b)` produces
//! `if (a) != (b) { panic("assert_eq failed") }`.

use mty_ast::{AstNode, File};
use mty_macros::{expand_to_source, MacroRegistry};
use mty_syntax::SyntaxNode;

const ASSERT_EQ_SRC: &str =
    "macro assert_eq(a, b) => { if a != b { panic(\"assert_eq failed\") } }\n";

fn registry(src: &str) -> MacroRegistry {
    let p = mty_syntax::parse(src);
    let root = SyntaxNode::new_root(p.green);
    let file = File::cast(root).expect("FILE root");
    MacroRegistry::from_file(&file.0)
}

#[test]
fn expands_with_literals() {
    let reg = registry(ASSERT_EQ_SRC);
    let def = reg.get("assert_eq").unwrap();
    let out = expand_to_source(def, &["1 + 1", "2"], 0).unwrap();
    // Both substitutions present, parens preserve precedence.
    assert!(out.contains("(1 + 1)"), "got: {out}");
    assert!(out.contains("(2)"), "got: {out}");
    // Free name `panic` is not mangled.
    assert!(out.contains("panic"), "got: {out}");
    assert!(out.contains("\"assert_eq failed\""), "got: {out}");
    // The control-flow keyword survives.
    assert!(out.contains("if"), "got: {out}");
    assert!(out.contains("!="), "got: {out}");
}

#[test]
fn expanded_source_reparses_as_expression() {
    let reg = registry(ASSERT_EQ_SRC);
    let def = reg.get("assert_eq").unwrap();
    let out = expand_to_source(def, &["x", "y"], 0).unwrap();
    let parsed = mty_syntax::parser::parse_expr(&out);
    assert!(
        parsed.errors.is_empty(),
        "expanded body did not reparse cleanly: {} (errors: {:?})",
        out,
        parsed.errors
    );
}
