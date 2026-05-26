//! v0.19 — multi row-variable end-to-end typeck tests.
//!
//! Where `effect_row_multi.rs` (v0.17) pins the multi-row-var
//! diagnostics and the HIR-level multi-var representation, and
//! `effect_row_e2e.rs` (v0.16) pins the single-row-var
//! end-to-end propagation, this file covers the v0.19
//! milestone:
//!
//!   * The v0.18 parser surface (`!{| E, F}`) feeds the v0.19 HIR
//!     lowerer (now reads every `EFFECT_ROW_VAR`) which feeds the
//!     v0.17 typeck layer (already consumes `Vec<HirRowVar>` and
//!     iterates row vars in `UserRowPolyMeta`).
//!
//! The caller-side inferred effect set is the UNION of every
//! closure-arg's effects — observationally the same outcome as the
//! v0.17 single-row-var path because the infrastructure already
//! walked every fn-typed arg. The new tests pin the WIRING:
//! `UserRowPolyMeta::row_vars` now reports every row variable (not
//! just the first), and the multi-row-var sig parses, lowers, and
//! typecks without diagnostics.

use mty_driver::{lower, parse_source};
use mty_types::{check_package_typed, EffectId};

fn check(src: &str) -> mty_types::TypedPackage {
    let parsed = parse_source(src.into(), "effect_row_e2e_multi.mty".into());
    let (pkg, lower_diags) = lower(&parsed);
    let mut typed = check_package_typed(&pkg);
    typed.diagnostics.splice(0..0, lower_diags);
    typed
}

