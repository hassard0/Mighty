//! Slice 5: effect inference (spec §9).
//!
//! Walks every fn body bottom-up and computes the inferred effect set.
//! Recursion is handled by a simple call-graph fixpoint over the per-fn
//! effect map.
//!
//! Public-fn discipline: the declared `effect ...` clause must be a
//! superset of the inferred set. Else `SD4001 effect_undeclared`.
//!
//! Strict profile (`profile = "core"`) bans `alloc` — `SD4002 alloc_in_core`.

use crate::defs::*;
use crate::ty::*;
use sdust_diagnostics::Diagnostic;
use sdust_hir::*;
use std::collections::{HashMap, HashSet};

/// Profile loaded from `star.toml` (slice 5). `Host` is permissive; `Core`
/// is the strict embedded-target profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Host,
    Web,
    Edge,
    Core,
}

impl Profile {
    pub fn parse_profile(s: &str) -> Self {
        match s {
            "core" => Profile::Core,
            "edge" => Profile::Edge,
            "web" => Profile::Web,
            _ => Profile::Host,
        }
    }
}

/// Per-fn inferred effects.
pub type EffectSet = HashSet<EffectId>;

/// Run effect inference + validation over the whole package.
pub fn infer_and_validate(
    pkg: &Package,
    defs: &mut DefMap,
    arena: &TyArena,
    profile: Profile,
    diagnostics: &mut Vec<Diagnostic>,
) -> HashMap<FnId, Vec<EffectId>> {
    // Intern the effects we'll attribute.
    let alloc = defs.intern_effect("alloc");
    let net = defs.intern_effect("net");
    let fs = defs.intern_effect("fs");
    let clock = defs.intern_effect("time");
    let dom = defs.intern_effect("dom");
    let model = defs.intern_effect("model");
    let spawn = defs.intern_effect("spawn");
    let time = clock;
    let unsafe_e = defs.intern_effect("unsafe");

    let known = KnownEffects {
        alloc,
        net,
        fs,
        clock,
        dom,
        model,
        spawn,
        time,
        unsafe_e,
    };

    // Initial pass: per-fn body-walk inferred effects (no recursion).
    let mut fn_effects: HashMap<FnId, EffectSet> = HashMap::new();
    let mut fn_calls: HashMap<FnId, Vec<FnDefId>> = HashMap::new();

    // Build a per-fn callee list during the first walk so the fixpoint can
    // union effects from callees.
    for (fid, hir_fn) in pkg.fns.iter() {
        let body = match hir_fn.body {
            Some(b) => b,
            None => continue,
        };
        let mut effects = EffectSet::default();
        let mut callees: Vec<FnDefId> = vec![];
        walk_block_effects(body, pkg, defs, arena, &known, &mut effects, &mut callees);
        fn_effects.insert(fid, effects);
        fn_calls.insert(fid, callees);
    }

    // Fixpoint over callees. Bound at 32 iterations.
    for _ in 0..32 {
        let mut changed = false;
        let snapshot: HashMap<FnId, EffectSet> = fn_effects.clone();
        for (fid, callees) in &fn_calls {
            for callee_def in callees {
                // Look up the callee's hir_fn id, then its inferred set.
                let callee_hir = defs.fn_def(*callee_def).and_then(|f| f.hir_fn);
                if let Some(chid) = callee_hir {
                    if let Some(callee_effects) = snapshot.get(&chid).cloned() {
                        let cur = fn_effects.entry(*fid).or_default();
                        for e in &callee_effects {
                            if cur.insert(*e) {
                                changed = true;
                            }
                        }
                    }
                }
                // Also union the callee's declared effects (for opaque /
                // prelude / extern fns).
                if let Some(callee_def) = defs.fn_def(*callee_def) {
                    let declared = callee_def.effects.clone();
                    let cur = fn_effects.entry(*fid).or_default();
                    for e in &declared {
                        if cur.insert(*e) {
                            changed = true;
                        }
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }

    // Public-fn validation.
    for (fid, hir_fn) in pkg.fns.iter() {
        if !hir_fn.is_pub {
            continue;
        }
        let inferred = match fn_effects.get(&fid) {
            Some(s) => s.clone(),
            None => continue,
        };
        if inferred.is_empty() {
            continue;
        }
        let declared: HashSet<EffectId> = hir_fn
            .effects
            .iter()
            .map(|n| defs.intern_effect(n.clone()))
            .collect();
        let missing: Vec<String> = inferred
            .iter()
            .filter(|e| !declared.contains(e))
            .filter_map(|e| effect_name(defs, *e))
            .collect();
        if !missing.is_empty() {
            let mut sorted = missing;
            sorted.sort();
            diagnostics.push(crate::diag::effect_undeclared(
                &hir_fn.name,
                &sorted,
                &hir_fn.span,
            ));
        }
        // Strict profile: ban alloc.
        if profile == Profile::Core && inferred.contains(&known.alloc) {
            diagnostics.push(crate::diag::alloc_in_core(&hir_fn.name, &hir_fn.span));
        }
    }

    // Produce the deterministic-order output map.
    fn_effects
        .into_iter()
        .map(|(k, v)| {
            let mut xs: Vec<EffectId> = v.into_iter().collect();
            xs.sort_by_key(|e| e.0);
            (k, xs)
        })
        .collect()
}

struct KnownEffects {
    alloc: EffectId,
    net: EffectId,
    fs: EffectId,
    clock: EffectId,
    dom: EffectId,
    model: EffectId,
    spawn: EffectId,
    time: EffectId,
    unsafe_e: EffectId,
}

fn effect_name(defs: &DefMap, eid: EffectId) -> Option<String> {
    defs.effects
        .iter()
        .find(|(_, v)| **v == eid)
        .map(|(k, _)| k.clone())
}

fn walk_block_effects(
    bid: BlockId,
    pkg: &Package,
    defs: &DefMap,
    arena: &TyArena,
    known: &KnownEffects,
    out: &mut EffectSet,
    callees: &mut Vec<FnDefId>,
) {
    let block = &pkg.blocks[bid];
    for stmt in &block.stmts {
        match stmt {
            HirStmt::Let { init, .. } => {
                if let Some(e) = init {
                    walk_expr_effects(*e, pkg, defs, arena, known, out, callees);
                }
            }
            HirStmt::Expr(e) => {
                walk_expr_effects(*e, pkg, defs, arena, known, out, callees);
            }
        }
    }
    if let Some(t) = block.tail {
        walk_expr_effects(t, pkg, defs, arena, known, out, callees);
    }
}

fn walk_expr_effects(
    eid: ExprId,
    pkg: &Package,
    defs: &DefMap,
    arena: &TyArena,
    known: &KnownEffects,
    out: &mut EffectSet,
    callees: &mut Vec<FnDefId>,
) {
    let expr = &pkg.exprs[eid];
    match expr {
        HirExpr::Literal(_) | HirExpr::Path(_) | HirExpr::PathGeneric { .. } | HirExpr::Error => {}
        HirExpr::Block(b) => walk_block_effects(*b, pkg, defs, arena, known, out, callees),
        HirExpr::Tuple(xs) | HirExpr::Array(xs) => {
            for x in xs {
                walk_expr_effects(*x, pkg, defs, arena, known, out, callees);
            }
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            walk_expr_effects(*lhs, pkg, defs, arena, known, out, callees);
            walk_expr_effects(*rhs, pkg, defs, arena, known, out, callees);
        }
        HirExpr::Unary { rhs, .. } => {
            walk_expr_effects(*rhs, pkg, defs, arena, known, out, callees);
        }
        HirExpr::Borrow { inner, .. } => {
            walk_expr_effects(*inner, pkg, defs, arena, known, out, callees);
        }
        HirExpr::Move(inner) => walk_expr_effects(*inner, pkg, defs, arena, known, out, callees),
        HirExpr::Call { callee, args } => {
            walk_expr_effects(*callee, pkg, defs, arena, known, out, callees);
            for a in args {
                walk_expr_effects(a.value, pkg, defs, arena, known, out, callees);
            }
            // If the callee path resolves to a known fn, remember the call.
            if let HirExpr::Path(segs) = &pkg.exprs[*callee] {
                if segs.len() == 1 {
                    if let Some(DefRef::Fn(fid)) = defs.lookup(&segs[0]) {
                        callees.push(fid);
                    }
                } else if segs.len() >= 2 {
                    // Cap-style call: `net.get(...)`, `fs.read(...)`, etc.
                    match segs[0].as_str() {
                        "fs" => {
                            out.insert(known.fs);
                        }
                        "net" => {
                            out.insert(known.net);
                        }
                        "clock" => {
                            out.insert(known.clock);
                        }
                        "dom" => {
                            out.insert(known.dom);
                        }
                        "model" => {
                            out.insert(known.model);
                        }
                        _ => {}
                    }
                }
            }
        }
        HirExpr::MethodCall {
            receiver,
            method,
            args,
        } => {
            walk_expr_effects(*receiver, pkg, defs, arena, known, out, callees);
            for a in args {
                walk_expr_effects(a.value, pkg, defs, arena, known, out, callees);
            }
            // Capability method effect heuristic: net.* / fs.* / clock.* / dom.* / model.*
            // Use the receiver's PATH name as a hint.
            if let HirExpr::Path(segs) = &pkg.exprs[*receiver] {
                if let Some(first) = segs.first() {
                    if matches!(first.as_str(), "fs") {
                        out.insert(known.fs);
                    } else if matches!(first.as_str(), "net") {
                        out.insert(known.net);
                    } else if matches!(first.as_str(), "clock") {
                        out.insert(known.clock);
                    } else if matches!(first.as_str(), "dom") {
                        out.insert(known.dom);
                    } else if matches!(first.as_str(), "model") {
                        out.insert(known.model);
                    }
                }
            }
            // Container method heuristic: .push/.pop/.insert/.encode/.collect → alloc
            if matches!(
                method.as_str(),
                "push" | "pop" | "insert" | "encode" | "collect" | "to_string" | "clone"
            ) {
                out.insert(known.alloc);
            }
        }
        HirExpr::Field { receiver, .. } => {
            walk_expr_effects(*receiver, pkg, defs, arena, known, out, callees);
        }
        HirExpr::Index { receiver, idx } => {
            walk_expr_effects(*receiver, pkg, defs, arena, known, out, callees);
            walk_expr_effects(*idx, pkg, defs, arena, known, out, callees);
        }
        HirExpr::If { cond, then, else_ } => {
            walk_expr_effects(*cond, pkg, defs, arena, known, out, callees);
            walk_block_effects(*then, pkg, defs, arena, known, out, callees);
            if let Some(e) = else_ {
                walk_expr_effects(*e, pkg, defs, arena, known, out, callees);
            }
        }
        HirExpr::IfLet {
            scrutinee,
            then,
            else_,
            ..
        } => {
            walk_expr_effects(*scrutinee, pkg, defs, arena, known, out, callees);
            walk_block_effects(*then, pkg, defs, arena, known, out, callees);
            if let Some(e) = else_ {
                walk_expr_effects(*e, pkg, defs, arena, known, out, callees);
            }
        }
        HirExpr::Match { scrutinee, arms } => {
            walk_expr_effects(*scrutinee, pkg, defs, arena, known, out, callees);
            for arm in arms {
                walk_expr_effects(arm.body, pkg, defs, arena, known, out, callees);
                if let Some(g) = arm.guard {
                    walk_expr_effects(g, pkg, defs, arena, known, out, callees);
                }
            }
        }
        HirExpr::For { iter, body, .. } => {
            walk_expr_effects(*iter, pkg, defs, arena, known, out, callees);
            walk_block_effects(*body, pkg, defs, arena, known, out, callees);
        }
        HirExpr::While { cond, body } => {
            walk_expr_effects(*cond, pkg, defs, arena, known, out, callees);
            walk_block_effects(*body, pkg, defs, arena, known, out, callees);
        }
        HirExpr::Loop { body } => walk_block_effects(*body, pkg, defs, arena, known, out, callees),
        HirExpr::Return(Some(e)) => walk_expr_effects(*e, pkg, defs, arena, known, out, callees),
        HirExpr::Return(None) => {}
        HirExpr::Struct { fields, .. } => {
            for (_, e) in fields {
                walk_expr_effects(*e, pkg, defs, arena, known, out, callees);
            }
        }
        HirExpr::Map(entries) => {
            // Maps require allocation.
            out.insert(known.alloc);
            for (k, v) in entries {
                walk_expr_effects(*k, pkg, defs, arena, known, out, callees);
                walk_expr_effects(*v, pkg, defs, arena, known, out, callees);
            }
        }
        HirExpr::Send { target, args, .. } | HirExpr::Ask { target, args, .. } => {
            out.insert(known.spawn);
            walk_expr_effects(*target, pkg, defs, arena, known, out, callees);
            for a in args {
                walk_expr_effects(a.value, pkg, defs, arena, known, out, callees);
            }
        }
        HirExpr::Deadline { inner, dur } => {
            out.insert(known.time);
            walk_expr_effects(*inner, pkg, defs, arena, known, out, callees);
            walk_expr_effects(*dur, pkg, defs, arena, known, out, callees);
        }
        HirExpr::Question(inner) => {
            walk_expr_effects(*inner, pkg, defs, arena, known, out, callees);
        }
        HirExpr::Spawn { inner, .. } => {
            out.insert(known.spawn);
            walk_expr_effects(*inner, pkg, defs, arena, known, out, callees);
        }
        HirExpr::Detach(inner) | HirExpr::Join(inner) | HirExpr::Run(inner) => {
            if matches!(expr, HirExpr::Detach(_)) {
                out.insert(known.spawn);
            }
            walk_expr_effects(*inner, pkg, defs, arena, known, out, callees);
        }
        HirExpr::HtmlTemplate(_) => {
            out.insert(known.alloc);
        }
        HirExpr::Unsafe(b) => {
            out.insert(known.unsafe_e);
            walk_block_effects(*b, pkg, defs, arena, known, out, callees);
        }
        HirExpr::Arena { body, .. } => {
            out.insert(known.alloc);
            walk_expr_effects(*body, pkg, defs, arena, known, out, callees);
        }
        HirExpr::TaskScope { body, .. } => {
            walk_block_effects(*body, pkg, defs, arena, known, out, callees);
        }
        HirExpr::Budget { entries, body } => {
            for (_, e) in entries {
                walk_expr_effects(*e, pkg, defs, arena, known, out, callees);
            }
            walk_expr_effects(*body, pkg, defs, arena, known, out, callees);
        }
        HirExpr::Sandbox { entries, body, .. } => {
            for (_, e) in entries {
                walk_expr_effects(*e, pkg, defs, arena, known, out, callees);
            }
            walk_block_effects(*body, pkg, defs, arena, known, out, callees);
        }
        HirExpr::Cast { lhs, .. } => {
            walk_expr_effects(*lhs, pkg, defs, arena, known, out, callees);
        }
        HirExpr::Lambda { body, .. } => {
            walk_block_effects(*body, pkg, defs, arena, known, out, callees);
        }
    }
}
