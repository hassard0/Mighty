//! Slice 5: effect inference (spec §9).
//!
//! Walks every fn body bottom-up and computes the inferred effect set.
//! Recursion is handled by a simple call-graph fixpoint over the per-fn
//! effect map.
//!
//! Public-fn discipline: the declared `effect ...` clause must be a
//! superset of the inferred set. Else `MT4001 effect_undeclared`.
//!
//! Strict profile (`profile = "core"`) bans `alloc` — `MT4002 alloc_in_core`.
//!
//! v0.13: this module also hosts the **effect-row polymorphism**
//! infrastructure (RFC-008). The row machinery is additive — none of
//! the existing call-graph fixpoint or public-fn validation routes go
//! through it; instead, the row API is a stand-alone module that the
//! HOF-call site checker (and v0.14 surface-syntax extensions) build
//! on top of. See [`row`] sub-module.

use crate::defs::*;
use crate::ty::*;
use mty_diagnostics::Diagnostic;
use mty_hir::*;
use std::collections::{HashMap, HashSet};

// v0.13 — RFC-008 effect-row polymorphism. Defined at the bottom of
// this file as an inline `mod row { ... }` block (alongside the unit
// tests) so it sits next to the existing closed-row inference and
// doesn't perturb the crate's existing module layout.
pub use self::row::{
    apply_row_subst, instantiate_row_sig, pretty_row, stdlib_list_map_sig, subsume_closed,
    unify_rows, EffectRow, RowError, RowPolySig, RowSubst, RowVar,
};
// v0.14 — RFC-008 §"v0.14 follow-up" — additional stdlib HOF row-poly
// signatures. ADDITIVE re-exports; the v0.13 `stdlib_list_map_sig` is
// preserved as the canonical example/anchor.
pub use self::row::stdlib_sigs;

/// Profile loaded from `mighty.toml` (slice 5). `Host` is permissive; `Core`
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
        let Some(body) = hir_fn.body else { continue };
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

    // v0.15: row-poly stdlib HOF call-site validation. Runs BEFORE the
    // fn-level MT4001 check so authors see the more specific MT4050
    // ("closure passed to `map` introduces effects {fs} ...") first,
    // followed by MT4001 ("fn `f` is missing effects: fs") as the
    // catch-all. The two are intentionally complementary — MT4050
    // points to the exact call site, MT4001 to the fn signature.
    validate_row_dispatch(pkg, defs, arena, &known, diagnostics);

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

/// v0.15: post-inference pass that walks every pub fn body looking for
/// stdlib HOF method calls whose closure arguments contribute effects
/// the caller's declared effect clause does not allow. Emits MT4050
/// (`row_subsumption_fail`) per RFC-008 §"v0.14 follow-up".
///
/// Architecturally this runs AFTER the initial inference + fixpoint:
/// at that point `fn_effects` holds the inferred-set for every fn (so
/// closures over local `let f = || ...` bindings can be characterized
/// via the eventual caller's set if we ever surface that), and every
/// pub fn's declared row is known via `hir_fn.effects`. The check is
/// local to each call site — no fixpoint needed.
///
/// This pass is ADDITIVE to the existing MT4001 `effect_undeclared`
/// check: MT4001 fires at the fn-level when the inferred set isn't a
/// subset of the declared set; MT4050 fires at the CALL site when the
/// specific offending effects came in via a row-poly HOF closure. The
/// two diagnostics will often co-occur (and that's intentional — the
/// MT4050 message points to the exact HOF call, which MT4001's fn-level
/// message doesn't).
fn validate_row_dispatch(
    pkg: &Package,
    defs: &DefMap,
    arena: &TyArena,
    known: &KnownEffects,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (_fid, hir_fn) in pkg.fns.iter() {
        if !hir_fn.is_pub {
            continue;
        }
        let Some(body) = hir_fn.body else {
            continue;
        };
        // Match the public-fn validator's interpretation: an absent
        // (or empty) `effect ...` clause is a closed-empty declared
        // row — any HOF closure with effects is a violation. This is
        // distinct from the legacy permissive fresh-fn path; the
        // public-fn validation point uses the same closed-empty rule.
        let declared: HashSet<EffectId> = hir_fn
            .effects
            .iter()
            .map(|n| {
                defs.effects
                    .get(n.as_str())
                    .copied()
                    .unwrap_or(EffectId(u32::MAX))
            })
            .collect();
        // Empty declared row ⇒ caller declared `effect ` (no effects);
        // any HOF closure with effects is a violation. Non-empty
        // declared row ⇒ closures may carry effects in the declared
        // set but not outside.
        walk_block_for_row_violations(body, pkg, defs, arena, known, &declared, diagnostics);
    }
}

fn walk_block_for_row_violations(
    bid: BlockId,
    pkg: &Package,
    defs: &DefMap,
    arena: &TyArena,
    known: &KnownEffects,
    declared: &HashSet<EffectId>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let block = &pkg.blocks[bid];
    for stmt in &block.stmts {
        match stmt {
            HirStmt::Let { init: Some(e), .. } => {
                walk_expr_for_row_violations(*e, pkg, defs, arena, known, declared, diagnostics);
            }
            HirStmt::Expr(e) => {
                walk_expr_for_row_violations(*e, pkg, defs, arena, known, declared, diagnostics);
            }
            _ => {}
        }
    }
    if let Some(t) = block.tail {
        walk_expr_for_row_violations(t, pkg, defs, arena, known, declared, diagnostics);
    }
}

