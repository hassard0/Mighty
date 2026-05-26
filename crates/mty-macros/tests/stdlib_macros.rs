//! Integration smoke test for the bundled standard macros.
//!
//! Verifies that `assert!`, `assert_eq!`, `assert_ne!`, `debug!`, and
//! `unreachable!()` are loadable, expand cleanly when called inline,
//! and produce sensible source after expansion.
//!
//! v0.15 migration: uses the set-of-scopes expander
//! (`expand_scoped_to_source`) — the legacy `expand_to_source` was
//! deleted in v0.15.

use mty_ast::{AstNode, File};
use mty_macros::{expand_scoped_to_source, stdlib, MacroRegistry, PackageMacros, ScopeGen, Scopes};
use mty_syntax::SyntaxNode;

fn parse_file(src: &str) -> SyntaxNode {
    let p = mty_syntax::parse(src);
    let root = SyntaxNode::new_root(p.green);
    File::cast(root).unwrap().0
}

fn expand_src(def: &mty_macros::MacroDef, args: &[&str]) -> String {
    let mut gen = ScopeGen::new();
    let (src, _exp) =
        expand_scoped_to_source(def, args, &mut gen, Scopes::empty(), Scopes::empty()).unwrap();
    src
}

#[test]
fn bundled_macros_are_loadable() {
    let mut pm = PackageMacros::new();
    let added = stdlib::load_into(&mut pm);
    assert!(added >= 5);
    for name in ["assert", "assert_eq", "assert_ne", "debug", "unreachable"] {
        assert!(pm.local.contains(name), "{name} missing");
    }
}

#[test]
fn assert_eq_expands_to_an_if_check() {
    let file = parse_file(stdlib::ASSERT_SD);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("assert_eq").unwrap();
    let s = expand_src(def, &["x", "y"]);
    assert!(s.contains("if"), "expansion missing if: {s}");
    assert!(s.contains("!="), "expansion missing !=: {s}");
    assert!(s.contains("panic"), "expansion missing panic: {s}");
}

#[test]
fn assert_expands_to_negation_check() {
    let file = parse_file(stdlib::ASSERT_SD);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("assert").unwrap();
    let s = expand_src(def, &["x > 0"]);
    assert!(s.contains("if"), "expansion missing if: {s}");
    assert!(s.contains('!'), "expansion missing !: {s}");
    assert!(s.contains("(x > 0)"), "arg not wrapped: {s}");
}

#[test]
fn assert_ne_expands_to_eq_check() {
    let file = parse_file(stdlib::ASSERT_SD);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("assert_ne").unwrap();
    let s = expand_src(def, &["a", "b"]);
    assert!(s.contains("=="), "expansion missing ==: {s}");
}

#[test]
fn debug_expands_to_eprintln() {
    let file = parse_file(stdlib::DEBUG_SD);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("debug").unwrap();
    let s = expand_src(def, &["my_var"]);
    assert!(s.contains("eprintln"), "expansion missing eprintln: {s}");
    assert!(s.contains("(my_var)"), "arg not wrapped: {s}");
}

#[test]
fn unreachable_expands_to_panic() {
    let file = parse_file(stdlib::UNREACHABLE_SD);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("unreachable").unwrap();
    let s = expand_src(def, &[]);
    assert!(s.contains("panic"), "expansion missing panic: {s}");
    assert!(
        s.contains("unreachable"),
        "panic message should mention unreachable: {s}"
    );
}
