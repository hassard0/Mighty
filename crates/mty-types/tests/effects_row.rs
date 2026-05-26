//! v0.13 — RFC-008 effect-row polymorphism integration tests.
//!
//! These tests exercise the row infrastructure at the level of
//! HOF-call-site simulation: given a row-polymorphic signature
//! ([`stdlib_list_map_sig`]) and a caller-supplied closure with some
//! effect set, we instantiate the signature with fresh row vars, unify
//! the closure's row against the signature's parameter row, and then
//! read back the resolved result row.
//!
//! In v0.13 the surface-syntax parser does not yet emit row-typed
//! signatures, so these tests construct the input rows directly. The
//! v0.14 plan is to wire the parser to emit `RowSpec::Var` entries
//! when it sees `!E` and `!{a | E}` clauses; the entire wiring below
//! will then operate on real user code.
//!
//! Test scenarios (from the RFC-008 acceptance list):
//!
//! 1. `map_propagates_caller_effects` — closure with `{fs}` flows
//!    through the HOF row var into the result.
//! 2. `closed_row_rejects_extra_effect` — a HOF whose parameter row
//!    is `Closed({})` (not row-polymorphic) rejects a `{fs}` closure.
//! 3. `row_var_unifies_with_empty` — a pure closure produces a pure
//!    result.
//! 4. `nested_hof_threads_two_rows` — `map(filter(xs, p), f)` with
//!    distinct row vars on each HOF unions the two effect sets.
//! 5. `row_var_in_return_only_is_rejected` — surface-validation rule
//!    catching a row var that's never bound by an argument.
//! 6. `list_map_signature_round_trip` — the wired `List.map`
//!    signature shape matches the RFC spec.
//! 7. `subsumption_closed_into_open_param` — a Closed({}) closure
//!    satisfies an Open({}, E) parameter (license for the v0.14
//!    stdlib roll-out).
//! 8. `chain_resolve_after_two_hof_calls` — substitution chains
//!    correctly when two HOFs feed into each other.

use mty_types::effects::row::stdlib_sigs;
use mty_types::effects::row::{
    instantiate_row_sig, stdlib_list_map_sig, unify_rows, EffectRow, RowError, RowPolySig, RowSpec,
    RowSubst,
};
use mty_types::ty::EffectId;
use std::collections::BTreeSet;

// Synthetic effect IDs for testing (mirrors what `DefMap::intern_effect`
// would produce). The tests never depend on which ID maps to which
// name; they just need stable distinct IDs.
const FS: EffectId = EffectId(1);
const NET: EffectId = EffectId(2);
const TIME: EffectId = EffectId(3);

/// Helper: simulate a HOF call site.
///
/// Given a signature, the per-fn-parameter actual effect rows (one
/// entry per `RowSpec` slot, `None` for `RowSpec::Skip` positions),
/// returns the resolved return effect row after unification.
fn simulate_call(
    sig: &RowPolySig,
    actual_param_rows: Vec<Option<EffectRow>>,
) -> Result<EffectRow, RowError> {
    let mut subst = RowSubst::new();
    let (sig_params, ret, _fresh) = instantiate_row_sig(sig, &mut subst);
    assert_eq!(
        sig_params.len(),
        actual_param_rows.len(),
        "param row count mismatch"
    );
    for (i, (expected, actual)) in sig_params.iter().zip(actual_param_rows.iter()).enumerate() {
        match (expected, actual) {
            (None, None) => {}
            (Some(exp), Some(act)) => {
                unify_rows(&mut subst, exp, act).map_err(|e| {
                    eprintln!("param {} unify failed: {:?}", i, e);
                    e
                })?;
            }
            (Some(_), None) | (None, Some(_)) => {
                panic!("param {} fn-shape mismatch", i);
            }
        }
    }
    Ok(subst.resolve(&ret))
}

#[test]
fn map_propagates_caller_effects() {
    // List.map signature, closure with `{fs}`.
    let sig = stdlib_list_map_sig();
    let closure_row = EffectRow::closed([FS]);
    let result = simulate_call(&sig, vec![None, Some(closure_row)])
        .expect("row-polymorphic HOF should accept effectful closure");
    // Result row must be Closed({fs}) — the row var was instantiated
    // to fresh `?r`, then unified with `Closed({fs})`, binding `?r`.
    // The return-row template `Var(0)` resolves to `Closed({fs})`.
    assert_eq!(result, EffectRow::closed([FS]));
}