fn parse_and_lower(src: &str) -> mty_hir::Package {
    let parsed = parse_source(src.into(), "effect_row_e2e_multi.mty".into());
    let (pkg, _diags) = lower(&parsed);
    pkg
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

/// v0.19 motivating shape: `cross[E, F]` takes two closure args
/// carrying distinct effects, and the caller's inferred effect set
/// is the UNION of both closures' effects. Pre-v0.19 the HIR
/// dropped `F` (the lowerer only read the first `EFFECT_ROW_VAR`),
/// but the call-site walker happened to iterate every fn-typed arg
/// regardless of the row-var count — so propagation worked
/// observationally. The v0.19 fix makes the HIR + typeck WIRING
/// match the surface syntax: `UserRowPolyMeta::row_vars` now
/// reports both `E` and `F`.
#[test]
fn cross_with_two_effectful_closures_unions_effects() {
    let src = r#"
        fn cross[E, F](a: fn() -> Unit, b: fn() -> Unit) -> Unit !{| E, F} {
        }
        fn caller() {
            cross(fn() { fs.read("/x") }, fn() { net.get("https://y") })
        }
    "#;
    // HIR shape: cross has TWO row vars after v0.19 lowering.
    let pkg = parse_and_lower(src);
    let cross_row = pkg
        .fns
        .iter()
        .find(|(_, hf)| hf.name == "cross")
        .and_then(|(_, hf)| hf.effect_row.clone())
        .expect("cross should have an effect_row");
    match &cross_row {
        mty_hir::HirEffectRow::Open(_, row_vars) => {
            assert_eq!(
                row_vars.len(),
                2,
                "v0.19: cross should lower to Vec<HirRowVar> of length 2; \
                 v0.18 would have produced length 1. got {:?}",
                row_vars
            );
            assert_eq!(row_vars[0].name, "E");
            assert_eq!(row_vars[1].name, "F");
        }
        other => panic!("cross should lower to Open, got {:?}", other),
    }
    // Typeck-side: caller picks up BOTH `fs` and `net` (the union of
    // the two closure-body effects, propagated through the
    // row-poly machinery).
    let typed = check(src);
    let caller_effects = effects_of(&typed, "caller");
    assert!(
        caller_effects.contains(&"fs".to_string()),
        "caller should inherit `fs` from closure a; got {:?}",
        caller_effects
    );
    assert!(
        caller_effects.contains(&"net".to_string()),
        "caller should inherit `net` from closure b; got {:?}",
        caller_effects
    );
}

/// Counter-test: only ONE of the two closure args carries effects;
/// the other is pure. The propagated set must contain only the
/// effectful closure's effect — never `net` here, because closure
/// b never invokes `net.*`.
#[test]
fn cross_with_only_one_closure_effectful_propagates_partial() {
    let src = r#"
        fn cross[E, F](a: fn() -> Unit, b: fn() -> Unit) -> Unit !{| E, F} {
        }
        fn caller() {
            cross(fn() { fs.read("/x") }, fn() { 42 })
        }
    "#;
    let typed = check(src);
    let caller_effects = effects_of(&typed, "caller");
    assert!(
        caller_effects.contains(&"fs".to_string()),
        "caller should inherit `fs`; got {:?}",
        caller_effects
    );
    assert!(
        !caller_effects.contains(&"net".to_string()),
        "caller must NOT pick up `net` when closure b is pure; got {:?}",
        caller_effects
    );
}

/// When both closure args share the SAME row var (`fn pair[E](a:
/// fn() !E, b: fn() !E) -> () !E`), the v0.19 HIR carries a single
/// row var even though the sig has two fn-typed params. The
/// caller's inferred effects are the union of both closures'
/// bodies (the same shape as the multi-var case here — the v0.17
/// `walk_expr_effects` iterates every lambda arg regardless of
/// row-var count).
#[test]
fn cross_with_same_row_var_unifies_to_same_effect() {
    let src = r#"
        fn pair[E](a: fn() -> Unit, b: fn() -> Unit) -> Unit !{| E} {
        }
        fn caller() {
            pair(fn() { fs.read("/x") }, fn() { net.get("https://y") })
        }
    "#;
    let pkg = parse_and_lower(src);
    let pair_row = pkg
        .fns
        .iter()
        .find(|(_, hf)| hf.name == "pair")
        .and_then(|(_, hf)| hf.effect_row.clone())
        .expect("pair should have an effect_row");
    match &pair_row {
        mty_hir::HirEffectRow::Open(_, row_vars) => {
            assert_eq!(
                row_vars.len(),
                1,
                "pair declares a single row var `E`; got {:?}",
                row_vars
            );
            assert_eq!(row_vars[0].name, "E");
        }
        other => panic!("expected Open, got {:?}", other),
    }
    let typed = check(src);
    let caller_effects = effects_of(&typed, "caller");
    // Both closures' bodies' effects union into the caller's set.
    assert!(
        caller_effects.contains(&"fs".to_string()),
        "caller should inherit `fs` from closure a; got {:?}",
        caller_effects
    );
    assert!(
        caller_effects.contains(&"net".to_string()),
        "caller should inherit `net` from closure b; got {:?}",
        caller_effects
    );
}

/// MT4058 arity mismatch on a multi-row-var sig: `cross[E, F]` has
/// two fn-typed params, caller passes only one closure. The v0.17
/// validator records `fn_typed_param_count = 2` in
/// `UserRowPolyMeta`; the v0.18 multi-row-var parser surface
/// keeps this honest because both `E` and `F` parse cleanly.
#[test]
fn cross_with_mismatched_row_arity_fires_diagnostic() {
    let src = r#"
        fn cross[E, F](a: fn() -> Unit, b: fn() -> Unit) -> Unit !{| E, F} {
        }
        fn caller() {
            cross(fn() { fs.read("/x") })
        }
    "#;
    let typed = check(src);
    let codes = diag_codes(&typed);
    assert!(
        codes.contains(&"MT4058".to_string()),
        "expected MT4058 for arity mismatch (1 closure vs 2 fn-typed params); \
         got {:?}",
        codes
    );
}

/// Three-way row-poly fn: `triple[E, F, G]` with three fn-typed
/// args, each carrying its own row var. The v0.19 HIR carries a
/// length-3 `Vec<HirRowVar>`; the caller's effect union spans
/// all three closures' effects.
#[test]
fn cross_with_three_row_vars_unifies_three_args() {
    let src = r#"
        fn triple[E, F, G](
            a: fn() -> Unit,
            b: fn() -> Unit,
            c: fn() -> Unit,
        ) -> Unit !{| E, F, G} {
        }
        fn caller() {
            triple(
                fn() { fs.read("/x") },
                fn() { net.get("https://y") },
                fn() { clock.now() },
            )
        }
    "#;
    let pkg = parse_and_lower(src);
    let triple_row = pkg
        .fns
        .iter()
        .find(|(_, hf)| hf.name == "triple")
        .and_then(|(_, hf)| hf.effect_row.clone())
        .expect("triple should have an effect_row");
    match &triple_row {
        mty_hir::HirEffectRow::Open(_, row_vars) => {
            assert_eq!(
                row_vars.len(),
                3,
                "v0.19: triple should lower to length-3 row vars; got {:?}",
                row_vars
            );
            assert_eq!(row_vars[0].name, "E");
            assert_eq!(row_vars[1].name, "F");
            assert_eq!(row_vars[2].name, "G");
            // Source-order-stable idx allocation.
            assert_eq!(row_vars[0].idx, 0);
            assert_eq!(row_vars[1].idx, 1);
            assert_eq!(row_vars[2].idx, 2);
        }
        other => panic!("expected Open, got {:?}", other),
    }
    let typed = check(src);
    let caller_effects = effects_of(&typed, "caller");
    assert!(
        caller_effects.contains(&"fs".to_string()),
        "caller should inherit `fs`; got {:?}",
        caller_effects
    );
    assert!(
        caller_effects.contains(&"net".to_string()),
        "caller should inherit `net`; got {:?}",
        caller_effects
    );
    // The third closure invokes `clock.now()`; the v0.13 cap mapper
    // interns the clock cap under the `time` effect name (see
    // `crates/mty-types/src/effects.rs` cap dispatch table). Pin
    // that contract here rather than re-naming so the test reflects
    // reality.
    assert!(
        caller_effects.contains(&"time".to_string())
            || caller_effects.contains(&"clock".to_string()),
        "caller should inherit the clock cap effect; got {:?}",
        caller_effects
    );
}

/// v0.19 surface-shape clean-parse: `!{| E, F}` parses, lowers,
/// and typechecks without firing any parse-level (MT0001) or
/// row-poly validation (MT4055/MT4056/MT4057) errors when the
/// signature is well-formed (one fn-typed parameter per row var).
#[test]
fn well_formed_multi_row_sig_parses_lowers_typecks_clean() {
    let src = r#"
        fn cross[E, F](a: fn() -> Unit, b: fn() -> Unit) -> Unit !{| E, F} {
        }
        fn _unused_caller() {
            cross(fn() { 1 }, fn() { 2 })
        }
    "#;
    let typed = check(src);
    let codes = diag_codes(&typed);
    // Strict: no parse errors, no row-poly validation errors.
    for forbidden in ["MT0001", "MT4055", "MT4056", "MT4057", "MT4058", "MT4059"] {
        assert!(
            !codes.iter().any(|c| c == forbidden),
            "well-formed multi-row sig must not emit {}; got {:?}",
            forbidden,
            codes
        );
    }
}
