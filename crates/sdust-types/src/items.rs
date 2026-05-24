//! Top-level item checking: pub-signature validation, fn body checking,
//! agent handlers, etc.

use crate::check::*;
use crate::diag;
use crate::infer::Substitution;
use crate::resolve::{build_def_map, ParamScope};
use crate::ty::*;
use crate::FnDefId;
use crate::ParamId;
use crate::TypedPackage;
use sdust_diagnostics::Diagnostic;
use sdust_hir::*;
use std::collections::{HashMap, HashSet};

/// Slice-4 entry point: returns the typed package along with diagnostics.
pub fn check_typed(pkg: &Package) -> TypedPackage {
    let mut arena = TyArena::new();
    let r = build_def_map(pkg, &mut arena);
    let mut defs = r.defs;
    let prelude = r.prelude;
    let mut diagnostics = r.diagnostics;
    let mut expr_ty: HashMap<ExprId, TyId> = HashMap::new();
    let mut fn_params: HashMap<FnId, Vec<(String, TyId)>> = HashMap::new();
    let mut fn_ret: HashMap<FnId, TyId> = HashMap::new();

    // Pub-signature validation: every pub fn param must have an explicit type.
    for item_id in &pkg.top_level {
        let item = &pkg.items[*item_id];
        if let Item::Fn(fid) = item {
            let hf = &pkg.fns[*fid];
            if hf.is_pub {
                for p in &hf.params {
                    if p.ty.is_none() {
                        diagnostics.push(diag::pub_param_needs_type(&p.name, &p.span));
                    }
                }
            }
        }
    }

    // Build a lookup from HirFn id → agent that owns it (for tolerance).
    let mut hir_fn_to_agent: HashMap<sdust_hir::FnId, sdust_hir::AgentId> = HashMap::new();
    for item_id in &pkg.top_level {
        if let Item::Agent(aid) = &pkg.items[*item_id] {
            let agent = &pkg.agents[*aid];
            for mfid in &agent.methods {
                hir_fn_to_agent.insert(*mfid, *aid);
            }
        }
    }

    // Type-check each top-level fn with a body.
    let fn_def_count = defs.fns.len();
    for fdef_id in 0..fn_def_count {
        let id = FnDefId(fdef_id as u32);
        let (body, ret, generics, params, hir_fn) = match defs.fn_def(id) {
            Some(f) if f.body.is_some() => (
                f.body,
                f.ret,
                f.generics.clone(),
                f.params.clone(),
                f.hir_fn,
            ),
            _ => continue,
        };
        let mut param_scope = ParamScope::default();
        for (i, g) in generics.iter().enumerate() {
            param_scope.push(g.name.clone(), ParamId(i as u32));
        }
        let _ = hir_fn;

        let mut subst = Substitution::new();
        let mut cx_diag: Vec<Diagnostic> = vec![];
        let mut local_expr_ty: HashMap<ExprId, TyId> = HashMap::new();
        // Tolerance: if this fn is an agent method, use the owning agent's
        // tolerance set so the method body can reference state / siblings.
        let tolerance = if let Some(fid) = hir_fn {
            if let Some(aid) = hir_fn_to_agent.get(&fid) {
                build_agent_tolerance(&pkg.agents[*aid], pkg)
            } else {
                build_tolerance_for_fn(pkg, hir_fn)
            }
        } else {
            build_tolerance_for_fn(pkg, hir_fn)
        };
        let mut cx = Cx {
            pkg,
            defs: &mut defs,
            arena: &mut arena,
            subst: &mut subst,
            diag: &mut cx_diag,
            locals: LocalScope::default(),
            return_ty: ret,
            result_id: prelude.result,
            option_id: prelude.option,
            agent_ref_id: prelude.agent_ref,
            param_scope,
            tolerance,
            tolerance_open: false,
            expr_ty: &mut local_expr_ty,
        };
        cx.locals.enter();
        for (name, ty) in &params {
            cx.locals.bind(name.clone(), *ty);
        }
        if let Some(b) = body {
            let body_ty = check_block(&mut cx, b, Some(ret));
            let _ = body_ty;
        }
        cx.locals.leave();
        diagnostics.extend(cx_diag);
        // Defaulting pass for this fn's expr types.
        for (e, t) in local_expr_ty.iter_mut() {
            *t = default_ty(*t, &subst, &mut arena);
            expr_ty.insert(*e, *t);
        }
        if let Some(fid) = hir_fn {
            fn_params.insert(
                fid,
                params
                    .iter()
                    .map(|(n, t)| (n.clone(), default_ty(*t, &subst, &mut arena)))
                    .collect(),
            );
            fn_ret.insert(fid, default_ty(ret, &subst, &mut arena));
        }
    }

    // Agent state init / handlers / methods.
    for item_id in &pkg.top_level {
        let item = &pkg.items[*item_id];
        if let Item::Agent(aid) = item {
            let hir_agent = &pkg.agents[*aid].clone();
            let agent_tolerance = build_agent_tolerance(hir_agent, pkg);
            // State initializers.
            for state in &hir_agent.state {
                if let Some(init) = state.init {
                    let mut subst = Substitution::new();
                    let mut cx_diag = vec![];
                    let unit_id = arena.unit;
                    let mut local_expr_ty: HashMap<ExprId, TyId> = HashMap::new();
                    let mut cx = Cx {
                        pkg,
                        defs: &mut defs,
                        arena: &mut arena,
                        subst: &mut subst,
                        diag: &mut cx_diag,
                        locals: LocalScope::default(),
                        return_ty: unit_id,
                        result_id: prelude.result,
                        option_id: prelude.option,
                        agent_ref_id: prelude.agent_ref,
                        param_scope: ParamScope::default(),
                        tolerance: agent_tolerance.clone(),
                        tolerance_open: false,
                        expr_ty: &mut local_expr_ty,
                    };
                    let _ = synth_expr(&mut cx, init);
                    diagnostics.extend(cx_diag);
                    for (e, t) in local_expr_ty.iter_mut() {
                        *t = default_ty(*t, &subst, &mut arena);
                        expr_ty.insert(*e, *t);
                    }
                }
            }
            // Handlers — protocol-aware param typing.
            for handler in &hir_agent.handlers {
                let handler_param_tys =
                    lookup_protocol_msg_types(&defs, &hir_agent.protocols, pkg, &handler.message);
                let mut subst = Substitution::new();
                let mut cx_diag = vec![];
                let unit_id = arena.unit;
                let mut local_expr_ty: HashMap<ExprId, TyId> = HashMap::new();
                let mut cx = Cx {
                    pkg,
                    defs: &mut defs,
                    arena: &mut arena,
                    subst: &mut subst,
                    diag: &mut cx_diag,
                    locals: LocalScope::default(),
                    return_ty: unit_id,
                    result_id: prelude.result,
                    option_id: prelude.option,
                    agent_ref_id: prelude.agent_ref,
                    param_scope: ParamScope::default(),
                    tolerance: agent_tolerance.clone(),
                    tolerance_open: false,
                    expr_ty: &mut local_expr_ty,
                };
                cx.locals.enter();
                // Bind handler params: prefer protocol-declared types,
                // else fresh inference vars (with a SD2026 warning if no
                // protocol declares the message).
                match handler_param_tys {
                    HandlerParamLookup::Found(ptys) => {
                        for (i, pname) in handler.params.iter().enumerate() {
                            let ty = ptys.get(i).copied().unwrap_or_else(|| {
                                let v = cx.subst.fresh_var();
                                cx.arena.var(v)
                            });
                            cx.locals.bind(pname.clone(), ty);
                        }
                    }
                    HandlerParamLookup::NoProtocols => {
                        for pname in &handler.params {
                            let v = cx.subst.fresh_var();
                            let vt = cx.arena.var(v);
                            cx.locals.bind(pname.clone(), vt);
                        }
                    }
                    HandlerParamLookup::Unknown => {
                        cx.diag
                            .push(diag::protocol_msg_unknown(&handler.message, &handler.span));
                        for pname in &handler.params {
                            let v = cx.subst.fresh_var();
                            let vt = cx.arena.var(v);
                            cx.locals.bind(pname.clone(), vt);
                        }
                    }
                }
                let _ = check_block(&mut cx, handler.body, None);
                cx.locals.leave();
                diagnostics.extend(cx_diag);
                for (e, t) in local_expr_ty.iter_mut() {
                    *t = default_ty(*t, &subst, &mut arena);
                    expr_ty.insert(*e, *t);
                }
            }
            // Agent methods are checked via the fn loop above (they get
            // FnDefIds registered in build_def_map). We do not re-check
            // them here.
        }
        // Supervisor child expressions — type-check them under a
        // tolerance set covering their child names.
        if let Item::Supervisor(sid) = item {
            let sup = &pkg.supervisors[*sid].clone();
            let mut tol: HashSet<String> = HashSet::new();
            for (child_name, _) in &sup.children {
                tol.insert(child_name.clone());
            }
            for (_, child_expr) in &sup.children {
                let mut subst = Substitution::new();
                let mut cx_diag = vec![];
                let unit_id = arena.unit;
                let mut local_expr_ty: HashMap<ExprId, TyId> = HashMap::new();
                let mut cx = Cx {
                    pkg,
                    defs: &mut defs,
                    arena: &mut arena,
                    subst: &mut subst,
                    diag: &mut cx_diag,
                    locals: LocalScope::default(),
                    return_ty: unit_id,
                    result_id: prelude.result,
                    option_id: prelude.option,
                    agent_ref_id: prelude.agent_ref,
                    param_scope: ParamScope::default(),
                    tolerance: tol.clone(),
                    // Supervisor child expressions are spawn-shaped
                    // (`spawn Agent(cap, cap)`); the capability identifiers
                    // come from a supervisor's enclosing capability context
                    // which slice 4 does not model. Open tolerance for
                    // these one-line expressions.
                    tolerance_open: true,
                    expr_ty: &mut local_expr_ty,
                };
                let _ = synth_expr(&mut cx, *child_expr);
                diagnostics.extend(cx_diag);
                for (e, t) in local_expr_ty.iter_mut() {
                    *t = default_ty(*t, &subst, &mut arena);
                    expr_ty.insert(*e, *t);
                }
            }
        }
    }

    TypedPackage {
        def_map: defs,
        ty_arena: arena,
        expr_ty,
        fn_params,
        fn_ret,
        diagnostics,
    }
}

