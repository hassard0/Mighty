//! Top-level item lowering: fns, structs, enums, agents.

use super::ctx::*;
use super::exprs;
use super::ty::lower_ty;
use crate::ir::*;
use mty_hir::{HirAgent, HirEnum, HirFn, HirStruct, Item, ItemId, SourceSpan};
use mty_types::{AdtId, AdtKind, DefRef};

pub fn lower_all_items(ctx: &mut LowerCtx) {
    // Pass 1: register all ADTs (so calls and constructors find them).
    register_adts(ctx);
    // Pass 2: allocate fn shells (so call-site resolution sees ids).
    register_fn_shells(ctx);
    // Pass 3: register agents (synthesize state structs + fn shells).
    register_agents(ctx);
    // Pass 4: lower fn bodies.
    lower_fn_bodies(ctx);
    // Pass 5: lower agent bodies (ctor + handlers).
    lower_agent_bodies(ctx);
}

fn register_adts(ctx: &mut LowerCtx) {
    for (_, item) in ctx.pkg.items.iter() {
        match item {
            Item::Struct(sid) => {
                let s: &HirStruct = &ctx.pkg.structs[*sid];
                if let Some(adt_id) = ctx.typed.def_map.hir_struct_to_adt.get(sid).copied() {
                    add_adt(ctx, adt_id, AdtRefKind::Struct);
                    let _ = s;
                }
            }
            Item::Enum(eid) => {
                let _: &HirEnum = &ctx.pkg.enums[*eid];
                if let Some(adt_id) = ctx.typed.def_map.hir_enum_to_adt.get(eid).copied() {
                    add_adt(ctx, adt_id, AdtRefKind::Enum);
                }
            }
            _ => {}
        }
    }
    // Also include prelude ADTs (Option, Result, AgentRef) referenced
    // by user programs.
    for (name, dref) in &ctx.typed.def_map.by_name {
        if let DefRef::Adt(adt_id) = dref {
            if ctx.prog.adts.iter().any(|a| a.adt == *adt_id) {
                continue;
            }
            let def = ctx.typed.def_map.adt(*adt_id);
            if let Some(def) = def {
                let kind = match def.kind {
                    AdtKind::Struct => AdtRefKind::Struct,
                    AdtKind::Enum => AdtRefKind::Enum,
                    AdtKind::Opaque => AdtRefKind::Opaque,
                };
                let variants: Vec<VariantRef> = def
                    .variants
                    .iter()
                    .map(|v| VariantRef {
                        name: v.name.clone(),
                        fields: v
                            .fields
                            .iter()
                            .map(|f| FieldRef {
                                name: f.name.clone(),
                                ty: lower_ty(f.ty, ctx.ty_arena()),
                            })
                            .collect(),
                    })
                    .collect();
                ctx.prog.adts.push(AdtRef {
                    adt: *adt_id,
                    name: name.clone(),
                    kind,
                    variants,
                });
            }
        }
    }
}

fn add_adt(ctx: &mut LowerCtx, adt_id: AdtId, kind: AdtRefKind) {
    if ctx.prog.adts.iter().any(|a| a.adt == adt_id) {
        return;
    }
    let Some(def) = ctx.typed.def_map.adt(adt_id) else {
        return;
    };
    let variants: Vec<VariantRef> = def
        .variants
        .iter()
        .map(|v| VariantRef {
            name: v.name.clone(),
            fields: v
                .fields
                .iter()
                .map(|f| FieldRef {
                    name: f.name.clone(),
                    ty: lower_ty(f.ty, ctx.ty_arena()),
                })
                .collect(),
        })
        .collect();
    ctx.prog.adts.push(AdtRef {
        adt: adt_id,
        name: def.name.clone(),
        kind,
        variants,
    });
}