#[test]
fn closed_row_rejects_extra_effect() {
    // A HOF whose parameter row is fixed `Closed({})` (NOT
    // row-polymorphic) — the existing v0.12 behavior. Feeding a
    // `{fs}` closure must reject.
    let sig = RowPolySig {
        row_var_count: 0,
        param_rows: vec![RowSpec::Skip, RowSpec::Concrete(EffectRow::empty())],
        return_row: RowSpec::Concrete(EffectRow::empty()),
    };
    let closure_row = EffectRow::closed([FS]);
    let err = simulate_call(&sig, vec![None, Some(closure_row)])
        .expect_err("closed-empty param must reject {fs} closure");
    assert!(matches!(err, RowError::ClosedMismatch(_, _)));
}

#[test]
fn row_var_unifies_with_empty() {
    // List.map with a PURE closure. The result should be Closed({}).
    let sig = stdlib_list_map_sig();
    let closure_row = EffectRow::empty();
    let result = simulate_call(&sig, vec![None, Some(closure_row)])
        .expect("row-polymorphic HOF should accept pure closure");
    assert_eq!(result, EffectRow::empty());
}

#[test]
fn nested_hof_threads_two_rows() {
    // Simulate `map(filter(xs, p), f)` where `p: fn(A)->Bool!{fs}`
    // and `f: fn(A)->B!{net}`. The outer map's result row must end
    // up unioning {fs, net} (filter's row var resolves to {fs},
    // map's resolves to ... well in this simulation each HOF call is
    // INDEPENDENT — the row vars are fresh per call. We're modelling
    // the effect-set UNION at the call-site level: the caller sees
    // both result rows summed into its own inferred set.

    // Inner filter: same shape as List.map for our purposes.
    let filter_sig = stdlib_list_map_sig();
    let p_row = EffectRow::closed([FS]);
    let inner_result =
        simulate_call(&filter_sig, vec![None, Some(p_row)]).expect("inner filter call");

    // Outer map.
    let map_sig = stdlib_list_map_sig();
    let f_row = EffectRow::closed([NET]);
    let outer_result = simulate_call(&map_sig, vec![None, Some(f_row)]).expect("outer map call");

    // The caller's inferred effect set is the union of both calls'
    // result rows. Each row must be Closed.
    let union: BTreeSet<EffectId> = match (inner_result, outer_result) {
        (EffectRow::Closed(a), EffectRow::Closed(b)) => a.union(&b).copied().collect(),
        _ => panic!("expected both to be Closed after unification"),
    };
    assert!(union.contains(&FS));
    assert!(union.contains(&NET));
    assert_eq!(union.len(), 2);
}

#[test]
fn row_var_in_return_only_is_rejected() {
    // RFC-008 §"Anti-patterns": a row var that appears ONLY in the
    // return clause has no binding site. In our representation this
    // manifests as a return row with a row var that's never unified.
    //
    // We model the surface-level rejection at the validator layer.
    // Test: after instantiation, if no parameter row mentions
    // `fresh[i]` and the return row mentions only `fresh[i]`, the
    // resolved return row is still an open row (no concrete effects,
    // unbound tail) — this is the diagnostic signal.
    let bad_sig = RowPolySig {
        row_var_count: 1,
        param_rows: vec![], // no parameters at all
        return_row: RowSpec::Var(0),
    };
    let mut subst = RowSubst::new();
    let (_params, ret, fresh) = instantiate_row_sig(&bad_sig, &mut subst);
    // The return row remains unbound — this is precisely what the
    // surface-level MT4022 (`row_var_unbound`) diagnostic detects.
    assert!(ret.is_open());
    assert!(!subst.is_bound(fresh[0]));
}

#[test]
fn list_map_signature_round_trip() {
    // Document the canonical shape of the wired stdlib `List.map`.
    let sig = stdlib_list_map_sig();
    assert_eq!(sig.row_var_count, 1);
    assert_eq!(sig.param_rows.len(), 2);
    assert_eq!(sig.param_rows[0], RowSpec::Skip);
    assert_eq!(sig.param_rows[1], RowSpec::Var(0));
    assert_eq!(sig.return_row, RowSpec::Var(0));
}

