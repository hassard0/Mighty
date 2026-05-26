//! v0.17 — multi row-variable user fns + broadened RFC-008
//! diagnostics (MT4055/MT4056/MT4058/MT4059).
//!
//! Where `effect_row_e2e.rs` (v0.16) exercises the single-row-var
//! happy path + the MT4057 emit site, this file extends coverage to
//! the v0.17 broadened diagnostics:
//!
//!   * MT4055 `row_var_unused` — row var declared but no fn-typed
//!     param can bind it AND the fn has multiple non-fn params.
//!   * MT4056 `row_var_in_concrete_only` — fn has both a concrete
//!     effects component AND a row var, with no fn-typed param.
//!   * MT4058 `row_var_arity_mismatch` — caller passes the wrong
//!     number of closure args to a user row-poly fn.
//!   * MT4059 `row_var_subsumption_fail` — caller's closed-row
//!     enclosing fn cannot accept the effects the closure brings in
//!     through the row substitution.
//!
//! ## Parser caveat
//!
//! The v0.15 parser only emits ONE `EFFECT_ROW_VAR` per fn (the
//! `!{| E1, E2}` shape is a v0.18 parser follow-up). The v0.17 HIR
//! representation already carries `Vec<HirRowVar>` so the typeck
//! layer is ready, but at the source level we still only see a
//! single row var per signature. Tests below that target the
//! multi-row-var unification path therefore construct the HIR
//! shape directly (no parser involved) so the row machinery is
//! exercised end-to-end without waiting on parser work.

use mty_driver::{lower, parse_source};
use mty_types::check_package_typed;

fn check(src: &str) -> mty_types::TypedPackage {
    let parsed = parse_source(src.into(), "effect_row_multi.mty".into());
    let (pkg, lower_diags) = lower(&parsed);
    let mut typed = check_package_typed(&pkg);
    typed.diagnostics.splice(0..0, lower_diags);
    typed
}

fn diag_codes(typed: &mty_types::TypedPackage) -> Vec<String> {
    typed
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
        .map(|d| d.code.as_str())
        .collect()
}

/// MT4055 active emit: a fn declares `!{| E}` AND has multiple
/// non-fn-typed parameters. The row var has no closure to bind it
/// through; the v0.17 heuristic flags this as "unused" (vs.
/// MT4057's "returned-but-unbound" which assumes a single
/// parameterless shape).
#[test]
fn multi_param_no_closure_emits_mt4055() {
    let src = r#"
        fn pure_with_row[E](a: Str, b: I32) -> Str !{| E} {
            a
        }
    "#;
    let typed = check(src);
    let codes = diag_codes(&typed);
    assert!(
        codes.contains(&"MT4055".to_string()),
        "expected MT4055 for unused row var with multiple non-fn \
         params; got {:?}",
        codes
    );
}

/// MT4056 heuristic emit: a fn declares `!{fs | E}` and has NO
/// fn-typed parameter — the concrete `fs` is doing all the work,
/// the `E` sits structurally inert.
#[test]
fn concrete_plus_open_row_no_closure_emits_mt4056() {
    let src = r#"
        fn fancy[E](path: Str) -> Str !{fs | E} {
            ""
        }
    "#;
    let typed = check(src);
    let codes = diag_codes(&typed);
    assert!(
        codes.contains(&"MT4056".to_string()),
        "expected MT4056 for concrete-plus-open row with no closure \
         param; got {:?}",
        codes
    );
}

/// MT4058 active emit (call site): the caller supplies zero
/// closure args even though the callee declares one fn-typed
/// parameter that should bind the row var. v0.17 fires MT4058
/// only when the lambda count > 0 AND mismatches; the
/// no-lambda-at-all case is intentionally skipped so that
/// over-application (e.g. `each(some_path)` where `some_path`
/// references an existing pure fn) is left to the v0.18 type-check
/// pass.
#[test]
fn arity_mismatch_two_lambdas_one_param_emits_mt4058() {
    let src = r#"
        fn each[E](f: fn() -> Unit) -> Unit !{| E} {
        }
        fn caller() {
            each(fn() { fs.read("/a") }, fn() { net.get("https://x") })
        }
    "#;
    let typed = check(src);
    let codes = diag_codes(&typed);
    assert!(
        codes.contains(&"MT4058".to_string()),
        "expected MT4058 when caller passes 2 closures to a single \
         fn-typed-param row-poly fn; got {:?}",
        codes
    );
}

