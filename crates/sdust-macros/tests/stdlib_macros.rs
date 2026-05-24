//! Integration smoke test for the bundled standard macros.
//!
//! Verifies that `assert!`, `assert_eq!`, `assert_ne!`, `debug!`, and
//! `unreachable!()` are loadable, expand cleanly when called inline,
//! and produce sensible source after expansion.

use sdust_ast::{AstNode, File};
use sdust_macros::{expand_to_source, stdlib, MacroRegistry, PackageMacros};
use sdust_syntax::SyntaxNode;

fn parse_file(src: &str) -> SyntaxNode {
    let p = sdust_syntax::parse(src);
    let root = SyntaxNode::new_root(p.green);
    File::cast(root).unwrap().0
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
    let s = expand_to_source(def, &["x", "y"], 1).unwrap();
    assert!(s.contains("if"), "expansion missing if: {s}");
    assert!(s.contains("!="), "expansion missing !=: {s}");
    assert!(s.contains("panic"), "expansion missing panic: {s}");
}

#[test]
fn assert_expands_to_negation_check() {
    let file = parse_file(stdlib::ASSERT_SD);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("assert").unwrap();
    let s = expand_to_source(def, &["x > 0"], 1).unwrap();
    assert!(s.contains("if"), "expansion missing if: {s}");
    assert!(s.contains("!"), "expansion missing !: {s}");
    assert!(s.contains("(x > 0)"), "arg not wrapped: {s}");
}

#[test]
fn assert_ne_expands_to_eq_check() {
    let file = parse_file(stdlib::ASSERT_SD);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("assert_ne").unwrap();
    let s = expand_to_source(def, &["a", "b"], 1).unwrap();
    assert!(s.contains("=="), "expansion missing ==: {s}");
}

#[test]
fn debug_expands_to_eprintln() {
    let file = parse_file(stdlib::DEBUG_SD);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("debug").unwrap();
    let s = expand_to_source(def, &["my_var"], 1).unwrap();
    assert!(s.contains("eprintln"), "expansion missing eprintln: {s}");
    assert!(s.contains("(my_var)"), "arg not wrapped: {s}");
}

#[test]
fn unreachable_expands_to_panic() {
    let file = parse_file(stdlib::UNREACHABLE_SD);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("unreachable").unwrap();
    let s = expand_to_source(def, &[], 1).unwrap();
    assert!(s.contains("panic"), "expansion missing panic: {s}");
    assert!(
        s.contains("unreachable"),
        "panic message should mention unreachable: {s}"
    );
}