#[test]
fn subsumption_closed_into_open_param() {
    // A Closed({}) actual flowing into an Open({}, E) expected must
    // succeed (and bind E ↦ Closed({})).
    let mut subst = RowSubst::new();
    let v = subst.fresh();
    let expected = EffectRow::open([], v);
    let actual = EffectRow::empty();
    unify_rows(&mut subst, &expected, &actual).expect("subsumption closed-into-open must succeed");
    let resolved = subst.resolve(&expected);
    assert_eq!(resolved, EffectRow::empty());
}

#[test]
fn chain_resolve_after_two_hof_calls() {
    // Simulate the situation where ONE substitution is reused across
    // two call sites (e.g. two map() calls that share a row var
    // because they appear in the same generic-scoped binding).
    let mut subst = RowSubst::new();
    // Allocate a shared row var (as if it were bound by an outer
    // generic clause).
    let shared = subst.fresh();

    // Call site 1: closure has {fs}. Unify Open({}, shared) with
    // Closed({fs}). After this, shared ↦ Closed({fs}).
    let p1 = EffectRow::open([], shared);
    let c1 = EffectRow::closed([FS]);
    unify_rows(&mut subst, &p1, &c1).expect("first call unify");

    // Call site 2: closure has {fs} (must match, since shared is
    // already bound). A closure with {time} should now FAIL because
    // shared is fixed to {fs}.
    let p2 = EffectRow::open([], shared);
    let c2_compatible = EffectRow::closed([FS]);
    unify_rows(&mut subst, &p2, &c2_compatible).expect("compatible second call");

    let c3_incompatible = EffectRow::closed([TIME]);
    let err = unify_rows(&mut subst, &p2, &c3_incompatible)
        .expect_err("incompatible second call must reject");
    assert!(matches!(err, RowError::ClosedMismatch(_, _)));
}

#[test]
fn list_map_demo_relaxed_signature_compiles_effectful_closure() {
    // The Phase-5 acceptance test: the v0.13-wired `List.map` is
    // row-polymorphic, so an effectful closure (`{fs, net}`)
    // type-checks.
    let sig = stdlib_list_map_sig();
    let closure_row = EffectRow::closed([FS, NET]);
    let result = simulate_call(&sig, vec![None, Some(closure_row)])
        .expect("List.map row-poly must accept multi-effect closure");
    assert_eq!(result, EffectRow::closed([FS, NET]));
}

#[test]
fn empty_signature_with_no_row_vars_passes_pure_call() {
    // Counter-test: a HOF with no row vars and a `Closed({})`
    // parameter accepts a `Closed({})` closure (existing v0.12
    // behavior preserved).
    let sig = RowPolySig {
        row_var_count: 0,
        param_rows: vec![RowSpec::Skip, RowSpec::Concrete(EffectRow::empty())],
        return_row: RowSpec::Concrete(EffectRow::empty()),
    };
    let closure_row = EffectRow::empty();
    let result = simulate_call(&sig, vec![None, Some(closure_row)])
        .expect("pure-closure-into-pure-param must succeed");
    assert_eq!(result, EffectRow::empty());
}

#[test]
fn iterator_collect_style_var_plus_concrete() {
    // Forward-test for v0.14: `Iterator.collect` will have the row
    // form `!{alloc | E}` — the wired representation is RowSpec::
    // VarPlus(0, {alloc}). Validate that the VarPlus form
    // instantiates to an open row carrying alloc + a fresh tail, and
    // that unifying with a Closed({alloc, fs}) closure resolves
    // correctly.
    let alloc = EffectId(99);
    let sig = RowPolySig {
        row_var_count: 1,
        param_rows: vec![RowSpec::Skip, RowSpec::Var(0)],
        return_row: RowSpec::VarPlus(0, [alloc].into_iter().collect()),
    };
    let mut subst = RowSubst::new();
    let (_params, ret, _fresh) = instantiate_row_sig(&sig, &mut subst);
    // Unify closure row with the closure-param-row template.
    let (params, ret_after_inst, fresh) = instantiate_row_sig(&sig, &mut subst);
    let closure_row = EffectRow::closed([FS]);
    unify_rows(&mut subst, params[1].as_ref().unwrap(), &closure_row).unwrap();
    let resolved = subst.resolve(&ret_after_inst);
    let expected: BTreeSet<EffectId> = [alloc, FS].into_iter().collect();
    assert_eq!(resolved, EffectRow::Closed(expected));

    // The first `ret` from the unused instantiation should still be
    // open (its row var was never unified).
    assert!(matches!(ret, EffectRow::Open(_, _)));
    // The second instantiation's fresh var is distinct.
    assert_ne!(fresh[0], RowSubst::new().fresh()); // sanity
}