fn register_fn_shells(ctx: &mut LowerCtx) {
    // Walk top-level fns, also fns inside impl blocks.
    let mut fn_ids: Vec<mty_hir::FnId> = vec![];
    for (_, item) in ctx.pkg.items.iter() {
        collect_fn_ids(item, ctx, &mut fn_ids);
    }
    for fid in fn_ids {
        let f: &HirFn = &ctx.pkg.fns[fid];
        let ret_ty = ctx
            .typed
            .fn_ret
            .get(&fid)
            .copied()
            .map(|t| lower_ty(t, ctx.ty_arena()))
            .unwrap_or(IrTy::Unit);
        let sid = ctx.alloc_fn_shell(f.name.clone(), ret_ty, Some(fid), f.span.clone());
        ctx.fn_map.insert(fid, sid);
        if let Some(def_id) = ctx.typed.def_map.hir_fn_to_def.get(&fid).copied() {
            ctx.fn_def_to_sir.insert(def_id.0, sid);
        }
    }
    // v0.25 Track B — populate `prog.extern_bindings` for every fn
    // that came from an `extern <abi> { fn ... }` block. The wasm
    // emitter's `predeclare_extern_js_imports` reads this table to
    // turn each entry into a real `(import "mty:web/js" ...)` in the
    // core module instead of an empty user fn (the v0.24 Track E
    // "extern js is documentation" gap).
    record_extern_bindings(ctx);
}

fn record_extern_bindings(ctx: &mut LowerCtx) {
    // Collect (hir_fn_id, abi, name, is_variadic) tuples first so we
    // don't hold a borrow on `ctx.pkg` while mutating `ctx.prog`.
    let mut bindings: Vec<(mty_hir::FnId, String, String, bool)> = Vec::new();
    for (_, item) in ctx.pkg.items.iter() {
        if let Item::ExternBlock(eb) = item {
            // Default ABI is "c" when the user wrote a bare `extern { }`;
            // the wasm emitter currently treats anything except "js" as
            // a no-op (the legacy stub-fn behaviour).
            let abi = eb.abi.clone().unwrap_or_else(|| "c".to_string());
            for fid in &eb.fns {
                let hf = &ctx.pkg.fns[*fid];
                bindings.push((*fid, abi.clone(), hf.name.clone(), hf.is_variadic));
            }
        }
    }
    for (hir_id, abi, name, is_variadic) in bindings {
        if let Some(sirid) = ctx.fn_map.get(&hir_id).copied() {
            ctx.prog.extern_bindings.insert(
                sirid,
                crate::ir::ExternBinding {
                    abi,
                    name,
                    is_variadic,
                },
            );
        }
    }
}

fn collect_fn_ids(item: &Item, _ctx: &LowerCtx, out: &mut Vec<mty_hir::FnId>) {
    match item {
        Item::Fn(id) => out.push(*id),
        Item::Impl(im) => {
            for m in &im.methods {
                out.push(*m);
            }
        }
        Item::Trait(t) => {
            for m in &t.methods {
                out.push(*m);
            }
        }
        Item::ExternBlock(eb) => {
            for f in &eb.fns {
                out.push(*f);
            }
        }
        Item::ExportDecl(ed) => {
            // Recurse into the exported item.
            collect_fn_ids(&ed.item, _ctx, out);
        }
        Item::Agent(_aid) => {
            // Agents' synthesized fns are handled by register_agents.
        }
        _ => {}
    }
}

fn register_agents(ctx: &mut LowerCtx) {
    let mut agent_ids: Vec<mty_hir::AgentId> = vec![];
    for (_, item) in ctx.pkg.items.iter() {
        if let Item::Agent(aid) = item {
            agent_ids.push(*aid);
        }
    }
    for aid in agent_ids {
        let a: &HirAgent = &ctx.pkg.agents[aid];
        // Synthesize a state ADT.
        let adt_id = synth_agent_state_adt(ctx, &a.name);
        let agent_id = AgentIrId(ctx.prog.agents.len() as u32);

        // Constructor: returns the state struct.
        let ctor_ret = IrTy::Adt(adt_id, vec![]);
        let ctor = ctx.alloc_fn_shell(
            format!("__{}::__new", a.name),
            ctor_ret,
            None,
            a.span.clone(),
        );

        // Handlers: one fn per `on Msg(args)` handler.
        let mut handlers = vec![];
        for h in &a.handlers {
            let ret_ty = IrTy::Unit; // handler reply: slice-6 simplification
            let h_fn = ctx.alloc_fn_shell(
                format!("__{}::on_{}", a.name, h.message),
                ret_ty,
                None,
                h.span.clone(),
            );
            handlers.push((h.message.clone(), h_fn));
        }

        ctx.prog.agents.push(Agent {
            id: agent_id,
            name: a.name.clone(),
            state_adt: adt_id,
            ctor,
            handlers,
            span: a.span.clone(),
        });
        ctx.agent_map.insert(a.name.clone(), agent_id);
    }
}

