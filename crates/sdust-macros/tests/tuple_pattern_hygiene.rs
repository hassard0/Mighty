//! v0.5: extended hygiene covers tuple, struct, ref patterns.
//!
//! v0.4 only mangled `let IDENT = ...` bindings; v0.5 extends to the
//! common pattern shapes so macros can locally bind multi-value `let`
//! results without capturing caller scope.

use sdust_ast::{AstNode, File};
use sdust_macros::{expand_to_source, MacroRegistry};
use sdust_syntax::SyntaxNode;

fn parse_file(src: &str) -> SyntaxNode {
    let p = sdust_syntax::parse(src);
    let root = SyntaxNode::new_root(p.green);
    File::cast(root).unwrap().0
}

#[test]
fn tuple_pattern_does_not_capture_caller_a() {
    let src = "macro pair(x) => { let (a, b) = x; a + b }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("pair").unwrap();
    // Expand with arg `1`. Without hygiene, the macro body would bind
    // `a` and `b` in the caller's scope and shadow any existing `a`.
    let s = expand_to_source(def, &["thing"], 42).unwrap();
    // Both `a` and `b` must be mangled in both binding + use position.
    assert!(s.contains("let (__mac_42_a, __mac_42_b)"), "got: {s}");
    assert!(s.contains("__mac_42_a + __mac_42_b"), "got: {s}");
    // The literal `a` token must not appear unmangled in the binding or
    // use position (substituted arg is `(thing)`, not `a`).
    assert!(!s.contains("let (a,"), "raw a in tuple binding leaks: {s}");
}

#[test]
fn struct_shorthand_pattern_mangles_both_fields() {
    let src = "macro split(u) => { let User { id, name } = u; id }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("split").unwrap();
    let s = expand_to_source(def, &["x"], 7).unwrap();
    assert!(s.contains("__mac_7_id"), "id not mangled: {s}");
    assert!(s.contains("__mac_7_name"), "name not mangled: {s}");
    // `User` must NOT be mangled (it's a type name, not a binding).
    assert!(s.contains("User"), "User type was mangled: {s}");
    assert!(!s.contains("__mac_7_User"));
}

#[test]
fn struct_renamed_pattern_only_mangles_the_alias() {
    let src = "macro get_id(u) => { let User { id: x } = u; x }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("get_id").unwrap();
    let s = expand_to_source(def, &["x_outer"], 1).unwrap();
    // `x` is the binding; `id` is just the field selector.
    assert!(s.contains("__mac_1_x"), "got: {s}");
    assert!(
        !s.contains("__mac_1_id"),
        "id is a selector, not a binding: {s}"
    );
}

#[test]
fn ref_pattern_mangles_inner_ident() {
    let src = "macro deref(p) => { let &val = p; val }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("deref").unwrap();
    let s = expand_to_source(def, &["r"], 5).unwrap();
    assert!(s.contains("__mac_5_val"), "got: {s}");
}

#[test]
fn ref_mut_pattern_mangles_inner_ident() {
    let src = "macro deref(p) => { let &mut val = p; val }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("deref").unwrap();
    let s = expand_to_source(def, &["r"], 6).unwrap();
    assert!(s.contains("__mac_6_val"), "got: {s}");
}

#[test]
fn ref_keyword_pattern_mangles_inner_ident() {
    let src = "macro takeref(p) => { let ref val = p; val }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("takeref").unwrap();
    let s = expand_to_source(def, &["r"], 9).unwrap();
    assert!(s.contains("__mac_9_val"), "got: {s}");
}

#[test]
fn mut_keyword_pattern_mangles_inner_ident() {
    let src = "macro counter(start) => { let mut c = start; c = c + 1; c }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("counter").unwrap();
    let s = expand_to_source(def, &["0"], 11).unwrap();
    assert!(s.contains("let mut __mac_11_c"), "got: {s}");
}
