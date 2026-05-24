//! v0.5 — `for x in 1..5` lowers to HirExpr::For with a Range binary
//! operator as its iterator expression. The actual iterator probe is
//! materialised by the SIR lowering (see `crates/mty-ir/tests/for_range.rs`).

use mty_ast::{AstNode, File};
use mty_hir::{BinOp, HirExpr};
use mty_syntax::{parse, SyntaxNode};

fn lower(src: &str) -> mty_hir::Package {
    let r = parse(src);
    let f = File::cast(SyntaxNode::new_root(r.green)).unwrap();
    let (pkg, _) = mty_hir::lower::LoweringCtx::new().lower_file(f);
    pkg
}

#[test]
fn for_in_exclusive_range_lowers() {
    let p = lower("fn f() { for i in 1..5 { } }");
    let (iter_expr, body_present) = p
        .exprs
        .values()
        .find_map(|e| match e {
            HirExpr::For { iter, body, .. } => Some((*iter, *body)),
            _ => None,
        })
        .expect("for-expr present");
    let iter = &p.exprs[iter_expr];
    match iter {
        HirExpr::Binary { op, .. } => assert_eq!(*op, BinOp::Range),
        other => panic!("expected Binary::Range, got {:?}", other),
    }
    let _ = body_present;
}

#[test]
fn for_in_inclusive_range_lowers() {
    let p = lower("fn f() { for i in 1..=5 { } }");
    let iter_expr = p
        .exprs
        .values()
        .find_map(|e| match e {
            HirExpr::For { iter, .. } => Some(*iter),
            _ => None,
        })
        .expect("for-expr present");
    let iter = &p.exprs[iter_expr];
    match iter {
        HirExpr::Binary { op, .. } => assert_eq!(*op, BinOp::RangeEq),
        other => panic!("expected Binary::RangeEq, got {:?}", other),
    }
}

#[test]
fn for_with_array_iter_lowers() {
    let p = lower("fn f() { for v in arr { } }");
    let iter_expr = p
        .exprs
        .values()
        .find_map(|e| match e {
            HirExpr::For { iter, .. } => Some(*iter),
            _ => None,
        })
        .expect("for-expr present");
    // arr is a path expr.
    matches!(p.exprs[iter_expr], HirExpr::Path(_));
}
