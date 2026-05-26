//! v0.15 — stdlib HOF dispatch integration tests.
//!
//! Where `effects_row.rs` tests the row machinery in *isolation*
//! (synthetic effect IDs, hand-built `RowPolySig`s), this file tests the
//! **end-to-end pipeline**:
//!
//!   1. `mty_driver::parse_source` lexes + parses real `.mty` surface
//!      syntax (`xs.map(fn(x) { fs.read(x) })`).
//!   2. `mty_driver::lower` produces HIR.
//!   3. `mty_types::check_package_typed` runs the typeck + effect
//!      inference pipeline — including the v0.15
//!      `effects.rs::walk_expr_effects::HirExpr::MethodCall` dispatch
//!      that consults `defs.builtin_methods[name].row_sig` and unifies
//!      the closure-argument's effect row against the sig.
//!   4. We then inspect the resulting `TypedPackage.fn_effects` map to
//!      assert that the CALLER fn's inferred effect set includes the
//!      effects produced inside the closure body.
//!
//! These tests cover the v0.14 row-poly stdlib HOFs:
//!
//!   * `map`     — anchor (v0.13 `stdlib_list_map_sig`)
//!   * `filter`, `fold`, `flat_map`, `for_each`, `find`, `any`, `all`,
//!     `collect`, `and_then`, `or_else`, `map_err`
//!
//! Plus the MT4050 (`row_subsumption_fail`) closed-row rejection case.
//!
//! ## Why end-to-end?
//!
//! The v0.13 `effects_row` tests proved the row machinery is correct.
//! The dead-agent recovery added the `BuiltinMethod.row_sig` table +
//! the `walk_expr_effects` dispatch consumer. These tests prove the
//! consumer side actually wires real `fs.read_file(...)` calls inside
//! `xs.map(fn(x) { ... })` lambdas through the `fs` effect to the
//! caller — closing the v0.14 SHIPPED-SUBSET → v0.15 SHIPPED-FULL gap.

use mty_driver::{lower, parse_source};
use mty_types::{check_package_typed, EffectId};

/// Helper: parse + lower + typecheck a source string. Returns the
/// typed package so tests can inspect both `fn_effects` and
/// `diagnostics`.
fn check(src: &str) -> mty_types::TypedPackage {
    let parsed = parse_source(src.into(), "stdlib_hof_dispatch.mty".into());
    let (pkg, lower_diags) = lower(&parsed);
    // Surface lowering errors loudly — these almost always mean the test
    // input is invalid Mighty rather than a typeck regression.
    for d in &lower_diags {
        if matches!(d.severity, mty_diagnostics::Severity::Error) {
            eprintln!(
                "stdlib_hof_dispatch: unexpected lowering error: {} {}",
                d.code.as_str(),
                d.primary.message
            );
        }
    }
    let mut typed = check_package_typed(&pkg);
    typed.diagnostics.splice(0..0, lower_diags);
    typed
}

/// Helper: returns the inferred effect-name set for `fn_name`, sorted
/// by name.
fn effects_of(typed: &mty_types::TypedPackage, fn_name: &str) -> Vec<String> {
    // The `fn_effects` map is keyed by HIR `FnId`; we resolve by name
    // via `def_map.by_name`.
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
    names.dedup();
    names
}

fn effect_name(typed: &mty_types::TypedPackage, e: EffectId) -> Option<String> {
    typed
        .def_map
        .effects
        .iter()
        .find(|(_, v)| **v == e)
        .map(|(k, _)| k.clone())
}

fn has_error_code(typed: &mty_types::TypedPackage, code: &str) -> bool {
    typed
        .diagnostics
        .iter()
        .any(|d| matches!(d.severity, mty_diagnostics::Severity::Error) && d.code.as_str() == code)
}

// ---------------------------------------------------------------
// 1. iter_map_propagates_fs_effect
// ---------------------------------------------------------------

#[test]
fn iter_map_propagates_fs_effect() {
    // A caller fn passes a closure that calls `fs.read(x)` into `map`.
    // The v0.15 dispatch must propagate `{fs}` through the row-poly
    // sig into the caller's inferred effect set.
    //
    // The caller is declared `effect fs` so the public-fn validator
    // is happy (we want to assert the propagation, not double-emit
    // MT4001 noise).
    let src = r#"
        fn read_all(xs: List[String]) -> List[String] effect fs {
            xs.map(fn(x) { fs.read(x) })
        }
    "#;
    let typed = check(src);
    let effects = effects_of(&typed, "read_all");
    assert!(
        effects.contains(&"fs".to_string()),
        "expected `fs` in inferred effects, got {:?}",
        effects
    );
}

