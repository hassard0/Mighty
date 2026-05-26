//! Statement-level lowering (HirStmt → MtyIR Stmts).
//!
//! v0.22 — extracted from `exprs.rs` so the span-threading work has a
//! single home. `lower_stmt` is the canonical entry point; it sets the
//! builder's `cur_span` from the source HIR shape (currently the
//! enclosing fn's span — HIR does not yet expose per-stmt spans; v0.23
//! will replace the fallback with a per-stmt lookup) and dispatches into
//! the existing pattern-binding machinery.
//!
//! The bind-pattern helper lives here too because it is the single user
//! of `Stmt::Assign` that originates from `let` statements specifically.

use super::ctx::{FnBuilder, LowerCtx};
use super::exprs::lower_expr;
use crate::ir::*;
use mty_hir::{HirPat, HirStmt};

/// Lower a single HIR statement, emitting Stmts into the current block
/// of `fb`. v0.22: every Stmt + Terminator emitted under this call
/// carries the current `fb.cur_span` (set on entry by the enclosing
/// fn's lowerer to the fn's `SourceSpan`).
///
/// We DO NOT panic on shapes we don't understand — the lowerer is
/// designed to be total so partly-checked programs still produce
/// executable SIR.
pub fn lower_stmt(ctx: &mut LowerCtx, fb: &mut FnBuilder, s: &HirStmt) {
    match s {
        HirStmt::Let {
            pat,
            ty,
            init,
            mutable,
        } => {
            let init_op = match init {
                Some(e) => lower_expr(ctx, fb, *e),
                None => Operand::Const(Const::Unit),
            };
            // Bind via the pattern. For the common case `let x = expr`,
            // pat is `Binding{ name: "x", sub: None }`.
            bind_pat_assign(ctx, fb, *pat, init_op, *mutable, ty.is_some());
        }
        HirStmt::Expr(e) => {
            let _ = lower_expr(ctx, fb, *e);
        }
    }
}

/// Bind a pattern at let-position to the given right-hand-side operand.
/// Emits the necessary `Stmt::Assign`s to project the rhs into the
/// per-binding locals.
pub(crate) fn bind_pat_assign(
    ctx: &mut LowerCtx,
    fb: &mut FnBuilder,
    pat_id: mty_hir::PatId,
    rhs: Operand,
    mutable: bool,
    _annotated: bool,
) {
    let p = ctx.pkg.pats[pat_id].clone();
    match p {
        HirPat::Binding { name, sub } => {
            // Pick a slice-6 default type — we look up the rhs's type
            // through the typed table when available. For statements we
            // don't have an ExprId for the rhs after lowering, so use
            // IrTy::Error and trust the interpreter's permissive
            // Value enum.
            let ty = IrTy::Error;
            let l = fb.new_local(name, ty, mutable, LocalSource::UserLet);
            fb.push_stmt(Stmt::Assign(Place::local(l), Rvalue::Use(rhs)));
            if let Some(sp) = sub {
                bind_pat_assign(ctx, fb, sp, Operand::Copy(Place::local(l)), mutable, false);
            }
        }
        HirPat::Wildcard => {
            // Discard.
        }
        HirPat::Tuple(parts) => {
            // Stash rhs in a temp; project each element into its own local.
            let temp = fb.fresh_temp(IrTy::Error);
            fb.push_stmt(Stmt::Assign(Place::local(temp), Rvalue::Use(rhs)));
            for (i, sp) in parts.into_iter().enumerate() {
                let elt_temp = fb.fresh_temp(IrTy::Error);
                fb.push_stmt(Stmt::Assign(
                    Place::local(elt_temp),
                    Rvalue::TupleRead {
                        receiver: Place::local(temp),
                        idx: i,
                    },
                ));
                bind_pat_assign(
                    ctx,
                    fb,
                    sp,
                    Operand::Move(Place::local(elt_temp)),
                    mutable,
                    false,
                );
            }
        }
        _ => {
            // Other patterns at let-position are rare in the canonical
            // examples. Slice 6 falls back to stashing the rhs in an
            // anonymous local.
            let l = fb.new_local("", IrTy::Error, mutable, LocalSource::UserLet);
            fb.push_stmt(Stmt::Assign(Place::local(l), Rvalue::Use(rhs)));
        }
    }
}
