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
            // v0.39 T3: thread the init expression's TyId through so
            // `bind_pat_assign` can resolve the binding's real type
            // when one was inferred upstream (was `IrTy::Error` before,
            // which lost `Vec[T]` info before reaching codegen).
            let init_ty = init.map(|e| ctx.expr_ty(e));
            // Bind via the pattern. For the common case `let x = expr`,
            // pat is `Binding{ name: "x", sub: None }`.
            bind_pat_assign(ctx, fb, *pat, init_op, *mutable, ty.is_some(), init_ty);
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
    init_ty: Option<mty_types::TyId>,
) {
    let p = ctx.pkg.pats[pat_id].clone();
    match p {
        HirPat::Binding { name, sub } => {
            // v0.39 T3: pick up the init expression's inferred type
            // *only* for ADTs that the codegen backend types-aware
            // dispatch needs (currently `Vec[T]` for typed-slot
            // storage). Everything else continues to default to
            // `IrTy::Error` so the slice-6 aggregate-slot lazy-init
            // path stays unchanged. Doing the full lower-ty here
            // breaks the Str / String let-rebind path (the agg slot
            // gets created mid-rebind and overwrites the source
            // string-pair address).
            let ty = match init_ty {
                Some(tyid) => {
                    let lowered = crate::lower::ty::lower_ty(tyid, &ctx.typed.ty_arena);
                    if let IrTy::Adt(id, _) = &lowered {
                        let is_vec = matches!(
                            ctx.typed.def_map.lookup("Vec"),
                            Some(mty_types::DefRef::Adt(a)) if a == *id
                        );
                        if is_vec {
                            lowered
                        } else {
                            IrTy::Error
                        }
                    } else {
                        IrTy::Error
                    }
                }
                None => IrTy::Error,
            };
            // v0.25 — propagate the per-fn `canvas_locals` taint across
            // let-binding. `let canvas = std.web.Canvas.new(W, H)`
            // lowers the rhs to a `Move(temp)` whose temp is already
            // in `canvas_locals` (the `lower_call` arm tagged it on
            // the way out); we propagate that tag onto the user's
            // visible binding so subsequent `canvas.fill_rect(...)`
            // calls — which resolve the receiver through this
            // name-bound local — still route to
            // `BuiltinId::CanvasOp(...)`. Without this hand-off the
            // tag would die in the anonymous temp.
            let rhs_is_canvas = match &rhs {
                Operand::Move(p) | Operand::Copy(p) => {
                    p.proj.is_empty() && fb.is_canvas_local(p.local)
                }
                Operand::Const(_) => false,
            };
            let l = fb.new_local(name, ty, mutable, LocalSource::UserLet);
            if rhs_is_canvas {
                fb.mark_canvas_local(l);
            }
            // v0.41 T1 — track the HIR-resolved init type for this
            // local so multi-segment `Path(["x","field"])` projection
            // can resolve `field` to the correct ADT field index
            // instead of falling back to 0 (L15 / struct-field-read
            // collapse). We record the type *whether or not* the
            // codegen-typed slot above kept it, because the path
            // lowerer only needs the def-map resolution, not the
            // IR-level slot type.
            if let Some(tyid) = init_ty {
                fb.set_local_ty(l, tyid);
            }
            fb.push_stmt(Stmt::Assign(Place::local(l), Rvalue::Use(rhs)));
            if let Some(sp) = sub {
                bind_pat_assign(
                    ctx,
                    fb,
                    sp,
                    Operand::Copy(Place::local(l)),
                    mutable,
                    false,
                    init_ty,
                );
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
                    None,
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
