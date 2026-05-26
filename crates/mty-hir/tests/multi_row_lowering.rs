//! v0.19 — multi row-variable HIR lowering completeness tests.
//!
//! v0.18 broadened the parser surface (`!{| E1, E2}` and friends)
//! but the HIR lowerer kept the v0.18-and-earlier first-row-var-only
//! path: `EffectClause::row_var_name()` returned only the FIRST
//! `EFFECT_ROW_VAR` child, silently dropping any extras. The v0.17
//! typeck layer already consumes `Vec<HirRowVar>`, so the gap lived
//! entirely in `mty-hir::lower::items::lower_effect_clause`.
//!
//! v0.19 replaces the first-only call with a full iteration over
//! [`mty_ast::EffectClause::row_var_names`], producing a
//! source-order `Vec<HirRowVar>` with stable `idx` values (0, 1, ...).
//!
//! Each test below lowers a tiny source fragment exercising one of
//! the v0.18 multi-row-var surface shapes and asserts the resulting
//! [`HirEffectRow::Open`] carries every row variable.

use mty_ast::{AstNode, File};
use mty_hir::{HirEffectRow, Package};
use mty_syntax::{parse, SyntaxNode};

fn lower(src: &str) -> Package {
    let r = parse(src);
    let f = File::cast(SyntaxNode::new_root(r.green)).expect("File::cast");
    let (pkg, _diags) = mty_hir::lower::LoweringCtx::new().lower_file(f);
    pkg
}

/// Look up the single fn in `pkg` and return its `effect_row` clone.
fn fn_effect_row(pkg: &Package, name: &str) -> Option<HirEffectRow> {
    pkg.fns
        .iter()
        .find(|(_, hf)| hf.name == name)
        .and_then(|(_, hf)| hf.effect_row.clone())
}

/// Back-compat with v0.18 single-var case: `!{| E}` lowers to a
/// length-1 `Vec<HirRowVar>`, idx 0, name "E". This is the same
/// shape v0.18 produced — pinning it prevents the multi-var
/// expansion from accidentally re-numbering or reordering.
#[test]
fn single_row_var_lowers_to_one_hir_var() {
    let pkg = lower("fn f() -> Unit !{| E} { }");
    let row = fn_effect_row(&pkg, "f").expect("f should have an effect_row");
    match row {
        HirEffectRow::Open(concrete, row_vars) => {
            assert!(
                concrete.is_empty(),
                "no concrete effects expected, got {:?}",
                concrete
            );
            assert_eq!(row_vars.len(), 1, "expected exactly one row var");
            assert_eq!(row_vars[0].name, "E");
            assert_eq!(row_vars[0].idx, 0);
        }
        other => panic!("expected Open, got {:?}", other),
    }
}

/// v0.19 motivating shape: `!{| E, F}` must lower to a length-2
/// `Vec<HirRowVar>` with idx 0 ("E") and idx 1 ("F") in source
/// order. v0.18 dropped "F" entirely.
#[test]
fn two_row_vars_lower_to_two_hir_vars() {
    let pkg = lower("fn f() -> Unit !{| E, F} { }");
    let row = fn_effect_row(&pkg, "f").expect("f should have an effect_row");
    match row {
        HirEffectRow::Open(concrete, row_vars) => {
            assert!(concrete.is_empty(), "no concrete effects expected");
            assert_eq!(
                row_vars.len(),
                2,
                "expected two row vars; v0.18 lowerer would have dropped F"
            );
            assert_eq!(row_vars[0].name, "E");
            assert_eq!(row_vars[0].idx, 0);
            assert_eq!(row_vars[1].name, "F");
            assert_eq!(row_vars[1].idx, 1);
        }
        other => panic!("expected Open, got {:?}", other),
    }
}

/// Three-way row-poly: `!{| E, F, G}` must lower to a length-3
/// `Vec<HirRowVar>` with idx 0/1/2 in source order. Mirror of the
/// v0.18 parser test `parse_three_row_vars` — the lowerer must
/// scale beyond two row vars.
#[test]
fn three_row_vars() {
    let pkg = lower("fn f() -> Unit !{| E, F, G} { }");
    let row = fn_effect_row(&pkg, "f").expect("f should have an effect_row");
    match row {
        HirEffectRow::Open(concrete, row_vars) => {
            assert!(concrete.is_empty());
            assert_eq!(row_vars.len(), 3);
            assert_eq!(row_vars[0].name, "E");
            assert_eq!(row_vars[0].idx, 0);
            assert_eq!(row_vars[1].name, "F");
            assert_eq!(row_vars[1].idx, 1);
            assert_eq!(row_vars[2].name, "G");
            assert_eq!(row_vars[2].idx, 2);
        }
        other => panic!("expected Open, got {:?}", other),
    }
}

