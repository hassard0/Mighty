//! Top-level item checking: pub-signature validation, fn body checking,
//! agent handlers, etc.

use crate::check::*;
use crate::defs::DefMap;
use crate::diag;
use crate::infer::Substitution;
use crate::resolve::{build_def_map, ParamScope};
use crate::ty::*;
use crate::FnDefId;
use crate::ParamId;
use crate::TypedPackage;
use mty_diagnostics::Diagnostic;
use mty_hir::*;
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
    // v0.37 Track T3 — accumulators for FFI call-site coercions. The Cx
    // for each fn body writes into per-body sinks (`local_coerce_*`),
    // which we merge into these top-level sets at the end of every
    // fn-body loop so the TypedPackage gets the union across all fns.
    let mut coerce_str_to_ptr: std::collections::HashSet<ExprId> = std::collections::HashSet::new();
    let mut coerce_addr_of: std::collections::HashSet<ExprId> = std::collections::HashSet::new();
    // v0.38 Track T3 — `#[ffi_nul_ok]` accelerated-Str path sink. A
    // subset of `coerce_str_to_ptr` whose corresponding extern-c param
    // carries the `#[ffi_nul_ok]` attribute. See `coerce_nul_ok` on
    // [`TypedPackage`] for the lowering contract.
    let mut coerce_nul_ok: std::collections::HashSet<ExprId> = std::collections::HashSet::new();

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
    let mut hir_fn_to_agent: HashMap<mty_hir::FnId, mty_hir::AgentId> = HashMap::new();
    for item_id in &pkg.top_level {
        if let Item::Agent(aid) = &pkg.items[*item_id] {
            let agent = &pkg.agents[*aid];
            for mfid in &agent.methods {
                hir_fn_to_agent.insert(*mfid, *aid);
            }
        }
    }

    // v0.36 Track T2 — populate `fn_params` + `fn_ret` for body-less
    // fns (extern c blocks, trait methods without a default). The
    // body-check loop below skips them so without this pre-pass the
    // SIR lowerer would receive an empty params Vec for every extern
    // fn and downstream codegen would emit empty-signature stubs.
    //
    // The resolver already filled `FnDef.params` / `FnDef.ret` from
    // the HIR (see the `for (fid, fdef_id)` loop in
    // `crates/mty-types/src/resolve.rs`), so we just forward those
    // values into the typed-package output.
    let fn_def_count = defs.fns.len();
    for fdef_id in 0..fn_def_count {
        let id = FnDefId(fdef_id as u32);
        if let Some(f) = defs.fn_def(id) {
            if f.body.is_none() {
                if let Some(fid) = f.hir_fn {
                    fn_params.insert(fid, f.params.clone());
                    fn_ret.insert(fid, f.ret);
                }
            }
        }
    }

    // Type-check each top-level fn with a body.
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
        let is_agent_method = hir_fn.and_then(|fid| hir_fn_to_agent.get(&fid)).is_some();
        let tolerance = if let Some(fid) = hir_fn {
            if let Some(aid) = hir_fn_to_agent.get(&fid) {
                build_agent_tolerance(&pkg.agents[*aid], pkg)
            } else {
                build_tolerance_for_fn(pkg, hir_fn)
            }
        } else {
            build_tolerance_for_fn(pkg, hir_fn)
        };
        // v0.3 (A65): top-level fns stay permissive (slice-3 A21 behavior);
        // agent methods enter a strict AgentBody scope so unknown names
        // (other than the agent's tolerance set) hard-error.
        let scope_kind = if is_agent_method {
            ScopeKind::AgentBody
        } else {
            ScopeKind::TopLevelFn
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
            scope_kind,
            expr_ty: &mut local_expr_ty,
            coerce_str_to_ptr: &mut coerce_str_to_ptr,
            coerce_addr_of: &mut coerce_addr_of,
            coerce_nul_ok: &mut coerce_nul_ok,
        };
        cx.locals.enter();
        for (name, ty) in &params {
            cx.locals.bind(name.clone(), *ty);
        }
        if let Some(b) = body {
            // v0.22 Coverage Closure (MT2019 emit-site): drive the body
            // check through a custom path that:
            //   1. checks each statement (existing behaviour),
            //   2. synthesises the tail expression's type WITHOUT
            //      expected-propagation, so a tail-shape mismatch
            //      doesn't fire MT2001 first,
            //   3. unifies the tail's type with the declared return
            //      type, and emits MT2019 on failure.
            // Blocks with no tail (e.g. `fn main() { stmt; }`) fall
            // back to the existing `Some(ret)` behaviour so MT2001
            // still surfaces on internal let / call mismatches.
            let block = pkg.blocks[b].clone();
            if let Some(tail_expr) = block.tail {
                cx.locals.enter();
                for stmt in &block.stmts {
                    crate::check::check_stmt_pub(&mut cx, stmt);
                }
                let tail_ty = synth_expr(&mut cx, tail_expr);
                cx.locals.leave();
                if crate::infer::unify(tail_ty, ret, cx.subst, cx.arena).is_err() {
                    cx.diag.push(crate::diag::return_type_mismatch(
                        ret,
                        tail_ty,
                        &mty_hir::SourceSpan { start: 0, end: 0 },
                        cx.arena,
                        cx.subst,
                        cx.defs,
                    ));
                }
            } else {
                let _ = check_block(&mut cx, b, Some(ret));
            }
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
                        // v0.3 (A65): state initializers run inside the
                        // strict agent body.
                        scope_kind: ScopeKind::AgentBody,
                        expr_ty: &mut local_expr_ty,
                        coerce_str_to_ptr: &mut coerce_str_to_ptr,
                        coerce_addr_of: &mut coerce_addr_of,
                        coerce_nul_ok: &mut coerce_nul_ok,
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
            //
            // v0.3 (A65): when the agent implements a **known local /
            // prelude** protocol declaring this message, we now:
            // 1. bind each handler param to a fresh inference var,
            // 2. let the handler body infer the param's actual usage
            //    type,
            // 3. unify each inferred type with the protocol's declared
            //    type — mismatch surfaces as MT4031 with both spans.
            //
            // For external (unknown-to-defs) protocols we keep the
            // slice-5 behavior — bind params at the declared types and
            // skip the MT4031 check so v0.2 examples still compile.
            for handler in &hir_agent.handlers {
                let handler_param_tys =
                    lookup_protocol_msg_types(&defs, &hir_agent.protocols, pkg, &handler.message);
                let local_protocol =
                    is_handler_protocol_local(&defs, &hir_agent.protocols, pkg, &handler.message);
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
                    // v0.3 (A65): handler bodies are strict.
                    scope_kind: ScopeKind::HandlerBody,
                    expr_ty: &mut local_expr_ty,
                    coerce_str_to_ptr: &mut coerce_str_to_ptr,
                    coerce_addr_of: &mut coerce_addr_of,
                    coerce_nul_ok: &mut coerce_nul_ok,
                };
                cx.locals.enter();
                // Bind handler params: prefer protocol-declared types,
                // else fresh inference vars (with a MT2026 warning if no
                // protocol declares the message).
                //
                // v0.3 (A65): for **local** protocols we bind to fresh
                // vars and post-check via MT4031; for external protocols
                // we keep the legacy bind-to-declared behavior.
                let mut handler_param_record: Vec<(String, TyId, TyId)> = vec![];
                match handler_param_tys.clone() {
                    HandlerParamLookup::Found(ptys) => {
                        for (i, pname) in handler.params.iter().enumerate() {
                            let declared = ptys.get(i).copied().unwrap_or_else(|| {
                                let v = cx.subst.fresh_var();
                                cx.arena.var(v)
                            });
                            let inferred = if local_protocol {
                                let v = cx.subst.fresh_var();
                                cx.arena.var(v)
                            } else {
                                declared
                            };
                            cx.locals.bind(pname.clone(), inferred);
                            handler_param_record.push((pname.clone(), declared, inferred));
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
                // v0.3 (A65): post-check MT4031 for local protocols.
                if local_protocol {
                    let proto_name = first_protocol_name(&hir_agent.protocols, pkg)
                        .unwrap_or_else(|| "<unknown>".into());
                    for (pname, declared, inferred) in &handler_param_record {
                        let inferred_resolved = cx.subst.resolve(*inferred, cx.arena);
                        let declared_resolved = cx.subst.resolve(*declared, cx.arena);
                        // Skip when the inferred type is still an unbound
                        // fresh var (handler never used the param) or
                        // either side is Error (cascade suppression).
                        if matches!(cx.arena.get(inferred_resolved), TyData::Var(_))
                            || matches!(cx.arena.get(inferred_resolved), TyData::Error)
                            || matches!(cx.arena.get(declared_resolved), TyData::Error)
                        {
                            continue;
                        }
                        if crate::infer::unify(
                            inferred_resolved,
                            declared_resolved,
                            cx.subst,
                            cx.arena,
                        )
                        .is_err()
                        {
                            cx.diag.push(diag::protocol_param_type_mismatch(
                                &handler.message,
                                &proto_name,
                                pname,
                                declared_resolved,
                                inferred_resolved,
                                &handler.span,
                                cx.arena,
                                cx.subst,
                                cx.defs,
                            ));
                        }
                    }
                }
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
                    //
                    // v0.3 (A65): we still mark the scope as SupervisorBody
                    // for documentation; tolerance_open keeps the runtime
                    // policy permissive, so MT2021 won't fire here. Once
                    // slice 7 wires supervisor cap-scopes properly, drop
                    // `tolerance_open` and the SupervisorBody strict
                    // policy will fire automatically.
                    tolerance_open: true,
                    scope_kind: ScopeKind::SupervisorBody,
                    expr_ty: &mut local_expr_ty,
                    coerce_str_to_ptr: &mut coerce_str_to_ptr,
                    coerce_addr_of: &mut coerce_addr_of,
                    coerce_nul_ok: &mut coerce_nul_ok,
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

    // Slice 5: effect inference + validation (after typecheck).
    let profile = load_profile_from_star_toml();
    let fn_effects =
        crate::effects::infer_and_validate(pkg, &mut defs, &arena, profile, &mut diagnostics);

    // Slice 5: strict protocol coverage / extra-handler checks.
    check_protocols_strict(pkg, &defs, &mut diagnostics);

    // v0.21: cap-name resolver pass — emits MT4060..MT4065 over
    // capability-typed method calls. Operates on the partially-built
    // TypedPackage; we assemble it once for the resolver pass to walk.
    let typed_for_resolver = TypedPackage {
        def_map: defs,
        ty_arena: arena,
        expr_ty,
        fn_params,
        fn_ret,
        fn_effects,
        coerce_str_to_ptr: coerce_str_to_ptr.clone(),
        coerce_addr_of: coerce_addr_of.clone(),
        coerce_nul_ok: coerce_nul_ok.clone(),
        diagnostics: vec![],
    };
    let mut cap_diags = vec![];
    crate::cap_check::run(&typed_for_resolver, pkg, &mut cap_diags);
    diagnostics.extend(cap_diags);

    // v0.30 Track A — taint-flow pass. Operates on the HIR + the
    // already-built TypedPackage. Emits MT4099 (TAINTED_VALUE_TO_SINK)
    // when a tainted value reaches a known sink (`fs.write`,
    // `process.Command.arg`, `sql.execute`, `net.Request.body`).
    // See `crates/mty-types/src/taint.rs`.
    let taint_diags = crate::taint::check(pkg, &typed_for_resolver);
    diagnostics.extend(taint_diags);

    // Unpack — Rust moves are not partial, so we destructure the
    // shim back out into the final TypedPackage. (No effective cost:
    // the inner arena / def_map / hash maps are not cloned.)
    let TypedPackage {
        def_map,
        ty_arena,
        expr_ty,
        fn_params,
        fn_ret,
        fn_effects,
        coerce_str_to_ptr: _,
        coerce_addr_of: _,
        coerce_nul_ok: _,
        diagnostics: _,
    } = typed_for_resolver;

    TypedPackage {
        def_map,
        ty_arena,
        expr_ty,
        fn_params,
        fn_ret,
        fn_effects,
        coerce_str_to_ptr,
        coerce_addr_of,
        coerce_nul_ok,
        diagnostics,
    }
}

/// Slice 5: look for `profile = "core"` in `./mighty.toml`. Best-effort —
/// any I/O failure resolves to `Host`.
fn load_profile_from_star_toml() -> crate::effects::Profile {
    use std::fs;
    let Ok(s) = fs::read_to_string("mighty.toml") else {
        return crate::effects::Profile::Host;
    };
    for line in s.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("profile") {
            let rest = rest.trim_start_matches(|c: char| c == '=' || c.is_whitespace());
            let rest = rest.trim_matches('"');
            return crate::effects::Profile::parse_profile(rest);
        }
    }
    crate::effects::Profile::Host
}

/// Slice 5: protocol-strict checks. For each agent:
/// - MT4032 protocol_missing_handler: implemented protocol declares a
///   message that has no `on Msg(...)` handler.
/// - MT4033 protocol_extra_handler: handler refers to a message that
///   no implemented protocol declares.
/// - MT4030 protocol_arity_mismatch: handler's param count differs from
///   the protocol-declared signature.
fn check_protocols_strict(
    pkg: &Package,
    defs: &crate::defs::DefMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for item_id in &pkg.top_level {
        let item = &pkg.items[*item_id];
        let agent = match item {
            Item::Agent(aid) => &pkg.agents[*aid],
            _ => continue,
        };
        // Collect protocol names from the agent's declared protocols.
        let proto_names: Vec<String> = agent
            .protocols
            .iter()
            .flat_map(|ptid| {
                let ty = &pkg.types[*ptid];
                collect_protocol_names(ty)
            })
            .collect();
        if proto_names.is_empty() {
            continue;
        }
        // Slice-5 conservative behavior: skip strict checks when ANY
        // declared protocol is unknown to us (e.g. external protocol
        // declared in another module). This keeps slice-3/4 examples
        // green while still enforcing strictness on locally-declared
        // protocols.
        let any_unknown = proto_names
            .iter()
            .any(|pn| !defs.protocol_msg_names.contains_key(pn));
        if any_unknown {
            continue;
        }
        // Compose declared messages.
        let declared_msgs: HashMap<String, Vec<TyId>> = proto_names
            .iter()
            .flat_map(|pn| {
                let names = defs.protocol_msg_names.get(pn).cloned().unwrap_or_default();
                names.into_iter().filter_map(move |mname| {
                    defs.protocol_msgs
                        .get(&(pn.clone(), mname.clone()))
                        .cloned()
                        .map(|p| (mname, p))
                })
            })
            .collect();
        // MT4032: missing handlers.
        let provided: std::collections::HashSet<String> =
            agent.handlers.iter().map(|h| h.message.clone()).collect();
        for mname in declared_msgs.keys() {
            if !provided.contains(mname) {
                // Choose the first protocol that declares the message.
                let proto = proto_names
                    .iter()
                    .find(|pn| {
                        defs.protocol_msg_names
                            .get(*pn)
                            .map(|v| v.contains(mname))
                            .unwrap_or(false)
                    })
                    .cloned()
                    .unwrap_or_else(|| proto_names[0].clone());
                diagnostics.push(crate::diag::protocol_missing_handler(
                    mname,
                    &proto,
                    &agent.span,
                ));
            }
        }
        // MT4030 + MT4033 per handler.
        for h in &agent.handlers {
            match declared_msgs.get(&h.message) {
                Some(decl_params) => {
                    if decl_params.len() != h.params.len() {
                        diagnostics.push(crate::diag::protocol_arity_mismatch(
                            &h.message,
                            decl_params.len(),
                            h.params.len(),
                            &h.span,
                        ));
                    }
                }
                None => {
                    diagnostics.push(crate::diag::protocol_extra_handler(&h.message, &h.span));
                }
            }
        }
    }
}

/// Diagnostics-only entry point (back-compat wrapper).
pub fn check(pkg: &Package) -> Vec<Diagnostic> {
    check_typed(pkg).diagnostics
}

#[derive(Clone)]
enum HandlerParamLookup {
    Found(Vec<TyId>),
    NoProtocols,
    Unknown,
}

/// v0.3 (A65): true iff *any* protocol the agent implements that declares
/// `msg_name` is **local** — i.e. its name appears in
/// `defs.protocol_msg_names`. External protocols (e.g. `http.Handler` from
/// example 19) live in another module and are not yet visible to defs;
/// for those we skip the MT4031 strict param-type check and continue
/// emitting MT2026 warnings instead.
fn is_handler_protocol_local(
    defs: &DefMap,
    proto_type_ids: &[TypeId],
    pkg: &Package,
    msg_name: &str,
) -> bool {
    for ptid in proto_type_ids {
        let ty = &pkg.types[*ptid];
        for pname in collect_protocol_names(ty) {
            if let Some(names) = defs.protocol_msg_names.get(&pname) {
                if names.iter().any(|n| n == msg_name) {
                    return true;
                }
            }
        }
    }
    false
}

/// v0.3 (A65): the first protocol name attached to an agent, used in the
/// MT4031 diagnostic's "protocol declares ..." note.
fn first_protocol_name(proto_type_ids: &[TypeId], pkg: &Package) -> Option<String> {
    for ptid in proto_type_ids {
        let ty = &pkg.types[*ptid];
        if let Some(n) = collect_protocol_names(ty).into_iter().next() {
            return Some(n);
        }
    }
    None
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