fn synth_agent_state_adt(ctx: &mut LowerCtx, agent_name: &str) -> AdtId {
    // Use a high range to avoid collisions with real AdtIds. Slice 6
    // tolerates this because the interpreter looks up by AdtId only
    // through `prog.adt_by_id` (which scans).
    let next = (10_000 + ctx.prog.adts.len()) as u32;
    let adt_id = AdtId(next);
    ctx.prog.adts.push(AdtRef {
        adt: adt_id,
        name: format!("__{}::State", agent_name),
        kind: AdtRefKind::Struct,
        variants: vec![VariantRef {
            name: format!("__{}::State", agent_name),
            fields: vec![], // populated lazily as state fields appear in handlers
        }],
    });
    adt_id
}

fn lower_fn_bodies(ctx: &mut LowerCtx) {
    let ids: Vec<(mty_hir::FnId, IrFnId)> = ctx.fn_map.iter().map(|(h, s)| (*h, *s)).collect();
    for (hid, sid) in ids {
        lower_one_fn(ctx, hid, sid);
    }
}

fn lower_one_fn(ctx: &mut LowerCtx, hir_id: mty_hir::FnId, sir_id: IrFnId) {
    let f: HirFn = ctx.pkg.fns[hir_id].clone();
    let ret_ty = ctx
        .typed
        .fn_ret
        .get(&hir_id)
        .copied()
        .map(|t| lower_ty(t, ctx.ty_arena()))
        .unwrap_or(IrTy::Unit);
    let mut fb = FnBuilder::new(sir_id, ret_ty.clone());
    // v0.22: prime the builder's current span with the fn's HIR span,
    // so every Stmt/Term emitted from this fn's body picks up a real
    // (non-zero) span. HIR does not yet expose per-expression spans
    // (`HirExpr` arms are not span-tagged in `mty-hir/src/nodes.rs`),
    // so the fn's span is the best fallback the lowerer can produce
    // today; v0.23 will replace this with a real per-expr span lookup
    // once mty-hir grows an `exprs_spans` table.
    fb.set_cur_span(f.span.clone());

    // Params: allocate one local per param. Param types live in
    // typed.fn_params.
    let params_ty = ctx
        .typed
        .fn_params
        .get(&hir_id)
        .cloned()
        .unwrap_or_default();
    // v0.26 Track D — collect the HIR param's syntactic type (the
    // resolver doesn't model `std.web.Canvas` so the typed entry comes
    // back as `Error`; the only place the canvas-handle hint survives
    // is the source-level `HirType::Path(["std","web","Canvas"])`).
    // We walk in parallel with `params_ty` so the indices line up. If
    // the HIR fn has fewer params than `params_ty` (impossible in
    // practice but defensive), the extra `params_ty` entries get an
    // empty hint.
    let hir_param_types: Vec<Option<mty_hir::TypeId>> =
        ctx.pkg.fns[hir_id].params.iter().map(|p| p.ty).collect();
    for (idx, (name, ty)) in params_ty.iter().enumerate() {
        let l = fb.new_local(
            name.clone(),
            lower_ty(*ty, ctx.ty_arena()),
            true,
            LocalSource::Param,
        );
        fb.params.push(l);
        // v0.41 T1 — record the param's HIR-resolved type so
        // `resolve_path` can resolve user struct field projections
        // on parameter receivers (`fn f(p: Point) -> I32 { p.y }`).
        fb.set_local_ty(l, *ty);
        // v0.26 Track D — if the source-level param type is
        // `std.web.Canvas`, mark the local as a canvas handle so
        // `is_canvas_handle_receiver` (in `exprs.rs`) routes
        // `c.fill_rect(...)` to `BuiltinId::CanvasOp(...)`. Closes the
        // v0.25 Track F §A gap where a Canvas handle passed as a fn
        // parameter dropped the canvas-routing taint.
        if let Some(hir_ty_id) = hir_param_types.get(idx).copied().flatten() {
            if is_std_web_canvas_type(ctx.pkg, hir_ty_id) {
                fb.mark_canvas_local(l);
            }
        }
    }

    let body = match &f.body {
        Some(b) => *b,
        None => {
            // Extern / trait-method-without-body: emit a trivial return.
            let unit = Operand::Const(Const::Unit);
            fb.set_term(Term::Return(unit));
            install_fn(
                ctx,
                sir_id,
                fb,
                &f.name,
                ret_ty,
                f.span.clone(),
                Some(hir_id),
            );
            return;
        }
    };

    // Lower the body expression: HirBlock.
    let block = ctx.pkg.blocks[body].clone();
    let result = exprs::lower_block(ctx, &mut fb, &block);

    // Tail terminator: return the result.
    fb.set_term(Term::Return(result));

    install_fn(
        ctx,
        sir_id,
        fb,
        &f.name,
        ret_ty,
        f.span.clone(),
        Some(hir_id),
    );
}