/// Diagnostics-only entry point (back-compat wrapper).
pub fn check(pkg: &Package) -> Vec<Diagnostic> {
    check_typed(pkg).diagnostics
}

enum HandlerParamLookup {
    Found(Vec<TyId>),
    NoProtocols,
    Unknown,
}

/// Look up the parameter types for an agent handler's message by searching
/// the agent's implemented protocols.
fn lookup_protocol_msg_types(
    defs: &crate::defs::DefMap,
    proto_type_ids: &[TypeId],
    pkg: &Package,
    msg_name: &str,
) -> HandlerParamLookup {
    if proto_type_ids.is_empty() {
        return HandlerParamLookup::NoProtocols;
    }
    for ptid in proto_type_ids {
        let ty = &pkg.types[*ptid];
        // Get the protocol name (and inline composition if `protocol Web = A + B`).
        let proto_names = collect_protocol_names(ty);
        for pname in proto_names {
            if let Some(ptys) = defs
                .protocol_msgs
                .get(&(pname.clone(), msg_name.to_string()))
            {
                return HandlerParamLookup::Found(ptys.clone());
            }
        }
    }
    HandlerParamLookup::Unknown
}

/// Recursively gather protocol names from an HirType (handles `A + B` composition).
fn collect_protocol_names(ty: &HirType) -> Vec<String> {
    match ty {
        HirType::Path { segments, .. } => {
            if let Some(last) = segments.last() {
                vec![last.clone()]
            } else {
                vec![]
            }
        }
        _ => vec![],
    }
}