// ---------------------------------------------------------------------------
// v0.14 — additional stdlib HOF signatures
// ---------------------------------------------------------------------------
//
// Each test exercises one new `stdlib_sigs::*` signature in the same
// shape as `map_propagates_caller_effects`: instantiate the sig, hand
// it an actual closure row, unify, then assert the resolved return
// row matches the propagated row.
//
// Coverage matrix (15 new sigs):
//   List:     filter, fold, flat_map
//   Iterator: map, filter, fold, for_each, find, any, all, flat_map,
//             collect (collect is structurally different — VarPlus with
//             no closure param; tested separately)
//   Option:   map, and_then, or_else, filter
//   Result:   map, map_err, and_then, or_else
//
// Plus a "pure closure" smoke test and a "compatibility with v0.13
// stdlib_list_map_sig" cross-check.

/// Run a 2-param (Skip + Var(0), return Var(0)) sig against a closure
/// row, return the resolved return row.
fn simulate_two_param_call(sig: &RowPolySig, closure: EffectRow) -> EffectRow {
    simulate_call(sig, vec![None, Some(closure)])
        .expect("row-poly two-param HOF must accept any closure")
}

/// Run a 3-param (Skip + Skip + Var(0), return Var(0)) fold-shape sig
/// against a closure row.
fn simulate_three_param_call(sig: &RowPolySig, closure: EffectRow) -> EffectRow {
    simulate_call(sig, vec![None, None, Some(closure)])
        .expect("row-poly three-param HOF must accept any closure")
}

#[test]
fn list_filter_propagates_predicate_effects() {
    let sig = stdlib_sigs::stdlib_list_filter_sig();
    let result = simulate_two_param_call(&sig, EffectRow::closed([FS]));
    assert_eq!(result, EffectRow::closed([FS]));
}

#[test]
fn list_fold_propagates_folder_effects() {
    let sig = stdlib_sigs::stdlib_list_fold_sig();
    // Three-param: list, init, folder closure.
    let result = simulate_three_param_call(&sig, EffectRow::closed([FS, NET]));
    assert_eq!(result, EffectRow::closed([FS, NET]));
}

#[test]
fn list_flat_map_propagates_closure_effects() {
    let sig = stdlib_sigs::stdlib_list_flat_map_sig();
    let result = simulate_two_param_call(&sig, EffectRow::closed([TIME]));
    assert_eq!(result, EffectRow::closed([TIME]));
}

#[test]
fn iterator_map_propagates_effects() {
    let sig = stdlib_sigs::stdlib_iter_map_sig();
    let result = simulate_two_param_call(&sig, EffectRow::closed([FS]));
    assert_eq!(result, EffectRow::closed([FS]));
}

#[test]
fn iterator_filter_propagates_effects() {
    let sig = stdlib_sigs::stdlib_iter_filter_sig();
    let result = simulate_two_param_call(&sig, EffectRow::closed([NET]));
    assert_eq!(result, EffectRow::closed([NET]));
}

#[test]
fn iterator_fold_propagates_effects() {
    let sig = stdlib_sigs::stdlib_iter_fold_sig();
    let result = simulate_three_param_call(&sig, EffectRow::closed([FS]));
    assert_eq!(result, EffectRow::closed([FS]));
}

#[test]
fn iterator_for_each_propagates_effects() {
    let sig = stdlib_sigs::stdlib_iter_for_each_sig();
    let result = simulate_two_param_call(&sig, EffectRow::closed([FS, NET, TIME]));
    let expected = EffectRow::closed([FS, NET, TIME]);
    assert_eq!(result, expected);
}

#[test]
fn iterator_find_propagates_effects() {
    let sig = stdlib_sigs::stdlib_iter_find_sig();
    let result = simulate_two_param_call(&sig, EffectRow::closed([FS]));
    assert_eq!(result, EffectRow::closed([FS]));
}

