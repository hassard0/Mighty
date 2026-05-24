//! Pattern lowering used by `match` arms and `if let`.
//!
//! The function `lower_pat_match` emits SIR that tests a value (held in
//! `discr`) against a pattern. On success, control flows to `succ`; on
//! failure, to `fail`. Bindings produced by the pattern are inserted
//! into the FnBuilder's local map so the success block can reference
//! them by name.

use super::ctx::*;
use crate::sir::*;
use sdust_hir::{HirLiteral, HirPat, PatId};

/// Top-level pattern matching. Emits stmts on `fb`'s current block and
/// finishes by setting a terminator that jumps to `succ` on match or
/// `fail` otherwise. The current block on return is undefined; callers
/// should switch to `succ` (or to the next chain link) themselves.
pub fn lower_pat_match(
    ctx: &mut LowerCtx,
    fb: &mut FnBuilder,
    discr: Place,
    pat_id: PatId,
    succ: BlockId,
    fail: BlockId,
) {
    let pat = ctx.pkg.pats[pat_id].clone();
    match pat {
        HirPat::Wildcard => {
            fb.set_term(Term::Goto(succ));
        }
        HirPat::Binding { name, sub } => {
            // Bind `name` to the entire discriminant by Copy.
            let ty = local_ty(fb, &discr);
            let l = fb.new_local(name, ty, true, LocalSource::UserLet);
            fb.push_stmt(Stmt::Assign(
                Place::local(l),
                Rvalue::Use(Operand::Copy(discr.clone())),
            ));
            if let Some(sp) = sub {
                lower_pat_match(ctx, fb, discr, sp, succ, fail);
            } else {
                fb.set_term(Term::Goto(succ));
            }
        }
        HirPat::Literal(lit) => {
            let lit_const = lit_to_const(&lit);
            let cmp = fb.fresh_temp(SirTy::Bool);
            fb.push_stmt(Stmt::Assign(
                Place::local(cmp),
                Rvalue::BinOp(BinOp::Eq, Operand::Copy(discr), Operand::Const(lit_const)),
            ));
            fb.set_term(Term::If {
                cond: Operand::Copy(Place::local(cmp)),
                then: succ,
                else_: fail,
            });
        }
        HirPat::Tuple(parts) => {
            // For each tuple element, project + recurse. Chain successive
            // success blocks; any failure jumps to `fail`.
            let mut current = fb.current_block();
            for (i, sub) in parts.into_iter().enumerate() {
                fb.switch_to(current);
                let proj = Place {
                    local: discr.local,
                    proj: {
                        let mut p = discr.proj.clone();
                        p.push(Projection::TupleIndex(i));
                        p
                    },
                };
                let next_succ = fb.new_block();
                lower_pat_match(ctx, fb, proj, sub, next_succ, fail);
                current = next_succ;
            }
            fb.switch_to(current);
            fb.set_term(Term::Goto(succ));
        }
        HirPat::Enum { path, args } => {
            let variant_idx = resolve_variant(ctx, &path);
            // SwitchVariant; we don't know the AdtId from path alone in
            // slice 6, so we use a generic placeholder discriminant compare.
            // For interpreter correctness we look up the variant by name
            // at runtime, so the lowering encodes only the index.
            let succ_block = fb.new_block();
            fb.set_term(Term::SwitchVariant {
                discr: Operand::Copy(discr.clone()),
                adt: sdust_types::AdtId(0), // interpreter ignores the AdtId; uses Enum.variant
                arms: vec![(variant_idx, succ_block)],
                default: fail,
            });
            fb.switch_to(succ_block);
            // Bind each payload element.
            let mut current = succ_block;
            for (i, sub) in args.into_iter().enumerate() {
                fb.switch_to(current);
                let proj = Place {
                    local: discr.local,
                    proj: {
                        let mut p = discr.proj.clone();
                        p.push(Projection::VariantField(variant_idx, i));
                        p
                    },
                };
                let next_succ = fb.new_block();
                lower_pat_match(ctx, fb, proj, sub, next_succ, fail);
                current = next_succ;
            }
            fb.switch_to(current);
            fb.set_term(Term::Goto(succ));
        }
        HirPat::Struct { path, fields } => {
            // Struct pattern: each field is tested in order. Path resolves
            // to a single-variant ADT (variant 0).
            let variant_idx = 0;
            let _ = path;
            let mut current = fb.current_block();
            for (idx, (_name, sub)) in fields.into_iter().enumerate() {
                fb.switch_to(current);
                let proj = Place {
                    local: discr.local,
                    proj: {
                        let mut p = discr.proj.clone();
                        p.push(Projection::Field(idx));
                        p
                    },
                };
                let next_succ = fb.new_block();
                match sub {
                    Some(sp) => lower_pat_match(ctx, fb, proj, sp, next_succ, fail),
                    None => {
                        fb.set_term(Term::Goto(next_succ));
                    }
                }
                current = next_succ;
            }
            fb.switch_to(current);
            fb.set_term(Term::Goto(succ));
            let _ = variant_idx;
        }
        HirPat::Range { lo, hi, inclusive } => {
            let lo_pat = ctx.pkg.pats[lo].clone();
            let hi_pat = ctx.pkg.pats[hi].clone();
            let lo_c = pat_to_const(&lo_pat);
            let hi_c = pat_to_const(&hi_pat);
            let cmp = fb.fresh_temp(SirTy::Bool);
            let cmp2 = fb.fresh_temp(SirTy::Bool);
            // discr >= lo
            fb.push_stmt(Stmt::Assign(
                Place::local(cmp),
                Rvalue::BinOp(
                    BinOp::Ge,
                    Operand::Copy(discr.clone()),
                    Operand::Const(lo_c),
                ),
            ));
            // discr < hi (exclusive) or <= hi (inclusive)
            fb.push_stmt(Stmt::Assign(
                Place::local(cmp2),
                Rvalue::BinOp(
                    if inclusive { BinOp::Le } else { BinOp::Lt },
                    Operand::Copy(discr.clone()),
                    Operand::Const(hi_c),
                ),
            ));
            let combined = fb.fresh_temp(SirTy::Bool);
            fb.push_stmt(Stmt::Assign(
                Place::local(combined),
                Rvalue::BinOp(
                    BinOp::And,
                    Operand::Copy(Place::local(cmp)),
                    Operand::Copy(Place::local(cmp2)),
                ),
            ));
            fb.set_term(Term::If {
                cond: Operand::Copy(Place::local(combined)),
                then: succ,
                else_: fail,
            });
        }
        HirPat::Ref { inner, .. } => {
            // &x patterns: deref the discriminant and recurse.
            let proj = Place {
                local: discr.local,
                proj: {
                    let mut p = discr.proj.clone();
                    p.push(Projection::Deref);
                    p
                },
            };
            lower_pat_match(ctx, fb, proj, inner, succ, fail);
        }
    }
}

