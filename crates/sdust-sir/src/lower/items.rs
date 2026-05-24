//! Top-level item lowering: fns, structs, enums, agents.

use super::ctx::*;
use super::exprs;
use super::ty::lower_ty;
use crate::sir::*;
use sdust_hir::{HirAgent, HirEnum, HirFn, HirStruct, Item, ItemId, SourceSpan};
use sdust_types::{AdtId, AdtKind, DefRef};

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
                let _e: &HirEnum = &ctx.pkg.enums[*eid];
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
    let def = match ctx.typed.def_map.adt(adt_id) {
        Some(d) => d,
        None => return,
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
    let mut fn_ids: Vec<sdust_hir::FnId> = vec![];
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
            .unwrap_or(SirTy::Unit);
        let sid = ctx.alloc_fn_shell(f.name.clone(), ret_ty, Some(fid), f.span.clone());
        ctx.fn_map.insert(fid, sid);
        if let Some(def_id) = ctx.typed.def_map.hir_fn_to_def.get(&fid).copied() {
            ctx.fn_def_to_sir.insert(def_id.0, sid);
        }
    }
}

fn collect_fn_ids(item: &Item, _ctx: &LowerCtx, out: &mut Vec<sdust_hir::FnId>) {
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
    let mut agent_ids: Vec<sdust_hir::AgentId> = vec![];
    for (_, item) in ctx.pkg.items.iter() {
        if let Item::Agent(aid) = item {
            agent_ids.push(*aid);
        }
    }
    for aid in agent_ids {
        let a: &HirAgent = &ctx.pkg.agents[aid];
        // Synthesize a state ADT.
        let adt_id = synth_agent_state_adt(ctx, &a.name);
        let agent_id = AgentSirId(ctx.prog.agents.len() as u32);

        // Constructor: returns the state struct.
        let ctor_ret = SirTy::Adt(adt_id, vec![]);
        let ctor = ctx.alloc_fn_shell(
            format!("__{}::__new", a.name),
            ctor_ret,
            None,
            a.span.clone(),
        );

        // Handlers: one fn per `on Msg(args)` handler.
        let mut handlers = vec![];
        for h in &a.handlers {
            let ret_ty = SirTy::Unit; // handler reply: slice-6 simplification
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
    let ids: Vec<(sdust_hir::FnId, SirFnId)> = ctx.fn_map.iter().map(|(h, s)| (*h, *s)).collect();
    for (hid, sid) in ids {
        lower_one_fn(ctx, hid, sid);
    }
}

fn lower_one_fn(ctx: &mut LowerCtx, hir_id: sdust_hir::FnId, sir_id: SirFnId) {
    let f: HirFn = ctx.pkg.fns[hir_id].clone();
    let ret_ty = ctx
        .typed
        .fn_ret
        .get(&hir_id)
        .copied()
        .map(|t| lower_ty(t, ctx.ty_arena()))
        .unwrap_or(SirTy::Unit);
    let mut fb = FnBuilder::new(sir_id, ret_ty.clone());

    // Params: allocate one local per param. Param types live in
    // typed.fn_params.
    let params_ty = ctx
        .typed
        .fn_params
        .get(&hir_id)
        .cloned()
        .unwrap_or_default();
    for (name, ty) in &params_ty {
        let l = fb.new_local(
            name.clone(),
            lower_ty(*ty, ctx.ty_arena()),
            true,
            LocalSource::Param,
        );
        fb.params.push(l);
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
    id: SirFnId,
    fb: FnBuilder,
    name: &str,
    ret_ty: SirTy,
    span: SourceSpan,
    hir_fn: Option<sdust_hir::FnId>,
) {
    let mut func = fb.finish(hir_fn, span);
    func.id = id;
    func.name = name.to_string();
    func.ret_ty = ret_ty;
    ctx.prog.fns[id.0 as usize] = func;
}

fn lower_agent_bodies(ctx: &mut LowerCtx) {
    let agents: Vec<(sdust_hir::AgentId, AgentSirId)> = ctx
        .pkg
        .agents
        .iter()
        .filter_map(|(aid, a)| ctx.agent_map.get(&a.name).map(|sirid| (aid, *sirid)))
        .collect();
    for (hir_aid, sirid) in agents {
        lower_one_agent(ctx, hir_aid, sirid);
    }
}

fn lower_one_agent(ctx: &mut LowerCtx, hir_aid: sdust_hir::AgentId, sirid: AgentSirId) {
    let a: HirAgent = ctx.pkg.agents[hir_aid].clone();
    let ag = ctx.prog.agents[sirid.0 as usize].clone();

    // ------- collect state field info from the HIR -------
    let mut state_fields: Vec<(String, SirTy)> = vec![];
    for st in &a.state {
        let ty = match st.ty {
            Some(t) => crate::lower::ty::lower_ty(lookup_hir_type(ctx, t), ctx.ty_arena()),
            None => SirTy::Int(sdust_types::IntKind::I32), // permissive default for `n = 0`
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
        let state_ty = SirTy::Adt(ag.state_adt, vec![]);
        let mut fb = FnBuilder::new(ag.ctor, state_ty.clone());
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
                    init_ops.push(Operand::Const(Const::Int(0, sdust_types::IntKind::I32)));
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
        let ret_ty = SirTy::Unit;
        let mut fb = FnBuilder::new(h_sir_id, ret_ty.clone());
        // Param 0: &mut state.
        let state_ref_ty = SirTy::Ref {
            mutable: true,
            inner: Box::new(SirTy::Adt(ag.state_adt, vec![])),
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
            let l = fb.new_local(p.clone(), SirTy::Str, true, LocalSource::Param);
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
// SirTy::Error flows through fine).
fn lookup_hir_type(ctx: &LowerCtx, t: sdust_hir::TypeId) -> sdust_types::TyId {
    use sdust_hir::HirType;
    let _ = DefRef::Adt; // imported but only used through trait-object access; keep silence
    match &ctx.pkg.types[t] {
        HirType::Unit => ctx.typed.ty_arena.unit,
        _ => ctx.typed.ty_arena.error,
    }
}

// Default impl for sdust_hir::ItemId so the `let _` pattern compiles.
trait ItemIdDefault {
    fn default() -> Self;
}
impl ItemIdDefault for sdust_hir::ItemId {
    fn default() -> Self {
        use la_arena::RawIdx;
        sdust_hir::ItemId::from_raw(RawIdx::from(0))
    }
}