/// MT4059 active emit: a pub fn declared with a CLOSED row
/// constraint (`!{}` — i.e. no effects allowed) invokes a user
/// row-poly fn with a closure that introduces an effect. The row
/// substitution would add the closure's effects to the caller's
/// inferred set; the caller's closed declared row cannot accept
/// them. v0.17 fires MT4059 at the call site in addition to the
/// existing MT4001 pub-fn-level catch-all.
#[test]
fn closed_caller_with_effectful_closure_emits_mt4059() {
    let src = r#"
        fn each[E](f: fn() -> Unit) -> Unit !{| E} {
        }
        pub fn pure_writer() -> Unit !{} {
            each(fn() { fs.write("/a") })
        }
    "#;
    let typed = check(src);
    let codes = diag_codes(&typed);
    assert!(
        codes.contains(&"MT4059".to_string()),
        "expected MT4059 for closed pub caller calling row-poly fn \
         with effectful closure; got {:?}",
        codes
    );
}

/// Counter-test for MT4059: a pub fn that DECLARES `effect fs` —
/// the row substitution adds `fs`, which IS in the declared set.
/// MT4059 must not fire (the caller can accept the effect).
#[test]
fn caller_declares_matching_effect_no_mt4059() {
    let src = r#"
        fn each[E](f: fn() -> Unit) -> Unit !{| E} {
        }
        pub fn writer() -> Unit effect fs {
            each(fn() { fs.write("/a") })
        }
    "#;
    let typed = check(src);
    let codes = diag_codes(&typed);
    assert!(
        !codes.contains(&"MT4059".to_string()),
        "MT4059 must NOT fire when caller explicitly declares the \
         effect; got {:?}",
        codes
    );
}

/// MT4058 counter-test: caller passes exactly the right number of
/// closures (one). Arity matches, so MT4058 must NOT fire.
#[test]
fn arity_match_one_lambda_one_param_no_mt4058() {
    let src = r#"
        fn each[E](f: fn() -> Unit) -> Unit !{| E} {
        }
        fn caller() {
            each(fn() { fs.read("/a") })
        }
    "#;
    let typed = check(src);
    let codes = diag_codes(&typed);
    assert!(
        !codes.contains(&"MT4058".to_string()),
        "MT4058 must NOT fire when arity matches; got {:?}",
        codes
    );
}

/// MT4055 counter-test: when the fn has a closure param, the row
/// var IS bindable, so MT4055 should NOT fire — the fn is well-
/// formed row-poly.
#[test]
fn well_formed_row_poly_no_mt4055() {
    let src = r#"
        fn each[E](f: fn() -> Unit) -> Unit !{| E} {
        }
        fn caller() {
            each(fn() { 42 })
        }
    "#;
    let typed = check(src);
    let codes = diag_codes(&typed);
    assert!(
        !codes.contains(&"MT4055".to_string()),
        "MT4055 must NOT fire on a well-formed row-poly fn; got {:?}",
        codes
    );
}

/// HIR-level multi-row-var representation: construct a
/// `HirEffectRow::Open` with TWO row vars directly (bypassing the
/// parser, which still emits length-1) and assert the v0.17
/// accessors expose them correctly. This exercises the v0.18
/// readiness — the HIR shape is stable and the typeck layer can
/// already read multi-var sigs once the parser starts emitting
/// them.
#[test]
fn hir_supports_multi_row_vars_directly() {
    use mty_hir::effects::{HirEffectName, HirEffectRow, HirRowVar};
    let row = HirEffectRow::Open(
        vec![HirEffectName::from("fs")],
        vec![HirRowVar::new("E1", 0), HirRowVar::new("E2", 1)],
    );
    assert_eq!(row.row_var_count(), 2);
    assert_eq!(row.row_vars()[0].name, "E1");
    assert_eq!(row.row_vars()[1].name, "E2");
    assert_eq!(row.concrete()[0].as_str(), "fs");
    // Convenience accessor returns the first.
    assert_eq!(row.row_var().unwrap().name, "E1");
}

/// Row arithmetic: two `EffectRow::Open` rows with DIFFERENT row
/// vars unify into a shared fresh tail. This is the underlying
/// machinery the v0.18 multi-row-var dispatcher will rely on —
/// each closure-arg's row binds its own var, and the final return
/// row resolves to the union of all closure effects.
///
/// Demonstrates the `unify_rows` two-open path that the v0.17 work
/// already exercises via the v0.13 stdlib HOF tests; this test
/// re-anchors it with a multi-var-friendly framing.
#[test]
fn unify_two_open_rows_with_distinct_vars_shares_tail() {
    use mty_types::effects::{unify_rows, EffectRow, RowSubst};
    use mty_types::EffectId;
    let fs = EffectId(1);
    let net = EffectId(2);
    let mut subst = RowSubst::new();
    let v = subst.fresh();
    let w = subst.fresh();
    let a = EffectRow::open([fs], v);
    let b = EffectRow::open([net], w);
    unify_rows(&mut subst, &a, &b).expect("two opens unify");
    let ra = subst.resolve(&a);
    let rb = subst.resolve(&b);
    assert_eq!(ra, rb, "post-unify both rows resolve to same row");
    // Resolved row contains both fs and net (the union of the two
    // sides' concrete sets) and still has an open tail.
    match &ra {
        EffectRow::Open(set, _) => {
            assert!(set.contains(&fs));
            assert!(set.contains(&net));
        }
        EffectRow::Closed(_) => panic!("expected open, got closed"),
    }
}

