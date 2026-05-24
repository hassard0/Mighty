//! Top-level item checking: pub-signature validation, fn body checking,
//! agent handlers, etc.

use crate::check::*;
use crate::diag;
use crate::infer::Substitution;
use crate::resolve::{build_def_map, ParamScope};
use crate::ty::*;
use crate::FnDefId;
use crate::ParamId;
use sdust_diagnostics::Diagnostic;
use sdust_hir::*;

pub fn check(pkg: &Package) -> Vec<Diagnostic> {
    let mut arena = TyArena::new();
    let r = build_def_map(pkg, &mut arena);
    let mut defs = r.defs;
    let prelude = r.prelude;
    let mut diagnostics = r.diagnostics;

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

    // Type-check each fn with a body.
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
        // Build param scope from the fn's generics.
        let mut param_scope = ParamScope::default();
        // Allocate a fresh ParamId for each generic in scope (these are
        // distinct from the slot ids stored in fdef.generics).
        for (i, g) in generics.iter().enumerate() {
            param_scope.push(g.name.clone(), ParamId(i as u32));
        }
        // The fn's params/ret were resolved using arbitrary ParamId slots; we
        // don't need to remap them here because we don't try to verify generic
        // soundness in slice 3 (no bounds, no coherence).
        let _ = hir_fn;

        let mut subst = Substitution::new();
        let mut cx_diag: Vec<Diagnostic> = vec![];
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
        };
        cx.locals.enter();
        // Bind parameters.
        for (name, ty) in &params {
            cx.locals.bind(name.clone(), *ty);
        }
        if let Some(b) = body {
            let body_ty = check_block(&mut cx, b, Some(ret));
            // If body produces Unit but ret expects something else, the
            // tail-mismatch is already reported by check_block (Some path).
            let _ = body_ty;
        }
        cx.locals.leave();
        diagnostics.extend(cx_diag);
    }

    // Type-check agent message handlers + state initializers + methods.
    for item_id in &pkg.top_level {
        let item = &pkg.items[*item_id];
        if let Item::Agent(aid) = item {
            let hir_agent = &pkg.agents[*aid];
            // State initializers.
            for state in &hir_agent.state {
                if let Some(init) = state.init {
                    let mut subst = Substitution::new();
                    let mut cx_diag = vec![];
                    let unit_id = arena.unit;
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
                    };
                    let _ = synth_expr(&mut cx, init);
                    diagnostics.extend(cx_diag);
                }
            }
            // Handlers.
            for handler in &hir_agent.handlers {
                let mut subst = Substitution::new();
                let mut cx_diag = vec![];
                // We can't safely borrow arena.unit while arena is borrowed
                // mutably in Cx, so capture the id first.
                let unit_id = arena.unit;
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
                };
                cx.locals.enter();
                // Bind handler params as fresh vars (protocol-aware
                // checking is deferred).
                for pname in &handler.params {
                    let v = cx.subst.fresh_var();
                    let vt = cx.arena.var(v);
                    cx.locals.bind(pname.clone(), vt);
                }
                let _ = check_block(&mut cx, handler.body, None);
                cx.locals.leave();
                diagnostics.extend(cx_diag);
            }
        }
    }

    diagnostics
}