fn install_fn(
    ctx: &mut LowerCtx,
    id: IrFnId,
    fb: FnBuilder,
    name: &str,
    ret_ty: IrTy,
    span: SourceSpan,
    hir_fn: Option<mty_hir::FnId>,
) {
    // v0.22: split the function out + capture its span table so we can
    // register both in the program in one pass.
    let (mut func, spans) = fb.finish_with_spans(hir_fn, span);
    func.id = id;
    func.name = name.to_string();
    func.ret_ty = ret_ty;
    ctx.prog.fns[id.0 as usize] = func;
    if !spans.stmt_spans.is_empty() || !spans.terminator_spans.is_empty() {
        ctx.prog.span_table.insert(id, spans);
    }
}

fn lower_agent_bodies(ctx: &mut LowerCtx) {
    let agents: Vec<(mty_hir::AgentId, AgentIrId)> = ctx
        .pkg
        .agents
        .iter()
        .filter_map(|(aid, a)| ctx.agent_map.get(&a.name).map(|sirid| (aid, *sirid)))
        .collect();
    for (hir_aid, sirid) in agents {
        lower_one_agent(ctx, hir_aid, sirid);
    }
}

fn lower_one_agent(ctx: &mut LowerCtx, hir_aid: mty_hir::AgentId, sirid: AgentIrId) {
    let a: HirAgent = ctx.pkg.agents[hir_aid].clone();
    let ag = ctx.prog.agents[sirid.0 as usize].clone();

    // ------- collect state field info from the HIR -------
    let mut state_fields: Vec<(String, IrTy)> = vec![];
    for st in &a.state {
        let ty = match st.ty {
            Some(t) => crate::lower::ty::lower_ty(lookup_hir_type(ctx, t), ctx.ty_arena()),
            None => IrTy::Int(mty_types::IntKind::I32), // permissive default for `n = 0`
        };
        state_fields.push((st.name.clone(), ty));
    }
    // Patch the synthetic state ADT.
    if let Some(adt_ref) = ctx.prog.adts.iter_mut().find(|x| x.adt == ag.state_adt) {
        adt_ref.variants[0].fields = state_fields
            .iter()
            .map(|(n, t)| FieldRef {
                name: Some(n.clone()),
                ty: t.clone(),
            })
            .collect();
    }

    // ------- ctor: build an AdtInit with each field initialized -------
    {
        let state_ty = IrTy::Adt(ag.state_adt, vec![]);
        let mut fb = FnBuilder::new(ag.ctor, state_ty.clone());
        // v0.22: prime span with agent's HIR span (best fallback until
        // HIR exposes per-init-expr spans).
        fb.set_cur_span(a.span.clone());
        // No params for slice 6 ctor.
        let mut init_ops: Vec<Operand> = vec![];
        for st in &a.state {
            // For each state field, lower its init expr (if any) or use
            // a type-appropriate zero literal.
            match st.init {
                Some(init_expr) => {
                    let op = exprs::lower_expr(ctx, &mut fb, init_expr);
                    init_ops.push(op);
                }
                None => {
                    init_ops.push(Operand::Const(Const::Int(0, mty_types::IntKind::I32)));
                }
            }
        }
        let temp = fb.fresh_temp(state_ty.clone());
        fb.push_stmt(Stmt::Assign(
            Place::local(temp),
            Rvalue::AdtInit {
                adt: ag.state_adt,
                variant: 0,
                fields: init_ops,
            },
        ));
        fb.set_term(Term::Return(Operand::Move(Place::local(temp))));
        install_fn(
            ctx,
            ag.ctor,
            fb,
            &format!("__{}::__new", a.name),
            state_ty,
            a.span.clone(),
            None,
        );
    }

    // ------- handlers -------
    for h in &a.handlers {
        let h_sir_id = ag
            .handlers
            .iter()
            .find(|(m, _)| m == &h.message)
            .map(|(_, id)| *id)
            .unwrap_or_else(|| panic!("missing handler sir id for {}", h.message));
        let ret_ty = IrTy::Unit;
        let mut fb = FnBuilder::new(h_sir_id, ret_ty.clone());
        // v0.22: prime span with the handler's HIR span.
        fb.set_cur_span(h.span.clone());
        // Param 0: &mut state.
        let state_ref_ty = IrTy::Ref {
            mutable: true,
            inner: Box::new(IrTy::Adt(ag.state_adt, vec![])),
        };
        let state_param = fb.new_local("self", state_ref_ty, true, LocalSource::Param);
        fb.params.push(state_param);
        // Bind state field names to "self.f<idx>" places.
        for (idx, (fname, fty)) in state_fields.iter().enumerate() {
            let l = fb.new_local(fname.clone(), fty.clone(), true, LocalSource::Temp);
            fb.push_stmt(Stmt::Assign(
                Place::local(l),
                Rvalue::FieldRead {
                    receiver: Place {
                        local: state_param,
                        proj: vec![Projection::Deref],
                    },
                    field: idx,
                },
            ));
        }
        // Message params.
        for p in &h.params {
            let l = fb.new_local(p.clone(), IrTy::Str, true, LocalSource::Param);
            fb.params.push(l);
        }

        let body = ctx.pkg.blocks[h.body].clone();
        let result = exprs::lower_block(ctx, &mut fb, &body);

        // Write back any updated state fields (slice 6 is conservative
        // and writes every field back).
        for (idx, (fname, _)) in state_fields.iter().enumerate() {
            if let Some(local) = fb.locals_by_name.get(fname).copied() {
                fb.push_stmt(Stmt::Assign(
                    Place {
                        local: state_param,
                        proj: vec![Projection::Deref, Projection::Field(idx)],
                    },
                    Rvalue::Use(Operand::Copy(Place::local(local))),
                ));
            }
        }

        // Handler returns its reply value (in slice 6 we just discard
        // it through Unit; the interpreter's Send/Ask uses the returned
        // value directly).
        fb.set_term(Term::Return(result));
        install_fn(
            ctx,
            h_sir_id,
            fb,
            &format!("__{}::on_{}", a.name, h.message),
            ret_ty,
            h.span.clone(),
            None,
        );
    }

    let _ = (ItemId::default(),);
    let _ = sirid;
}