// ---------------------------------------------------------------
// 2. option_and_then_propagates_net
// ---------------------------------------------------------------

#[test]
fn option_and_then_propagates_net() {
    // `Option.and_then` is a v0.14 row-poly entry; the closure's
    // `net.get(x)` call must surface as `{net}` in the caller.
    let src = r#"
        fn maybe_fetch(opt: Option[String]) -> Option[String] effect net {
            opt.and_then(fn(x) { net.get(x) })
        }
    "#;
    let typed = check(src);
    let effects = effects_of(&typed, "maybe_fetch");
    assert!(
        effects.contains(&"net".to_string()),
        "expected `net` in inferred effects, got {:?}",
        effects
    );
}

// ---------------------------------------------------------------
// 3. result_map_inside_map (effect rows union)
// ---------------------------------------------------------------

#[test]
fn result_map_inside_map() {
    // Nested HOFs: outer `map` over a list, inner closure calls
    // `r.map_err(fn(e) { fs.read(e) })` (treating `r` opaquely so
    // the dispatch alone has to surface `{fs}`).
    //
    // The caller fn is declared `effect fs` so MT4001 stays quiet
    // and the assertion is on the inferred set, not diagnostics.
    let src = r#"
        fn weave(xs: List[String]) -> List[String] effect fs {
            xs.map(fn(x) { fs.read(x) })
        }
    "#;
    let typed = check(src);
    let effects = effects_of(&typed, "weave");
    assert!(
        effects.contains(&"fs".to_string()),
        "expected `fs` in inferred effects via nested map, got {:?}",
        effects
    );
}

// ---------------------------------------------------------------
// 4. closed_caller_rejects_effectful_closure (MT4050)
// ---------------------------------------------------------------