/// Build the tolerance set for a top-level fn. Empty by default — only the
/// fn's own params and prelude built-ins resolve cleanly.
fn build_tolerance_for_fn(_pkg: &Package, _hir_fn: Option<FnId>) -> HashSet<String> {
    // Slice 4: top-level fn bodies have NO tolerance. The body must resolve
    // everything through locals/params or the prelude.
    // The unsafe/sandbox/budget/arena sub-scopes contribute via the walker
    // (see check_block / synth_expr opening sub-scopes).
    HashSet::new()
}

/// Build the tolerance set for an agent body (state init, handler, method).
/// Includes the agent's state names, ctor-param names, method names, and
/// per-protocol message names (so cross-handler references like `draw()`
/// from `on Click` work, and so capability identifiers don't error).
fn build_agent_tolerance(agent: &HirAgent, pkg: &Package) -> HashSet<String> {
    let mut t = HashSet::new();
    for s in &agent.state {
        t.insert(s.name.clone());
    }
    for c in &agent.ctor_params {
        t.insert(c.clone());
    }
    for mfid in &agent.methods {
        let hf = &pkg.fns[*mfid];
        t.insert(hf.name.clone());
    }
    t
}

/// Walk the substitution to resolve `ty` and pin IntInfer→I32, FloatInfer→F64.
pub(crate) fn default_ty(ty: TyId, subst: &Substitution, arena: &mut TyArena) -> TyId {
    let resolved = subst.resolve(ty, arena);
    let data = arena.get(resolved).clone();
    match data {
        TyData::Int(IntKind::IntInfer) => arena.i32,
        TyData::Float(FloatKind::FloatInfer) => arena.f64,
        TyData::Tuple(xs) => {
            let new: Vec<TyId> = xs
                .into_iter()
                .map(|t| default_ty(t, subst, arena))
                .collect();
            arena.tuple(new)
        }
        TyData::Array { elem, len } => {
            let e = default_ty(elem, subst, arena);
            arena.array(e, len)
        }
        TyData::Ref { mutable, inner } => {
            let i = default_ty(inner, subst, arena);
            arena.ref_to(mutable, i)
        }
        TyData::Fn {
            params,
            ret,
            effects,
        } => {
            let p: Vec<TyId> = params
                .into_iter()
                .map(|t| default_ty(t, subst, arena))
                .collect();
            let r = default_ty(ret, subst, arena);
            arena.fn_ty(p, r, effects)
        }
        TyData::Adt(id, args) => {
            let new: Vec<TyId> = args
                .into_iter()
                .map(|t| default_ty(t, subst, arena))
                .collect();
            arena.adt(id, new)
        }
        TyData::RawPtr(inner) => {
            let i = default_ty(inner, subst, arena);
            arena.raw_ptr(i)
        }
        _ => resolved,
    }
}
