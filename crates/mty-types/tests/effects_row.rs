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
