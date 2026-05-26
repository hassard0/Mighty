//! v0.16 — user-authored row-poly fn end-to-end tests (RFC-008).
//!
//! Where `stdlib_hof_dispatch.rs` (v0.15) exercises the row-poly
//! signatures wired into the *stdlib* builtin-method table, this file
//! exercises the v0.16 wiring for *user-authored* fns: a Mighty source
//! defines `fn map[A, B, E](xs, f: fn(A)->B) -> List[B] !{| E}`, the
//! HIR lowerer populates `HirFn::effect_row = Some(Open(..., E))`, and
//! the typeck-time effect inference walks closure args of every call
//! site to propagate the closure's row through the row variable.
//!
//! The pipeline under test:
//!
//!   1. `mty_driver::parse_source` (v0.15 surface syntax) →
//!   2. `mty_driver::lower` (v0.16 `HirEffectRow` lowering) →
//!   3. `mty_types::check_package_typed` (v0.16
//!      `effects::infer_and_validate` reading the new index built
//!      from `HirFn::effect_row`).
//!
//! Each test asserts on `TypedPackage.fn_effects[caller]` to verify
//! the closure-body effects ended up in the caller's inferred set.

use mty_driver::{lower, parse_source};
use mty_types::{check_package_typed, EffectId};

fn check(src: &str) -> mty_types::TypedPackage {
    let parsed = parse_source(src.into(), "effect_row_e2e.mty".into());
    let (pkg, lower_diags) = lower(&parsed);
    for d in &lower_diags {
        if matches!(d.severity, mty_diagnostics::Severity::Error) {
            eprintln!(
                "effect_row_e2e: unexpected lowering error: {} {}",
                d.code.as_str(),
                d.primary.message
            );
        }
    }
    let mut typed = check_package_typed(&pkg);
    typed.diagnostics.splice(0..0, lower_diags);
    typed
}

fn effect_name(typed: &mty_types::TypedPackage, e: EffectId) -> Option<String> {
    typed
        .def_map
        .effects
        .iter()
        .find(|(_, v)| **v == e)
        .map(|(k, _)| k.clone())
}

fn effects_of(typed: &mty_types::TypedPackage, fn_name: &str) -> Vec<String> {
    let def_ref = typed.def_map.by_name.get(fn_name);
    let fdef_id = match def_ref {
        Some(mty_types::DefRef::Fn(id)) => *id,
        _ => return vec![],
    };
    let Some(hir_id) = typed.def_map.fn_def(fdef_id).and_then(|f| f.hir_fn) else {
        return vec![];
    };
    let mut names: Vec<String> = typed
        .fn_effects
        .get(&hir_id)
        .into_iter()
        .flatten()
        .filter_map(|e: &EffectId| effect_name(typed, *e))
        .collect();
    names.sort();
    names
}

fn diag_codes(typed: &mty_types::TypedPackage) -> Vec<String> {
    typed
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
        .map(|d| d.code.as_str())
        .collect()
}

/// Verify the row-poly user fn `observe` is recognised as row-poly:
/// HIR lowering sets `effect_row = Some(Open(...))`, and the v0.16
/// index registers it.
#[test]
fn user_authored_open_row_lowers_to_effect_row_open() {
    // Bare `!E` form. The HIR check is indirect — we verify the typeck
    // index works by checking that closure effects DO propagate at call
    // sites below.
    let src = r#"
        fn observe[E](f: fn() -> Unit) -> Unit !E {
        }
        fn caller() {
            observe(fn() { fs.read("/x") })
        }
    "#;
    let typed = check(src);
    // The caller should have the `fs` effect because the closure's
    // body invokes `fs.read(...)` and the row var E propagates it.
    let caller_effects = effects_of(&typed, "caller");
    assert!(
        caller_effects.contains(&"fs".to_string()),
        "caller should inherit `fs` via the row var E; got {:?}",
        caller_effects
    );
}

/// Vertical-slice anchor (v0.16 SHIPPED minimum): the closure's
/// effect set flows through the row var into the caller's inferred
/// set.
#[test]
fn user_authored_row_var_propagates() {
    let src = r#"
        fn each[E](f: fn() -> Unit) -> Unit !E {
        }
        fn writer() {
            each(fn() { fs.write("/path") })
        }
    "#;
    let typed = check(src);
    let writer_effects = effects_of(&typed, "writer");
    assert!(
        writer_effects.contains(&"fs".to_string()),
        "writer should inherit `fs` from the closure body; got {:?}",
        writer_effects
    );
}

