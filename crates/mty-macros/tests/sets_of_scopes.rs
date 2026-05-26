//! Set-of-scopes hygiene (RFC-009) — integration tests.
//!
//! These tests exercise the scope-aware expander (`expand_scoped`) and
//! the scope-resolver (`resolve`) together. They cover scenarios
//! where Racket-style single marks are known to fail under macro
//! composition (Flatt 2016, POPL): swap macros, nested macros, and
//! recursive macros that all introduce same-named bindings.
//!
//! Vocabulary used in the assertions:
//!   * "name scope" — the scope set carried by a reference token
//!   * "binding scope" — the scope set recorded for a binding occurrence
//!
//! The resolver picks the binding whose scope set is the largest
//! subset of the name scope (a sensible "innermost wins" under the
//! set-of-scopes model).

use mty_ast::{AstNode, File};
use mty_macros::{expand_scoped, resolve, MacroRegistry, ScopeGen, Scopes};
use mty_syntax::SyntaxNode;

fn registry(src: &str) -> MacroRegistry {
    let p = mty_syntax::parse(src);
    let root = SyntaxNode::new_root(p.green);
    let file = File::cast(root).expect("FILE root");
    MacroRegistry::from_file(&file.0)
}

fn def(reg: &MacroRegistry, name: &str) -> mty_macros::MacroDef {
    reg.get(name)
        .cloned()
        .unwrap_or_else(|| panic!("missing macro `{name}`"))
}

// ---------------------------------------------------------------------------
// Test 1: identity macro doesn't capture the caller's bindings.
// ---------------------------------------------------------------------------
#[test]
fn identity_macro_doesnt_capture() {
    // The identity macro returns its argument verbatim. The argument
    // is a reference to a caller-scope `x`. After expansion, that
    // reference must still resolve to the caller's `x`, not to
    // anything the macro might have introduced.
    let reg = registry("macro id(z) => { z }\n");
    let def = def(&reg, "id");
    let mut gen = ScopeGen::new();

    let caller_scopes = Scopes::empty(); // top-level user code
    let exp = expand_scoped(
        &def,
        &["x"],
        &mut gen,
        Scopes::empty(),
        caller_scopes.clone(),
    )
    .unwrap();

    // The argument's `x` token must carry the caller's scopes
    // (empty), NOT the macro's intro scope.
    let x_tok = exp
        .tokens
        .iter()
        .find(|st| st.tok.text == "x")
        .expect("x must appear");
    assert_eq!(
        x_tok.scopes, caller_scopes,
        "user `x` was contaminated by macro scope; got {:?}",
        x_tok.scopes
    );
}

// ---------------------------------------------------------------------------
// Test 2: macro introducing `let tmp = ...` doesn't leak to caller.
// ---------------------------------------------------------------------------
#[test]
fn macro_introducing_let_doesnt_leak() {
    // The macro defines its own `tmp`. A caller that already has a
    // `tmp` binding must not see it shadowed by the macro's `tmp`.
    let reg = registry("macro double(x) => { let tmp = x; tmp + tmp }\n");
    let def = def(&reg, "double");
    let mut gen = ScopeGen::new();

    // Caller's `tmp` lives in scope set {99} (pretend the caller is
    // inside some outer expansion).
    let caller_tmp_scope = Scopes::empty().with(99);

    let exp = expand_scoped(&def, &["3"], &mut gen, Scopes::empty(), Scopes::empty()).unwrap();

    // The macro introduced one binding for `tmp`; its scope set must
    // include the intro scope so it differs from the caller's `tmp`.
    let bind = exp
        .bindings
        .iter()
        .find(|(n, _)| n == "tmp")
        .expect("tmp binding recorded");
    assert!(
        bind.1.iter().any(|s| s == exp.intro),
        "macro-introduced tmp missing intro scope"
    );

    // Resolving the caller's `tmp` reference should NOT pick the
    // macro's `tmp` — the macro binding's scope (intro) is NOT a
    // subset of the caller's name scope ({99}).
    let pick = resolve(&caller_tmp_scope, [(&bind.1, "macro_tmp")]).unwrap();
    assert_eq!(
        pick, None,
        "caller's tmp accidentally resolved to macro's tmp"
    );
}