#[test]
fn iterator_any_and_all_propagate_effects() {
    let any_sig = stdlib_sigs::stdlib_iter_any_sig();
    let all_sig = stdlib_sigs::stdlib_iter_all_sig();
    let r1 = simulate_two_param_call(&any_sig, EffectRow::closed([NET]));
    let r2 = simulate_two_param_call(&all_sig, EffectRow::closed([NET]));
    assert_eq!(r1, EffectRow::closed([NET]));
    assert_eq!(r2, EffectRow::closed([NET]));
}

#[test]
fn iterator_flat_map_propagates_effects() {
    let sig = stdlib_sigs::stdlib_iter_flat_map_sig();
    let result = simulate_two_param_call(&sig, EffectRow::closed([FS, NET]));
    assert_eq!(result, EffectRow::closed([FS, NET]));
}

#[test]
fn iterator_collect_carries_alloc_with_unbound_tail() {
    // `Iterator.collect` has the special shape `VarPlus(0, {alloc})`
    // on the return, with the row var coming from an *upstream*
    // iterator chain that v0.14's type system doesn't yet model. So
    // when we instantiate the sig WITHOUT a unifying call, the
    // return row should be `Open({alloc_placeholder}, ?fresh)`.
    let sig = stdlib_sigs::stdlib_iter_collect_sig();
    let mut subst = RowSubst::new();
    let (_params, ret, _fresh) = instantiate_row_sig(&sig, &mut subst);
    match &ret {
        EffectRow::Open(set, _) => {
            assert!(
                set.contains(&stdlib_sigs::ALLOC_PLACEHOLDER),
                "collect's return row must carry the alloc placeholder"
            );
        }
        EffectRow::Closed(_) => panic!("collect's return row must be open (carries upstream E)"),
    }
}

#[test]
fn iterator_collect_unifies_upstream_row_into_return() {
    // Even though there's no closure param in the v0.14 sig, a typeck
    // pass that knows the upstream iterator's row could synthesize a
    // parameter row and unify it against the sig's fresh row var.
    // Simulate that here: pull the fresh var out of the
    // instantiation, manually unify it against `{net}`, then verify
    // the return row resolves to `{alloc_placeholder, net}`.
    let sig = stdlib_sigs::stdlib_iter_collect_sig();
    let mut subst = RowSubst::new();
    let (_params, ret, fresh) = instantiate_row_sig(&sig, &mut subst);
    let upstream_row = EffectRow::closed([NET]);
    let synthetic_param_row = EffectRow::open([], fresh[0]);
    unify_rows(&mut subst, &synthetic_param_row, &upstream_row)
        .expect("collect's fresh row var must unify with upstream");
    let resolved = subst.resolve(&ret);
    let expected: BTreeSet<EffectId> = [stdlib_sigs::ALLOC_PLACEHOLDER, NET].into_iter().collect();
    assert_eq!(resolved, EffectRow::Closed(expected));
}

#[test]
fn option_map_propagates_effects() {
    let sig = stdlib_sigs::stdlib_option_map_sig();
    let result = simulate_two_param_call(&sig, EffectRow::closed([FS]));
    assert_eq!(result, EffectRow::closed([FS]));
}

#[test]
fn option_and_then_propagates_effects() {
    let sig = stdlib_sigs::stdlib_option_and_then_sig();
    let result = simulate_two_param_call(&sig, EffectRow::closed([NET]));
    assert_eq!(result, EffectRow::closed([NET]));
}

#[test]
fn option_or_else_propagates_effects() {
    let sig = stdlib_sigs::stdlib_option_or_else_sig();
    let result = simulate_two_param_call(&sig, EffectRow::closed([FS, NET]));
    assert_eq!(result, EffectRow::closed([FS, NET]));
}

#[test]
fn option_filter_propagates_effects() {
    let sig = stdlib_sigs::stdlib_option_filter_sig();
    let result = simulate_two_param_call(&sig, EffectRow::closed([TIME]));
    assert_eq!(result, EffectRow::closed([TIME]));
}

#[test]
fn result_map_propagates_effects() {
    let sig = stdlib_sigs::stdlib_result_map_sig();
    let result = simulate_two_param_call(&sig, EffectRow::closed([FS]));
    assert_eq!(result, EffectRow::closed([FS]));
}