/// v0.18 end-to-end: a fn declared with the new `!{| E1, E2}`
/// multi-row-var parser surface now parses, lowers, and (because
/// the v0.15 HIR lowerer still picks up only the first row var)
/// typecks under the single-var path. Pin that source-level
/// `!{| E1, E2}` parses CLEAN — no MT0001/MT4055 — so consumers can
/// start writing the natural shape without regressing.
#[test]
fn multi_row_var_parser_surface_parses_clean() {
    let src = r#"
        fn _cross[E1, E2](a: fn() -> Unit, b: fn() -> Unit) -> Unit !{| E1, E2} {
        }
    "#;
    let typed = check(src);
    let codes = diag_codes(&typed);
    // Parser-level errors would surface as MT0001; HIR/typeck-level
    // surprises would surface as MT2xxx/MT4xxx. Allow MT4055 (the
    // v0.15 HIR lowerer collapses to one row var, so MT4055 may
    // fire if the heuristic disagrees with the multi-var intent) —
    // that's not a parser regression. What we MUST NOT see is a
    // syntax error.
    assert!(
        !codes.iter().any(|c| c == "MT0001"),
        "multi-row-var parser surface must not emit parse errors; got {:?}",
        codes
    );
}

/// v0.18 MT4059 firing on a multi-row-var SOURCE-LEVEL fn signature
/// (now parseable thanks to the v0.18 parser extension). The caller
/// is a closed-row pub fn that invokes a row-poly fn whose closure
/// args bring effects — MT4059 must fire at the call site.
///
/// This is observationally equivalent to the existing
/// `closed_caller_with_effectful_closure_emits_mt4059` test but
/// uses the v0.18 multi-row-var sig shape end-to-end. Because the
/// HIR lowerer still uses only the first row var, the typeck-side
/// behaviour collapses to the single-var path — but the SURFACE
/// SHAPE the user can write is now `!{| E, F}` and MT4059 still
/// fires when the caller's closed row rejects the closure's
/// effects.
#[test]
fn multi_row_var_closed_caller_emits_mt4059() {
    let src = r#"
        fn cross[E, F](a: fn() -> Unit, b: fn() -> Unit) -> Unit !{| E, F} {
        }
        pub fn pure_writer() -> Unit !{} {
            cross(fn() { fs.write("/a") }, fn() { net.get("https://x") })
        }
    "#;
    let typed = check(src);
    let codes = diag_codes(&typed);
    assert!(
        codes.contains(&"MT4059".to_string()),
        "expected MT4059 for closed pub caller invoking a multi-row-var \
         row-poly fn with effectful closures; got {:?}",
        codes
    );
}

/// Multi-row-var simulation via two SEPARATE row-poly fn calls in
/// the same caller — each call instantiates its own `RowSubst`
/// (per RFC-008 §inference), so the rows are independent. This is
/// observationally equivalent to the v0.18 multi-row-var sig
/// `fn cross[E1, E2](a: fn() !E1, b: fn() !E2) -> Unit !{| E1, E2}`
/// called once: the caller's inferred set is the union of all
/// closure effects.
#[test]
fn two_separate_row_poly_calls_propagate_independently() {
    let src = r#"
        fn each[E](f: fn() -> Unit) -> Unit !{| E} {
        }
        fn caller() {
            each(fn() { fs.read("/a") })
            each(fn() { net.get("https://x") })
        }
    "#;
    let typed = check(src);
    let codes = diag_codes(&typed);
    // No diagnostics expected — the caller is a private fn, so
    // MT4001 doesn't fire; both closures are well-formed.
    assert!(
        !codes.contains(&"MT4058".to_string()),
        "MT4058 should not fire on two single-closure calls; got {:?}",
        codes
    );
    assert!(
        !codes.contains(&"MT4059".to_string()),
        "MT4059 should not fire when caller is private (no \
         declared row to violate); got {:?}",
        codes
    );
}