// ---------------------------------------------------------------------------
// Test 3: swap-macro composition — canonical Flatt failure case.
// ---------------------------------------------------------------------------
#[test]
fn swap_macro_composition() {
    // Two macros, each introducing a `t`, called in sequence on the
    // same caller-side `t`. Under simple marks this collides; under
    // set-of-scopes the two `t`s carry distinct scope sets so
    // resolution can tell them apart.
    let reg = registry(
        "macro setA(x) => { let t = x; t }\n\
         macro setB(x) => { let t = x; t }\n",
    );
    let a = def(&reg, "setA");
    let b = def(&reg, "setB");
    let mut gen = ScopeGen::new();

    let exp_a = expand_scoped(&a, &["v"], &mut gen, Scopes::empty(), Scopes::empty()).unwrap();
    let exp_b = expand_scoped(&b, &["w"], &mut gen, Scopes::empty(), Scopes::empty()).unwrap();

    // Both bound a `t`, but with different intro scopes.
    let t_a = exp_a.bindings.iter().find(|(n, _)| n == "t").unwrap();
    let t_b = exp_b.bindings.iter().find(|(n, _)| n == "t").unwrap();
    assert_ne!(
        t_a.1, t_b.1,
        "swap-macro `t` bindings collided under set-of-scopes"
    );

    // The macro A reference (a `t` token in A's body) should resolve
    // to A's binding, NOT B's, even though both bindings have the
    // same text.
    let ref_a = exp_a
        .tokens
        .iter()
        .find(|st| st.tok.text.starts_with("__mac_") && st.tok.text.ends_with("_t"))
        .expect("A's mangled t");
    // Both bindings visible; the resolver picks the one whose scope
    // set is a subset of A's reference scope set. Only A's binding
    // matches (since A's scope set is in ref_a.scopes).
    let pick = resolve(&ref_a.scopes, [(&t_a.1, "A"), (&t_b.1, "B")]).unwrap();
    assert_eq!(pick, Some("A"));
}

// ---------------------------------------------------------------------------
// Test 4: recursive macro accumulates scopes without colliding.
// ---------------------------------------------------------------------------
#[test]
fn recursive_macro() {
    // A macro called twice (simulating self-recursion): each call
    // mints a distinct intro scope, so two same-named bindings
    // remain distinguishable.
    let reg = registry("macro twice(x) => { let y = x; y + y }\n");
    let def = def(&reg, "twice");
    let mut gen = ScopeGen::new();

    let outer = expand_scoped(&def, &["1"], &mut gen, Scopes::empty(), Scopes::empty()).unwrap();
    // The inner call's def_scopes inherit the outer's body scope —
    // that simulates "this macro was textually invoked inside the
    // outer macro's expansion".
    let inner_def_scopes = Scopes::empty().with(outer.intro);
    let inner = expand_scoped(
        &def,
        &["2"],
        &mut gen,
        inner_def_scopes.clone(),
        Scopes::empty(),
    )
    .unwrap();

    // Distinct intro scopes ⇒ distinct binding scope sets.
    let y_outer = outer.bindings.iter().find(|(n, _)| n == "y").unwrap();
    let y_inner = inner.bindings.iter().find(|(n, _)| n == "y").unwrap();
    assert_ne!(y_outer.1, y_inner.1);

    // The inner binding's scope set is a superset of the outer's.
    assert!(y_outer.1.is_subset(&y_inner.1));

    // A reference inside the inner macro's body has scope set =
    // inner.body_scopes. It must resolve to the inner `y` (the
    // larger subset).
    let inner_ref = inner
        .tokens
        .iter()
        .find(|st| st.tok.text.starts_with("__mac_") && st.tok.text.ends_with("_y"))
        .expect("inner mangled y");
    let pick = resolve(
        &inner_ref.scopes,
        [(&y_outer.1, "outer"), (&y_inner.1, "inner")],
    )
    .unwrap();
    assert_eq!(pick, Some("inner"));
}

