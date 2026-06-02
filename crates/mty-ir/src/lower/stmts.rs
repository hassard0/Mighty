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
            // v0.42 T4 (L23 fix) — also keep scalar IrTy on the local
            // slot. Pre-fix, every non-`Vec` let-binding was typed as
            // `IrTy::Error` to avoid mid-rebind aggregate-slot
            // breakage on Str/String (see Vec carve-out below). That
            // hid the operand type from codegen's typed-log dispatch
            // (`log(n)` for `n: I32` couldn't tell which
            // `mty_runtime_log_*` to call). We now propagate scalar
            // shapes (Int / Float / Bool / Char / Size / Duration)
            // onto the slot too; aggregates (Str/String/Tuple/Adt
            // other than Vec) still fall back to `IrTy::Error` so the
            // existing string-rebind path stays intact.
            let ty = match init_ty {
                Some(tyid) => {
                    let lowered = crate::lower::ty::lower_ty(tyid, &ctx.typed.ty_arena);
                    match &lowered {
                        IrTy::Adt(id, _) => {
                            let is_vec = matches!(
                                ctx.typed.def_map.lookup("Vec"),
                                Some(mty_types::DefRef::Adt(a)) if a == *id
                            );
                            if is_vec {
                                lowered
                            } else {
                                IrTy::Error
                            }
                        }
                        // Scalars + Size/Duration: keep the real
                        // shape on the slot so codegen typed dispatch
                        // (v0.42 T4 log/print, future to_str on
                        // scalars) reads the right kind.
                        IrTy::Int(_)
                        | IrTy::Float(_)
                        | IrTy::Bool
                        | IrTy::Char
                        | IrTy::Size
                        | IrTy::Duration => lowered,
                        // Str/String/Bytes/Tuple/etc. — keep the
                        // pre-v0.42 behaviour (Error) so the
                        // aggregate-slot lazy-init in let-rebind keeps
                        // working.
                        _ => IrTy::Error,
                    }
                }
                None => IrTy::Error,
            };
            // v0.46 T4 — when typeck returned a fresh-var / Error type
            // but the rhs's IR temp resolves to a typed ADT we know
            // the codegen needs (Metadata's named-field projection,
            // DirIter's `.next()` method dispatch), promote the
            // binding's slot type to that ADT. Without this hand-off,
            // `let md = std.fs.metadata(p); if md.is_file { ... }` would
            // keep `md` at `IrTy::Error`, and `place_addr`'s
            // best-effort `idx*8` fallback would mis-resolve
            // is_file@+16 / is_dir@+17 (would land at 16 / 24
            // instead of 16 / 17). Same logic carries the DirIter
            // handle's opaque-ADT typing across the binding so the
            // method-dispatch arm matches.
            let ty = if matches!(ty, IrTy::Error) {
                if let Operand::Move(p) | Operand::Copy(p) = &rhs {
                    if p.proj.is_empty() {
                        let rhs_ty = fb.locals[p.local.0 as usize].ty.clone();
                        if let IrTy::Adt(id, _) = &rhs_ty {
                            let is_fs_record = matches!(
                                ctx.typed.def_map.lookup("Metadata"),
                                Some(mty_types::DefRef::Adt(a)) if a == *id
                            ) || matches!(
                                ctx.typed.def_map.lookup("DirIter"),
                                Some(mty_types::DefRef::Adt(a)) if a == *id
                            );
                            if is_fs_record {
                                rhs_ty
                            } else {
                                ty
                            }
                        } else {
                            ty
                        }
                    } else {
                        ty
                    }
                } else {
                    ty
                }
            } else {
                ty
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