// Helper: resolve a HIR TypeId (a syntactic type) into a resolved TyId
// for lowering. Slice 6 keeps this minimal — `HirType::Unit` → unit, and
// everything else falls back to `Error` (the lowerer is total because
// IrTy::Error flows through fine).
fn lookup_hir_type(ctx: &LowerCtx, t: mty_hir::TypeId) -> mty_types::TyId {
    use mty_hir::HirType;
    let _ = DefRef::Adt; // imported but only used through trait-object access; keep silence
    match &ctx.pkg.types[t] {
        HirType::Unit => ctx.typed.ty_arena.unit,
        _ => ctx.typed.ty_arena.error,
    }
}

/// v0.26 Track D — detect a `std.web.Canvas` type at the HIR source
/// level. The type checker stamps `Error` on this path (no `std.web`
/// module / `Canvas` ADT in the prelude — same blocker that drove the
/// per-fn `canvas_locals` workaround in v0.25); the only place the
/// canvas-handle hint survives a parameter declaration is the raw
/// `HirType::Path` segments.
///
/// Accepts the canonical multi-segment form (`std.web.Canvas`) plus
/// the single-segment `Canvas` (in case a future `use std::web::Canvas`
/// shorthand lands). Mutable / immutable borrow wrappers are peeled.
///
/// Closes the v0.25 Track F §A gap. The `lower_fn_bodies` path consults
/// this from `lower_one_fn` to mark each canvas-typed param's local in
/// `FnBuilder::canvas_locals`.
pub(crate) fn is_std_web_canvas_type(pkg: &mty_hir::Package, ty: mty_hir::TypeId) -> bool {
    use mty_hir::HirType;
    match &pkg.types[ty] {
        HirType::Path { segments, .. } => match segments.len() {
            1 => segments[0] == "Canvas",
            3 => segments[0] == "std" && segments[1] == "web" && segments[2] == "Canvas",
            _ => false,
        },
        HirType::Borrow { inner, .. } => is_std_web_canvas_type(pkg, *inner),
        _ => false,
    }
}