/// v0.5 (for-loop binding): bind `pat`'s names to the operand without
/// emitting any tests. Used by `for x in iter { ... }` where the
/// iterator protocol has already determined the element is present.
pub fn lower_pat_bind(
    ctx: &mut LowerCtx,
    fb: &mut FnBuilder,
    pat_id: PatId,
    rhs: Operand,
) {
    let pat = ctx.pkg.pats[pat_id].clone();
    bind_pat_recursive(ctx, fb, &pat, rhs);
}

fn bind_pat_recursive(
    ctx: &mut LowerCtx,
    fb: &mut FnBuilder,
    pat: &HirPat,
    rhs: Operand,
) {
    match pat {
        HirPat::Binding { name, sub } => {
            let l = fb.new_local(name.clone(), SirTy::Error, true, LocalSource::UserLet);
            fb.push_stmt(Stmt::Assign(Place::local(l), Rvalue::Use(rhs)));
            if let Some(sp) = sub {
                let sub_pat = ctx.pkg.pats[*sp].clone();
                bind_pat_recursive(ctx, fb, &sub_pat, Operand::Copy(Place::local(l)));
            }
        }
        HirPat::Wildcard | HirPat::Literal(_) | HirPat::Range { .. } => {
            // No bindings; nothing to write.
        }
        HirPat::Tuple(parts) => {
            // Stash rhs in a temp; project each element.
            let temp = fb.fresh_temp(SirTy::Error);
            fb.push_stmt(Stmt::Assign(Place::local(temp), Rvalue::Use(rhs)));
            for (i, sp) in parts.iter().enumerate() {
                let elt = fb.fresh_temp(SirTy::Error);
                fb.push_stmt(Stmt::Assign(
                    Place::local(elt),
                    Rvalue::TupleRead {
                        receiver: Place::local(temp),
                        idx: i,
                    },
                ));
                let sub_pat = ctx.pkg.pats[*sp].clone();
                bind_pat_recursive(ctx, fb, &sub_pat, Operand::Move(Place::local(elt)));
            }
        }
        HirPat::Ref { inner, .. } => {
            let sub_pat = ctx.pkg.pats[*inner].clone();
            bind_pat_recursive(ctx, fb, &sub_pat, rhs);
        }
        HirPat::Struct { fields, .. } => {
            let temp = fb.fresh_temp(SirTy::Error);
            fb.push_stmt(Stmt::Assign(Place::local(temp), Rvalue::Use(rhs)));
            for (idx, (_name, sub)) in fields.iter().enumerate() {
                if let Some(sp) = sub {
                    let f = fb.fresh_temp(SirTy::Error);
                    fb.push_stmt(Stmt::Assign(
                        Place::local(f),
                        Rvalue::FieldRead {
                            receiver: Place::local(temp),
                            field: idx,
                        },
                    ));
                    let sub_pat = ctx.pkg.pats[*sp].clone();
                    bind_pat_recursive(ctx, fb, &sub_pat, Operand::Move(Place::local(f)));
                }
            }
        }
        HirPat::Enum { args, .. } => {
            let temp = fb.fresh_temp(SirTy::Error);
            fb.push_stmt(Stmt::Assign(Place::local(temp), Rvalue::Use(rhs)));
            for (i, sp) in args.iter().enumerate() {
                let f = fb.fresh_temp(SirTy::Error);
                fb.push_stmt(Stmt::Assign(
                    Place::local(f),
                    Rvalue::TupleRead {
                        receiver: Place::local(temp),
                        idx: i,
                    },
                ));
                let sub_pat = ctx.pkg.pats[*sp].clone();
                bind_pat_recursive(ctx, fb, &sub_pat, Operand::Move(Place::local(f)));
            }
        }
    }
}