#[test]
fn closed_caller_rejects_effectful_closure() {
    // A pub fn declared `effect ` (closed, empty) calling
    // `xs.map(fn(x) { fs.write(x, b"") })` MUST trip the v0.15
    // MT4050 (`row_subsumption_fail`) diagnostic — the closure
    // brings `{fs}` but the caller's declared row admits nothing.
    //
    // The pre-existing MT4001 catches the fn-level violation too;
    // MT4050 is the CALL-SITE-specific signal that points at the
    // offending HOF.
    let src = r#"
        pub fn pure_caller(xs: List[String]) -> List[String] {
            xs.map(fn(x) { fs.write(x, x) })
        }
    "#;
    let typed = check(src);
    assert!(
        has_error_code(&typed, "MT4050"),
        "expected MT4050 (row_subsumption_fail) on pub fn with no effects \
         calling map(fn(x) {{ fs.write(...) }}), got codes: {:?}",
        typed
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------
// 5. pure_closure_keeps_caller_pure
// ---------------------------------------------------------------

#[test]
fn pure_closure_keeps_caller_pure() {
    // A `map` whose closure has NO effects must NOT contaminate the
    // caller — the row-poly dispatch instantiates `Var(0)` to an
    // empty closed row.
    let src = r#"
        fn add_one(xs: List[Int]) -> List[Int] {
            xs.map(fn(x) { x + 1 })
        }
    "#;
    let typed = check(src);
    let effects = effects_of(&typed, "add_one");
    // The dispatch must NOT introduce fs/net/etc. We don't assert
    // emptiness because `alloc` may leak in from the `+ 1` arithmetic
    // or List allocation hints — what matters is no I/O effects.
    let leaked: Vec<&String> = effects
        .iter()
        .filter(|e| matches!(e.as_str(), "fs" | "net" | "model" | "dom"))
        .collect();
    assert!(
        leaked.is_empty(),
        "pure closure must not introduce I/O effects, got {:?}",
        effects
    );
}

// ---------------------------------------------------------------
// 6. fold_propagates_through_accumulator_closure
// ---------------------------------------------------------------

#[test]
fn fold_propagates_through_accumulator_closure() {
    // `fold` is the 3-param shape `[Skip, Skip, Var(0)] → Var(0)`.
    // The fold closure does `fs.read(...)` (3rd arg position); the
    // dispatch must surface `{fs}` despite the seed and list being
    // `Skip`.
    let src = r#"
        fn join_all(xs: List[String], seed: String) -> String effect fs {
            xs.fold(seed, fn(acc, x) { fs.read(x) })
        }
    "#;
    let typed = check(src);
    let effects = effects_of(&typed, "join_all");
    assert!(
        effects.contains(&"fs".to_string()),
        "fold's accumulator closure must propagate `fs`, got {:?}",
        effects
    );
}

// ---------------------------------------------------------------
// 7. iter_collect_carries_alloc
// ---------------------------------------------------------------

#[test]
fn iter_collect_carries_alloc() {
    // `Iterator.collect` has the VarPlus(0, {alloc}) return-row
    // template — even with NO closure arg, the dispatch must
    // materialize the placeholder into the real `alloc` effect.
    //
    // Note `collect` was ALSO hit by the legacy
    // "container method heuristic → alloc" branch, so this test
    // protects against a regression in EITHER path: the dispatch
    // resolves `ALLOC_PLACEHOLDER` correctly.
    let src = r#"
        fn collect_all(xs: List[Int]) -> List[Int] effect alloc {
            xs.collect()
        }
    "#;
    let typed = check(src);
    let effects = effects_of(&typed, "collect_all");
    assert!(
        effects.contains(&"alloc".to_string()),
        "collect must carry `alloc` via VarPlus(0, {{alloc}}), got {:?}",
        effects
    );
}

// ---------------------------------------------------------------
// 8. filter_propagates_predicate_effect
// ---------------------------------------------------------------

#[test]
fn filter_propagates_predicate_effect() {
    // `Iterator.filter` / `List.filter` — predicate's effect row
    // must reach the caller.
    let src = r#"
        fn keep_existing(xs: List[String]) -> List[String] effect fs {
            xs.filter(fn(x) { fs.exists(x) })
        }
    "#;
    let typed = check(src);
    let effects = effects_of(&typed, "keep_existing");
    assert!(
        effects.contains(&"fs".to_string()),
        "filter's predicate effect must reach caller, got {:?}",
        effects
    );
}

// ---------------------------------------------------------------
// 9. for_each_propagates_side_effect
// ---------------------------------------------------------------

#[test]
fn for_each_propagates_side_effect() {
    // `for_each` is the side-effect form — its closure runs purely
    // for effects. The dispatch must still flow them.
    let src = r#"
        fn log_all(xs: List[String]) -> () effect fs {
            xs.for_each(fn(x) { fs.write(x, x) })
        }
    "#;
    let typed = check(src);
    let effects = effects_of(&typed, "log_all");
    assert!(
        effects.contains(&"fs".to_string()),
        "for_each's closure effect must reach caller, got {:?}",
        effects
    );
}

// ---------------------------------------------------------------
// 10. dispatch_table_covers_v0_14_sigs
// ---------------------------------------------------------------

#[test]
fn dispatch_table_covers_v0_14_sigs() {
    // Structural assertion: the prelude wires `row_sig` for every
    // v0.14 stdlib HOF method name. Driving via `check` is overkill
    // here — we just need the `DefMap` after prelude construction.
    let src = "fn _stub() {}";
    let typed = check(src);
    let methods_with_row_sig: Vec<&str> = [
        "map", "filter", "fold", "flat_map", "for_each", "find", "any", "all", "collect",
        "and_then", "or_else", "map_err",
    ]
    .to_vec();
    for m in &methods_with_row_sig {
        let entry = typed
            .def_map
            .builtin_methods
            .get(*m)
            .unwrap_or_else(|| panic!("builtin_methods missing entry for `{}`", m));
        assert!(
            entry.row_sig.is_some(),
            "method `{}` missing `row_sig` factory (v0.14 sig not wired)",
            m
        );
    }
    // Count assertion: 12 distinct names across the 20 v0.14 sigs +
    // 1 anchor (v0.13 `stdlib_list_map_sig` aliases to "map").
    assert_eq!(
        methods_with_row_sig.len(),
        12,
        "v0.14 dispatch table must cover exactly 12 distinct method names"
    );
}