/// Concrete + multi-row-var tail: `!{fs | E, F}` produces both a
/// `concrete` vec with "fs" AND a row-vars vec with two entries.
/// The v0.17 typeck layer reads both — concrete to seed
/// `UserRowPolyMeta::concrete_effects`, row vars to drive
/// per-call-site RowSubst.
#[test]
fn concrete_plus_two_row_vars() {
    let pkg = lower("fn f() -> Unit !{fs | E, F} { }");
    let row = fn_effect_row(&pkg, "f").expect("f should have an effect_row");
    match row {
        HirEffectRow::Open(concrete, row_vars) => {
            assert_eq!(concrete.len(), 1, "expected one concrete effect");
            assert_eq!(concrete[0].as_str(), "fs");
            assert_eq!(row_vars.len(), 2);
            assert_eq!(row_vars[0].name, "E");
            assert_eq!(row_vars[0].idx, 0);
            assert_eq!(row_vars[1].name, "F");
            assert_eq!(row_vars[1].idx, 1);
        }
        other => panic!("expected Open, got {:?}", other),
    }
}

/// Legacy keyword form picks up multi-row-var "for free" because
/// both surface shapes route through the parser's `effect_row_tail`.
/// `effect fs | E, F` must lower to the same shape as
/// `!{fs | E, F}`.
#[test]
fn legacy_effect_form_with_multi_row_tail() {
    let pkg = lower("fn f() -> Page effect net | E, F { }");
    let row = fn_effect_row(&pkg, "f").expect("f should have an effect_row");
    match row {
        HirEffectRow::Open(concrete, row_vars) => {
            let names: Vec<&str> = concrete.iter().map(|n| n.as_str()).collect();
            assert!(
                names.contains(&"net"),
                "legacy keyword form should preserve `net`; got {:?}",
                names
            );
            assert_eq!(row_vars.len(), 2);
            assert_eq!(row_vars[0].name, "E");
            assert_eq!(row_vars[0].idx, 0);
            assert_eq!(row_vars[1].name, "F");
            assert_eq!(row_vars[1].idx, 1);
        }
        other => panic!("expected Open from legacy form, got {:?}", other),
    }
}

/// Closed-row signatures (`!{fs, net}` with no `|`) keep their
/// existing v0.16 lowering: the row-var pass produces an empty
/// vec, so the effect_row falls through to the `effect_set().is_some()`
/// branch and emits `HirEffectRow::Closed`. The multi-row-var
/// rewrite must not perturb the closed shape.
#[test]
fn closed_row_unchanged_by_multi_var_path() {
    let pkg = lower("fn f() -> Unit !{fs, net} { }");
    let row = fn_effect_row(&pkg, "f").expect("f should have an effect_row");
    match row {
        HirEffectRow::Closed(concrete) => {
            let names: Vec<&str> = concrete.iter().map(|n| n.as_str()).collect();
            assert!(names.contains(&"fs"));
            assert!(names.contains(&"net"));
        }
        other => panic!("expected Closed, got {:?}", other),
    }
}

/// Lower `examples/24_multi_row_full.mty` from disk and pin the
/// HIR shape: the `_cross` fn must have a length-2
/// `Vec<HirRowVar>`. This is a regression guard — if a future
/// refactor of `lower_effect_clause` re-introduces the
/// first-only path, the on-disk example will instantly fail
/// here.
#[test]
fn example_24_multi_row_full_lowers_two_row_vars() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .join("examples")
        .join("24_multi_row_full.mty");
    let src =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let pkg = lower(&src);
    let row = fn_effect_row(&pkg, "_cross").expect("_cross should have an effect_row");
    match row {
        HirEffectRow::Open(_concrete, row_vars) => {
            assert_eq!(
                row_vars.len(),
                2,
                "examples/24: _cross must lower to two row vars"
            );
            assert_eq!(row_vars[0].name, "E");
            assert_eq!(row_vars[1].name, "F");
        }
        other => panic!("expected Open from _cross, got {:?}", other),
    }
}

/// The `cross[E, F]` motivating signature from
/// `examples/24_multi_row_full.mty`: TWO fn-typed parameters, each
/// carrying its own row variable, with the return row declaring the
/// union `!{| E, F}`. Pin the HIR shape end-to-end — this is the
/// canonical multi-row-var fn signature.
#[test]
fn cross_two_closure_args_multi_var_signature() {
    let pkg = lower("fn cross[E, F](a: fn() -> Unit, b: fn() -> Unit) -> Unit !{| E, F} { }");
    let row = fn_effect_row(&pkg, "cross").expect("cross should have an effect_row");
    match row {
        HirEffectRow::Open(concrete, row_vars) => {
            assert!(concrete.is_empty());
            assert_eq!(row_vars.len(), 2);
            assert_eq!(row_vars[0].name, "E");
            assert_eq!(row_vars[1].name, "F");
            // Per-row-var idx must be source-order-stable so the
            // typeck-side per-call-site RowSubst slot layout
            // matches the HIR sig.
            assert_eq!(row_vars[0].idx, 0);
            assert_eq!(row_vars[1].idx, 1);
        }
        other => panic!("expected Open for cross, got {:?}", other),
    }
    // Sanity: cross has two parameters.
    let cross_fn = pkg
        .fns
        .iter()
        .find(|(_, hf)| hf.name == "cross")
        .map(|(_, hf)| hf)
        .expect("cross fn");
    assert_eq!(cross_fn.params.len(), 2);
}