fn local_ty(fb: &FnBuilder, p: &Place) -> SirTy {
    // Take the underlying local's type. Projections are slice-6
    // permissive: tuple/enum projections retain the parent type
    // (interpreter does the right thing at runtime); we only need a
    // type here to declare a binding local, and the binding accepts
    // any value shape via the polymorphic `Value` enum.
    fb.locals[p.local.0 as usize].ty.clone()
}

fn lit_to_const(lit: &HirLiteral) -> Const {
    match lit {
        HirLiteral::Int(v, suf) => Const::Int(*v, parse_int_suffix(suf.as_deref())),
        HirLiteral::Float(v, suf) => Const::Float(*v, parse_float_suffix(suf.as_deref())),
        HirLiteral::Str(s) => Const::Str(s.clone()),
        HirLiteral::Char(c) => Const::Char(*c),
        HirLiteral::Bool(b) => Const::Bool(*b),
        HirLiteral::Duration { value, unit } => Const::Duration {
            value: *value,
            unit: unit.clone(),
        },
        HirLiteral::Size { value, unit } => Const::Size {
            value: *value,
            unit: unit.clone(),
        },
    }
}

fn parse_int_suffix(s: Option<&str>) -> sdust_types::IntKind {
    use sdust_types::IntKind::*;
    match s {
        Some("i8") => I8,
        Some("i16") => I16,
        Some("i32") => I32,
        Some("i64") => I64,
        Some("i128") => I128,
        Some("u8") => U8,
        Some("u16") => U16,
        Some("u32") => U32,
        Some("u64") => U64,
        Some("u128") => U128,
        Some("usize") => USize,
        Some("isize") => ISize,
        _ => IntInfer,
    }
}

fn parse_float_suffix(s: Option<&str>) -> sdust_types::FloatKind {
    use sdust_types::FloatKind::*;
    match s {
        Some("f32") => F32,
        Some("f64") => F64,
        _ => FloatInfer,
    }
}

fn pat_to_const(p: &HirPat) -> Const {
    match p {
        HirPat::Literal(l) => lit_to_const(l),
        _ => Const::Int(0, sdust_types::IntKind::IntInfer),
    }
}

fn resolve_variant(ctx: &LowerCtx, path: &[String]) -> usize {
    // path looks like ["Shape", "Circle"] or ["Some"].
    let name = path.last().cloned().unwrap_or_default();
    use sdust_types::DefRef;
    if let Some(DefRef::Variant(_adt, idx)) = ctx.typed.def_map.lookup(&name) {
        return idx;
    }
    // For `Shape.Circle` (qualified path) the second segment is the variant.
    if let Some(DefRef::Variant(_adt, idx)) = ctx.typed.def_map.lookup_path(path) {
        return idx;
    }
    0
}