// ---------------------------------------------------------------------------
// Test 5: macro composition inside `let` binding RHS.
// ---------------------------------------------------------------------------
#[test]
fn macro_in_let_binding() {
    // The caller writes `let m = mac!(0); m`. The `m` introduced by
    // the caller's `let` and any `m` introduced inside the macro
    // body must remain distinguishable.
    let reg = registry("macro one(_dummy) => { let m = 1; m }\n");
    let def = def(&reg, "one");
    let mut gen = ScopeGen::new();

    // Caller's `m` lives at empty scope set.
    let caller_m_scope = Scopes::empty();
    let exp = expand_scoped(&def, &["0"], &mut gen, Scopes::empty(), Scopes::empty()).unwrap();

    let macro_m = exp.bindings.iter().find(|(n, _)| n == "m").unwrap();
    assert!(macro_m.1.iter().any(|s| s == exp.intro));

    // The caller's reference to `m` (empty scope set) must NOT
    // resolve to the macro's `m` (which carries the intro scope).
    let pick = resolve(&caller_m_scope, [(&macro_m.1, "macro_m")]).unwrap();
    assert_eq!(pick, None, "caller's m was captured by macro's m");
}

// ---------------------------------------------------------------------------
// Test 6: bindings with empty scope sets are universally accessible.
// ---------------------------------------------------------------------------
#[test]
fn empty_scope_binding_is_global() {
    // A top-level binding (scope set empty) is a subset of EVERY
    // name's scope set, so it can be referenced from any expansion
    // — matching the intuitive "global names are always in scope".
    let global = Scopes::empty();
    let inside_macro = Scopes::empty().with(42);
    let pick = resolve(&inside_macro, [(&global, "println")]).unwrap();
    assert_eq!(pick, Some("println"));
}

// ---------------------------------------------------------------------------
// Test 7: shadowing — inner binding wins over outer with smaller subset.
// ---------------------------------------------------------------------------
#[test]
fn inner_binding_shadows_outer() {
    // Outer binding scope: {1}. Inner binding scope: {1, 2}.
    // A reference inside the inner expansion (scope {1, 2}) picks
    // the inner binding because its scope set is the larger subset.
    let name = Scopes::from_iter([1, 2]);
    let outer = Scopes::from_iter([1]);
    let inner = Scopes::from_iter([1, 2]);
    let pick = resolve(&name, [(&outer, "outer"), (&inner, "inner")]).unwrap();
    assert_eq!(pick, Some("inner"));
}

// ---------------------------------------------------------------------------
// Test 8: ambiguity is reported (no silent miscompile).
// ---------------------------------------------------------------------------
#[test]
fn ambiguous_resolution_reports_error() {
    // Two distinct bindings with the same scope set, both subsets
    // of the reference's scope set. The resolver must flag this so
    // the front-end can emit MT5901.
    let name = Scopes::from_iter([1, 2]);
    let a = Scopes::from_iter([1]);
    let b = Scopes::from_iter([1]);
    let err = resolve(&name, [(&a, "a"), (&b, "b")]).unwrap_err();
    assert_eq!(err, mty_macros::ResolveAmbiguity);
}

// ---------------------------------------------------------------------------
// Test 9: parameter substitution preserves the user's scope set.
// ---------------------------------------------------------------------------
#[test]
fn parameter_substitution_preserves_user_scopes() {
    // The argument tokens come from the caller with their own scope
    // set; the macro body's surrounding scope must NOT contaminate
    // them on splice.
    let reg = registry("macro id(x) => { x + 1 }\n");
    let def = def(&reg, "id");
    let mut gen = ScopeGen::new();

    let caller_scopes = Scopes::empty().with(77);
    let exp = expand_scoped(
        &def,
        &["arg"],
        &mut gen,
        Scopes::empty(),
        caller_scopes.clone(),
    )
    .unwrap();

    // The `arg` token came from the caller and must retain caller
    // scopes (77 ONLY), not include the macro's intro scope.
    let arg_tok = exp
        .tokens
        .iter()
        .find(|st| st.tok.text == "arg")
        .expect("arg token");
    assert_eq!(arg_tok.scopes, caller_scopes);
    assert!(!arg_tok.scopes.iter().any(|s| s == exp.intro));

    // The literal `1` was introduced by the body; it MUST carry the
    // macro's intro scope.
    let one_tok = exp
        .tokens
        .iter()
        .find(|st| st.tok.text == "1")
        .expect("1 literal");
    assert!(one_tok.scopes.iter().any(|s| s == exp.intro));
}

