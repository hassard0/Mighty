//! Parameters are substituted by the source text of the call site
//! argument, wrapped in parens to preserve precedence.
//!
//! v0.15 migration: uses the set-of-scopes expander
//! (`expand_scoped_to_source`) — the legacy `expand_to_source` was
//! deleted in v0.15. Argument substitution shape is unchanged.

use mty_ast::{AstNode, File};
use mty_macros::{expand_scoped_to_source, MacroRegistry, ScopeGen, Scopes};
use mty_syntax::SyntaxNode;

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
fn single_param_substitutes() {
    let reg = registry("macro id(x) => { x }\n");
    let def = reg.get("id").unwrap();
    let out = expand_src(def, &["123"]);
    assert!(out.contains("(123)"), "got: {out}");
}

#[test]
fn multiple_params_substitute_in_order() {
    let reg = registry("macro pair(a, b) => { (a, b) }\n");
    let def = reg.get("pair").unwrap();
    let out = expand_src(def, &["1", "2"]);
    // Both arguments should appear in order.
    let one_at = out.find('1').expect("missing 1");
    let two_at = out.find('2').expect("missing 2");
    assert!(one_at < two_at, "argument order not preserved: {out}");
}

#[test]
fn expression_argument_keeps_precedence() {
    let reg = registry("macro double(x) => { x + x }\n");
    let def = reg.get("double").unwrap();
    // `1 + 2` substituted naively would yield `1 + 2 + 1 + 2 = 6`. The
    // expander wraps in parens, so the macro reads as `(1 + 2) + (1 + 2)`,
    // which is the same 6 — but more importantly, multiplication doesn't
    // re-associate. Test the textual form to lock the contract.
    let out = expand_src(def, &["1 + 2"]);
    assert!(out.contains("(1 + 2) + (1 + 2)"), "got: {out}");
}

#[test]
fn unused_parameter_does_not_appear() {
    let reg = registry("macro first(a, b) => { a }\n");
    let def = reg.get("first").unwrap();
    let out = expand_src(def, &["7", "9"]);
    assert!(out.contains("(7)"), "got: {out}");
    assert!(!out.contains('9'), "unused arg leaked: {out}");
}
