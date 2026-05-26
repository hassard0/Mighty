//! Macro-introduced `let` bindings are renamed (`__mac_<ctx>_<orig>`)
//! so they cannot capture or be captured by the caller's bindings.
//!
//! v0.13 additionally checks that the scope-aware expander
//! (`expand_scoped`) records bindings with the right scope set so the
//! set-of-scopes resolver (RFC-009) can pick them up.
//!
//! v0.15 migration: the legacy `expand` / `expand_to_source` were
//! deleted, so the v0.13 "scoped output matches legacy output" parity
//! test is gone too (no legacy left to parity-check against). All
//! remaining assertions exercise the scope-aware path directly.

use mty_ast::{AstNode, File};
use mty_macros::{expand_scoped, expand_scoped_to_source, MacroRegistry, ScopeGen, Scopes};
use mty_syntax::SyntaxNode;

fn registry(src: &str) -> MacroRegistry {
    let p = mty_syntax::parse(src);
    let root = SyntaxNode::new_root(p.green);
    let file = File::cast(root).expect("FILE root");
    MacroRegistry::from_file(&file.0)
}

#[test]
fn introduced_binding_is_mangled() {
    let reg = registry("macro twice(x) => { let y = x; y + y }\n");
    let def = reg.get("twice").unwrap();
    let mut gen = ScopeGen::new();
    let (out, exp) =
        expand_scoped_to_source(def, &["3"], &mut gen, Scopes::empty(), Scopes::empty()).unwrap();
    let intro = exp.intro;
    let mangled = format!("__mac_{intro}_y");
    assert!(out.contains(&format!("let {mangled}")), "got: {out}");
    assert!(
        out.contains(&format!("{mangled} + {mangled}")),
        "got: {out}"
    );
    // The original `y` must not appear as a bare identifier (only as part
    // of the mangled form).
    assert!(
        !contains_bare_ident(&out, "y"),
        "raw `y` leaked into expansion: {out}"
    );
}

#[test]
fn parameter_named_y_does_not_get_mangled() {
    // If the caller passes its own `y`, the expander wraps it as `(y)` —
    // but the macro-introduced `y` from the body still gets a different
    // mangled identity, so the call site's `y` and the macro's `y` cannot
    // be the same binding after re-parse.
    let reg = registry("macro use_y(z) => { let y = z; y + 1 }\n");
    let def = reg.get("use_y").unwrap();
    let mut gen = ScopeGen::new();
    let (out, exp) =
        expand_scoped_to_source(def, &["y"], &mut gen, Scopes::empty(), Scopes::empty()).unwrap();
    let intro = exp.intro;
    // The arg `y` is wrapped in parens.
    assert!(out.contains("(y)"), "got: {out}");
    // The macro's `y` becomes `__mac_<intro>_y`.
    assert!(out.contains(&format!("__mac_{intro}_y")), "got: {out}");
}

#[test]
fn nested_expansions_get_different_contexts() {
    let reg = registry("macro twice(x) => { let y = x; y + y }\n");
    let def = reg.get("twice").unwrap();
    // Share a ScopeGen so the two invocations get *distinct* intro IDs
    // (mirrors how the HIR lowering threads one allocator through the
    // whole translation unit).
    let mut gen = ScopeGen::new();
    let (s_a, exp_a) =
        expand_scoped_to_source(def, &["1"], &mut gen, Scopes::empty(), Scopes::empty()).unwrap();
    let (s_b, exp_b) =
        expand_scoped_to_source(def, &["1"], &mut gen, Scopes::empty(), Scopes::empty()).unwrap();
    assert_ne!(exp_a.intro, exp_b.intro);
    assert!(s_a.contains(&format!("__mac_{}_y", exp_a.intro)));
    assert!(s_b.contains(&format!("__mac_{}_y", exp_b.intro)));
    assert_ne!(s_a, s_b);
}

// ---------------------------------------------------------------------------
// v0.13 set-of-scopes binding-record check (RFC-009).
// ---------------------------------------------------------------------------

#[test]
fn scoped_expansion_records_binding_for_let() {
    let reg = registry("macro twice(x) => { let y = x; y + y }\n");
    let def = reg.get("twice").unwrap();
    let mut gen = ScopeGen::new();
    let exp = expand_scoped(def, &["3"], &mut gen, Scopes::empty(), Scopes::empty()).unwrap();
    assert_eq!(exp.bindings.len(), 1);
    assert_eq!(exp.bindings[0].0, "y");
    assert!(exp.bindings[0].1.iter().any(|s| s == exp.intro));
}

/// True iff `needle` appears as a standalone identifier (not as a
/// substring of a longer identifier).
fn contains_bare_ident(haystack: &str, needle: &str) -> bool {
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(needle) {
        let abs = start + pos;
        let before_ok = abs == 0
            || !haystack.as_bytes()[abs - 1].is_ascii_alphanumeric()
                && haystack.as_bytes()[abs - 1] != b'_';
        let after = abs + needle.len();
        let after_ok = after == haystack.len()
            || !haystack.as_bytes()[after].is_ascii_alphanumeric()
                && haystack.as_bytes()[after] != b'_';
        if before_ok && after_ok {
            return true;
        }
        start = abs + needle.len();
    }
    false
}
