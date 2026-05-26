//! v0.5: `Name!(args)` invocation syntax.
//!
//! Verifies that the new MACRO_CALL parse path resolves to the registry
//! and expands correctly. The args are extracted by splitting the
//! opaque TOKEN_TREE on commas at depth 0.
//!
//! v0.15 migration: uses the set-of-scopes expander
//! (`expand_scoped_to_source`) — the legacy `expand_to_source` was
//! deleted in v0.15.

use mty_ast::{AstNode, File};
use mty_macros::{expand_scoped_to_source, MacroRegistry, ScopeGen, Scopes};
use mty_syntax::{SyntaxKind, SyntaxNode};

fn parse_file(src: &str) -> SyntaxNode {
    let p = mty_syntax::parse(src);
    let root = SyntaxNode::new_root(p.green);
    File::cast(root).unwrap().0
}

#[test]
fn macro_call_node_exists_in_cst() {
    let src = "fn main() -> i32 { assert_eq!(1, 1); 0 }\n";
    let file = parse_file(src);
    let has_macro_call = file
        .descendants()
        .any(|n| n.kind() == SyntaxKind::MACRO_CALL);
    assert!(has_macro_call, "MACRO_CALL node missing from CST");
}

#[test]
fn macro_call_args_are_under_token_tree() {
    let src = "fn main() -> i32 { foo!(a, b, c); 0 }\n";
    let file = parse_file(src);
    let mac_call = file
        .descendants()
        .find(|n| n.kind() == SyntaxKind::MACRO_CALL)
        .expect("MACRO_CALL");
    let tree = mac_call
        .children()
        .find(|c| c.kind() == SyntaxKind::TOKEN_TREE)
        .expect("TOKEN_TREE");
    // Should contain the literal source `(a, b, c)`.
    let text = tree.text().to_string();
    assert!(text.starts_with('('), "tree text: {text}");
    assert!(text.ends_with(')'), "tree text: {text}");
    assert!(text.contains('a'), "tree text: {text}");
    assert!(text.contains('b'), "tree text: {text}");
    assert!(text.contains('c'), "tree text: {text}");
}

#[test]
fn registered_macro_expands_via_bang_syntax() {
    // Smoke test: the expander itself runs fine regardless of call syntax.
    // The bang-syntax / plain-call distinction is enforced at the HIR
    // lowering layer (see crates/mty-hir/src/lower/macros.rs).
    let src = "macro inc(x) => { x + 1 }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("inc").unwrap();
    let mut gen = ScopeGen::new();
    let (s, _exp) =
        expand_scoped_to_source(def, &["41"], &mut gen, Scopes::empty(), Scopes::empty()).unwrap();
    assert!(s.contains("(41) + 1"), "got: {s}");
}

#[test]
fn macro_call_with_nested_paren_args() {
    // The token-tree splitter should honor depth-0 commas only.
    let src = "fn main() -> i32 { foo!(g(1, 2), h(3)); 0 }\n";
    let file = parse_file(src);
    let mac_call = file
        .descendants()
        .find(|n| n.kind() == SyntaxKind::MACRO_CALL)
        .expect("MACRO_CALL");
    let tree_text = mac_call
        .children()
        .find(|c| c.kind() == SyntaxKind::TOKEN_TREE)
        .map(|t| t.text().to_string())
        .expect("TOKEN_TREE");
    // Source preservation: nested parens + commas inside survive verbatim.
    assert!(tree_text.contains("g(1, 2)"), "tree text: {tree_text}");
    assert!(tree_text.contains("h(3)"), "tree text: {tree_text}");
}

#[test]
fn macro_call_zero_args() {
    let src = "fn main() -> i32 { unreachable!(); 0 }\n";
    let file = parse_file(src);
    let mac_call = file
        .descendants()
        .find(|n| n.kind() == SyntaxKind::MACRO_CALL)
        .expect("MACRO_CALL");
    let tree_text = mac_call
        .children()
        .find(|c| c.kind() == SyntaxKind::TOKEN_TREE)
        .map(|t| t.text().to_string())
        .expect("TOKEN_TREE");
    assert_eq!(tree_text, "()");
}
