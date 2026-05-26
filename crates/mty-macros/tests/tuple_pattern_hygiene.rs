//! v0.5: extended hygiene covers tuple, struct, ref patterns.
//!
//! v0.4 only mangled `let IDENT = ...` bindings; v0.5 extends to the
//! common pattern shapes so macros can locally bind multi-value `let`
//! results without capturing caller scope.
//!
//! v0.15 migration: uses the set-of-scopes expander
//! (`expand_scoped_to_source`) — the legacy `expand_to_source` was
//! deleted in v0.15. We use the `intro` scope ID returned by the
//! expander when forming the expected `__mac_<intro>_<name>` text,
//! rather than hardcoding the `ctx` argument the legacy fn took.

use mty_ast::{AstNode, File};
use mty_macros::{expand_scoped_to_source, MacroRegistry, ScopeGen, Scopes};
use mty_syntax::SyntaxNode;

fn parse_file(src: &str) -> SyntaxNode {
    let p = mty_syntax::parse(src);
    let root = SyntaxNode::new_root(p.green);
    File::cast(root).unwrap().0
}

/// Expand `def` with `args` and return `(source, intro_scope_id)`.
fn expand_with_intro(def: &mty_macros::MacroDef, args: &[&str]) -> (String, u32) {
    let mut gen = ScopeGen::new();
    let (src, exp) =
        expand_scoped_to_source(def, args, &mut gen, Scopes::empty(), Scopes::empty()).unwrap();
    (src, exp.intro)
}

#[test]
fn tuple_pattern_does_not_capture_caller_a() {
    let src = "macro pair(x) => { let (a, b) = x; a + b }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("pair").unwrap();
    // Expand with arg `thing`. Without hygiene, the macro body would bind
    // `a` and `b` in the caller's scope and shadow any existing `a`.
    let (s, intro) = expand_with_intro(def, &["thing"]);
    // Both `a` and `b` must be mangled in both binding + use position.
    let expect_a = format!("__mac_{intro}_a");
    let expect_b = format!("__mac_{intro}_b");
    assert!(
        s.contains(&format!("let ({expect_a}, {expect_b})")),
        "got: {s}"
    );
    assert!(s.contains(&format!("{expect_a} + {expect_b}")), "got: {s}");
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
    let (s, intro) = expand_with_intro(def, &["x"]);
    let expect_id = format!("__mac_{intro}_id");
    let expect_name = format!("__mac_{intro}_name");
    assert!(s.contains(&expect_id), "id not mangled: {s}");
    assert!(s.contains(&expect_name), "name not mangled: {s}");
    // `User` must NOT be mangled (it's a type name, not a binding).
    assert!(s.contains("User"), "User type was mangled: {s}");
    assert!(!s.contains(&format!("__mac_{intro}_User")));
}

#[test]
fn struct_renamed_pattern_only_mangles_the_alias() {
    let src = "macro get_id(u) => { let User { id: x } = u; x }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("get_id").unwrap();
    let (s, intro) = expand_with_intro(def, &["x_outer"]);
    // `x` is the binding; `id` is just the field selector.
    let expect_x = format!("__mac_{intro}_x");
    let expect_id = format!("__mac_{intro}_id");
    assert!(s.contains(&expect_x), "got: {s}");
    assert!(
        !s.contains(&expect_id),
        "id is a selector, not a binding: {s}"
    );
}

#[test]
fn ref_pattern_mangles_inner_ident() {
    let src = "macro deref(p) => { let &val = p; val }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("deref").unwrap();
    let (s, intro) = expand_with_intro(def, &["r"]);
    let expect = format!("__mac_{intro}_val");
    assert!(s.contains(&expect), "got: {s}");
}

#[test]
fn ref_mut_pattern_mangles_inner_ident() {
    let src = "macro deref(p) => { let &mut val = p; val }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("deref").unwrap();
    let (s, intro) = expand_with_intro(def, &["r"]);
    let expect = format!("__mac_{intro}_val");
    assert!(s.contains(&expect), "got: {s}");
}

#[test]
fn ref_keyword_pattern_mangles_inner_ident() {
    let src = "macro takeref(p) => { let ref val = p; val }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("takeref").unwrap();
    let (s, intro) = expand_with_intro(def, &["r"]);
    let expect = format!("__mac_{intro}_val");
    assert!(s.contains(&expect), "got: {s}");
}

#[test]
fn mut_keyword_pattern_mangles_inner_ident() {
    let src = "macro counter(start) => { let mut c = start; c = c + 1; c }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("counter").unwrap();
    let (s, intro) = expand_with_intro(def, &["0"]);
    let expect = format!("let mut __mac_{intro}_c");
    assert!(s.contains(&expect), "got: {s}");
}