fn walk_expr_for_row_violations(
    eid: ExprId,
    pkg: &Package,
    defs: &DefMap,
    arena: &TyArena,
    known: &KnownEffects,
    declared: &HashSet<EffectId>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let expr = &pkg.exprs[eid];
    match expr {
        HirExpr::MethodCall {
            receiver,
            method,
            args,
        } => {
            walk_expr_for_row_violations(*receiver, pkg, defs, arena, known, declared, diagnostics);
            // Look up: is this a row-poly stdlib HOF?
            let is_row_hof = defs
                .builtin_methods
                .get(method.as_str())
                .and_then(|m| m.row_sig)
                .is_some();
            if is_row_hof {
                // For each lambda arg, walk its body, collect the
                // effects, intersect against the *complement* of
                // `declared` — anything left is a violation.
                for a in args {
                    let arg_expr = &pkg.exprs[a.value];
                    if let HirExpr::Lambda { body, .. } = arg_expr {
                        let mut closure_effects = EffectSet::default();
                        let mut sink: Vec<FnDefId> = vec![];
                        walk_block_effects(
                            *body,
                            pkg,
                            defs,
                            arena,
                            known,
                            &mut closure_effects,
                            &mut sink,
                        );
                        let disallowed: Vec<EffectId> = closure_effects
                            .iter()
                            .copied()
                            .filter(|e| !declared.contains(e))
                            .collect();
                        if !disallowed.is_empty() {
                            let mut names: Vec<String> = disallowed
                                .iter()
                                .filter_map(|e| effect_name(defs, *e))
                                .collect();
                            names.sort();
                            // HIR doesn't store per-expr spans (see
                            // `Cx::span_of_expr`); use a default span —
                            // the diagnostic message still carries the
                            // method name and the offending effects so
                            // the user can pinpoint the call.
                            diagnostics.push(crate::diag::hof_closure_effects_rejected(
                                method.as_str(),
                                &names,
                                &SourceSpan { start: 0, end: 0 },
                            ));
                        }
                    }
                    // Recurse: a non-lambda arg might itself contain a
                    // nested HOF call.
                    walk_expr_for_row_violations(
                        a.value,
                        pkg,
                        defs,
                        arena,
                        known,
                        declared,
                        diagnostics,
                    );
                }
                return;
            }
            // Non-row-poly method: recurse into args.
            for a in args {
                walk_expr_for_row_violations(
                    a.value,
                    pkg,
                    defs,
                    arena,
                    known,
                    declared,
                    diagnostics,
                );
            }
        }
        HirExpr::Block(b) => {
            walk_block_for_row_violations(*b, pkg, defs, arena, known, declared, diagnostics);
        }
        HirExpr::Tuple(xs) | HirExpr::Array(xs) => {
            for x in xs {
                walk_expr_for_row_violations(*x, pkg, defs, arena, known, declared, diagnostics);
            }
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            walk_expr_for_row_violations(*lhs, pkg, defs, arena, known, declared, diagnostics);
            walk_expr_for_row_violations(*rhs, pkg, defs, arena, known, declared, diagnostics);
        }
        HirExpr::Unary { rhs, .. } => {
            walk_expr_for_row_violations(*rhs, pkg, defs, arena, known, declared, diagnostics);
        }
        HirExpr::Borrow { inner, .. } | HirExpr::Move(inner) => {
            walk_expr_for_row_violations(*inner, pkg, defs, arena, known, declared, diagnostics);
        }
        HirExpr::Call { callee, args } => {
            walk_expr_for_row_violations(*callee, pkg, defs, arena, known, declared, diagnostics);
            // v0.15: when the parser-folded Path callee's last segment
            // is a row-poly stdlib HOF method (e.g. `xs.map(...)` →
            // `Call(Path([xs, map]))`), apply the closed-row
            // subsumption check to each lambda arg's effect set.
            if let HirExpr::Path(segs) = &pkg.exprs[*callee] {
                if segs.len() >= 2 {
                    let last = segs.last().expect("len>=2");
                    let is_row_hof = defs
                        .builtin_methods
                        .get(last.as_str())
                        .and_then(|m| m.row_sig)
                        .is_some();
                    if is_row_hof {
                        for a in args {
                            let arg_expr = &pkg.exprs[a.value];
                            if let HirExpr::Lambda { body, .. } = arg_expr {
                                let mut closure_effects = EffectSet::default();
                                let mut sink: Vec<FnDefId> = vec![];
                                walk_block_effects(
                                    *body,
                                    pkg,
                                    defs,
                                    arena,
                                    known,
                                    &mut closure_effects,
                                    &mut sink,
                                );
                                let disallowed: Vec<EffectId> = closure_effects
                                    .iter()
                                    .copied()
                                    .filter(|e| !declared.contains(e))
                                    .collect();
                                if !disallowed.is_empty() {
                                    let mut names: Vec<String> = disallowed
                                        .iter()
                                        .filter_map(|e| effect_name(defs, *e))
                                        .collect();
                                    names.sort();
                                    diagnostics.push(crate::diag::hof_closure_effects_rejected(
                                        last.as_str(),
                                        &names,
                                        &SourceSpan { start: 0, end: 0 },
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            for a in args {
                walk_expr_for_row_violations(
                    a.value,
                    pkg,
                    defs,
                    arena,
                    known,
                    declared,
                    diagnostics,
                );
            }
        }
        HirExpr::Field { receiver, .. } => {
            walk_expr_for_row_violations(*receiver, pkg, defs, arena, known, declared, diagnostics);
        }
        HirExpr::Index { receiver, idx } => {
            walk_expr_for_row_violations(*receiver, pkg, defs, arena, known, declared, diagnostics);
            walk_expr_for_row_violations(*idx, pkg, defs, arena, known, declared, diagnostics);
        }
        HirExpr::If { cond, then, else_ } => {
            walk_expr_for_row_violations(*cond, pkg, defs, arena, known, declared, diagnostics);
            walk_block_for_row_violations(*then, pkg, defs, arena, known, declared, diagnostics);
            if let Some(e) = else_ {
                walk_expr_for_row_violations(*e, pkg, defs, arena, known, declared, diagnostics);
            }
        }
        HirExpr::IfLet {
            scrutinee,
            then,
            else_,
            ..
        } => {
            walk_expr_for_row_violations(
                *scrutinee,
                pkg,
                defs,
                arena,
                known,
                declared,
                diagnostics,
            );
            walk_block_for_row_violations(*then, pkg, defs, arena, known, declared, diagnostics);
            if let Some(e) = else_ {
                walk_expr_for_row_violations(*e, pkg, defs, arena, known, declared, diagnostics);
            }
        }
        HirExpr::Match { scrutinee, arms } => {
            walk_expr_for_row_violations(
                *scrutinee,
                pkg,
                defs,
                arena,
                known,
                declared,
                diagnostics,
            );
            for arm in arms {
                walk_expr_for_row_violations(
                    arm.body,
                    pkg,
                    defs,
                    arena,
                    known,
                    declared,
                    diagnostics,
                );
                if let Some(g) = arm.guard {
                    walk_expr_for_row_violations(g, pkg, defs, arena, known, declared, diagnostics);
                }
            }
        }
        HirExpr::For { iter, body, .. } => {
            walk_expr_for_row_violations(*iter, pkg, defs, arena, known, declared, diagnostics);
            walk_block_for_row_violations(*body, pkg, defs, arena, known, declared, diagnostics);
        }
        HirExpr::While { cond, body } => {
            walk_expr_for_row_violations(*cond, pkg, defs, arena, known, declared, diagnostics);
            walk_block_for_row_violations(*body, pkg, defs, arena, known, declared, diagnostics);
        }
        HirExpr::Loop { body } => {
            walk_block_for_row_violations(*body, pkg, defs, arena, known, declared, diagnostics);
        }
        HirExpr::Return(Some(e)) | HirExpr::Break(Some(e)) => {
            walk_expr_for_row_violations(*e, pkg, defs, arena, known, declared, diagnostics);
        }
        HirExpr::Struct { fields, .. } => {
            for (_, e) in fields {
                walk_expr_for_row_violations(*e, pkg, defs, arena, known, declared, diagnostics);
            }
        }
        HirExpr::Map(entries) => {
            for (k, v) in entries {
                walk_expr_for_row_violations(*k, pkg, defs, arena, known, declared, diagnostics);
                walk_expr_for_row_violations(*v, pkg, defs, arena, known, declared, diagnostics);
            }
        }
        HirExpr::Send { target, args, .. } | HirExpr::Ask { target, args, .. } => {
            walk_expr_for_row_violations(*target, pkg, defs, arena, known, declared, diagnostics);
            for a in args {
                walk_expr_for_row_violations(
                    a.value,
                    pkg,
                    defs,
                    arena,
                    known,
                    declared,
                    diagnostics,
                );
            }
        }
        HirExpr::Deadline { inner, dur } => {
            walk_expr_for_row_violations(*inner, pkg, defs, arena, known, declared, diagnostics);
            walk_expr_for_row_violations(*dur, pkg, defs, arena, known, declared, diagnostics);
        }
        HirExpr::Question(inner)
        | HirExpr::Spawn { inner, .. }
        | HirExpr::Detach(inner)
        | HirExpr::Join(inner)
        | HirExpr::Run(inner) => {
            walk_expr_for_row_violations(*inner, pkg, defs, arena, known, declared, diagnostics);
        }
        HirExpr::Unsafe(b) => {
            walk_block_for_row_violations(*b, pkg, defs, arena, known, declared, diagnostics);
        }
        HirExpr::Arena { body, .. } => {
            // `Arena.body` is an `ExprId` (not a block) per HIR shape.
            walk_expr_for_row_violations(*body, pkg, defs, arena, known, declared, diagnostics);
        }
        HirExpr::Lambda { body, .. } => {
            // A lambda OUTSIDE a HOF arg position (e.g. assigned to a
            // local) still has its body walked — closures bound to a
            // local are caller-effect-relevant too. But violations are
            // checked against the OUTER fn's declared row.
            walk_block_for_row_violations(*body, pkg, defs, arena, known, declared, diagnostics);
        }
        _ => {}
    }
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

/// v0.15: compute the inferred effect ROW for a HOF call's
/// closure-argument. Returns `Some(EffectRow::Closed(...))` when the
/// argument is a lambda (we walk the body to collect concrete effects
/// and box them as a closed row). Returns `None` when the argument is
/// something other than a lambda — the dispatcher then leaves the
/// signature's parameter row template unbound (an open row).
///
/// The closed-row return models the v0.15 invariant: the closure's
/// effect set is *known* at the call site (we just walked its body), so
/// we encode it as a fully-concrete row that unifies cleanly with the
/// signature's `Var(0)` parameter slot.
fn compute_arg_effect_row(
    eid: ExprId,
    pkg: &Package,
    defs: &DefMap,
    arena: &TyArena,
    known: &KnownEffects,
) -> Option<row::EffectRow> {
    let expr = &pkg.exprs[eid];
    // Only meaningful for lambda arguments — other arg shapes don't
    // carry a closure row in the v0.15 sigs.
    let body = match expr {
        HirExpr::Lambda { body, .. } => *body,
        _ => return None,
    };
    let mut body_effects = EffectSet::default();
    let mut sink: Vec<FnDefId> = vec![];
    walk_block_effects(body, pkg, defs, arena, known, &mut body_effects, &mut sink);
    let set: std::collections::BTreeSet<EffectId> = body_effects.into_iter().collect();
    Some(row::EffectRow::Closed(set))
}

/// v0.15: the row-poly stdlib HOF dispatch logic, factored out of
/// `walk_expr_effects::HirExpr::MethodCall` so the dotted-Path-folded
/// `Call(Path([receiver, method]))` shape in the `Call` branch can
/// reuse it (e.g. `xs.collect()` which the parser folds into a Path
/// rather than a MethodCall).
///
/// Dispatch policy:
/// 1. If `defs.builtin_methods[method].row_sig` is `None`, this is a
///    no-op (the caller's existing per-arg walk has already covered
///    the effect bookkeeping).
/// 2. Otherwise instantiate the sig, compute each closure-arg's
///    effect row, unify against the sig's parameter rows, resolve the
///    return row, remap `ALLOC_PLACEHOLDER` → real alloc, and union
///    the concrete effects into `out`.
fn dispatch_row_poly_call(
    method: &str,
    args: &[HirArg],
    pkg: &Package,
    defs: &DefMap,
    arena: &TyArena,
    known: &KnownEffects,
    out: &mut EffectSet,
) {
    let row_factory = defs.builtin_methods.get(method).and_then(|m| m.row_sig);
    let Some(factory) = row_factory else {
        return;
    };
    let sig = factory();
    let mut arg_rows: Vec<Option<row::EffectRow>> = Vec::with_capacity(args.len());
    for a in args {
        let closure_body_effects = compute_arg_effect_row(a.value, pkg, defs, arena, known);
        arg_rows.push(closure_body_effects);
    }
    let mut subst = row::RowSubst::new();
    let (sig_params, sig_ret, _fresh) = row::instantiate_row_sig(&sig, &mut subst);
    let sig_param_iter: Vec<&Option<row::EffectRow>> = sig_params.iter().skip(1).collect();
    for (i, expected) in sig_param_iter.iter().enumerate() {
        let Some(actual) = arg_rows.get(i).and_then(|r| r.as_ref()) else {
            continue;
        };
        let Some(exp_row) = expected.as_ref() else {
            continue;
        };
        let _ = row::unify_rows(&mut subst, exp_row, actual);
    }
    let resolved = subst.resolve(&sig_ret);
    let real_alloc = known.alloc;
    let extract = |s: &std::collections::BTreeSet<EffectId>| -> Vec<EffectId> {
        s.iter()
            .copied()
            .map(|e| {
                if e == row::stdlib_sigs::ALLOC_PLACEHOLDER {
                    real_alloc
                } else {
                    e
                }
            })
            .collect()
    };
    let concrete: Vec<EffectId> = match &resolved {
        row::EffectRow::Closed(s) | row::EffectRow::Open(s, _) => extract(s),
    };
    for e in concrete {
        out.insert(e);
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
                    // v0.15: dotted Path calls that the parser folded
                    // through `Call(Path([receiver, method]))` rather
                    // than `MethodCall { receiver, method, args }` (e.g.
                    // bare `xs.collect()` with no args). The row-poly
                    // dispatch keys on method name only, so we can
                    // honour those here too. Forwards to the same
                    // dispatch logic used in the `MethodCall` branch.
                    let last = segs.last().expect("len>=2");
                    dispatch_row_poly_call(last.as_str(), args, pkg, defs, arena, known, out);
                    // Container method heuristic for the Path-folded
                    // shape: keep parity with the MethodCall branch's
                    // alloc-on-`.collect`/`.push`/... rule.
                    if matches!(
                        last.as_str(),
                        "push" | "pop" | "insert" | "encode" | "collect" | "to_string" | "clone"
                    ) {
                        out.insert(known.alloc);
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

            // v0.15: row-polymorphic stdlib HOF dispatch. When `method`
            // is registered in `defs.builtin_methods` with a `row_sig`
            // factory, instantiate the sig, compute the
            // closure-argument's inferred effect row (per RFC-008), unify
            // it against the sig's parameter row, then propagate the
            // resolved return row into the caller's effect set.
            //
            // For the v0.14-shape sigs (all `Var(0) → Var(0)`) the
            // resolved return row equals the closure's row — so this
            // is OBSERVATIONALLY equivalent to the legacy "walk the
            // lambda body's effects into `out`" behavior. The
            // difference is structural: the wiring now flows through
            // the row machinery, which (a) exercises
            // `instantiate_row_sig` + `unify_rows` end-to-end, (b)
            // lets `Iterator.collect`'s `VarPlus(0, {alloc})` template
            // materialize as the real `alloc` effect, and (c)
            // unblocks future per-receiver discrimination + MT4020-25
            // diagnostics in v0.16.
            let row_factory = defs
                .builtin_methods
                .get(method.as_str())
                .and_then(|m| m.row_sig);
            if let Some(factory) = row_factory {
                let sig = factory();
                // Step 1: compute each closure-arg's effect row by
                // walking its body in isolation (so the closure's own
                // effects are NOT double-counted in `out`).
                let mut arg_rows: Vec<Option<row::EffectRow>> = Vec::with_capacity(args.len());
                for a in args {
                    let closure_body_effects =
                        compute_arg_effect_row(a.value, pkg, defs, arena, known);
                    arg_rows.push(closure_body_effects);
                }
                // Step 2: instantiate the sig (fresh row vars per
                // call site).
                let mut subst = row::RowSubst::new();
                let (sig_params, sig_ret, _fresh) = row::instantiate_row_sig(&sig, &mut subst);
                // Step 3: align actual args to sig param slots. The
                // sig's first param is always `Skip` (receiver) — but
                // the receiver isn't in `args` (it's `receiver`); so
                // sig_params[0] is the FIRST positional arg, etc.
                // Skip the leading `Skip` slot (receiver) before
                // aligning.
                let sig_param_iter: Vec<&Option<row::EffectRow>> =
                    sig_params.iter().skip(1).collect();
                for (i, expected) in sig_param_iter.iter().enumerate() {
                    let Some(actual) = arg_rows.get(i).and_then(|r| r.as_ref()) else {
                        continue;
                    };
                    let Some(exp_row) = expected.as_ref() else {
                        continue;
                    };
                    // Unify ignoring errors here — v0.15 dispatch is
                    // permissive (MT4020-25 emit-sites land in v0.16).
                    let _ = row::unify_rows(&mut subst, exp_row, actual);
                }
                // Step 4: resolve the return row and union its
                // concrete effects into `out`. For `VarPlus`-shaped
                // returns (`collect`), remap the alloc placeholder to
                // the real `alloc` effect id.
                let resolved = subst.resolve(&sig_ret);
                let real_alloc = known.alloc;
                let extract = |s: &std::collections::BTreeSet<EffectId>| {
                    let mut concrete: Vec<EffectId> = s.iter().copied().collect();
                    // Remap the synthetic ALLOC_PLACEHOLDER → real alloc.
                    for e in concrete.iter_mut() {
                        if *e == row::stdlib_sigs::ALLOC_PLACEHOLDER {
                            *e = real_alloc;
                        }
                    }
                    concrete
                };
                let concrete: Vec<EffectId> = match &resolved {
                    row::EffectRow::Closed(s) => extract(s),
                    row::EffectRow::Open(s, _) => extract(s),
                };
                for e in concrete {
                    out.insert(e);
                }
                // Also walk args once for non-effect bookkeeping (the
                // closure body's effects already flowed through the
                // row dispatch above; but `callees` collection still
                // needs the walk so the fixpoint sees nested fn calls
                // inside the closure body).
                let mut sink = EffectSet::default();
                for a in args {
                    walk_expr_effects(a.value, pkg, defs, arena, known, &mut sink, callees);
                    // Even with row dispatch, take the closure-body
                    // effects too — the v0.15 wiring is ADDITIVE so
                    // we don't lose any prior over-approximation; the
                    // row machinery is verified-equivalent here for
                    // the present sig shapes.
                    for e in &sink {
                        out.insert(*e);
                    }
                    sink.clear();
                }
            } else {
                // Legacy path: just walk args.
                for a in args {
                    walk_expr_effects(a.value, pkg, defs, arena, known, out, callees);
                }
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
        HirExpr::Break(Some(e)) => walk_expr_effects(*e, pkg, defs, arena, known, out, callees),
        HirExpr::Break(None) | HirExpr::Continue => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_profile_recognises_core() {
        assert_eq!(Profile::parse_profile("core"), Profile::Core);
        assert_eq!(Profile::parse_profile("edge"), Profile::Edge);
        assert_eq!(Profile::parse_profile("web"), Profile::Web);
        // Anything else falls back to Host.
        assert_eq!(Profile::parse_profile("host"), Profile::Host);
        assert_eq!(Profile::parse_profile(""), Profile::Host);
        assert_eq!(Profile::parse_profile("gibberish"), Profile::Host);
    }

    /// v0.3 (A65) `core_profile_rejects_alloc`: the strict core profile
    /// emits MT4002 whenever a public fn's inferred effect set includes
    /// `alloc`. This drives the public-facing rule documented in the
    /// `effect_checking/05_strict_core_profile` conformance case shape.
    #[test]
    fn core_profile_rejects_alloc() {
        use mty_diagnostics::{Diagnostic, Severity};
        use mty_hir::{
            BlockId, ExprId, FnId, HirBlock, HirExpr, HirFn, HirStmt, Item, Package, SourceSpan,
        };

        let mut pkg = Package::default();
        // Synthesize: pub fn f() -> Unit { arena tmp { 0 } }
        // The arena block triggers `out.insert(known.alloc)`.
        let zero = pkg
            .exprs
            .alloc(HirExpr::Literal(mty_hir::HirLiteral::Int(0, None)));
        let arena_body: ExprId = pkg.exprs.alloc(HirExpr::Arena {
            name: "tmp".into(),
            body: zero,
        });
        let block: BlockId = pkg.blocks.alloc(HirBlock {
            stmts: vec![HirStmt::Expr(arena_body)],
            tail: None,
        });
        let fid: FnId = pkg.fns.alloc(HirFn {
            name: "f".into(),
            is_pub: true,
            is_unsafe: false,
            params: vec![],
            ret: None,
            effects: vec![],
            effect_row: None,
            generics: vec![],
            body: Some(block),
            span: SourceSpan { start: 0, end: 0 },
        });
        let iid = pkg.items.alloc(Item::Fn(fid));
        pkg.top_level.push(iid);

        // Run effect inference under the Core profile.
        let arena = TyArena::new();
        let mut defs = DefMap::default();
        // Pre-intern; not strictly required but matches the live flow.
        let _ = defs.intern_effect("alloc");
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let _ = infer_and_validate(&pkg, &mut defs, &arena, Profile::Core, &mut diagnostics);

        let codes: Vec<String> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, Severity::Error))
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            codes.contains(&"MT4002".to_string()),
            "expected MT4002 in core profile, got {:?}",
            codes
        );
    }

    /// Counter-test: Host profile tolerates alloc without MT4002 (it
    /// still wants MT4001 because alloc is undeclared, but no MT4002).
    #[test]
    fn host_profile_allows_alloc_but_demands_declaration() {
        use mty_diagnostics::{Diagnostic, Severity};
        use mty_hir::{
            BlockId, ExprId, FnId, HirBlock, HirExpr, HirFn, HirStmt, Item, Package, SourceSpan,
        };

        let mut pkg = Package::default();
        let zero = pkg
            .exprs
            .alloc(HirExpr::Literal(mty_hir::HirLiteral::Int(0, None)));
        let arena_body: ExprId = pkg.exprs.alloc(HirExpr::Arena {
            name: "tmp".into(),
            body: zero,
        });
        let block: BlockId = pkg.blocks.alloc(HirBlock {
            stmts: vec![HirStmt::Expr(arena_body)],
            tail: None,
        });
        let fid: FnId = pkg.fns.alloc(HirFn {
            name: "f".into(),
            is_pub: true,
            is_unsafe: false,
            params: vec![],
            ret: None,
            effects: vec![],
            effect_row: None,
            generics: vec![],
            body: Some(block),
            span: SourceSpan { start: 0, end: 0 },
        });
        let iid = pkg.items.alloc(Item::Fn(fid));
        pkg.top_level.push(iid);

        let arena = TyArena::new();
        let mut defs = DefMap::default();
        let _ = defs.intern_effect("alloc");
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let _ = infer_and_validate(&pkg, &mut defs, &arena, Profile::Host, &mut diagnostics);

        let codes: Vec<String> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, Severity::Error))
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            !codes.contains(&"MT4002".to_string()),
            "Host profile must NOT emit MT4002, got {:?}",
            codes
        );
        // MT4001 (effect_undeclared) IS expected because the fn doesn't
        // declare `effect alloc`.
        assert!(
            codes.contains(&"MT4001".to_string()),
            "Host profile should still demand effect declaration; got {:?}",
            codes
        );
    }
}

// ---------------------------------------------------------------------------
// v0.13 — RFC-008 effect-row polymorphism
// ---------------------------------------------------------------------------

/// Row-polymorphism infrastructure for effect rows.
///
/// See `docs/spec/rfcs/RFC-008-effect-rows.md` for the full design.
///
/// This module is **additive** to the existing closed-set effect
/// system: nothing in [`infer_and_validate`] consults it. It is the
/// substrate that the v0.14 surface-syntax and HOF-call-site checker
/// will be built on, and ships with one wired example signature
/// (`stdlib_list_map_sig`) to validate the end-to-end path in v0.13.
pub mod row {
    use super::EffectId;
    use std::collections::{BTreeMap, BTreeSet};

    /// A row variable identifier. Allocated by [`RowSubst::fresh`].
    ///
    /// IDs are u32 and densely allocated starting from 0 within a single
    /// substitution. They are intentionally scoped to one substitution
    /// table — two `RowVar(0)` from different tables are *unrelated*.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub struct RowVar(pub u32);

    /// An effect row. Either fully closed (a concrete finite set), or
    /// open with a polymorphic tail.
    ///
    /// The set is stored as a `BTreeSet<EffectId>` so that `Debug`
    /// printing, hashing, and equality are deterministic — important
    /// because diagnostic messages compare rows by structural equality.
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub enum EffectRow {
        /// `!{a, b, c}` — exactly these effects.
        Closed(BTreeSet<EffectId>),
        /// `!{a, b | E}` — at least these effects, plus whatever the
        /// row variable resolves to.
        Open(BTreeSet<EffectId>, RowVar),
    }

    impl EffectRow {
        /// Empty closed row `!{}`.
        pub fn empty() -> Self {
            EffectRow::Closed(BTreeSet::new())
        }

        /// Closed row from an iterator of effects.
        pub fn closed<I: IntoIterator<Item = EffectId>>(eff: I) -> Self {
            EffectRow::Closed(eff.into_iter().collect())
        }

        /// Open row `!{eff... | v}`.
        pub fn open<I: IntoIterator<Item = EffectId>>(eff: I, v: RowVar) -> Self {
            EffectRow::Open(eff.into_iter().collect(), v)
        }

        /// The concrete-effects component (the visible part of the
        /// row, ignoring any tail).
        pub fn concrete(&self) -> &BTreeSet<EffectId> {
            match self {
                EffectRow::Closed(s) | EffectRow::Open(s, _) => s,
            }
        }

        /// True iff this row has a polymorphic tail.
        pub fn is_open(&self) -> bool {
            matches!(self, EffectRow::Open(_, _))
        }

        /// The free row variables in this row. (At most one — the tail
        /// — in v0.13. Reserved for future row-variable-in-set forms.)
        pub fn free_row_vars(&self) -> Vec<RowVar> {
            match self {
                EffectRow::Closed(_) => vec![],
                EffectRow::Open(_, v) => vec![*v],
            }
        }
    }

    /// Errors raised by row unification / subsumption.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum RowError {
        /// Two closed rows have different concrete effect sets.
        /// Carries the two sets for diagnostic rendering.
        ClosedMismatch(BTreeSet<EffectId>, BTreeSet<EffectId>),
        /// Subsumption failure: the actual row contains effects the
        /// expected closed row does not allow.
        SubsumptionFail(BTreeSet<EffectId>, BTreeSet<EffectId>),
        /// Occurs-check failure: a row variable would be bound to a
        /// row containing itself.
        Occurs(RowVar),
    }

    /// Substitution table mapping row variables to rows.
    ///
    /// `RowSubst::fresh()` allocates a new unbound row variable.
    /// `bind()` records a binding `v ↦ row`, after the occurs check.
    /// `resolve()` follows binding chains to canonical form.
    #[derive(Debug, Default, Clone)]
    pub struct RowSubst {
        next: u32,
        bindings: BTreeMap<RowVar, EffectRow>,
    }

    impl RowSubst {
        /// New empty substitution.
        pub fn new() -> Self {
            Self::default()
        }

        /// Allocate a fresh, unbound row variable.
        pub fn fresh(&mut self) -> RowVar {
            let v = RowVar(self.next);
            self.next += 1;
            v
        }

        /// Whether `v` is currently bound.
        pub fn is_bound(&self, v: RowVar) -> bool {
            self.bindings.contains_key(&v)
        }

        /// Direct lookup (one step, no chain following).
        pub fn lookup(&self, v: RowVar) -> Option<&EffectRow> {
            self.bindings.get(&v)
        }

        /// Bind `v ↦ row`, after an occurs check.
        ///
        /// Returns `Err(RowError::Occurs(v))` if `row` mentions `v`
        /// transitively (under the current substitution).
        pub fn bind(&mut self, v: RowVar, row: EffectRow) -> Result<(), RowError> {
            if self.occurs_in(v, &row) {
                return Err(RowError::Occurs(v));
            }
            self.bindings.insert(v, row);
            Ok(())
        }

        /// Recursive occurs check: does `v` appear in `row` after
        /// chasing bindings?
        fn occurs_in(&self, v: RowVar, row: &EffectRow) -> bool {
            match row {
                EffectRow::Closed(_) => false,
                EffectRow::Open(_, w) => {
                    if *w == v {
                        return true;
                    }
                    if let Some(next) = self.bindings.get(w) {
                        return self.occurs_in(v, next);
                    }
                    false
                }
            }
        }

        /// Resolve `row` to its canonical form: follow binding chains
        /// from the row's tail (if any) and merge concrete effects
        /// along the way.
        ///
        /// Example: if `v ↦ Open({fs}, w)` and `w ↦ Closed({net})`,
        /// then `resolve(Open({alloc}, v))` produces
        /// `Closed({alloc, fs, net})`.
        pub fn resolve(&self, row: &EffectRow) -> EffectRow {
            match row {
                EffectRow::Closed(_) => row.clone(),
                EffectRow::Open(s, v) => {
                    let mut acc = s.clone();
                    let mut current = *v;
                    let mut seen: BTreeSet<RowVar> = BTreeSet::new();
                    loop {
                        if !seen.insert(current) {
                            // Cycle — should be prevented by occurs check,
                            // but be defensive: return the partial row.
                            return EffectRow::Open(acc, current);
                        }
                        match self.bindings.get(&current) {
                            None => return EffectRow::Open(acc, current),
                            Some(EffectRow::Closed(t)) => {
                                acc.extend(t.iter().copied());
                                return EffectRow::Closed(acc);
                            }
                            Some(EffectRow::Open(t, w)) => {
                                acc.extend(t.iter().copied());
                                current = *w;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Convenience: apply a substitution and return a fresh row.
    pub fn apply_row_subst(subst: &RowSubst, row: &EffectRow) -> EffectRow {
        subst.resolve(row)
    }

    /// Unify two effect rows. On success, the substitution is extended
    /// so that `resolve(lhs) == resolve(rhs)`.
    ///
    /// Implements the four cases from RFC-008 §"Inference rules":
    ///
    /// | LHS         | RHS         | Action                                    |
    /// |-------------|-------------|-------------------------------------------|
    /// | `Closed(a)` | `Closed(b)` | succeed iff `a == b`                      |
    /// | `Closed(a)` | `Open(b,w)` | succeed iff `b ⊆ a`; bind `w ↦ a \ b`     |
    /// | `Open(a,v)` | `Closed(b)` | succeed iff `a ⊆ b`; bind `v ↦ b \ a`     |
    /// | `Open(a,v)` | `Open(b,w)` | bind `v ↦ Open(b \ a, fresh)`,            |
    /// |             |             |       `w ↦ Open(a \ b, same fresh)`       |
    pub fn unify_rows(
        subst: &mut RowSubst,
        lhs: &EffectRow,
        rhs: &EffectRow,
    ) -> Result<(), RowError> {
        let lhs = subst.resolve(lhs);
        let rhs = subst.resolve(rhs);
        match (lhs, rhs) {
            (EffectRow::Closed(a), EffectRow::Closed(b)) => {
                if a == b {
                    Ok(())
                } else {
                    Err(RowError::ClosedMismatch(a, b))
                }
            }
            (EffectRow::Closed(a), EffectRow::Open(b, w)) => {
                // The open row's concrete part must be a subset of the
                // closed row; the tail is bound to the difference.
                if !b.is_subset(&a) {
                    return Err(RowError::ClosedMismatch(a, b));
                }
                let diff: BTreeSet<EffectId> = a.difference(&b).copied().collect();
                subst.bind(w, EffectRow::Closed(diff))
            }
            (EffectRow::Open(a, v), EffectRow::Closed(b)) => {
                if !a.is_subset(&b) {
                    return Err(RowError::ClosedMismatch(a, b));
                }
                let diff: BTreeSet<EffectId> = b.difference(&a).copied().collect();
                subst.bind(v, EffectRow::Closed(diff))
            }
            (EffectRow::Open(a, v), EffectRow::Open(b, w)) => {
                if v == w && a == b {
                    return Ok(());
                }
                // Standard Koka algorithm: one fresh tail, both row
                // vars are bound to the *other* side's concrete-set
                // minus this side's concrete-set, plus the same fresh
                // tail. This preserves the invariant that after
                // unification both rows resolve to the same set.
                let fresh = subst.fresh();
                let b_minus_a: BTreeSet<EffectId> = b.difference(&a).copied().collect();
                let a_minus_b: BTreeSet<EffectId> = a.difference(&b).copied().collect();
                subst.bind(v, EffectRow::Open(b_minus_a, fresh))?;
                // The second bind targets the OTHER row var; if v == w
                // we'd be double-binding, but the `v == w && a == b`
                // early-return covers the only well-formed identity
                // case. If v == w but a != b, the unification is still
                // inconsistent — fall through to surface an occurs
                // error on the second bind.
                subst.bind(w, EffectRow::Open(a_minus_b, fresh))
            }
        }
    }

    /// Subsumption check for closed rows.
    ///
    /// Returns Ok(()) iff `actual ⊆ expected` (the actual concrete
    /// effects are accepted by the expected closed bound).
    ///
    /// This is the "narrower-is-OK" rule that lets a `Closed({})`
    /// closure satisfy a parameter declared `Closed({fs})`. The dual —
    /// a row with extra effects flowing into a fixed closed parameter
    /// — is rejected with `SubsumptionFail`.
    pub fn subsume_closed(
        actual: &BTreeSet<EffectId>,
        expected: &BTreeSet<EffectId>,
    ) -> Result<(), RowError> {
        if actual.is_subset(expected) {
            Ok(())
        } else {
            Err(RowError::SubsumptionFail(actual.clone(), expected.clone()))
        }
    }

    /// A row-polymorphic function signature — the v0.13 representation
    /// used by stdlib HOF entries.
    ///
    /// `param_rows[i]` is the effect row of the i-th parameter (only
    /// meaningful for parameters of `fn` type; ignore for other params).
    /// `return_row` is the row attributed to the call's *result effect
    /// set* (i.e. what gets added to the caller's effect set at this
    /// call site).
    /// `row_vars` is the list of row variables quantified in the
    /// signature — these are *symbolic*: each call site instantiates
    /// fresh row vars in place of them via [`instantiate_row_sig`].
    #[derive(Debug, Clone)]
    pub struct RowPolySig {
        /// Quantified row variable templates (de-Bruijn-style: each
        /// appearance in `param_rows`/`return_row` carries an index
        /// into this list rather than a raw RowVar).
        ///
        /// In v0.13 most stdlib signatures have a single row var (e.g.
        /// `List.map[A, B, E]`), so this is usually 1 element.
        pub row_var_count: u32,
        /// One entry per parameter. `RowSpec::Skip` for non-fn
        /// parameters; `RowSpec::Concrete` for fn-typed parameters
        /// with a fixed effect set; `RowSpec::Var(i)` for fn-typed
        /// parameters whose effect set is the i-th quantified row var.
        pub param_rows: Vec<RowSpec>,
        /// The result's effect row.
        pub return_row: RowSpec,
    }

    /// Parameter-row template. Used inside [`RowPolySig`]; resolved at
    /// instantiation time to a concrete [`EffectRow`].
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum RowSpec {
        /// This parameter is not a fn type — no row attached.
        Skip,
        /// A fixed effect row, independent of any row variable.
        Concrete(EffectRow),
        /// The effect row is the i-th quantified row variable (no
        /// concrete effects added).
        Var(u32),
        /// `Var(i) ∪ Concrete(eff)` — the quantified row var plus
        /// some fixed effects that this position always carries.
        /// Example: `Iterator.collect` is `!{alloc | E}`.
        VarPlus(u32, BTreeSet<EffectId>),
    }

    /// Instantiate a row-polymorphic signature: allocate one fresh
    /// row variable per quantified slot, then walk `param_rows` /
    /// `return_row` substituting the template indices with the fresh
    /// vars.
    ///
    /// Returns `(per_param_rows, return_row, fresh_vars)`. The
    /// `per_param_rows` parallels `sig.param_rows` 1:1.
    pub fn instantiate_row_sig(
        sig: &RowPolySig,
        subst: &mut RowSubst,
    ) -> (Vec<Option<EffectRow>>, EffectRow, Vec<RowVar>) {
        let fresh: Vec<RowVar> = (0..sig.row_var_count).map(|_| subst.fresh()).collect();
        let mat = |spec: &RowSpec| -> Option<EffectRow> {
            match spec {
                RowSpec::Skip => None,
                RowSpec::Concrete(r) => Some(r.clone()),
                RowSpec::Var(i) => Some(EffectRow::Open(BTreeSet::new(), fresh[*i as usize])),
                RowSpec::VarPlus(i, eff) => Some(EffectRow::Open(eff.clone(), fresh[*i as usize])),
            }
        };
        let params: Vec<Option<EffectRow>> = sig.param_rows.iter().map(&mat).collect();
        let ret = mat(&sig.return_row).unwrap_or_else(EffectRow::empty);
        (params, ret, fresh)
    }

    /// The v0.13 wired stdlib signature for `List.map`:
    /// `fn map[A, B, E](xs: List[A], f: fn(A) -> B!E) -> List[B]!E`.
    ///
    /// Row variable layout: one row var (`E`, index 0). The first
    /// parameter (the list) is `Skip`, the second (the closure) is
    /// `Var(0)`, the return row is `Var(0)`.
    pub fn stdlib_list_map_sig() -> RowPolySig {
        RowPolySig {
            row_var_count: 1,
            param_rows: vec![RowSpec::Skip, RowSpec::Var(0)],
            return_row: RowSpec::Var(0),
        }
    }

    /// v0.14 — RFC-008 §"v0.14 follow-up" — additional row-polymorphic
    /// signatures for the rest of the stdlib higher-order functions.
    ///
    /// Each `stdlib_sigs::<container>_<method>_sig()` mirrors the v0.13
    /// `stdlib_list_map_sig()` shape: one (or two) quantified row
    /// variables, `RowSpec::Skip` for the container/receiver, `Var(i)`
    /// for the closure parameter(s), and a return row equal to the
    /// closure's row variable (or `VarPlus` when the HOF itself carries
    /// fixed effects on top of the closure's row, e.g. `collect`'s
    /// `{alloc | E}`).
    ///
    /// These are SHIPPED-SUBSET in v0.14: the row machinery is in place
    /// and tested at this layer, but the v0.13 SHIPPED-SUBSET caveat
    /// still applies — the surface-syntax parser does not yet emit
    /// row-typed signatures, and the typeck pipeline does not yet
    /// consult these sigs at call sites. See
    /// `dev/history/notes/STDLIB_HOF_ROWPOLY_V0_14_NOTES.md` for the
    /// wiring plan and v0.15 follow-ups.
    pub mod stdlib_sigs {
        use super::*;
        use std::collections::BTreeSet;

        // -- List ----------------------------------------------------

        /// `List.filter[A, E](xs: List[A], p: fn(A)->Bool!E) -> List[A]!E`
        ///
        /// Same row-var shape as `stdlib_list_map_sig`: the predicate's
        /// effect row threads to the return.
        pub fn stdlib_list_filter_sig() -> RowPolySig {
            single_row_closure_sig()
        }

        /// `List.fold[A, B, E](xs: List[A], init: B, f: fn(B,A)->B!E) -> B!E`
        ///
        /// Three params (`[Skip, Skip, Var(0)]`): the list and the
        /// accumulator seed contribute no effects; only the folding
        /// closure carries the row var.
        pub fn stdlib_list_fold_sig() -> RowPolySig {
            RowPolySig {
                row_var_count: 1,
                param_rows: vec![RowSpec::Skip, RowSpec::Skip, RowSpec::Var(0)],
                return_row: RowSpec::Var(0),
            }
        }

        /// `List.flat_map[A, B, E](xs: List[A], f: fn(A)->List[B]!E) -> List[B]!E`
        ///
        /// Same row-var shape as `stdlib_list_map_sig` — the closure
        /// returns a `List[B]` instead of `B`, but the row-poly
        /// signature is structurally identical (rows attach to the fn
        /// type, not the container).
        pub fn stdlib_list_flat_map_sig() -> RowPolySig {
            single_row_closure_sig()
        }

        // -- Iterator ------------------------------------------------

        /// `Iterator.map[A, B, E](it: Iterator[A], f: fn(A)->B!E) -> Iterator[B]!E`
        pub fn stdlib_iter_map_sig() -> RowPolySig {
            single_row_closure_sig()
        }

        /// `Iterator.filter[A, E](it: Iterator[A], p: fn(A)->Bool!E) -> Iterator[A]!E`
        pub fn stdlib_iter_filter_sig() -> RowPolySig {
            single_row_closure_sig()
        }

        /// `Iterator.fold[A, B, E](it: Iterator[A], init: B, f: fn(B,A)->B!E) -> B!E`
        pub fn stdlib_iter_fold_sig() -> RowPolySig {
            RowPolySig {
                row_var_count: 1,
                param_rows: vec![RowSpec::Skip, RowSpec::Skip, RowSpec::Var(0)],
                return_row: RowSpec::Var(0),
            }
        }

        /// `Iterator.for_each[A, E](it: Iterator[A], f: fn(A)->Unit!E) -> Unit!E`
        pub fn stdlib_iter_for_each_sig() -> RowPolySig {
            single_row_closure_sig()
        }

        /// `Iterator.find[A, E](it: Iterator[A], p: fn(A)->Bool!E) -> Option[A]!E`
        pub fn stdlib_iter_find_sig() -> RowPolySig {
            single_row_closure_sig()
        }

        /// `Iterator.any[A, E](it: Iterator[A], p: fn(A)->Bool!E) -> Bool!E`
        pub fn stdlib_iter_any_sig() -> RowPolySig {
            single_row_closure_sig()
        }

        /// `Iterator.all[A, E](it: Iterator[A], p: fn(A)->Bool!E) -> Bool!E`
        pub fn stdlib_iter_all_sig() -> RowPolySig {
            single_row_closure_sig()
        }

        /// `Iterator.flat_map[A, B, E](it: Iterator[A], f: fn(A)->Iterator[B]!E) -> Iterator[B]!E`
        pub fn stdlib_iter_flat_map_sig() -> RowPolySig {
            single_row_closure_sig()
        }

        /// `Iterator.collect[A](it: Iterator[A]) -> List[A]!{alloc | E}`
        ///
        /// Quirk: `collect` has no closure parameter — its row var (`E`)
        /// is meant to come from the *upstream* iterator's accumulated
        /// effect chain. Until the typeck pipeline models iterator
        /// chains as row-carrying values (a v0.15 follow-up), the row
        /// var here is structurally bindable but has no parameter site
        /// from which to derive a binding. We expose it as `VarPlus(0,
        /// {alloc})` on the return so that any future per-receiver
        /// row-propagation pass can unify the parameter-side row into
        /// it; in v0.14 this sig will resolve to `{alloc | ?fresh}`
        /// (open) unless a caller unifies `fresh` explicitly. See the
        /// notes file's "v0.15 follow-ups" section.
        ///
        /// Returns concrete `{alloc}` plus an open tail for the
        /// upstream-iterator row var.
        pub fn stdlib_iter_collect_sig() -> RowPolySig {
            RowPolySig {
                row_var_count: 1,
                param_rows: vec![RowSpec::Skip],
                return_row: RowSpec::VarPlus(0, one_effect(ALLOC_PLACEHOLDER)),
            }
        }

        // -- Option --------------------------------------------------

        /// `Option.map[T, U, E](o: Option[T], f: fn(T)->U!E) -> Option[U]!E`
        pub fn stdlib_option_map_sig() -> RowPolySig {
            single_row_closure_sig()
        }

        /// `Option.and_then[T, U, E](o: Option[T], f: fn(T)->Option[U]!E) -> Option[U]!E`
        pub fn stdlib_option_and_then_sig() -> RowPolySig {
            single_row_closure_sig()
        }

        /// `Option.or_else[T, E](o: Option[T], f: fn()->Option[T]!E) -> Option[T]!E`
        pub fn stdlib_option_or_else_sig() -> RowPolySig {
            single_row_closure_sig()
        }

        /// `Option.filter[T, E](o: Option[T], p: fn(T)->Bool!E) -> Option[T]!E`
        pub fn stdlib_option_filter_sig() -> RowPolySig {
            single_row_closure_sig()
        }

        // -- Result --------------------------------------------------

        /// `Result.map[T, U, Err, E](r: Result[T,Err], f: fn(T)->U!E) -> Result[U,Err]!E`
        pub fn stdlib_result_map_sig() -> RowPolySig {
            single_row_closure_sig()
        }

        /// `Result.map_err[T, Err, Err2, E](r: Result[T,Err], f: fn(Err)->Err2!E) -> Result[T,Err2]!E`
        pub fn stdlib_result_map_err_sig() -> RowPolySig {
            single_row_closure_sig()
        }

        /// `Result.and_then[T, U, Err, E](r: Result[T,Err], f: fn(T)->Result[U,Err]!E) -> Result[U,Err]!E`
        pub fn stdlib_result_and_then_sig() -> RowPolySig {
            single_row_closure_sig()
        }

        /// `Result.or_else[T, Err, Err2, E](r: Result[T,Err], f: fn(Err)->Result[T,Err2]!E) -> Result[T,Err2]!E`
        pub fn stdlib_result_or_else_sig() -> RowPolySig {
            single_row_closure_sig()
        }

        // -- helpers -------------------------------------------------

        /// Synthetic effect id used for `collect`'s `{alloc | E}` template.
        ///
        /// Real call sites must remap this to the actual `alloc`
        /// `EffectId` interned in the live `DefMap` before unification.
        /// Kept as a high-numbered sentinel (`u32::MAX - 7`) so it
        /// cannot collide with any naturally-interned id.
        pub const ALLOC_PLACEHOLDER: EffectId = EffectId(u32::MAX - 7);

        /// Shared shape: two-parameter HOF where param 0 is the
        /// container (`Skip`) and param 1 is the closure (`Var(0)`),
        /// returning the same row var.
        ///
        /// Used by `map`, `filter`, `flat_map`, `for_each`, `find`,
        /// `any`, `all`, `and_then`, `or_else`, `map_err`. Pulled into
        /// a helper so the per-method `pub fn` body is one line and the
        /// individual functions stay greppable as "the row-poly sig
        /// for `<container>::<method>`".
        fn single_row_closure_sig() -> RowPolySig {
            RowPolySig {
                row_var_count: 1,
                param_rows: vec![RowSpec::Skip, RowSpec::Var(0)],
                return_row: RowSpec::Var(0),
            }
        }

        fn one_effect(e: EffectId) -> BTreeSet<EffectId> {
            let mut s = BTreeSet::new();
            s.insert(e);
            s
        }
    }

    /// Pretty-printer for an effect row. Renders concrete effects by
    /// looking up their names in `effect_name`.
    ///
    /// Examples:
    ///
    ///   `{}`        — closed empty row
    ///   `{fs}`      — closed singleton
    ///   `{fs | E}`  — open row, tail `E`
    ///   `E`         — bare row variable, no concrete effects
    pub fn pretty_row(row: &EffectRow, name: impl Fn(EffectId) -> Option<String>) -> String {
        let render_set = |s: &BTreeSet<EffectId>| -> String {
            let mut xs: Vec<String> = s
                .iter()
                .map(|e| name(*e).unwrap_or_else(|| format!("e{}", e.0)))
                .collect();
            xs.sort();
            xs.join(", ")
        };
        match row {
            EffectRow::Closed(s) => format!("{{{}}}", render_set(s)),
            EffectRow::Open(s, v) => {
                if s.is_empty() {
                    format!("E{}", v.0)
                } else {
                    format!("{{{} | E{}}}", render_set(s), v.0)
                }
            }
        }
    }
}

#[cfg(test)]
mod row_tests {
    use super::row::*;
    use super::*;
    use std::collections::BTreeSet;

    fn three_effects() -> (EffectId, EffectId, EffectId) {
        // Use raw EffectId values — the row machinery is independent of
        // DefMap interning. (Real call sites get IDs from `DefMap`.)
        (EffectId(1), EffectId(2), EffectId(3))
    }

    fn name_for(e: EffectId) -> Option<String> {
        match e.0 {
            1 => Some("fs".into()),
            2 => Some("net".into()),
            3 => Some("time".into()),
            _ => None,
        }
    }

    #[test]
    fn row_arith_01_closed_closed_equal() {
        let (fs, net, _) = three_effects();
        let mut s = RowSubst::new();
        let a = EffectRow::closed([fs, net]);
        let b = EffectRow::closed([fs, net]);
        unify_rows(&mut s, &a, &b).expect("equal closed rows unify");
    }

    #[test]
    fn row_arith_02_closed_closed_unequal_fails() {
        let (fs, net, _) = three_effects();
        let mut s = RowSubst::new();
        let a = EffectRow::closed([fs]);
        let b = EffectRow::closed([fs, net]);
        let err = unify_rows(&mut s, &a, &b).expect_err("unequal closed rows must reject");
        assert!(matches!(err, RowError::ClosedMismatch(_, _)));
    }

    #[test]
    fn row_arith_03_open_unifies_with_closed_binding_tail_to_diff() {
        let (fs, net, _) = three_effects();
        let mut s = RowSubst::new();
        let v = s.fresh();
        // Open({fs}, v) unifies with Closed({fs, net}): v ↦ {net}.
        let open = EffectRow::open([fs], v);
        let closed = EffectRow::closed([fs, net]);
        unify_rows(&mut s, &open, &closed).expect("subset must succeed");
        let bound = s.lookup(v).cloned().expect("v should be bound");
        assert_eq!(bound, EffectRow::closed([net]));
    }

    #[test]
    fn row_arith_04_open_with_empty_concrete_unifies_to_full_closed() {
        let (fs, net, _) = three_effects();
        let mut s = RowSubst::new();
        let v = s.fresh();
        // Empty open row Open({}, v) unifies with any closed row by
        // binding v to the whole set.
        let open = EffectRow::open([], v);
        let closed = EffectRow::closed([fs, net]);
        unify_rows(&mut s, &open, &closed).expect("empty open absorbs everything");
        assert_eq!(s.lookup(v).cloned().unwrap(), EffectRow::closed([fs, net]));
    }

    #[test]
    fn row_arith_05_closed_with_extra_rejects_subset_open() {
        let (fs, net, time) = three_effects();
        let mut s = RowSubst::new();
        let v = s.fresh();
        // Open({fs, net}, v) unifies with Closed({fs, time}): {fs,net}
        // is NOT a subset of {fs,time}, so unification rejects.
        let open = EffectRow::open([fs, net], v);
        let closed = EffectRow::closed([fs, time]);
        let err = unify_rows(&mut s, &open, &closed).expect_err("non-subset must reject");
        assert!(matches!(err, RowError::ClosedMismatch(_, _)));
    }

    #[test]
    fn row_arith_06_open_open_introduces_shared_fresh_tail() {
        let (fs, net, _) = three_effects();
        let mut s = RowSubst::new();
        let v = s.fresh();
        let w = s.fresh();
        // Open({fs}, v) unifies with Open({net}, w):
        //   v ↦ Open({net}, fresh)
        //   w ↦ Open({fs}, fresh) — same fresh
        // After unification, both rows resolve to {fs, net | fresh}.
        let a = EffectRow::open([fs], v);
        let b = EffectRow::open([net], w);
        unify_rows(&mut s, &a, &b).expect("two opens unify");
        let ra = s.resolve(&a);
        let rb = s.resolve(&b);
        assert_eq!(ra, rb, "post-unify resolution must agree");
        match ra {
            EffectRow::Open(set, _) => {
                assert!(set.contains(&fs));
                assert!(set.contains(&net));
            }
            EffectRow::Closed(_) => panic!("expected open row, got closed"),
        }
    }

    #[test]
    fn row_arith_07_occurs_check_rejects_direct_cycle() {
        let mut s = RowSubst::new();
        let v = s.fresh();
        // v ↦ Open({}, v) — direct self-reference.
        let err = s.bind(v, EffectRow::open([], v)).expect_err("occurs check");
        assert_eq!(err, RowError::Occurs(v));
    }

    #[test]
    fn row_arith_08_resolve_walks_chain_of_bindings() {
        let (fs, net, time) = three_effects();
        let mut s = RowSubst::new();
        let v = s.fresh();
        let w = s.fresh();
        // v ↦ Open({fs}, w); w ↦ Closed({net, time})
        // resolve(Open({}, v)) should yield Closed({fs, net, time}).
        s.bind(v, EffectRow::open([fs], w)).unwrap();
        s.bind(w, EffectRow::closed([net, time])).unwrap();
        let row = EffectRow::open([], v);
        let resolved = s.resolve(&row);
        assert_eq!(resolved, EffectRow::closed([fs, net, time]));
    }

    #[test]
    fn row_arith_09_subsume_closed_subset_ok() {
        let (fs, net, _) = three_effects();
        let actual: BTreeSet<EffectId> = [fs].into_iter().collect();
        let expected: BTreeSet<EffectId> = [fs, net].into_iter().collect();
        subsume_closed(&actual, &expected).expect("subset must subsume");
    }

    #[test]
    fn row_arith_10_subsume_closed_superset_rejects() {
        let (fs, net, _) = three_effects();
        let actual: BTreeSet<EffectId> = [fs, net].into_iter().collect();
        let expected: BTreeSet<EffectId> = [fs].into_iter().collect();
        let err = subsume_closed(&actual, &expected).expect_err("superset must reject");
        assert!(matches!(err, RowError::SubsumptionFail(_, _)));
    }

    #[test]
    fn row_arith_11_pretty_print_renders_named_effects() {
        let (fs, net, _) = three_effects();
        let mut s = RowSubst::new();
        let v = s.fresh();
        let row = EffectRow::open([fs, net], v);
        let txt = pretty_row(&row, name_for);
        assert_eq!(txt, format!("{{fs, net | E{}}}", v.0));
        let closed = EffectRow::closed([net]);
        assert_eq!(pretty_row(&closed, name_for), "{net}");
        let bare = EffectRow::open([], v);
        assert_eq!(pretty_row(&bare, name_for), format!("E{}", v.0));
    }

    #[test]
    fn row_arith_12_instantiate_list_map_sig_threads_row_var() {
        // Validates the v0.13-wired stdlib `List.map` signature: one
        // row var threaded from the closure parameter to the return
        // effect row. Both should reference the SAME fresh row var.
        let sig = stdlib_list_map_sig();
        let mut s = RowSubst::new();
        let (params, ret, fresh) = instantiate_row_sig(&sig, &mut s);
        assert_eq!(params.len(), 2);
        assert!(params[0].is_none(), "list param has no fn row");
        let closure_row = params[1].as_ref().expect("closure param has a row");
        // Both rows are Open({}, fresh[0]).
        let expected = EffectRow::open([], fresh[0]);
        assert_eq!(closure_row, &expected);
        assert_eq!(ret, expected);
    }
}