// ---------------------------------------------------------------------------
// Test 10: cross-macro reference resolution picks the right binding.
// ---------------------------------------------------------------------------
#[test]
fn cross_macro_references_resolve_to_their_origin() {
    // Two macros each introduce `q`. References from within each
    // macro's body resolve to that macro's own `q` — never the other
    // macro's `q`.
    let reg = registry(
        "macro mA(z) => { let q = z; q + 1 }\n\
         macro mB(z) => { let q = z; q + 2 }\n",
    );
    let ma = def(&reg, "mA");
    let mb = def(&reg, "mB");
    let mut gen = ScopeGen::new();

    let exp_a = expand_scoped(&ma, &["10"], &mut gen, Scopes::empty(), Scopes::empty()).unwrap();
    let exp_b = expand_scoped(&mb, &["20"], &mut gen, Scopes::empty(), Scopes::empty()).unwrap();

    let q_a = exp_a.bindings.iter().find(|(n, _)| n == "q").unwrap();
    let q_b = exp_b.bindings.iter().find(|(n, _)| n == "q").unwrap();

    // A's references resolve to A's q.
    let ref_a = exp_a
        .tokens
        .iter()
        .find(|st| st.tok.text.starts_with("__mac_") && st.tok.text.ends_with("_q"))
        .unwrap();
    let pick_a = resolve(&ref_a.scopes, [(&q_a.1, "A"), (&q_b.1, "B")]).unwrap();
    assert_eq!(pick_a, Some("A"));

    // B's references resolve to B's q.
    let ref_b = exp_b
        .tokens
        .iter()
        .find(|st| st.tok.text.starts_with("__mac_") && st.tok.text.ends_with("_q"))
        .unwrap();
    let pick_b = resolve(&ref_b.scopes, [(&q_a.1, "A"), (&q_b.1, "B")]).unwrap();
    assert_eq!(pick_b, Some("B"));
}

// ---------------------------------------------------------------------------
// Test 11: scope-gen monotonicity across many expansions.
// ---------------------------------------------------------------------------
#[test]
fn many_expansions_get_unique_intros() {
    let reg = registry("macro id(x) => { let y = x; y }\n");
    let def = def(&reg, "id");
    let mut gen = ScopeGen::new();

    let mut intros = std::collections::HashSet::new();
    for i in 0..20 {
        let exp = expand_scoped(
            &def,
            &[&i.to_string()],
            &mut gen,
            Scopes::empty(),
            Scopes::empty(),
        )
        .unwrap();
        assert!(
            intros.insert(exp.intro),
            "duplicate intro scope {}",
            exp.intro
        );
    }
    assert_eq!(intros.len(), 20);
}

// ---------------------------------------------------------------------------
// Test 12: definition-site scopes propagate into body tokens.
// ---------------------------------------------------------------------------
#[test]
fn definition_scopes_propagate_into_body() {
    // A macro defined inside an outer expansion carries the outer's
    // scope at its definition site; all of its body tokens must
    // include that outer scope.
    let reg = registry("macro inner(x) => { x }\n");
    let def = def(&reg, "inner");
    let mut gen = ScopeGen::new();

    let outer_intro = gen.fresh();
    let def_scopes = Scopes::empty().with(outer_intro);

    let exp = expand_scoped(
        &def,
        &["arg"],
        &mut gen,
        def_scopes.clone(),
        Scopes::empty(),
    )
    .unwrap();

    // Body tokens (anything that didn't come from the call site)
    // must include `outer_intro` in their scope set.
    // Find the L_PAREN token introduced around the arg — it's
    // body-introduced so MUST carry the outer scope.
    let body_tok = exp
        .tokens
        .iter()
        .find(|st| st.tok.text == "(")
        .expect("paren token");
    assert!(
        body_tok.scopes.iter().any(|s| s == outer_intro),
        "body-introduced token missing definition scope"
    );
}
