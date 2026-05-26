//! The canonical `assert_eq` macro expands as documented in
//! `examples/16_macro.mty`: `assert_eq(a, b)` produces
//! `if (a) != (b) { panic("assert_eq failed") }`.
//!
//! v0.15 migration: uses the set-of-scopes expander
//! (`expand_scoped_to_source`) — the legacy `expand_to_source` was
//! deleted in v0.15.

use mty_ast::{AstNode, File};
use mty_macros::{expand_scoped_to_source, MacroRegistry, ScopeGen, Scopes};
use mty_syntax::SyntaxNode;

const ASSERT_EQ_SRC: &str =
    "macro assert_eq(a, b) => { if a != b { panic(\"assert_eq failed\") } }\n";

fn registry(src: &str) -> MacroRegistry {
    let p = mty_syntax::parse(src);
    let root = SyntaxNode::new_root(p.green);
    let file = File::cast(root).expect("FILE root");
    MacroRegistry::from_file(&file.0)
}

fn expand_src(def: &mty_macros::MacroDef, args: &[&str]) -> String {
    let mut gen = ScopeGen::new();
    let (src, _exp) =
        expand_scoped_to_source(def, args, &mut gen, Scopes::empty(), Scopes::empty()).unwrap();
    src
}

#[test]
fn expands_with_literals() {
    let reg = registry(ASSERT_EQ_SRC);
    let def = reg.get("assert_eq").unwrap();
    let out = expand_src(def, &["1 + 1", "2"]);
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
    let out = expand_src(def, &["x", "y"]);
    let parsed = mty_syntax::parser::parse_expr(&out);
    assert!(
        parsed.errors.is_empty(),
        "expanded body did not reparse cleanly: {} (errors: {:?})",
        out,
        parsed.errors
    );
}
