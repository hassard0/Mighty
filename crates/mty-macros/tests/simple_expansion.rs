//! Zero-arg macro that returns a literal expands to that literal.
//!
//! v0.15 migration: uses the set-of-scopes expander
//! (`expand_scoped_to_source`) — the legacy `expand_to_source` was
//! deleted in v0.15. The source-text shape asserted here is identical
//! to what the legacy mangler emitted.

use mty_ast::{AstNode, File};
use mty_macros::{expand_scoped_to_source, MacroRegistry, ScopeGen, Scopes};
use mty_syntax::SyntaxNode;

fn registry(src: &str) -> MacroRegistry {
    let p = mty_syntax::parse(src);
    let root = SyntaxNode::new_root(p.green);
    let file = File::cast(root).expect("FILE root");
    MacroRegistry::from_file(&file.0)
}

#[test]
fn zero_arg_literal_macro() {
    let reg = registry("macro forty_two() => { 42 }\n");
    let def = reg.get("forty_two").unwrap();
    let mut gen = ScopeGen::new();
    let (src, _exp) =
        expand_scoped_to_source(def, &[], &mut gen, Scopes::empty(), Scopes::empty()).unwrap();
    let stripped: String = src.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        stripped.contains("42"),
        "expansion did not include 42: {src}"
    );
}

#[test]
fn zero_arg_block_macro() {
    let reg = registry("macro greet() => { print(\"hi\") }\n");
    let def = reg.get("greet").unwrap();
    let mut gen = ScopeGen::new();
    let (src, _exp) =
        expand_scoped_to_source(def, &[], &mut gen, Scopes::empty(), Scopes::empty()).unwrap();
    assert!(src.contains("print"), "expansion missing print: {src}");
    assert!(src.contains("\"hi\""), "expansion missing string: {src}");
}