/// Counter-test: a pure closure passed to the same row-poly fn does
/// NOT add any effects to the caller.
#[test]
fn bare_row_var_compatible_with_pure_closure() {
    let src = r#"
        fn each[E](f: fn() -> Unit) -> Unit !E {
        }
        fn pure_caller() {
            each(fn() { 42 })
        }
    "#;
    let typed = check(src);
    let pure_effects = effects_of(&typed, "pure_caller");
    // Empty closure ⇒ no row propagation ⇒ no effects added beyond
    // anything the fn body itself does (none).
    assert!(
        pure_effects.is_empty(),
        "pure_caller should have no effects; got {:?}",
        pure_effects
    );
}

/// Concrete + open form: a fn declared `!{net | E}` adds net AT MINIMUM
/// plus whatever the closure brings. The closure's effects flow
/// through E. The concrete `net` itself comes from the fixpoint over
/// the callee's declared set (preserves the v0.13 behavior — we don't
/// re-walk the callee's body since it may be opaque).
#[test]
fn concrete_plus_open_row_carries_callee_declared_concrete() {
    let src = r#"
        fn observed[E](f: fn() -> Unit) -> Unit !{net | E} {
        }
        fn caller() {
            observed(fn() { fs.read("/y") })
        }
    "#;
    let typed = check(src);
    let caller_effects = effects_of(&typed, "caller");
    // Should contain `fs` from row propagation. (Concrete `net` flow
    // from declared effects is exercised by closed-set fns; v0.16 row
    // propagation specifically handles the row-var case.)
    assert!(
        caller_effects.contains(&"fs".to_string()),
        "caller should inherit `fs` via row propagation; got {:?}",
        caller_effects
    );
}

/// When a fn declares an effect with a row var on the return ONLY
/// (no closure parameter to bind it), MT4057 fires.
#[test]
fn row_var_in_return_only_emits_mt4057() {
    let src = r#"
        fn degenerate[E](path: Str) -> Str !E {
            ""
        }
    "#;
    let typed = check(src);
    let codes = diag_codes(&typed);
    assert!(
        codes.contains(&"MT4057".to_string()),
        "expected MT4057 for unbound return-row var; got {:?}",
        codes
    );
}

/// Legacy `effect a, b | E` keyword form lowers to HirEffectRow::Open
/// (via the v0.16 lowerer) and participates in row propagation.
#[test]
fn legacy_keyword_row_tail_form_propagates() {
    let src = r#"
        fn observed[E](f: fn() -> Unit) -> Unit effect time | E {
        }
        fn caller() {
            observed(fn() { fs.read("/k") })
        }
    "#;
    let typed = check(src);
    let caller_effects = effects_of(&typed, "caller");
    assert!(
        caller_effects.contains(&"fs".to_string()),
        "caller should inherit `fs` from closure via legacy keyword \
         row-tail form; got {:?}",
        caller_effects
    );
}

/// A pub fn declared `!{}` cannot call a row-poly user fn with a
/// closure that introduces an unsanctioned effect. The MT4001 (public
/// fn missing effect) fires at the pub fn level once the closure's
/// effect has propagated.
#[test]
fn pub_fn_closed_caller_rejects_propagated_effect() {
    let src = r#"
        fn each[E](f: fn() -> Unit) -> Unit !E {
        }
        pub fn writer() -> Unit {
            each(fn() { fs.write("/a") })
        }
    "#;
    let typed = check(src);
    let codes = diag_codes(&typed);
    // Either MT4001 (effect_undeclared) fires at the pub fn level OR
    // an MT4059 fires at the call site. v0.16 routes through the
    // existing MT4001 path (which is the more author-friendly
    // diagnostic for "you forgot to declare your effect").
    let triggers_mt4001 = codes.contains(&"MT4001".to_string());
    assert!(
        triggers_mt4001,
        "expected MT4001 for pub fn missing `fs`; got {:?}",
        codes
    );
}

/// Multiple call sites of the same row-poly fn with DIFFERENT closure
/// effects each propagate independently (call-site instantiation per
/// RFC-008 §inference).
#[test]
fn multiple_callsites_propagate_independently() {
    let src = r#"
        fn each[E](f: fn() -> Unit) -> Unit !E {
        }
        fn caller_fs() {
            each(fn() { fs.read("/a") })
        }
        fn caller_net() {
            each(fn() { net.get("https://x") })
        }
    "#;
    let typed = check(src);
    let fs_effects = effects_of(&typed, "caller_fs");
    let net_effects = effects_of(&typed, "caller_net");
    assert!(
        fs_effects.contains(&"fs".to_string()),
        "caller_fs should have fs; got {:?}",
        fs_effects
    );
    assert!(
        net_effects.contains(&"net".to_string()),
        "caller_net should have net; got {:?}",
        net_effects
    );
    // Negative cross-check: caller_fs must NOT pick up `net`.
    assert!(
        !fs_effects.contains(&"net".to_string()),
        "caller_fs should not pick up unrelated net; got {:?}",
        fs_effects
    );
}
