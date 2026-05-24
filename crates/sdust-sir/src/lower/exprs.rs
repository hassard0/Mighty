//! Expression lowering: walk HIR expressions, emit SIR statements +
//! terminators, return the final operand carrying the expression's
//! result.

use super::ctx::*;
use super::pats;
use crate::sir::*;
use sdust_hir::{
    BinOp as HirBinOp, ExprId, HirArg, HirBlock, HirExpr, HirLiteral, HirStmt, UnOp as HirUnOp,
};
use sdust_types::{AdtId, DefRef, IntKind};

/// Lower a block expression. Returns the operand carrying the block's
/// tail value (or `Const::Unit` if no tail).
pub fn lower_block(ctx: &mut LowerCtx, fb: &mut FnBuilder, b: &HirBlock) -> Operand {
    for s in &b.stmts {
        lower_stmt(ctx, fb, s);
    }
    if let Some(tail) = b.tail {
        lower_expr(ctx, fb, tail)
    } else {
        Operand::Const(Const::Unit)
    }
}

fn lower_stmt(ctx: &mut LowerCtx, fb: &mut FnBuilder, s: &HirStmt) {
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

fn bind_pat_assign(
    ctx: &mut LowerCtx,
    fb: &mut FnBuilder,
    pat_id: sdust_hir::PatId,
    rhs: Operand,
    mutable: bool,
    _annotated: bool,
) {
    use sdust_hir::HirPat;
    let p = ctx.pkg.pats[pat_id].clone();
    match p {
        HirPat::Binding { name, sub } => {
            // Pick a slice-6 default type — we look up the rhs's type
            // through the typed table when available. For statements we
            // don't have an ExprId for the rhs after lowering, so use
            // SirTy::Error and trust the interpreter's permissive
            // Value enum.
            let ty = SirTy::Error;
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
            let temp = fb.fresh_temp(SirTy::Error);
            fb.push_stmt(Stmt::Assign(Place::local(temp), Rvalue::Use(rhs)));
            for (i, sp) in parts.into_iter().enumerate() {
                let elt_temp = fb.fresh_temp(SirTy::Error);
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
            let l = fb.new_local("", SirTy::Error, mutable, LocalSource::UserLet);
            fb.push_stmt(Stmt::Assign(Place::local(l), Rvalue::Use(rhs)));
        }
    }
}

/// Lower an expression. Returns an Operand carrying the result value.
/// The current block (`fb.cur`) may change as a side effect (control
/// flow expressions emit new blocks).
pub fn lower_expr(ctx: &mut LowerCtx, fb: &mut FnBuilder, eid: ExprId) -> Operand {
    let e = ctx.pkg.exprs[eid].clone();
    match e {
        HirExpr::Literal(lit) => Operand::Const(lit_const(&lit)),
        HirExpr::Path(segments) => resolve_path(ctx, fb, &segments),
        HirExpr::Call { callee, args } => lower_call(ctx, fb, callee, &args),
        HirExpr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let recv = lower_expr(ctx, fb, receiver);
            let arg_ops: Vec<Operand> = args.iter().map(|a| lower_expr(ctx, fb, a.value)).collect();
            // Module receiver -> effect call (heuristic: receiver is a
            // path that resolves to a module).
            if let Some(path) = receiver_module_path(ctx, receiver) {
                let effect = ctx.typed.def_map.effects.get(&infer_effect(&path)).copied();
                if let Some(eff) = effect {
                    let temp = fb.fresh_temp(SirTy::Error);
                    fb.push_stmt(Stmt::EffectInvoke {
                        effect: eff,
                        op: EffectOp::GenericCall {
                            path: path.clone(),
                            method: method.clone(),
                        },
                        args: arg_ops,
                        out: Some(Place::local(temp)),
                    });
                    return Operand::Move(Place::local(temp));
                }
            }
            let temp = fb.fresh_temp(SirTy::Error);
            fb.push_stmt(Stmt::Assign(
                Place::local(temp),
                Rvalue::MethodCall {
                    receiver: recv,
                    method,
                    args: arg_ops,
                },
            ));
            Operand::Move(Place::local(temp))
        }
        HirExpr::Field { receiver, name } => {
            let recv = lower_expr(ctx, fb, receiver);
            // Field-by-name → resolve to index via the receiver's typed
            // ADT.
            let field_idx = resolve_field_index(ctx, receiver, &name).unwrap_or(0);
            let temp = fb.fresh_temp(SirTy::Error);
            let recv_place = operand_to_place(fb, recv);
            fb.push_stmt(Stmt::Assign(
                Place::local(temp),
                Rvalue::FieldRead {
                    receiver: recv_place,
                    field: field_idx,
                },
            ));
            Operand::Move(Place::local(temp))
        }
        HirExpr::Index { receiver, idx } => {
            let recv = lower_expr(ctx, fb, receiver);
            let idx_op = lower_expr(ctx, fb, idx);
            let temp = fb.fresh_temp(SirTy::Error);
            let recv_place = operand_to_place(fb, recv);
            fb.push_stmt(Stmt::Assign(
                Place::local(temp),
                Rvalue::IndexRead {
                    receiver: recv_place,
                    index: idx_op,
                },
            ));
            Operand::Move(Place::local(temp))
        }
        HirExpr::Binary { op, lhs, rhs } => lower_binop(ctx, fb, op, lhs, rhs),
        HirExpr::Unary { op, rhs } => {
            let r = lower_expr(ctx, fb, rhs);
            let sir_op = match op {
                HirUnOp::Neg => UnOp::Neg,
                HirUnOp::Not => UnOp::Not,
                HirUnOp::Deref => {
                    let temp = fb.fresh_temp(SirTy::Error);
                    fb.push_stmt(Stmt::Assign(Place::local(temp), Rvalue::Deref(r)));
                    return Operand::Move(Place::local(temp));
                }
            };
            let temp = fb.fresh_temp(SirTy::Error);
            fb.push_stmt(Stmt::Assign(Place::local(temp), Rvalue::UnOp(sir_op, r)));
            Operand::Move(Place::local(temp))
        }
        HirExpr::If { cond, then, else_ } => lower_if(ctx, fb, cond, then, else_),
        HirExpr::Match { scrutinee, arms } => lower_match(ctx, fb, scrutinee, &arms),
        HirExpr::For { pat, iter, body } => lower_for(ctx, fb, pat, iter, body),
        HirExpr::While { cond, body } => lower_while(ctx, fb, cond, body),
        HirExpr::Loop { body } => lower_loop(ctx, fb, body),
        HirExpr::Return(opt) => {
            let v = match opt {
                Some(e) => lower_expr(ctx, fb, e),
                None => Operand::Const(Const::Unit),
            };
            fb.set_term(Term::Return(v));
            let dead = fb.new_block();
            fb.switch_to(dead);
            Operand::Const(Const::Unit)
        }
        HirExpr::Block(b) => {
            let block = ctx.pkg.blocks[b].clone();
            lower_block(ctx, fb, &block)
        }
        HirExpr::Tuple(parts) => {
            let ops: Vec<Operand> = parts.into_iter().map(|e| lower_expr(ctx, fb, e)).collect();
            let temp = fb.fresh_temp(SirTy::Error);
            fb.push_stmt(Stmt::Assign(Place::local(temp), Rvalue::TupleInit(ops)));
            Operand::Move(Place::local(temp))
        }
        HirExpr::Array(parts) => {
            let ops: Vec<Operand> = parts.into_iter().map(|e| lower_expr(ctx, fb, e)).collect();
            let temp = fb.fresh_temp(SirTy::Error);
            fb.push_stmt(Stmt::Assign(Place::local(temp), Rvalue::ArrayInit(ops)));
            Operand::Move(Place::local(temp))
        }
        HirExpr::Struct { path, fields } => {
            let (adt, variant) = resolve_struct_ctor(ctx, &path);
            // The HIR has named-field initializers; map them onto the
            // ADT's field order so the interpreter sees positional args.
            let field_ops = order_struct_fields(ctx, adt, variant, &fields, fb);
            let temp = fb.fresh_temp(SirTy::Adt(adt, vec![]));
            fb.push_stmt(Stmt::Assign(
                Place::local(temp),
                Rvalue::AdtInit {
                    adt,
                    variant,
                    fields: field_ops,
                },
            ));
            Operand::Move(Place::local(temp))
        }
        HirExpr::Map(entries) => {
            // Slice 6: represent a map as an array of (key, value) tuples.
            let mut entry_ops: Vec<Operand> = vec![];
            for (k, v) in entries {
                let ko = lower_expr(ctx, fb, k);
                let vo = lower_expr(ctx, fb, v);
                let temp = fb.fresh_temp(SirTy::Error);
                fb.push_stmt(Stmt::Assign(
                    Place::local(temp),
                    Rvalue::TupleInit(vec![ko, vo]),
                ));
                entry_ops.push(Operand::Move(Place::local(temp)));
            }
            let temp = fb.fresh_temp(SirTy::Error);
            fb.push_stmt(Stmt::Assign(
                Place::local(temp),
                Rvalue::ArrayInit(entry_ops),
            ));
            Operand::Move(Place::local(temp))
        }
        HirExpr::Send { target, msg, args } => {
            let t = lower_expr(ctx, fb, target);
            let arg_ops: Vec<Operand> = args.iter().map(|a| lower_expr(ctx, fb, a.value)).collect();
            let temp = fb.fresh_temp(SirTy::Unit);
            fb.push_stmt(Stmt::Assign(
                Place::local(temp),
                Rvalue::Send {
                    target: t,
                    msg,
                    args: arg_ops,
                },
            ));
            Operand::Move(Place::local(temp))
        }
        HirExpr::Ask { target, msg, args } => {
            let t = lower_expr(ctx, fb, target);
            let arg_ops: Vec<Operand> = args.iter().map(|a| lower_expr(ctx, fb, a.value)).collect();
            let temp = fb.fresh_temp(SirTy::Error);
            fb.push_stmt(Stmt::Assign(
                Place::local(temp),
                Rvalue::Ask {
                    target: t,
                    msg,
                    args: arg_ops,
                    deadline_ms: None,
                },
            ));
            Operand::Move(Place::local(temp))
        }
        HirExpr::Deadline { inner, dur } => {
            // Lower the deadline expression to its inner, attaching the
            // duration if the inner is an Ask.
            let dur_op = lower_expr(ctx, fb, dur);
            let dl_ms = const_duration_ms(&dur_op);
            // We have to detect Ask after lowering — re-fetch via the
            // typed-arena would be simpler. Easiest pragmatic path: just
            // lower the inner and ignore the deadline at SIR level. The
            // interpreter logs but doesn't enforce in slice 6.
            let inner_op = lower_expr(ctx, fb, inner);
            // Attach deadline metadata by re-emitting an Ask-with-deadline
            // if the inner was an Ask placed in a temp. Best-effort:
            let _ = (dl_ms, inner_op.clone());
            inner_op
        }
        HirExpr::Question(inner) => lower_question(ctx, fb, inner),
        HirExpr::Move(inner) => {
            // `move x` lowers to an explicit Move operand on x.
            let op = lower_expr(ctx, fb, inner);
            match op {
                Operand::Copy(p) | Operand::Move(p) => Operand::Move(p),
                Operand::Const(_) => op,
            }
        }
        HirExpr::Borrow { mutable, inner } => {
            let op = lower_expr(ctx, fb, inner);
            let place = operand_to_place(fb, op);
            let temp = fb.fresh_temp(SirTy::Ref {
                mutable,
                inner: Box::new(SirTy::Error),
            });
            fb.push_stmt(Stmt::Assign(
                Place::local(temp),
                Rvalue::Ref { mutable, place },
            ));
            Operand::Move(Place::local(temp))
        }
        HirExpr::Spawn { is_task: _, inner } => {
            // `spawn AgentName(args)` or `spawn fn`
            // Look at the inner: if it's a Call whose callee is a Path
            // resolving to an agent name, route to AgentSpawn. Otherwise
            // call builtin spawn.
            if let HirExpr::Call { callee, args } = &ctx.pkg.exprs[inner] {
                if let HirExpr::Path(segs) = &ctx.pkg.exprs[*callee] {
                    if let Some(name) = segs.last() {
                        if let Some(agent_id) = ctx.agent_map.get(name).copied() {
                            let arg_ops: Vec<Operand> =
                                args.iter().map(|a| lower_expr(ctx, fb, a.value)).collect();
                            let temp = fb.fresh_temp(SirTy::Error);
                            fb.push_stmt(Stmt::Assign(
                                Place::local(temp),
                                Rvalue::AgentSpawn {
                                    agent: agent_id,
                                    args: arg_ops,
                                },
                            ));
                            return Operand::Move(Place::local(temp));
                        }
                    }
                }
            }
            // Fallback: call the builtin `spawn`.
            let arg = lower_expr(ctx, fb, inner);
            let temp = fb.fresh_temp(SirTy::Error);
            fb.push_stmt(Stmt::Assign(
                Place::local(temp),
                Rvalue::Call {
                    func: FnRef::Builtin(BuiltinId::Spawn),
                    args: vec![arg],
                },
            ));
            Operand::Move(Place::local(temp))
        }
        HirExpr::Detach(inner) | HirExpr::Join(inner) => lower_expr(ctx, fb, inner),
        HirExpr::HtmlTemplate(s) => Operand::Const(Const::Str(s)),
        HirExpr::Unsafe(b) => {
            let block = ctx.pkg.blocks[b].clone();
            lower_block(ctx, fb, &block)
        }
        HirExpr::Arena { name: _, body } => {
            let arena = fb.fresh_arena();
            fb.push_stmt(Stmt::ArenaPush(arena));
            let v = lower_expr(ctx, fb, body);
            fb.push_stmt(Stmt::ArenaPop(arena));
            v
        }
        HirExpr::TaskScope { deadline: _, body } => {
            let block = ctx.pkg.blocks[body].clone();
            lower_block(ctx, fb, &block)
        }
        HirExpr::Budget { entries: _, body } => lower_expr(ctx, fb, body),
        HirExpr::Sandbox {
            name: _,
            entries: _,
            body,
        } => {
            let block = ctx.pkg.blocks[body].clone();
            lower_block(ctx, fb, &block)
        }
        HirExpr::Cast { lhs, ty } => {
            let lhs_op = lower_expr(ctx, fb, lhs);
            let sir_ty = match &ctx.pkg.types[ty] {
                sdust_hir::HirType::Unit => SirTy::Unit,
                _ => SirTy::Error,
            };
            let temp = fb.fresh_temp(sir_ty.clone());
            fb.push_stmt(Stmt::Assign(
                Place::local(temp),
                Rvalue::Cast {
                    src: lhs_op,
                    ty: sir_ty,
                },
            ));
            Operand::Move(Place::local(temp))
        }
        HirExpr::Lambda {
            params: _,
            ret: _,
            body,
        } => {
            // Slice 6: lower the body inline as a thunk that immediately
            // returns its tail value. Real lambda support (closures)
            // arrives in slice 7+. The result is just the block's tail
            // value.
            let b = ctx.pkg.blocks[body].clone();
            lower_block(ctx, fb, &b)
        }
        HirExpr::IfLet {
            pat,
            scrutinee,
            then,
            else_,
        } => {
            // Lower as: temp = scrutinee; pat-match; then-block / else-block.
            let scr = lower_expr(ctx, fb, scrutinee);
            let scr_place = operand_to_place(fb, scr);
            let then_block = fb.new_block();
            let else_block = fb.new_block();
            let join = fb.new_block();
            pats::lower_pat_match(ctx, fb, scr_place, pat, then_block, else_block);

            // THEN
            fb.switch_to(then_block);
            let then_body = ctx.pkg.blocks[then].clone();
            let _t_val = lower_block(ctx, fb, &then_body);
            fb.set_term(Term::Goto(join));

            // ELSE
            fb.switch_to(else_block);
            if let Some(e) = else_ {
                let _e_val = lower_expr(ctx, fb, e);
            }
            fb.set_term(Term::Goto(join));

            fb.switch_to(join);
            Operand::Const(Const::Unit)
        }
        HirExpr::Run(e) => lower_expr(ctx, fb, e),
        HirExpr::PathGeneric { segments, .. } => resolve_path(ctx, fb, &segments),
        HirExpr::Error => Operand::Const(Const::Unit),
    }
}

// ---------------------- helpers ----------------------

fn lit_const(lit: &HirLiteral) -> Const {
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

fn parse_int_suffix(s: Option<&str>) -> IntKind {
    use IntKind::*;
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

fn resolve_path(ctx: &mut LowerCtx, fb: &mut FnBuilder, segments: &[String]) -> Operand {
    // 1. Local variable (single-segment): look up in fb.locals_by_name.
    if segments.len() == 1 {
        if let Some(local) = fb.locals_by_name.get(&segments[0]).copied() {
            // Use copy by default; the borrow checker has already
            // certified the right of-use.
            return Operand::Copy(Place::local(local));
        }
    }
    // 2. Single segment matching a value defref.
    if segments.len() == 1 {
        if let Some(dref) = ctx.typed.def_map.lookup(&segments[0]) {
            match dref {
                DefRef::Fn(fdid) => {
                    if let Some(sirid) = ctx.fn_def_to_sir.get(&fdid.0).copied() {
                        return Operand::Const(Const::FnPtr(FnRef::User(sirid)));
                    }
                    if let Some(b) = builtin_for_name(&segments[0]) {
                        return Operand::Const(Const::FnPtr(FnRef::Builtin(b)));
                    }
                }
                DefRef::Variant(adt, idx) => {
                    // Bare variant reference (e.g. `None`). Produce an
                    // AdtInit with no payload.
                    let temp = fb.fresh_temp(SirTy::Adt(adt, vec![]));
                    fb.push_stmt(Stmt::Assign(
                        Place::local(temp),
                        Rvalue::AdtInit {
                            adt,
                            variant: idx,
                            fields: vec![],
                        },
                    ));
                    return Operand::Move(Place::local(temp));
                }
                _ => {}
            }
        }
    }
    // 3. Multi-segment value path (e.g. `Shape.Circle`).
    if let Some(DefRef::Variant(adt, idx)) = ctx.typed.def_map.lookup_path(segments) {
        let temp = fb.fresh_temp(SirTy::Adt(adt, vec![]));
        fb.push_stmt(Stmt::Assign(
            Place::local(temp),
            Rvalue::AdtInit {
                adt,
                variant: idx,
                fields: vec![],
            },
        ));
        return Operand::Move(Place::local(temp));
    }
    // 4. Builtin fns by name.
    if let Some(b) = builtin_for_name(&segments.join(".")) {
        return Operand::Const(Const::FnPtr(FnRef::Builtin(b)));
    }
    // 5. Fallback: poisoned, but valid.
    let _ = (ctx, fb);
    Operand::Const(Const::Unit)
}

fn builtin_for_name(name: &str) -> Option<BuiltinId> {
    Some(match name {
        "log" => BuiltinId::Log,
        "print" => BuiltinId::Print,
        "panic" => BuiltinId::Panic,
        "spawn" => BuiltinId::Spawn,
        "move" => BuiltinId::Move,
        "fetch" => BuiltinId::Fetch,
        "raw_ptr" => BuiltinId::RawPtr,
        "valid" => BuiltinId::Valid,
        "null" => BuiltinId::Null,
        _ => return None,
    })
}

fn lower_call(ctx: &mut LowerCtx, fb: &mut FnBuilder, callee: ExprId, args: &[HirArg]) -> Operand {
    // Special case: `local.method(args)` parses as a Call whose callee
    // is `Path([local, method])`. If `local` is a binding in scope,
    // re-route as a MethodCall. (The HIR lowerer only emits MethodCall
    // for chained receivers like `expr.method()`, so we have to detect
    // the simple-identifier-receiver case here.)
    if let HirExpr::Path(segments) = &ctx.pkg.exprs[callee] {
        if segments.len() >= 2 {
            let head = &segments[0];
            if fb.locals_by_name.contains_key(head) {
                let method = segments.last().cloned().unwrap_or_default();
                // Build receiver expression from the segments-prefix.
                let recv_local = fb.locals_by_name.get(head).copied().unwrap();
                let recv_op = Operand::Copy(Place::local(recv_local));
                // For 3+ segments (`x.a.b.foo()`), project intermediate
                // fields. We don't know field indices for arbitrary
                // chains in slice 6, so we route the whole thing through
                // a MethodCall on the head value (interpreter's method
                // table is permissive for unknown names).
                if segments.len() > 2 {
                    // Best-effort: leave recv as the head local.
                    let _ = recv_op.clone();
                }
                let arg_ops: Vec<Operand> =
                    args.iter().map(|a| lower_expr(ctx, fb, a.value)).collect();
                let temp = fb.fresh_temp(SirTy::Error);
                fb.push_stmt(Stmt::Assign(
                    Place::local(temp),
                    Rvalue::MethodCall {
                        receiver: recv_op,
                        method,
                        args: arg_ops,
                    },
                ));
                return Operand::Move(Place::local(temp));
            }
            // Receiver is a module (e.g. `fs.read(p)`) → effect invoke.
            if let Some(sdust_types::DefRef::Module(_)) = ctx.typed.def_map.lookup(head) {
                let method = segments.last().cloned().unwrap_or_default();
                let path: Vec<String> = segments[..segments.len() - 1].to_vec();
                let arg_ops: Vec<Operand> =
                    args.iter().map(|a| lower_expr(ctx, fb, a.value)).collect();
                let effect_name = infer_effect(&path);
                let effect = ctx
                    .typed
                    .def_map
                    .effects
                    .get(&effect_name)
                    .copied()
                    .unwrap_or(sdust_types::EffectId(0));
                let temp = fb.fresh_temp(SirTy::Error);
                fb.push_stmt(Stmt::EffectInvoke {
                    effect,
                    op: EffectOp::GenericCall { path, method },
                    args: arg_ops,
                    out: Some(Place::local(temp)),
                });
                return Operand::Move(Place::local(temp));
            }
        }
    }
    let arg_ops: Vec<Operand> = args.iter().map(|a| lower_expr(ctx, fb, a.value)).collect();

    // Inspect the callee to figure out which fn to call.
    let func = resolve_callee(ctx, callee);
    let temp = fb.fresh_temp(SirTy::Error);
    fb.push_stmt(Stmt::Assign(
        Place::local(temp),
        Rvalue::Call {
            func,
            args: arg_ops,
        },
    ));
    Operand::Move(Place::local(temp))
}

fn resolve_callee(ctx: &LowerCtx, callee: ExprId) -> FnRef {
    let e = &ctx.pkg.exprs[callee];
    match e {
        HirExpr::Path(segments) => {
            if segments.len() == 1 {
                if let Some(b) = builtin_for_name(&segments[0]) {
                    return FnRef::Builtin(b);
                }
                if let Some(DefRef::Fn(fdid)) = ctx.typed.def_map.lookup(&segments[0]) {
                    if let Some(sirid) = ctx.fn_def_to_sir.get(&fdid.0).copied() {
                        return FnRef::User(sirid);
                    }
                }
            }
            if let Some(DefRef::Fn(fdid)) = ctx.typed.def_map.lookup_path(segments) {
                if let Some(sirid) = ctx.fn_def_to_sir.get(&fdid.0).copied() {
                    return FnRef::User(sirid);
                }
            }
            // Multi-segment path that didn't resolve to a fn — treat as
            // an unknown extern.
            FnRef::Builtin(BuiltinId::Extern(segments.join(".")))
        }
        HirExpr::PathGeneric { segments, .. } => {
            if segments.len() == 1 {
                if let Some(DefRef::Fn(fdid)) = ctx.typed.def_map.lookup(&segments[0]) {
                    if let Some(sirid) = ctx.fn_def_to_sir.get(&fdid.0).copied() {
                        return FnRef::User(sirid);
                    }
                }
            }
            FnRef::Builtin(BuiltinId::Extern(segments.join(".")))
        }
        _ => FnRef::Builtin(BuiltinId::Extern("__unknown".into())),
    }
}

fn lower_binop(
    ctx: &mut LowerCtx,
    fb: &mut FnBuilder,
    op: HirBinOp,
    lhs: ExprId,
    rhs: ExprId,
) -> Operand {
    use HirBinOp::*;
    // Compound assignments + plain Assign: handle as side-effecting.
    if matches!(
        op,
        Assign
            | AssignAdd
            | AssignSub
            | AssignMul
            | AssignDiv
            | AssignRem
            | AssignBitAnd
            | AssignBitOr
            | AssignBitXor
            | AssignShl
            | AssignShr
    ) {
        return lower_assign(ctx, fb, op, lhs, rhs);
    }
    // Range -> array placeholder. Real Range type is post-v0.1.
    if matches!(op, Range | RangeEq) {
        let lo = lower_expr(ctx, fb, lhs);
        let hi = lower_expr(ctx, fb, rhs);
        let temp = fb.fresh_temp(SirTy::Error);
        fb.push_stmt(Stmt::Assign(
            Place::local(temp),
            Rvalue::TupleInit(vec![lo, hi]),
        ));
        return Operand::Move(Place::local(temp));
    }
    let l = lower_expr(ctx, fb, lhs);
    let r = lower_expr(ctx, fb, rhs);
    let sir_op = match op {
        Add => BinOp::Add,
        Sub => BinOp::Sub,
        Mul => BinOp::Mul,
        Div => BinOp::Div,
        Rem => BinOp::Rem,
        BitAnd => BinOp::BitAnd,
        BitOr => BinOp::BitOr,
        BitXor => BinOp::BitXor,
        Shl => BinOp::Shl,
        Shr => BinOp::Shr,
        Eq => BinOp::Eq,
        Ne => BinOp::Ne,
        Lt => BinOp::Lt,
        Le => BinOp::Le,
        Gt => BinOp::Gt,
        Ge => BinOp::Ge,
        And => BinOp::And,
        Or => BinOp::Or,
        _ => BinOp::Add, // unreachable per match above
    };
    let temp = fb.fresh_temp(SirTy::Error);
    fb.push_stmt(Stmt::Assign(
        Place::local(temp),
        Rvalue::BinOp(sir_op, l, r),
    ));
    Operand::Move(Place::local(temp))
}

fn lower_assign(
    ctx: &mut LowerCtx,
    fb: &mut FnBuilder,
    op: HirBinOp,
    lhs: ExprId,
    rhs: ExprId,
) -> Operand {
    use HirBinOp::*;
    let rhs_op = lower_expr(ctx, fb, rhs);
    // For compound ops, compute lhs first, then bin-op, then write.
    let lhs_op = lower_expr(ctx, fb, lhs);
    let lhs_place = operand_to_place(fb, lhs_op);
    let final_rhs = if matches!(op, Assign) {
        rhs_op
    } else {
        let bo = match op {
            AssignAdd => BinOp::Add,
            AssignSub => BinOp::Sub,
            AssignMul => BinOp::Mul,
            AssignDiv => BinOp::Div,
            AssignRem => BinOp::Rem,
            AssignBitAnd => BinOp::BitAnd,
            AssignBitOr => BinOp::BitOr,
            AssignBitXor => BinOp::BitXor,
            AssignShl => BinOp::Shl,
            AssignShr => BinOp::Shr,
            _ => BinOp::Add,
        };
        let temp = fb.fresh_temp(SirTy::Error);
        fb.push_stmt(Stmt::Assign(
            Place::local(temp),
            Rvalue::BinOp(bo, Operand::Copy(lhs_place.clone()), rhs_op),
        ));
        Operand::Move(Place::local(temp))
    };
    fb.push_stmt(Stmt::Assign(lhs_place, Rvalue::Use(final_rhs)));
    let _ = ctx;
    Operand::Const(Const::Unit)
}

fn lower_if(
    ctx: &mut LowerCtx,
    fb: &mut FnBuilder,
    cond: ExprId,
    then: sdust_hir::BlockId,
    else_: Option<ExprId>,
) -> Operand {
    let cond_op = lower_expr(ctx, fb, cond);
    let then_block = fb.new_block();
    let else_block = fb.new_block();
    let join = fb.new_block();
    let result = fb.fresh_temp(SirTy::Error);
    fb.set_term(Term::If {
        cond: cond_op,
        then: then_block,
        else_: else_block,
    });

    // THEN
    fb.switch_to(then_block);
    let then_body = ctx.pkg.blocks[then].clone();
    let t_val = lower_block(ctx, fb, &then_body);
    fb.push_stmt(Stmt::Assign(Place::local(result), Rvalue::Use(t_val)));
    fb.set_term(Term::Goto(join));

    // ELSE
    fb.switch_to(else_block);
    let e_val = match else_ {
        Some(e) => lower_expr(ctx, fb, e),
        None => Operand::Const(Const::Unit),
    };
    fb.push_stmt(Stmt::Assign(Place::local(result), Rvalue::Use(e_val)));
    fb.set_term(Term::Goto(join));

    fb.switch_to(join);
    Operand::Move(Place::local(result))
}

fn lower_match(
    ctx: &mut LowerCtx,
    fb: &mut FnBuilder,
    scrutinee: ExprId,
    arms: &[sdust_hir::HirMatchArm],
) -> Operand {
    let scr = lower_expr(ctx, fb, scrutinee);
    let scr_place = operand_to_place(fb, scr);
    let result = fb.fresh_temp(SirTy::Error);
    let join = fb.new_block();

    let mut next_test = fb.current_block();
    for arm in arms {
        fb.switch_to(next_test);
        let success = fb.new_block();
        let failure = fb.new_block();
        pats::lower_pat_match(ctx, fb, scr_place.clone(), arm.pat, success, failure);

        // SUCCESS: lower the arm body.
        fb.switch_to(success);
        let v = lower_expr(ctx, fb, arm.body);
        fb.push_stmt(Stmt::Assign(Place::local(result), Rvalue::Use(v)));
        fb.set_term(Term::Goto(join));

        next_test = failure;
    }
    // Final fallthrough: panic SD5005.
    fb.switch_to(next_test);
    fb.set_term(Term::Panic {
        msg: Operand::Const(Const::Str("SD5005 unreachable match".into())),
    });

    fb.switch_to(join);
    Operand::Move(Place::local(result))
}

fn lower_for(
    ctx: &mut LowerCtx,
    fb: &mut FnBuilder,
    _pat: sdust_hir::PatId,
    iter: ExprId,
    body: sdust_hir::BlockId,
) -> Operand {
    // v0.4: loop semantics fixed. `for x in iter { body }` lowers to a
    // proper header/body/exit triple where the body's terminator routes
    // back to the header.
    //
    // Slice 6 still has NO iterator-exhaustion check (real `Iterator`
    // protocol arrives post-v0.1), so on its own this would spin
    // forever — but the interpreter's step budget (default 1M) caps
    // any runaway. Examples that only compile (`sdust check`) are
    // unaffected; examples that actually *execute* a for-loop will
    // trip `RunResult::BudgetExceeded` until v0.5 wires the iterator
    // protocol + `break`/`continue` HIR nodes. Tracked in
    // SELFHOST_V0_4_NOTES.md / SLICE_V0_4.md.
    let iter_op = lower_expr(ctx, fb, iter);
    let iter_local = fb.fresh_temp(SirTy::Error);
    fb.push_stmt(Stmt::Assign(Place::local(iter_local), Rvalue::Use(iter_op)));

    let header = fb.new_block();
    let body_block = fb.new_block();
    let exit = fb.new_block();
    fb.set_term(Term::Goto(header));

    fb.switch_to(header);
    // Branch unconditionally to body — slice-6 simplification (no
    // iterator exhaustion). v0.5 will materialise the .len/.next probe
    // here so `for` actually terminates structurally.
    fb.set_term(Term::Goto(body_block));

    fb.switch_to(body_block);
    let block = ctx.pkg.blocks[body].clone();
    let _ = lower_block(ctx, fb, &block);
    // v0.4 loop-fix: route back to header so each pass re-enters the
    // body. Previously this was `Goto(exit)` which broke every loop
    // shape after one iteration.
    fb.set_term(Term::Goto(header));

    fb.switch_to(exit);
    Operand::Const(Const::Unit)
}

fn lower_while(
    ctx: &mut LowerCtx,
    fb: &mut FnBuilder,
    cond: ExprId,
    body: sdust_hir::BlockId,
) -> Operand {
    let header = fb.new_block();
    let body_block = fb.new_block();
    let exit = fb.new_block();
    fb.set_term(Term::Goto(header));

    fb.switch_to(header);
    let c = lower_expr(ctx, fb, cond);
    fb.set_term(Term::If {
        cond: c,
        then: body_block,
        else_: exit,
    });

    fb.switch_to(body_block);
    let block = ctx.pkg.blocks[body].clone();
    let _ = lower_block(ctx, fb, &block);
    // v0.4 loop-fix: route back to header so `cond` is re-evaluated.
    // Previously the terminator was `Goto(exit)`, which silently
    // collapsed every `while` into a one-shot if-block.
    fb.set_term(Term::Goto(header));

    fb.switch_to(exit);
    Operand::Const(Const::Unit)
}

fn lower_loop(ctx: &mut LowerCtx, fb: &mut FnBuilder, body: sdust_hir::BlockId) -> Operand {
    let header = fb.new_block();
    let exit = fb.new_block();
    fb.set_term(Term::Goto(header));

    fb.switch_to(header);
    let block = ctx.pkg.blocks[body].clone();
    let _ = lower_block(ctx, fb, &block);
    // v0.4 loop-fix: route back to header. There is still no `break`
    // in HIR, so a bare `loop { … }` that does not panic / return /
    // try-return-err runs until the interpreter's step budget trips.
    // The borrow checker continues to reject obvious uses; the v0.5
    // plan adds `break`/`continue` HIR nodes for early exit. Tracked
    // in SLICE_V0_4.md.
    fb.set_term(Term::Goto(header));

    fb.switch_to(exit);
    Operand::Const(Const::Unit)
}

fn lower_question(ctx: &mut LowerCtx, fb: &mut FnBuilder, inner: ExprId) -> Operand {
    // `inner?` ::=
    //   let tmp = inner;
    //   switch_variant tmp { Ok => { result = tmp.Ok.0 }, Err => try_return_err tmp.Err.0 }
    let v = lower_expr(ctx, fb, inner);
    let v_place = operand_to_place(fb, v);

    let result = fb.fresh_temp(SirTy::Error);
    let ok_block = fb.new_block();
    let err_block = fb.new_block();
    let join = fb.new_block();

    fb.set_term(Term::SwitchVariant {
        discr: Operand::Copy(v_place.clone()),
        adt: AdtId(0),
        // Convention: Result.Ok = 0, Result.Err = 1 (matches prelude).
        arms: vec![(0, ok_block), (1, err_block)],
        default: err_block,
    });

    fb.switch_to(ok_block);
    fb.push_stmt(Stmt::Assign(
        Place::local(result),
        Rvalue::Use(Operand::Move(Place {
            local: v_place.local,
            proj: {
                let mut p = v_place.proj.clone();
                p.push(Projection::VariantField(0, 0));
                p
            },
        })),
    ));
    fb.set_term(Term::Goto(join));

    fb.switch_to(err_block);
    let err_payload = Operand::Move(Place {
        local: v_place.local,
        proj: {
            let mut p = v_place.proj.clone();
            p.push(Projection::VariantField(1, 0));
            p
        },
    });
    fb.set_term(Term::TryReturnErr(err_payload));

    fb.switch_to(join);
    Operand::Move(Place::local(result))
}

fn operand_to_place(fb: &mut FnBuilder, op: Operand) -> Place {
    match op {
        Operand::Copy(p) | Operand::Move(p) => p,
        Operand::Const(c) => {
            let temp = fb.fresh_temp(SirTy::Error);
            fb.push_stmt(Stmt::Assign(Place::local(temp), Rvalue::Const(c)));
            Place::local(temp)
        }
    }
}

fn resolve_struct_ctor(ctx: &LowerCtx, path: &[String]) -> (AdtId, usize) {
    if let Some(DefRef::Variant(adt, idx)) = ctx.typed.def_map.lookup_path(path) {
        return (adt, idx);
    }
    if let Some(DefRef::Adt(adt)) = ctx.typed.def_map.lookup_path(path) {
        return (adt, 0);
    }
    (AdtId(0), 0)
}

fn order_struct_fields(
    ctx: &mut LowerCtx,
    adt: AdtId,
    variant: usize,
    fields: &[(String, ExprId)],
    fb: &mut FnBuilder,
) -> Vec<Operand> {
    let def = match ctx.typed.def_map.adt(adt) {
        Some(d) => d.clone(),
        None => {
            return fields
                .iter()
                .map(|(_, e)| lower_expr(ctx, fb, *e))
                .collect();
        }
    };
    if let Some(v) = def.variants.get(variant) {
        let mut ordered: Vec<Operand> = Vec::with_capacity(v.fields.len());
        for f in &v.fields {
            let val = if let Some((_, e)) = fields.iter().find(|(n, _)| Some(n) == f.name.as_ref())
            {
                lower_expr(ctx, fb, *e)
            } else {
                Operand::Const(Const::Unit)
            };
            ordered.push(val);
        }
        return ordered;
    }
    fields
        .iter()
        .map(|(_, e)| lower_expr(ctx, fb, *e))
        .collect()
}

fn resolve_field_index(ctx: &LowerCtx, receiver: ExprId, name: &str) -> Option<usize> {
    // Use the receiver's typed Adt to look up the field index.
    let ty = ctx.expr_ty(receiver);
    let td = ctx.typed.ty_arena.get(ty);
    let adt_id = match td {
        sdust_types::TyData::Adt(id, _) => Some(*id),
        sdust_types::TyData::Ref { inner, .. } => match ctx.typed.ty_arena.get(*inner) {
            sdust_types::TyData::Adt(id, _) => Some(*id),
            _ => None,
        },
        _ => None,
    }?;
    let def = ctx.typed.def_map.adt(adt_id)?;
    def.variants
        .first()?
        .fields
        .iter()
        .position(|f| f.name.as_deref() == Some(name))
}

fn receiver_module_path(ctx: &LowerCtx, receiver: ExprId) -> Option<Vec<String>> {
    let e = &ctx.pkg.exprs[receiver];
    if let HirExpr::Path(segs) = e {
        // Receiver resolves to a module if the resolved type is
        // `Module(name)`.
        let ty = ctx.expr_ty(receiver);
        if matches!(ctx.typed.ty_arena.get(ty), sdust_types::TyData::Module(_)) {
            return Some(segs.clone());
        }
    }
    None
}

fn infer_effect(path: &[String]) -> String {
    if path.is_empty() {
        return "io".into();
    }
    match path[0].as_str() {
        "fs" | "std" if path.get(1).map(|s| s.as_str()) == Some("fs") => "fs".into(),
        "fs" => "fs".into(),
        "net" => "net".into(),
        "dom" => "dom".into(),
        "model" => "model".into(),
        "time" | "clock" => "time".into(),
        _ => "io".into(),
    }
}

fn const_duration_ms(o: &Operand) -> Option<u64> {
    if let Operand::Const(Const::Duration { value, unit }) = o {
        let ms = match unit.as_str() {
            "ns" => *value / 1_000_000,
            "us" => *value / 1_000,
            "ms" => *value,
            "s" => *value * 1_000,
            "m" => *value * 60_000,
            "h" => *value * 3_600_000,
            _ => *value,
        };
        Some(ms)
    } else {
        None
    }
}
