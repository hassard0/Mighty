//! Expression lowering: walk HIR expressions, emit SIR statements +
//! terminators, return the final operand carrying the expression's
//! result.

use super::ctx::{FnBuilder, LoopFrame, LowerCtx};
use super::pats;
use super::stmts;
use crate::ir::*;
use mty_hir::{BinOp as HirBinOp, ExprId, HirArg, HirBlock, HirExpr, HirLiteral, UnOp as HirUnOp};
use mty_types::{AdtId, CapFamily, DefRef, IntKind, TyData};

/// Lower a block expression. Returns the operand carrying the block's
/// tail value (or `Const::Unit` if no tail).
pub fn lower_block(ctx: &mut LowerCtx, fb: &mut FnBuilder, b: &HirBlock) -> Operand {
    for s in &b.stmts {
        stmts::lower_stmt(ctx, fb, s);
    }
    if let Some(tail) = b.tail {
        lower_expr(ctx, fb, tail)
    } else {
        Operand::Const(Const::Unit)
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
            // v0.6: DOM cap receiver -> first-class `BuiltinId::DomOp`.
            // The wasm32-web backend routes this through `emit_dom_call`
            // to the `mty:web/dom` import set; the SIR interpreter
            // routes it through `host.extern_call("dom.<op>", args)` so
            // headless tests don't crash. Receiver is implicit (the JS
            // shim is the only DOM there is), so we pass only the
            // user-supplied args.
            if is_dom_cap_receiver(ctx, receiver) {
                let temp = fb.fresh_temp(IrTy::Error);
                fb.push_stmt(Stmt::Assign(
                    Place::local(temp),
                    Rvalue::Call {
                        func: FnRef::Builtin(BuiltinId::DomOp(method.clone())),
                        args: arg_ops,
                    },
                ));
                let _ = recv; // receiver value is dropped — DOM is host-side
                return Operand::Move(Place::local(temp));
            }
            // v0.25 — Canvas-handle receiver -> first-class
            // `BuiltinId::CanvasOp(kind)`. The wasm32-web emitter
            // (already wired by v0.24 Track A) routes these through
            // `emit_canvas_call` to the eight `mty:web/canvas@0.1`
            // imports. We detect the receiver by per-fn
            // `canvas_locals` tracking (populated when a let-binding
            // is initialized from `std.web.Canvas.new(...)` or moves
            // a previously-marked canvas local) AND the method name
            // is one of the canonical canvas methods. Mirrors the
            // DomOp branch above; receiver value is dropped because
            // canvas dispatch is host-side via the WIT import. Closes
            // the v0.23 unfinished business documented in
            // `dev/history/notes/DEMO06_CANVAS_DIRECT_V0_24_NOTES.md`
            // §A.
            if let Some(kind) = canvas_op_for_method(&method) {
                if is_canvas_handle_receiver(ctx, fb, receiver) {
                    let temp = fb.fresh_temp(IrTy::Error);
                    fb.push_stmt(Stmt::Assign(
                        Place::local(temp),
                        Rvalue::Call {
                            func: FnRef::Builtin(BuiltinId::CanvasOp(kind)),
                            args: arg_ops,
                        },
                    ));
                    let _ = recv; // receiver dropped — canvas is host-side
                    return Operand::Move(Place::local(temp));
                }
            }
            // Module receiver -> effect call (heuristic: receiver is a
            // path that resolves to a module).
            if let Some(path) = receiver_module_path(ctx, receiver) {
                let effect = ctx.typed.def_map.effects.get(&infer_effect(&path)).copied();
                if let Some(eff) = effect {
                    let temp = fb.fresh_temp(IrTy::Error);
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
            let temp = fb.fresh_temp(IrTy::Error);
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
            // ADT. v0.29 Track A: also consult the permissive stdlib
            // field-name table so `consensus.majority` / `.dissents` /
            // `.total_cost_cents` etc. read the right field on the
            // synthetic Consensus / MemberReply / DollarBudget structs
            // synthesised by the SIR swarm dispatcher (those values
            // carry `IrTy::Error` at lowering time — they're built by
            // a `BuiltinId::Swarm` call whose return type isn't a
            // typed ADT in the user program). Without this fallback
            // every chained access would read field 0 and lose the
            // shape.
            let field_idx = resolve_field_index(ctx, receiver, &name)
                .or_else(|| stdlib_field_index(&name))
                .unwrap_or(0);
            let temp = fb.fresh_temp(IrTy::Error);
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
            let temp = fb.fresh_temp(IrTy::Error);
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
                    let temp = fb.fresh_temp(IrTy::Error);
                    fb.push_stmt(Stmt::Assign(Place::local(temp), Rvalue::Deref(r)));
                    return Operand::Move(Place::local(temp));
                }
            };
            let temp = fb.fresh_temp(IrTy::Error);
            fb.push_stmt(Stmt::Assign(Place::local(temp), Rvalue::UnOp(sir_op, r)));
            Operand::Move(Place::local(temp))
        }
        HirExpr::If { cond, then, else_ } => lower_if(ctx, fb, cond, then, else_),
        HirExpr::Match { scrutinee, arms } => lower_match(ctx, fb, scrutinee, &arms),
        HirExpr::For { pat, iter, body } => lower_for(ctx, fb, pat, iter, body),
        HirExpr::While { cond, body } => lower_while(ctx, fb, cond, body),
        HirExpr::WhileLet {
            pat,
            scrutinee,
            body,
        } => lower_while_let(ctx, fb, pat, scrutinee, body),
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
        HirExpr::Break(opt) => {
            // v0.5: `break <value>?` unwinds to the nearest enclosing
            // loop. The value (if present) is stored in the loop's
            // result local before jumping to the exit BB.
            let v = match opt {
                Some(e) => lower_expr(ctx, fb, e),
                None => Operand::Const(Const::Unit),
            };
            if let Some(frame) = fb.current_loop() {
                fb.push_stmt(Stmt::Assign(
                    Place::local(frame.result_local),
                    Rvalue::Use(v),
                ));
                fb.set_term(Term::Goto(frame.exit_target));
            } else {
                // Stray `break` outside a loop: treat as Unreachable.
                // The type / borrow checker should have flagged it.
                fb.set_term(Term::Unreachable);
            }
            let dead = fb.new_block();
            fb.switch_to(dead);
            Operand::Const(Const::Unit)
        }
        HirExpr::Continue => {
            if let Some(frame) = fb.current_loop() {
                fb.set_term(Term::Goto(frame.continue_target));
            } else {
                fb.set_term(Term::Unreachable);
            }
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
            let temp = fb.fresh_temp(IrTy::Error);
            fb.push_stmt(Stmt::Assign(Place::local(temp), Rvalue::TupleInit(ops)));
            Operand::Move(Place::local(temp))
        }
        HirExpr::Array(parts) => {
            let ops: Vec<Operand> = parts.into_iter().map(|e| lower_expr(ctx, fb, e)).collect();
            let temp = fb.fresh_temp(IrTy::Error);
            fb.push_stmt(Stmt::Assign(Place::local(temp), Rvalue::ArrayInit(ops)));
            Operand::Move(Place::local(temp))
        }
        HirExpr::Struct { path, fields } => {
            let (adt, variant) = resolve_struct_ctor(ctx, &path);
            // The HIR has named-field initializers; map them onto the
            // ADT's field order so the interpreter sees positional args.
            let field_ops = order_struct_fields(ctx, adt, variant, &fields, fb);
            let temp = fb.fresh_temp(IrTy::Adt(adt, vec![]));
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
                let temp = fb.fresh_temp(IrTy::Error);
                fb.push_stmt(Stmt::Assign(
                    Place::local(temp),
                    Rvalue::TupleInit(vec![ko, vo]),
                ));
                entry_ops.push(Operand::Move(Place::local(temp)));
            }
            let temp = fb.fresh_temp(IrTy::Error);
            fb.push_stmt(Stmt::Assign(
                Place::local(temp),
                Rvalue::ArrayInit(entry_ops),
            ));
            Operand::Move(Place::local(temp))
        }
        HirExpr::Send { target, msg, args } => {
            let t = lower_expr(ctx, fb, target);
            let arg_ops: Vec<Operand> = args.iter().map(|a| lower_expr(ctx, fb, a.value)).collect();
            let temp = fb.fresh_temp(IrTy::Unit);
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
            let temp = fb.fresh_temp(IrTy::Error);
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
            let temp = fb.fresh_temp(IrTy::Ref {
                mutable,
                inner: Box::new(IrTy::Error),
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
                            let temp = fb.fresh_temp(IrTy::Error);
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
            let temp = fb.fresh_temp(IrTy::Error);
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
                mty_hir::HirType::Unit => IrTy::Unit,
                _ => IrTy::Error,
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

fn parse_float_suffix(s: Option<&str>) -> mty_types::FloatKind {
    use mty_types::FloatKind::*;
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
    // v0.29 Track A — multi-segment path starting with a local is a
    // chained field access (`local.field` or `local.f1.f2.f3`). The
    // CST parses `c.budget_exhausted` as a PATH_EXPR with segments
    // ["c", "budget_exhausted"] rather than a FIELD_EXPR, so without
    // this projection step the access falls through to `Const::Unit`
    // (the fallback at step 6) and the condition reads as Unit.
    // Project each remaining segment via `Rvalue::FieldRead` using
    // the permissive `stdlib_field_index` table (user struct field
    // names are not known at this layer; they fall back to index 0
    // — same behaviour as the bare `HirExpr::Field` lowering).
    if segments.len() >= 2 {
        if let Some(local) = fb.locals_by_name.get(&segments[0]).copied() {
            let mut cur = Operand::Copy(Place::local(local));
            for seg in &segments[1..] {
                let field_idx = stdlib_field_index(seg).unwrap_or(0);
                let projected = fb.fresh_temp(IrTy::Error);
                let recv_place = operand_to_place(fb, cur);
                fb.push_stmt(Stmt::Assign(
                    Place::local(projected),
                    Rvalue::FieldRead {
                        receiver: recv_place,
                        field: field_idx,
                    },
                ));
                cur = Operand::Move(Place::local(projected));
            }
            return cur;
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
                    let temp = fb.fresh_temp(IrTy::Adt(adt, vec![]));
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
        let temp = fb.fresh_temp(IrTy::Adt(adt, vec![]));
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
    // 5. v0.29 Track A — bare-path stdlib constants. `ConsensusStrategy.Majority`,
    // `ConsensusStrategy.Unanimous`, `ConsensusStrategy.FirstAgreed` parse as
    // multi-segment `Path` expressions with no variant in the def-map (the
    // `ConsensusStrategy` ADT isn't registered in user source — it's a
    // permissive stdlib name). Lower them as a zero-arg builtin call so the
    // interpreter's `try_stdlib_ctor` arm synthesises the matching tagged
    // value instead of falling through to `Const::Unit` (which would erase
    // the strategy from the swarm call site).
    if is_stdlib_const_path(segments) {
        let joined = segments.join(".");
        let temp = fb.fresh_temp(IrTy::Error);
        fb.push_stmt(Stmt::Assign(
            Place::local(temp),
            Rvalue::Call {
                func: FnRef::Builtin(BuiltinId::Extern(joined)),
                args: vec![],
            },
        ));
        return Operand::Move(Place::local(temp));
    }
    // 6. Fallback: poisoned, but valid.
    let _ = (ctx, fb);
    Operand::Const(Const::Unit)
}

/// v0.29 Track A — recognise the bare-path stdlib constants that
/// `resolve_path` must lower as zero-arg builtin calls so the
/// interpreter's `try_stdlib_ctor` arm sees them.
fn is_stdlib_const_path(segments: &[String]) -> bool {
    if segments.len() != 2 {
        return false;
    }
    matches!(
        (segments[0].as_str(), segments[1].as_str()),
        (
            "ConsensusStrategy",
            "Majority" | "Unanimous" | "FirstAgreed"
        )
    )
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
        // v0.29 Track A — `swarm(prompt, panel, budget, strategy)` lowers
        // to `BuiltinId::Swarm` so the SIR interpreter can resolve the
        // consensus synchronously (without going through the host's
        // extern table, which would return `Value::Unit`). Real async
        // dispatch lives in `mty_stdlib::swarm::swarm` for `mty build`.
        "swarm" => BuiltinId::Swarm,
        _ => return None,
    })
}

fn lower_call(ctx: &mut LowerCtx, fb: &mut FnBuilder, callee: ExprId, args: &[HirArg]) -> Operand {
    // v0.15 — variant-constructor call detection. `Some(42)`, `Ok(v)`,
    // `Result.Err(e)`, `Maybe.Just(x)`, etc. parse as a Call whose callee
    // resolves (via the type checker's def-map) to a variant constructor
    // — NOT a fn. Before v0.15 we routed these through the function-call
    // codepath, which then fell back to `BuiltinId::Extern(name)` and
    // broke the Wasm AOT pipeline. The Rust interpreter happened to
    // tolerate it because `extern_call` was a no-op, but the self-host
    // emitter needs the structurally correct
    // `Rvalue::AdtInit { variant, fields }`.
    //
    // Detection mirrors `mty_types::check::resolve_path_expr`:
    //   1. single-segment `Some(x)`   -> def_map.lookup(name) -> Variant
    //   2. multi-segment dotted name  -> def_map.lookup_path -> Variant
    //   3. `Enum.Variant(x)` pattern: first segment is an Adt; second is
    //      the variant short name. Variants are registered by short
    //      name only, so we look up the variant inside the Adt.
    if let Some((adt, variant)) = variant_for_call_callee(ctx, callee) {
        let arg_ops: Vec<Operand> = args.iter().map(|a| lower_expr(ctx, fb, a.value)).collect();
        let temp = fb.fresh_temp(IrTy::Adt(adt, vec![]));
        fb.push_stmt(Stmt::Assign(
            Place::local(temp),
            Rvalue::AdtInit {
                adt,
                variant,
                fields: arg_ops,
            },
        ));
        return Operand::Move(Place::local(temp));
    }
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
                let mut recv_op = Operand::Copy(Place::local(recv_local));
                // v0.29 Track A — for 3+ segments (`x.a.b.foo()`), emit
                // intermediate `Rvalue::FieldRead` projections so the
                // receiver of the final MethodCall is the right slot.
                // Pre-v0.29 we dropped the intermediate fields and routed
                // through the head local directly, which silently lost
                // the `.majority` / `.dissents` / `.body` accesses that
                // demo 08's swarm consensus rendering depends on. Field
                // indices come from `stdlib_field_index` (permissive
                // table); user struct fields fall back to index 0 — same
                // behaviour as the bare-field-access lowering above.
                if segments.len() > 2 {
                    for seg in &segments[1..segments.len() - 1] {
                        let field_idx = stdlib_field_index(seg).unwrap_or(0);
                        let projected = fb.fresh_temp(IrTy::Error);
                        let recv_place = operand_to_place(fb, recv_op);
                        fb.push_stmt(Stmt::Assign(
                            Place::local(projected),
                            Rvalue::FieldRead {
                                receiver: recv_place,
                                field: field_idx,
                            },
                        ));
                        recv_op = Operand::Move(Place::local(projected));
                    }
                }
                let arg_ops: Vec<Operand> =
                    args.iter().map(|a| lower_expr(ctx, fb, a.value)).collect();
                // v0.6: Dom-cap receiver -> first-class DomOp builtin
                // call (parallel branch to the MethodCall arm in
                // `lower_expr::HirExpr::MethodCall`). The receiver
                // value is dropped because DOM dispatch goes through
                // the wasm32-web JS shim, not through the SIR value.
                if segments.len() == 2
                    && matches!(
                        &fb.locals[recv_local.0 as usize].ty,
                        IrTy::Cap {
                            family: CapFamily::Dom,
                            ..
                        }
                    )
                {
                    let temp = fb.fresh_temp(IrTy::Error);
                    fb.push_stmt(Stmt::Assign(
                        Place::local(temp),
                        Rvalue::Call {
                            func: FnRef::Builtin(BuiltinId::DomOp(method)),
                            args: arg_ops,
                        },
                    ));
                    return Operand::Move(Place::local(temp));
                }
                // v0.25 — parallel canvas-handle dispatch for the
                // `local.method(args)` parse shape (mirrors the
                // canvas branch in the MethodCall arm of
                // `lower_expr`). Same predicate: the local was
                // previously marked as a canvas handle AND the
                // method name matches one of the eight canonical
                // `mty:web/canvas@0.1` ops.
                if segments.len() == 2 && fb.is_canvas_local(recv_local) {
                    if let Some(kind) = canvas_op_for_method(&method) {
                        let temp = fb.fresh_temp(IrTy::Error);
                        fb.push_stmt(Stmt::Assign(
                            Place::local(temp),
                            Rvalue::Call {
                                func: FnRef::Builtin(BuiltinId::CanvasOp(kind)),
                                args: arg_ops,
                            },
                        ));
                        return Operand::Move(Place::local(temp));
                    }
                }
                let temp = fb.fresh_temp(IrTy::Error);
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
            if let Some(mty_types::DefRef::Module(_)) = ctx.typed.def_map.lookup(head) {
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
                    .unwrap_or(mty_types::EffectId(0));
                // v0.25 — detect the canvas constructor by full
                // path. `std.web.Canvas.new(W, H)` returns the
                // canvas handle; we mark the temp so subsequent
                // `canvas.fill_rect(...)` calls route to
                // `BuiltinId::CanvasOp(...)` via
                // `is_canvas_handle_receiver`.
                let full_path = format!("{}.{}", path.join("."), method);
                let temp = fb.fresh_temp(IrTy::Error);
                if full_path == CANVAS_CONSTRUCTOR_PATH {
                    fb.mark_canvas_local(temp);
                }
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
    // v0.37 Track T3 — per-arg FFI coercion. When typeck flagged the
    // arg in `coerce_str_to_ptr`, lower the inner expr and wrap it in
    // `Rvalue::StrPtr` so the backend reads the ptr half (offset 0) of
    // the Str aggregate rather than passing the (ptr,len) slot address
    // verbatim. `coerce_addr_of` is already taken care of by the
    // existing `HirExpr::Borrow` lowering — borrow temps are
    // i64-sized in cranelift and hold the place's address, which the
    // call-arg coerce path then passes straight through.
    let arg_ops: Vec<Operand> = args
        .iter()
        .map(|a| {
            let op = lower_expr(ctx, fb, a.value);
            if ctx.typed.coerce_str_to_ptr.contains(&a.value) {
                // Materialise the ptr half via a fresh temp typed as
                // `*U8` (IR-side raw-ptr-shaped). Downstream call
                // lowering reads from this temp as an i64 scalar.
                let temp = fb.fresh_temp(IrTy::RawPtr(Box::new(IrTy::Int(IntKind::U8))));
                fb.push_stmt(Stmt::Assign(Place::local(temp), Rvalue::StrPtr(op)));
                Operand::Move(Place::local(temp))
            } else {
                op
            }
        })
        .collect();

    // Inspect the callee to figure out which fn to call.
    let func = resolve_callee(ctx, callee);
    let temp = fb.fresh_temp(IrTy::Error);
    fb.push_stmt(Stmt::Assign(
        Place::local(temp),
        Rvalue::Call {
            func,
            args: arg_ops,
        },
    ));
    Operand::Move(Place::local(temp))
}

/// v0.15 — resolve a call's callee path to a variant constructor if
/// it names one. Returns `(adt, variant_idx)` on hit, else `None`.
/// Mirrors the type-checker's path-resolution logic
/// (`mty_types::check::resolve_path_expr`) for `Some`, `Maybe.Just`,
/// `Some::<I32>`, etc.
fn variant_for_call_callee(ctx: &LowerCtx, callee: ExprId) -> Option<(AdtId, usize)> {
    let segments = match &ctx.pkg.exprs[callee] {
        HirExpr::Path(segs) => segs.clone(),
        HirExpr::PathGeneric { segments, .. } => segments.clone(),
        _ => return None,
    };
    if segments.is_empty() {
        return None;
    }
    // Single-segment short name: `Some(x)`, `Just(y)`.
    if segments.len() == 1 {
        if let Some(DefRef::Variant(adt, idx)) = ctx.typed.def_map.lookup(&segments[0]) {
            return Some((adt, idx));
        }
        return None;
    }
    // Multi-segment dotted name: `Result.Ok(x)` — try the joined form
    // first (works if def_map registered the dotted variant name).
    if let Some(DefRef::Variant(adt, idx)) = ctx.typed.def_map.lookup_path(&segments) {
        return Some((adt, idx));
    }
    // `Enum.Variant(x)` shape: first segment names an Adt, last segment
    // is the variant's short name. Variants are registered only by
    // short name, so we look the variant up inside the Adt's def.
    if let Some(DefRef::Adt(aid)) = ctx.typed.def_map.lookup(&segments[0]) {
        if segments.len() == 2 {
            let vname = &segments[1];
            if let Some(adt) = ctx.typed.def_map.adt(aid) {
                if let Some(idx) = adt.variants.iter().position(|v| &v.name == vname) {
                    return Some((aid, idx));
                }
            }
        }
    }
    None
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
    // Range -> Tuple(lo, hi, inclusive_marker). The marker (Bool) lets
    // the iterator protocol (`__mty_iter_next`) distinguish exclusive
    // (`1..5` yields 1..=4) from inclusive (`1..=5` yields 1..=5).
    if matches!(op, Range | RangeEq) {
        let lo = lower_expr(ctx, fb, lhs);
        let hi = lower_expr(ctx, fb, rhs);
        let inclusive = matches!(op, RangeEq);
        let temp = fb.fresh_temp(IrTy::Error);
        fb.push_stmt(Stmt::Assign(
            Place::local(temp),
            Rvalue::TupleInit(vec![lo, hi, Operand::Const(Const::Bool(inclusive))]),
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
    let temp = fb.fresh_temp(IrTy::Error);
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
        let temp = fb.fresh_temp(IrTy::Error);
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
    then: mty_hir::BlockId,
    else_: Option<ExprId>,
) -> Operand {
    let cond_op = lower_expr(ctx, fb, cond);
    let then_block = fb.new_block();
    let else_block = fb.new_block();
    let join = fb.new_block();
    let result = fb.fresh_temp(IrTy::Error);
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
    arms: &[mty_hir::HirMatchArm],
) -> Operand {
    let scr = lower_expr(ctx, fb, scrutinee);
    let scr_place = operand_to_place(fb, scr);
    let result = fb.fresh_temp(IrTy::Error);
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
    // Final fallthrough: panic MT5005.
    fb.switch_to(next_test);
    fb.set_term(Term::Panic {
        msg: Operand::Const(Const::Str("MT5005 unreachable match".into())),
    });

    fb.switch_to(join);
    Operand::Move(Place::local(result))
}

fn lower_for(
    ctx: &mut LowerCtx,
    fb: &mut FnBuilder,
    pat: mty_hir::PatId,
    iter: ExprId,
    body: mty_hir::BlockId,
) -> Operand {
    // v0.5: real iterator protocol (range + slice/array). `for x in iter`
    // lowers to:
    //
    //   iter_local := <iter>
    //   header:
    //     idx_local := iter_idx_next(iter_local, idx_local)
    //     if exhausted -> exit
    //     else         -> bind(pat); body
    //   body:
    //     ... ; goto continue_tgt
    //   continue_tgt:
    //     goto header
    //   exit:
    //     result_local
    //
    // For ranges (`lo..hi`) we use the iterator's internal counter; for
    // arrays/slices we index by `i`. Both shapes go through a single
    // `Stmt::Assign` with an `Rvalue::MethodCall` of "next" so the
    // interpreter's permissive method table can service both. The
    // result is a tuple `(Bool exhausted, Value element)`.
    let iter_op = lower_expr(ctx, fb, iter);
    let iter_local = fb.fresh_temp(IrTy::Error);
    fb.push_stmt(Stmt::Assign(Place::local(iter_local), Rvalue::Use(iter_op)));

    // Counter local for sequential iteration. The interpreter's
    // `__mty_iter_next` method below uses this to walk ranges/arrays.
    let idx_local = fb.fresh_temp(IrTy::Int(mty_types::IntKind::USize));
    fb.push_stmt(Stmt::Assign(
        Place::local(idx_local),
        Rvalue::Use(Operand::Const(Const::Int(0, mty_types::IntKind::USize))),
    ));

    let header = fb.new_block();
    let body_block = fb.new_block();
    let continue_tgt = fb.new_block();
    let exit = fb.new_block();
    let result_local = fb.fresh_temp(IrTy::Unit);
    fb.set_term(Term::Goto(header));

    // Header: probe the iterator. The result is a tuple
    // `(exhausted: Bool, element: T)` — we test `exhausted` and either
    // bind the element to `pat` or fall through to the exit. The temp
    // is typed as a 2-tuple (Bool, Error) so the AOT backends can
    // compute tuple offsets; the interpreter ignores the element type.
    fb.switch_to(header);
    let probe_temp = fb.fresh_temp(IrTy::Tuple(vec![IrTy::Bool, IrTy::Error]));
    fb.push_stmt(Stmt::Assign(
        Place::local(probe_temp),
        Rvalue::MethodCall {
            receiver: Operand::Copy(Place::local(iter_local)),
            method: "__mty_iter_next".into(),
            args: vec![Operand::Copy(Place::local(idx_local))],
        },
    ));
    // Bump the counter for the next iteration.
    fb.push_stmt(Stmt::Assign(
        Place::local(idx_local),
        Rvalue::BinOp(
            BinOp::Add,
            Operand::Copy(Place::local(idx_local)),
            Operand::Const(Const::Int(1, mty_types::IntKind::USize)),
        ),
    ));
    // Field 0 of the probe tuple is the "exhausted" Bool.
    let exhausted_temp = fb.fresh_temp(IrTy::Bool);
    fb.push_stmt(Stmt::Assign(
        Place::local(exhausted_temp),
        Rvalue::TupleRead {
            receiver: Place::local(probe_temp),
            idx: 0,
        },
    ));
    fb.set_term(Term::If {
        cond: Operand::Copy(Place::local(exhausted_temp)),
        then: exit,
        else_: body_block,
    });

    fb.switch_to(body_block);
    // Bind the pattern to field 1 of the probe tuple.
    let elem_temp = fb.fresh_temp(IrTy::Error);
    fb.push_stmt(Stmt::Assign(
        Place::local(elem_temp),
        Rvalue::TupleRead {
            receiver: Place::local(probe_temp),
            idx: 1,
        },
    ));
    super::pats::lower_pat_bind(ctx, fb, pat, Operand::Move(Place::local(elem_temp)));

    fb.push_loop(LoopFrame {
        continue_target: continue_tgt,
        exit_target: exit,
        result_local,
    });
    let block = ctx.pkg.blocks[body].clone();
    let _ = lower_block(ctx, fb, &block);
    fb.pop_loop();
    // Natural fall-through goes to the continue target, which then
    // routes back to the header.
    fb.set_term(Term::Goto(continue_tgt));

    fb.switch_to(continue_tgt);
    fb.set_term(Term::Goto(header));

    fb.switch_to(exit);
    Operand::Move(Place::local(result_local))
}

fn lower_while(
    ctx: &mut LowerCtx,
    fb: &mut FnBuilder,
    cond: ExprId,
    body: mty_hir::BlockId,
) -> Operand {
    let header = fb.new_block();
    let body_block = fb.new_block();
    let continue_tgt = fb.new_block();
    let exit = fb.new_block();
    let result_local = fb.fresh_temp(IrTy::Unit);
    fb.set_term(Term::Goto(header));

    fb.switch_to(header);
    let c = lower_expr(ctx, fb, cond);
    fb.set_term(Term::If {
        cond: c,
        then: body_block,
        else_: exit,
    });

    fb.switch_to(body_block);
    fb.push_loop(LoopFrame {
        continue_target: continue_tgt,
        exit_target: exit,
        result_local,
    });
    let block = ctx.pkg.blocks[body].clone();
    let _ = lower_block(ctx, fb, &block);
    fb.pop_loop();
    fb.set_term(Term::Goto(continue_tgt));

    fb.switch_to(continue_tgt);
    fb.set_term(Term::Goto(header));

    fb.switch_to(exit);
    Operand::Move(Place::local(result_local))
}

fn lower_while_let(
    ctx: &mut LowerCtx,
    fb: &mut FnBuilder,
    pat: mty_hir::PatId,
    scrutinee: ExprId,
    body: mty_hir::BlockId,
) -> Operand {
    // v0.29 Track D: `while let pat = scrutinee { body }`.
    //
    //   header:
    //     scr := <scrutinee>
    //     pat-match scr {
    //       succ -> body_block
    //       fail -> exit
    //     }
    //   body_block:
    //     <body>; goto continue_tgt
    //   continue_tgt:
    //     goto header
    //   exit:
    //     result_local
    //
    // The scrutinee is re-evaluated on every iteration, just like a
    // plain `while`'s condition. Bindings introduced by the pattern
    // live only inside the body block. `break`/`continue` route
    // through the loop frame so they keep their usual semantics.
    let header = fb.new_block();
    let body_block = fb.new_block();
    let continue_tgt = fb.new_block();
    let exit = fb.new_block();
    let result_local = fb.fresh_temp(IrTy::Unit);
    fb.set_term(Term::Goto(header));

    fb.switch_to(header);
    let scr = lower_expr(ctx, fb, scrutinee);
    let scr_place = operand_to_place(fb, scr);
    pats::lower_pat_match(ctx, fb, scr_place, pat, body_block, exit);

    fb.switch_to(body_block);
    fb.push_loop(LoopFrame {
        continue_target: continue_tgt,
        exit_target: exit,
        result_local,
    });
    let block = ctx.pkg.blocks[body].clone();
    let _ = lower_block(ctx, fb, &block);
    fb.pop_loop();
    fb.set_term(Term::Goto(continue_tgt));

    fb.switch_to(continue_tgt);
    fb.set_term(Term::Goto(header));

    fb.switch_to(exit);
    Operand::Move(Place::local(result_local))
}

fn lower_loop(ctx: &mut LowerCtx, fb: &mut FnBuilder, body: mty_hir::BlockId) -> Operand {
    let header = fb.new_block();
    let continue_tgt = fb.new_block();
    let exit = fb.new_block();
    let result_local = fb.fresh_temp(IrTy::Unit);
    // Initialise the result to Unit so an exit via budget / panic still
    // has a defined value (the interpreter never reads it in that
    // case, but the SIR shape stays well-formed).
    fb.push_stmt(Stmt::Assign(
        Place::local(result_local),
        Rvalue::Use(Operand::Const(Const::Unit)),
    ));
    fb.set_term(Term::Goto(header));

    fb.switch_to(header);
    fb.push_loop(LoopFrame {
        continue_target: continue_tgt,
        exit_target: exit,
        result_local,
    });
    let block = ctx.pkg.blocks[body].clone();
    let _ = lower_block(ctx, fb, &block);
    fb.pop_loop();
    fb.set_term(Term::Goto(continue_tgt));

    fb.switch_to(continue_tgt);
    fb.set_term(Term::Goto(header));

    fb.switch_to(exit);
    Operand::Move(Place::local(result_local))
}

fn lower_question(ctx: &mut LowerCtx, fb: &mut FnBuilder, inner: ExprId) -> Operand {
    // `inner?` ::=
    //   let tmp = inner;
    //   switch_variant tmp { Ok => { result = tmp.Ok.0 }, Err => try_return_err tmp.Err.0 }
    let v = lower_expr(ctx, fb, inner);
    let v_place = operand_to_place(fb, v);

    let result = fb.fresh_temp(IrTy::Error);
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
            let temp = fb.fresh_temp(IrTy::Error);
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

/// v0.29 Track A — permissive field-name table for the SIR swarm
/// dispatcher's synthetic value shapes. The `BuiltinId::Swarm` arm
/// returns a tagged `Value::Struct` whose fields are positional; this
/// table maps the user-facing field names (`consensus.majority`,
/// `consensus.dissents`, etc.) onto the matching positional index so
/// source-level field access reads the right slot.
///
/// Mirrors `swarm_dispatch::{consensus_value, reply_value, member_value,
/// budget_value}` in `crates/mty-ir/src/interp/run.rs`. Keep in sync
/// when adding new fields to those tagged shapes.
fn stdlib_field_index(name: &str) -> Option<usize> {
    Some(match name {
        // Consensus { majority, dissents, all_replies, budget_exhausted, strategy, total_cost_cents }
        "majority" => 0,
        "dissents" => 1,
        "all_replies" => 2,
        "budget_exhausted" => 3,
        "strategy" => 4,
        "total_cost_cents" => 5,
        // MemberReply { member, body, tokens_used, cost_cents, tool_uses }
        "member" => 0,
        "body" => 1,
        "tokens_used" => 2,
        "cost_cents" => 3,
        "tool_uses" => 4,
        // DollarBudget { limit_cents, consumed_cents }
        "limit_cents" => 0,
        "consumed_cents" => 1,
        _ => return None,
    })
}

fn resolve_field_index(ctx: &LowerCtx, receiver: ExprId, name: &str) -> Option<usize> {
    // Use the receiver's typed Adt to look up the field index.
    let ty = ctx.expr_ty(receiver);
    let td = ctx.typed.ty_arena.get(ty);
    let adt_id = match td {
        mty_types::TyData::Adt(id, _) => Some(*id),
        mty_types::TyData::Ref { inner, .. } => match ctx.typed.ty_arena.get(*inner) {
            mty_types::TyData::Adt(id, _) => Some(*id),
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
        if matches!(ctx.typed.ty_arena.get(ty), mty_types::TyData::Module(_)) {
            return Some(segs.clone());
        }
    }
    None
}

/// v0.6: return `true` if `receiver` resolves to a `Cap { family: Dom }`
/// type. Drives the DOM-method routing in `lower_expr` MethodCall.
fn is_dom_cap_receiver(ctx: &LowerCtx, receiver: ExprId) -> bool {
    matches!(
        ctx.expr_tydata(receiver),
        TyData::Cap {
            family: CapFamily::Dom,
            ..
        }
    )
}

/// v0.25 — map a method name (snake_case as it appears in Mighty
/// source) onto the matching [`CanvasOpKind`]. Pinned against the
/// eight WIT methods declared in
/// `crates/mty-codegen-wasm/wit/mty-web/canvas.wit` and mirrored by
/// `mty_stdlib::web::canvas`. Returns `None` for unknown names so
/// the caller falls back to the generic `Rvalue::MethodCall`
/// dispatch (and the type-checker / interpreter surface the right
/// "method not found" diagnostic for typos).
///
/// Centralised here so the MethodCall arm in `lower_expr` and the
/// `local.method(args)` arm in `lower_call` share a single source
/// of truth — keeps the routing table from drifting between the
/// two parse shapes.
pub(crate) fn canvas_op_for_method(name: &str) -> Option<CanvasOpKind> {
    Some(match name {
        "clear" => CanvasOpKind::Clear,
        "fill_rect" => CanvasOpKind::FillRect,
        "stroke_rect" => CanvasOpKind::StrokeRect,
        "fill_text" => CanvasOpKind::FillText,
        "set_fill_style" => CanvasOpKind::SetFillStyle,
        "width" => CanvasOpKind::Width,
        "height" => CanvasOpKind::Height,
        "request_animation_frame" => CanvasOpKind::RequestAnimationFrame,
        _ => return None,
    })
}

/// v0.25 — return `true` if `receiver` resolves to a value previously
/// marked as a `std.web.Canvas` handle (via
/// [`FnBuilder::mark_canvas_local`]).
///
/// Detection: when the receiver expression is a single-segment path
/// that names a local, consult the per-fn `canvas_locals` set. We
/// don't trust the typed receiver type here because v0.23-era
/// `std.web.Canvas.new(...)` lowers to an `effect_invoke` and the
/// type-checker stamps the result as `TyData::Error` (the `std.web`
/// module + `Canvas` ADT aren't modeled in the prelude). The
/// local-tagging hand-off keeps the pipeline working without forcing
/// a prelude shape change in the same slice as the routing fix.
///
/// v0.26 Track D — `canvas_locals` is now also populated for fn
/// parameters whose source-level type is `std.web.Canvas` (see
/// `lower::items::lower_one_fn`), so a fn like
/// `fn render(c: std.web.Canvas) { c.fill_rect(...) }` routes through
/// `BuiltinId::CanvasOp(...)` without needing an inline
/// `std.web.Canvas.new(...)` re-acquire. Closes the v0.25 Track F
/// §A unfinished business.
fn is_canvas_handle_receiver(ctx: &LowerCtx, fb: &FnBuilder, receiver: ExprId) -> bool {
    match &ctx.pkg.exprs[receiver] {
        HirExpr::Path(segs) if segs.len() == 1 => fb
            .locals_by_name
            .get(&segs[0])
            .map(|l| fb.is_canvas_local(*l))
            .unwrap_or(false),
        _ => false,
    }
}

/// v0.25 — canonical canvas-constructor path. The `lower_call`
/// module-receiver arm compares against this string before marking
/// the result temp as a canvas handle. Kept as a `pub(crate) const`
/// so the IR test suite can pin the source-side surface contract
/// without re-spelling it.
pub(crate) const CANVAS_CONSTRUCTOR_PATH: &str = "std.web.Canvas.new";

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