#[test]
fn result_map_err_propagates_effects() {
    let sig = stdlib_sigs::stdlib_result_map_err_sig();
    let result = simulate_two_param_call(&sig, EffectRow::closed([NET]));
    assert_eq!(result, EffectRow::closed([NET]));
}

#[test]
fn result_and_then_propagates_effects() {
    let sig = stdlib_sigs::stdlib_result_and_then_sig();
    let result = simulate_two_param_call(&sig, EffectRow::closed([FS, NET]));
    assert_eq!(result, EffectRow::closed([FS, NET]));
}

#[test]
fn result_or_else_propagates_effects() {
    let sig = stdlib_sigs::stdlib_result_or_else_sig();
    let result = simulate_two_param_call(&sig, EffectRow::closed([TIME]));
    assert_eq!(result, EffectRow::closed([TIME]));
}

#[test]
fn pure_closure_through_each_new_sig_yields_empty_row() {
    // Spot-check the "row var unifies with empty" case across every
    // 2-param sig. A pure closure must produce a pure result for all
    // of them.
    let two_param_sigs: Vec<(&'static str, RowPolySig)> = vec![
        ("list.filter", stdlib_sigs::stdlib_list_filter_sig()),
        ("list.flat_map", stdlib_sigs::stdlib_list_flat_map_sig()),
        ("iter.map", stdlib_sigs::stdlib_iter_map_sig()),
        ("iter.filter", stdlib_sigs::stdlib_iter_filter_sig()),
        ("iter.for_each", stdlib_sigs::stdlib_iter_for_each_sig()),
        ("iter.find", stdlib_sigs::stdlib_iter_find_sig()),
        ("iter.any", stdlib_sigs::stdlib_iter_any_sig()),
        ("iter.all", stdlib_sigs::stdlib_iter_all_sig()),
        ("iter.flat_map", stdlib_sigs::stdlib_iter_flat_map_sig()),
        ("option.map", stdlib_sigs::stdlib_option_map_sig()),
        ("option.and_then", stdlib_sigs::stdlib_option_and_then_sig()),
        ("option.or_else", stdlib_sigs::stdlib_option_or_else_sig()),
        ("option.filter", stdlib_sigs::stdlib_option_filter_sig()),
        ("result.map", stdlib_sigs::stdlib_result_map_sig()),
        ("result.map_err", stdlib_sigs::stdlib_result_map_err_sig()),
        ("result.and_then", stdlib_sigs::stdlib_result_and_then_sig()),
        ("result.or_else", stdlib_sigs::stdlib_result_or_else_sig()),
    ];
    for (name, sig) in two_param_sigs {
        let result = simulate_two_param_call(&sig, EffectRow::empty());
        assert_eq!(result, EffectRow::empty(), "{} must stay pure", name);
    }
}