// Default impl for mty_hir::ItemId so the `let _` pattern compiles.
trait ItemIdDefault {
    fn default() -> Self;
}
impl ItemIdDefault for mty_hir::ItemId {
    fn default() -> Self {
        use la_arena::RawIdx;
        mty_hir::ItemId::from_raw(RawIdx::from(0))
    }
}

/// v0.47 T4 — auto-Drop post-pass.
///
/// For every function in `prog`, find the `UserLet` bindings whose
/// `IrTy::Adt(adt, _)` has an entry in `prog.adt_drop_fns` — these are
/// the source-level owners of a drop-needing resource handle. Then
/// walk every block and inject `Stmt::Drop(local)` in front of each
/// fn-exit terminator (`Return`, `TryReturnErr`, `Panic`) so the value
/// gets closed even on early return / panic-unwind / `?` short-circuit.
///
/// Only `UserLet` locals are dropped — NOT compiler `Temp`s. A handle
/// is aliased by several locals (the call-result temp, method
/// receiver-copies, and the binding); dropping every alias would close
/// the one underlying allocation more than once (double-free / heap
/// corruption). Restricting to the owning binding closes each handle
/// exactly once. See the in-body comment for the full aliasing story.
///
/// Locals that are moved out — either by a direct rebind
/// (`b := Use(Move(a))`) or as the operand of a `Term::Return` — are
/// skipped: ownership transferred, so the destination (or the caller)
/// is responsible for the close, not this frame.
///
/// The pass runs AFTER fn-body lowering finished, so every block's
/// terminator is already populated; we never invent new blocks, only
/// prepend `Stmt::Drop` statements onto existing ones.
///
/// Idempotence vs explicit `.close()`: the codegen's `emit_*_close`
/// helper zeroes the receiver local after dispatching the runtime
/// drop, so the auto-Drop loads handle=0 and the runtime no-ops. The
/// runtime symbol contract (per `DefMap::mty_drop_fns`) MUST tolerate
/// handle=0.
pub fn inject_auto_drop_stmts(prog: &mut Program) {
    if prog.adt_drop_fns.is_empty() {
        return;
    }
    // Snapshot the drop-fn table by AdtId so the per-fn loops don't
    // re-borrow `prog`.
    let drop_adts: std::collections::HashSet<mty_types::AdtId> =
        prog.adt_drop_fns.keys().copied().collect();

    for f in prog.fns.iter_mut() {
        // v0.47 T4 fix — a resource handle (e.g. the i64 behind a
        // `DirIter`) is aliased by several IR locals: the `read_dir`
        // result `Temp`, the `it.next()` / `it.close()` receiver-copy
        // `Temp`s, AND the source-level `let` binding. They all carry
        // the SAME handle value, so closing every one of them would
        // free the single `Box<DirIterState>` more than once → a
        // double-free / heap corruption. Ownership lives in exactly
        // one place: the source-level binding. So auto-Drop only
        // `UserLet` locals (skipping `Temp`/`Param`/`Return`), giving
        // each handle exactly one owner that closes it once.
        //
        // The one remaining alias between two `UserLet`s is a direct
        // rebind `let b = a` (`b := Use(Move(a))`): ownership transfers
        // to `b`, so `a` must NOT also be dropped. Pre-scan for that
        // empty-proj `Use(Move)` shape and exclude the moved-from
        // local. (Method receivers use `Copy`, not `Move`, so the loop
        // variable `it` is never excluded by this; pass-by-move into a
        // callee is already safe because the callee skips `Param`
        // drops, leaving the caller's single drop as the only free.)
        let mut moved_out: std::collections::HashSet<Local> = std::collections::HashSet::new();
        for blk in f.blocks.iter() {
            for s in &blk.stmts {
                if let Stmt::Assign(_, Rvalue::Use(Operand::Move(p))) = s {
                    if p.proj.is_empty() {
                        moved_out.insert(p.local);
                    }
                }
            }
        }

        let mut drop_locals: Vec<Local> = Vec::new();
        for (idx, decl) in f.locals.iter().enumerate() {
            let local_id = Local(idx as u32);
            // Only source-level `let` bindings own a resource handle;
            // temporaries / params / the return slot alias it.
            if !matches!(decl.source, LocalSource::UserLet) {
                continue;
            }
            // Ownership moved to another binding — that binding drops it.
            if moved_out.contains(&local_id) {
                continue;
            }
            if let IrTy::Adt(adt, _) = &decl.ty {
                if drop_adts.contains(adt) {
                    drop_locals.push(local_id);
                }
            }
        }
        if drop_locals.is_empty() {
            continue;
        }

        // Walk every block; for each fn-exit terminator, inject
        // `Stmt::Drop(local)` for every drop-needing local (skipping
        // the local that is itself the Return operand, if any).
        for blk in f.blocks.iter_mut() {
            // Determine whether this block's terminator exits the fn.
            let (is_exit, returned_local) = match &blk.terminator {
                Term::Return(op) => {
                    let returned = match op {
                        Operand::Move(p) | Operand::Copy(p) if p.proj.is_empty() => Some(p.local),
                        _ => None,
                    };
                    (true, returned)
                }
                Term::TryReturnErr(_) | Term::Panic { .. } => (true, None),
                // `Unreachable` after a Panic / call to `never` fn —
                // dropping is moot (we're trapping), but keeping the
                // pass uniform avoids divergence between codegen and
                // interp. We do NOT drop here because Unreachable is
                // also used by partly-lowered shapes that the lowerer
                // never completed (the slice-6 lowerer is total but
                // optimistic). Safer to skip.
                _ => (false, None),
            };
            if !is_exit {
                continue;
            }
            // Build the drop block (prepended to existing stmts).
            let mut drops: Vec<Stmt> = Vec::with_capacity(drop_locals.len());
            for l in &drop_locals {
                if returned_local == Some(*l) {
                    continue;
                }
                drops.push(Stmt::Drop(*l));
            }
            if drops.is_empty() {
                continue;
            }
            // Append the drops at the END of the block's stmt list,
            // immediately before the terminator. Drops must run AFTER
            // any other stmts in the block (e.g. the let that computes
            // the return value) so the auto-Drop loads the correct
            // handle.
            blk.stmts.extend(drops);
        }
    }
}
