//! Macro-introduced `let` bindings are renamed (`__mac_<ctx>_<orig>`)
//! so they cannot capture or be captured by the caller's bindings.
//!
//! v0.13 additionally checks that the scope-aware expander
//! (`expand_scoped`) emits equivalent output to the legacy mangler,
//! confirming the set-of-scopes layer (RFC-009) is non-regressive
//! for the existing simple-capture cases.

#![allow(deprecated)] // exercises legacy `expand` / `expand_to_source` (removal scheduled for v0.15)

use mty_ast::{AstNode, File};
use mty_macros::{
    expand, expand_scoped, expand_to_source, strip_scopes, tokens_to_source, MacroRegistry,
    ScopeGen, Scopes,
};
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
    let out = expand_to_source(def, &["3"], 5).unwrap();
    assert!(out.contains("let __mac_5_y"), "got: {out}");
    assert!(out.contains("__mac_5_y + __mac_5_y"), "got: {out}");
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
    let out = expand_to_source(def, &["y"], 11).unwrap();
    // The arg `y` is wrapped in parens.
    assert!(out.contains("(y)"), "got: {out}");
    // The macro's `y` becomes `__mac_11_y`.
    assert!(out.contains("__mac_11_y"), "got: {out}");
}

#[test]
fn nested_expansions_get_different_contexts() {
    let reg = registry("macro twice(x) => { let y = x; y + y }\n");
    let def = reg.get("twice").unwrap();
    let toks_a = expand(def, &["1"], 1).unwrap();
    let toks_b = expand(def, &["1"], 2).unwrap();
    let s_a: String = toks_a.iter().map(|t| t.text.as_str()).collect();
    let s_b: String = toks_b.iter().map(|t| t.text.as_str()).collect();
    assert!(s_a.contains("__mac_1_y"));
    assert!(s_b.contains("__mac_2_y"));
    assert_ne!(s_a, s_b);
}

// ---------------------------------------------------------------------------
// v0.13 set-of-scopes parity checks (RFC-009).
// ---------------------------------------------------------------------------

#[test]
fn scoped_expansion_emits_same_source_as_legacy() {
    // The set-of-scopes expander must produce the same source text
    // as the legacy mangler — the scope set is *additional*
    // information, not a behavior change for current programs.
    let reg = registry("macro twice(x) => { let y = x; y + y }\n");
    let def = reg.get("twice").unwrap();

    // Legacy: expand_to_source with explicit ctx=1.
    let legacy = expand_to_source(def, &["3"], 1).unwrap();

    // Scoped: drive a ScopeGen so the first fresh scope is 1,
    // matching the legacy ctx exactly.
    let mut gen = ScopeGen::new();
    let exp = expand_scoped(def, &["3"], &mut gen, Scopes::empty(), Scopes::empty()).unwrap();
    let scoped_source = tokens_to_source(&strip_scopes(&exp.tokens));

    assert_eq!(legacy, scoped_source);
    assert_eq!(exp.intro, 1);
}

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
