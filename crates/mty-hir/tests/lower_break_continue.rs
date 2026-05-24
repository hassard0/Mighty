//! v0.5 — `break` / `continue` lower into HirExpr::Break / HirExpr::Continue.

use mty_ast::{AstNode, File};
use mty_hir::HirExpr;
use mty_syntax::{parse, SyntaxNode};

fn lower(src: &str) -> mty_hir::Package {
    let r = parse(src);
    let f = File::cast(SyntaxNode::new_root(r.green)).unwrap();
    let (pkg, _) = mty_hir::lower::LoweringCtx::new().lower_file(f);
    pkg
}

fn count_break_continue(pkg: &mty_hir::Package) -> (usize, usize) {
    let mut brk = 0;
    let mut cont = 0;
    for (_id, e) in pkg.exprs.iter() {
        match e {
            HirExpr::Break(_) => brk += 1,
            HirExpr::Continue => cont += 1,
            _ => {}
        }
    }
    (brk, cont)
}

#[test]
fn break_inside_loop_lowers() {
    let p = lower("fn f() { loop { break } }");
    let (brk, cont) = count_break_continue(&p);
    assert_eq!(brk, 1, "expected one HirExpr::Break");
    assert_eq!(cont, 0);
}

#[test]
fn continue_inside_for_lowers() {
    let p = lower("fn f() { for x in xs { continue } }");
    let (brk, cont) = count_break_continue(&p);
    assert_eq!(brk, 0);
    assert_eq!(cont, 1, "expected one HirExpr::Continue");
}

#[test]
fn break_with_value_carries_inner_expr() {
    let p = lower("fn f() { loop { if true { break 42 } } }");
    let (brk, _) = count_break_continue(&p);
    assert_eq!(brk, 1);
    // Find the break and verify it has an inner expr.
    let break_expr = p
        .exprs
        .values()
        .find_map(|e| match e {
            HirExpr::Break(inner) => Some(*inner),
            _ => None,
        })
        .expect("Break present");
    assert!(
        break_expr.is_some(),
        "break 42 should carry a value expr in the HIR"
    );
}

#[test]
fn bare_break_has_no_value() {
    let p = lower("fn f() { loop { break } }");
    let break_expr = p
        .exprs
        .values()
        .find_map(|e| match e {
            HirExpr::Break(inner) => Some(*inner),
            _ => None,
        })
        .expect("Break present");
    assert!(break_expr.is_none(), "bare break carries no inner expr");
}