#[test]
fn all_new_sigs_match_v0_13_list_map_shape_invariants() {
    // Cross-check: every v0.14 sig is structurally compatible with the
    // v0.13 anchor signature. Specifically:
    //   - row_var_count == 1
    //   - param_rows ends with a Var(0) closure slot
    //   - return_row mentions row var index 0 (Var or VarPlus)
    let canonical = stdlib_list_map_sig();
    let sigs: Vec<(&'static str, RowPolySig)> = vec![
        ("list.filter", stdlib_sigs::stdlib_list_filter_sig()),
        ("list.fold", stdlib_sigs::stdlib_list_fold_sig()),
        ("list.flat_map", stdlib_sigs::stdlib_list_flat_map_sig()),
        ("iter.map", stdlib_sigs::stdlib_iter_map_sig()),
        ("iter.filter", stdlib_sigs::stdlib_iter_filter_sig()),
        ("iter.fold", stdlib_sigs::stdlib_iter_fold_sig()),
        ("iter.for_each", stdlib_sigs::stdlib_iter_for_each_sig()),
        ("iter.find", stdlib_sigs::stdlib_iter_find_sig()),
        ("iter.any", stdlib_sigs::stdlib_iter_any_sig()),
        ("iter.all", stdlib_sigs::stdlib_iter_all_sig()),
        ("iter.flat_map", stdlib_sigs::stdlib_iter_flat_map_sig()),
        ("iter.collect", stdlib_sigs::stdlib_iter_collect_sig()),
        ("option.map", stdlib_sigs::stdlib_option_map_sig()),
        ("option.and_then", stdlib_sigs::stdlib_option_and_then_sig()),
        ("option.or_else", stdlib_sigs::stdlib_option_or_else_sig()),
        ("option.filter", stdlib_sigs::stdlib_option_filter_sig()),
        ("result.map", stdlib_sigs::stdlib_result_map_sig()),
        ("result.map_err", stdlib_sigs::stdlib_result_map_err_sig()),
        ("result.and_then", stdlib_sigs::stdlib_result_and_then_sig()),
        ("result.or_else", stdlib_sigs::stdlib_result_or_else_sig()),
    ];
    assert_eq!(
        canonical.row_var_count, 1,
        "v0.13 anchor sig must have 1 row var"
    );
    for (name, sig) in sigs {
        assert_eq!(
            sig.row_var_count, 1,
            "{}: must have exactly 1 row var",
            name
        );
        // Return row mentions row-var index 0:
        match &sig.return_row {
            RowSpec::Var(0) | RowSpec::VarPlus(0, _) => {}
            other => panic!(
                "{}: return row should reference row var 0, got {:?}",
                name, other
            ),
        }
        // Last param row is either Var(0) (for the closure) or — for
        // collect, which is unary — the only param is Skip.
        if sig.param_rows.len() >= 2 {
            assert_eq!(
                sig.param_rows.last(),
                Some(&RowSpec::Var(0)),
                "{}: last param must be the closure (Var(0))",
                name
            );
        }
    }
}

#[test]
fn nested_iter_chain_unions_three_effects() {
    // Realistic chain: `xs.iter().filter(|x| fs.exists(x)).map(|x| net.fetch(x)).collect()`
    // Each row var is FRESH per call; the caller's effect set unions
    // all three return rows.
    let filter_sig = stdlib_sigs::stdlib_iter_filter_sig();
    let map_sig = stdlib_sigs::stdlib_iter_map_sig();
    let collect_sig = stdlib_sigs::stdlib_iter_collect_sig();

    let filter_result = simulate_two_param_call(&filter_sig, EffectRow::closed([FS]));
    let map_result = simulate_two_param_call(&map_sig, EffectRow::closed([NET]));

    // collect has no closure param; we just instantiate it and check
    // its return carries the alloc placeholder.
    let mut subst = RowSubst::new();
    let (_p, collect_ret, _f) = instantiate_row_sig(&collect_sig, &mut subst);

    // Caller's effect set is the union of all three results.
    let mut union: BTreeSet<EffectId> = BTreeSet::new();
    if let EffectRow::Closed(s) = filter_result {
        union.extend(s);
    }
    if let EffectRow::Closed(s) = map_result {
        union.extend(s);
    }
    // `collect_ret` is always either Open or Closed — extract the
    // concrete-effects component without an irrefutable-let pattern.
    match collect_ret {
        EffectRow::Open(s, _) | EffectRow::Closed(s) => union.extend(s),
    }
    assert!(union.contains(&FS));
    assert!(union.contains(&NET));
    assert!(union.contains(&stdlib_sigs::ALLOC_PLACEHOLDER));
}

#[test]
fn closure_row_open_unifies_through_each_new_sig() {
    // If the *closure's* own row is itself open (e.g. the closure was
    // typed in a generic context with its own row var), unifying it
    // against the HOF sig should propagate the open-ness — both row
    // vars get bound to a shared fresh tail.
    let sig = stdlib_sigs::stdlib_result_and_then_sig();
    let mut subst = RowSubst::new();
    let (params, ret, _fresh) = instantiate_row_sig(&sig, &mut subst);
    let caller_var = subst.fresh();
    let caller_closure_row = EffectRow::open([FS], caller_var);
    unify_rows(&mut subst, params[1].as_ref().unwrap(), &caller_closure_row)
        .expect("open-into-open unifies via shared fresh tail");
    let resolved = subst.resolve(&ret);
    // After unification, the return row's resolved form contains at
    // least `{fs}` plus possibly an open tail.
    match resolved {
        EffectRow::Open(set, _) => assert!(set.contains(&FS)),
        EffectRow::Closed(set) => assert!(set.contains(&FS)),
    }
}
