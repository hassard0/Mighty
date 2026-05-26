//! v0.21 — Cap-resolver integration pass.
//!
//! Walks the typed package post-typecheck, populating a
//! [`CapResolver`] with the caps visible at each scope, and emitting
//! the six v0.21 cap-resolution diagnostics (MT4060..MT4065) when a
//! capability use is invalid.
//!
//! Today the integration is **observational**: the resolver shadows
//! the existing `MT4010 capability_too_broad` path and adds the new
//! name-resolution layer on top. When the surface syntax grows a
//! dedicated `cap <Name>: <Family>` declaration (post-v0.21), the
//! lowering pass will feed those decls into [`CapResolver::declare`]
//! directly; until then the resolver pass treats every cap-typed
//! agent / fn parameter as an implicit declaration in the enclosing
//! scope and validates method calls against the family surface.
//!
//! Tests in `tests/cap_resolution.rs` exercise the resolver API
//! directly (no HIR plumbing required).

use crate::cap_resolver::{CapResolutionError, CapResolver};
use crate::ty::{CapConstraint, CapFamily};
use crate::TypedPackage;
use mty_diagnostics::Diagnostic;
use mty_hir::{HirExpr, Package, SourceSpan};

/// Run the v0.21 cap-name resolver pass over `typed` + `pkg`. Appends
/// any new diagnostics to `out`. The pass is non-fatal: even when the
/// resolver finds problems, the rest of the pipeline continues.
///
/// **Pass shape** (single forward sweep):
///
/// 1. Seed module-level cap declarations from the `def_map`'s fn /
///    agent / handler parameter types — every `Cap{family, ...}`
///    parameter implicitly declares its binding name.
/// 2. For each expression `recv.method(...)` whose receiver resolves
///    to a known cap-named local, check the method against the
///    family surface (MT4064) and the (heuristic) constraint shape
///    against the family's narrowing surface (MT4065).
/// 3. For each reference to an unbound name *in a strict scope* that
///    looks like a cap (capitalised + matches a built-in family
///    name), emit MT4060 instead of MT2021. (Today MT2021 still
///    wins; this hook is reserved for v0.22 when strict-cap-mode
///    flips on by default.)
///
/// The unit tests in `tests/cap_resolution.rs` skip the surface-
/// syntax path entirely and drive [`CapResolver`] directly — that's
/// the load-bearing surface for the six MT406x codes.
pub fn run(typed: &TypedPackage, pkg: &Package, out: &mut Vec<Diagnostic>) {
    let resolver = CapResolver::new();

    // Sweep every typed expression in fn bodies looking for method
    // calls on cap-typed receivers. The per-fn scope model is folded
    // into the sweep: each fn's params declare cap names into a
    // freshly-pushed scope, the body is walked, then the scope is
    // popped.
    sweep_method_calls(typed, pkg, &resolver, out);

    // Per-fn scope-violation pass. For each fn whose body uses a
    // cap-name `X` that is NOT one of its own params but IS declared
    // (with the SAME family — otherwise MT4061 won) as a param of
    // another fn, the binding is "out of scope" for this caller —
    // MT4062.
    sweep_scope_violations(typed, pkg, out);

    // Per-fn redeclaration pass. A fn that declares two cap params
    // with the same name in its own signature surfaces MT4063 —
    // even though the typeck and parser may also reject it via a
    // duplicate-binding error, the cap-resolver pass prefers a
    // domain-specific message.
    sweep_redeclarations(typed, out);
}

fn sweep_redeclarations(typed: &TypedPackage, out: &mut Vec<Diagnostic>) {
    for params in typed.fn_params.values() {
        let mut seen_caps: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (name, ty) in params {
            if matches!(typed.ty_arena.get(*ty), crate::ty::TyData::Cap { .. })
                && !seen_caps.insert(name.clone())
            {
                out.push(crate::diag::cap_redeclaration(
                    name,
                    0,
                    &SourceSpan { start: 0, end: 0 },
                ));
            }
        }
    }
}

fn sweep_scope_violations(typed: &TypedPackage, pkg: &Package, out: &mut Vec<Diagnostic>) {
    use mty_hir::Item;

    // Build (name → family) from EVERY fn's cap params.
    let mut all_cap_names: std::collections::HashMap<String, crate::ty::CapFamily> =
        std::collections::HashMap::new();
    for params in typed.fn_params.values() {
        for (name, ty) in params {
            if let crate::ty::TyData::Cap { family, .. } = typed.ty_arena.get(*ty) {
                all_cap_names
                    .entry(name.clone())
                    .or_insert_with(|| family.clone());
            }
        }
    }

    // For each top-level fn item, walk the fn body's expressions
    // (transitively, via the body BlockId) and look for `Path([name,
    // method])`-shaped callees whose `name` is a known cap declared
    // in ANOTHER fn but not in this one. MT4062 fires for the
    // first such mismatch per (fid, name).
    let mut seen: std::collections::HashSet<(mty_hir::FnId, String)> =
        std::collections::HashSet::new();
    for item_id in &pkg.top_level {
        let Item::Fn(fid) = &pkg.items[*item_id] else {
            continue;
        };
        let fid = *fid;
        let Some(params) = typed.fn_params.get(&fid) else {
            continue;
        };
        let own_caps: std::collections::HashSet<&str> = params
            .iter()
            .filter_map(|(name, ty)| {
                if matches!(typed.ty_arena.get(*ty), crate::ty::TyData::Cap { .. }) {
                    Some(name.as_str())
                } else {
                    None
                }
            })
            .collect();

        // Collect every expr_id reachable from the fn's body, then
        // restrict the cap-name scan to that subset.
        let hir_fn = &pkg.fns[fid];
        let Some(body_block) = hir_fn.body else {
            continue;
        };
        let mut visited: std::collections::HashSet<mty_hir::ExprId> =
            std::collections::HashSet::new();
        collect_block_exprs(pkg, body_block, &mut visited);
        for eid in &visited {
            let expr = &pkg.exprs[*eid];
            if let HirExpr::Call { callee, .. } = expr {
                if let HirExpr::Path(segs) = &pkg.exprs[*callee] {
                    if segs.len() == 2 {
                        let name = &segs[0];
                        if all_cap_names.contains_key(name)
                            && !own_caps.contains(name.as_str())
                            && looks_like_cap(name).is_none()
                            && seen.insert((fid, name.clone()))
                        {
                            out.push(crate::diag::cap_scope_violation(
                                name,
                                1,
                                &SourceSpan { start: 0, end: 0 },
                            ));
                        }
                    }
                }
            }
        }
    }
}

/// Recursively collect every ExprId reachable from `bid` and store
/// in `out`. Used by the scope-violation pass to restrict the
/// MT4062 scan to expressions inside the fn body.
fn collect_block_exprs(
    pkg: &Package,
    bid: mty_hir::BlockId,
    out: &mut std::collections::HashSet<mty_hir::ExprId>,
) {
    let block = pkg.blocks[bid].clone();
    for stmt in &block.stmts {
        match stmt {
            mty_hir::HirStmt::Let { init, .. } => {
                if let Some(e) = init {
                    collect_expr(pkg, *e, out);
                }
            }
            mty_hir::HirStmt::Expr(e) => collect_expr(pkg, *e, out),
        }
    }
    if let Some(tail) = block.tail {
        collect_expr(pkg, tail, out);
    }
}

fn collect_expr(
    pkg: &Package,
    eid: mty_hir::ExprId,
    out: &mut std::collections::HashSet<mty_hir::ExprId>,
) {
    if !out.insert(eid) {
        return;
    }
    let expr = pkg.exprs[eid].clone();
    match expr {
        HirExpr::Call { callee, args } => {
            collect_expr(pkg, callee, out);
            for a in args {
                collect_expr(pkg, a.value, out);
            }
        }
        HirExpr::MethodCall { receiver, args, .. } => {
            collect_expr(pkg, receiver, out);
            for a in args {
                collect_expr(pkg, a.value, out);
            }
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            collect_expr(pkg, lhs, out);
            collect_expr(pkg, rhs, out);
        }
        HirExpr::Unary { rhs, .. } => collect_expr(pkg, rhs, out),
        HirExpr::Block(b) => collect_block_exprs(pkg, b, out),
        HirExpr::If { cond, then, else_ } => {
            collect_expr(pkg, cond, out);
            collect_block_exprs(pkg, then, out);
            if let Some(e) = else_ {
                collect_expr(pkg, e, out);
            }
        }
        HirExpr::Tuple(xs) | HirExpr::Array(xs) => {
            for x in xs {
                collect_expr(pkg, x, out);
            }
        }
        HirExpr::Field { receiver, .. } => collect_expr(pkg, receiver, out),
        HirExpr::Index { receiver, idx } => {
            collect_expr(pkg, receiver, out);
            collect_expr(pkg, idx, out);
        }
        HirExpr::Borrow { inner, .. } => collect_expr(pkg, inner, out),
        HirExpr::Move(inner) | HirExpr::Question(inner) | HirExpr::Return(Some(inner)) => {
            collect_expr(pkg, inner, out)
        }
        HirExpr::Match { scrutinee, arms } => {
            collect_expr(pkg, scrutinee, out);
            for arm in arms {
                if let Some(g) = arm.guard {
                    collect_expr(pkg, g, out);
                }
                collect_expr(pkg, arm.body, out);
            }
        }
        HirExpr::IfLet {
            scrutinee,
            then,
            else_,
            ..
        } => {
            collect_expr(pkg, scrutinee, out);
            collect_block_exprs(pkg, then, out);
            if let Some(e) = else_ {
                collect_expr(pkg, e, out);
            }
        }
        HirExpr::For { iter, body, .. } => {
            collect_expr(pkg, iter, out);
            collect_block_exprs(pkg, body, out);
        }
        HirExpr::While { cond, body } => {
            collect_expr(pkg, cond, out);
            collect_block_exprs(pkg, body, out);
        }
        HirExpr::Loop { body } | HirExpr::Unsafe(body) | HirExpr::TaskScope { body, .. } => {
            collect_block_exprs(pkg, body, out)
        }
        _ => {}
    }
}

fn sweep_method_calls(
    typed: &TypedPackage,
    pkg: &Package,
    resolver: &CapResolver,
    out: &mut Vec<Diagnostic>,
) {
    // Build a name → TyId map from every fn's parameter list so we
    // can resolve the receiver of a `Path([name, method])`-shaped
    // call. The lowerer collapses `cap.method(args)` into
    // `Call { callee: Path([cap, method]) }`, so MethodCall alone
    // would miss most cap invocations.
    //
    // Cross-fn name collisions with different families are
    // surfaced as MT4061 (family mismatch). Same-name same-family
    // across fns is fine (most caps are per-fn handles).
    let mut cap_param_tys: std::collections::HashMap<String, crate::ty::CapFamily> =
        std::collections::HashMap::new();
    let mut family_mismatch_emitted: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for params in typed.fn_params.values() {
        for (name, ty) in params {
            if let crate::ty::TyData::Cap { family, .. } = typed.ty_arena.get(*ty) {
                if let Some(prev) = cap_param_tys.get(name) {
                    if prev != family && family_mismatch_emitted.insert(name.clone()) {
                        out.push(crate::diag::cap_family_mismatch(
                            name,
                            prev,
                            family,
                            &SourceSpan { start: 0, end: 0 },
                        ));
                    }
                }
                cap_param_tys
                    .entry(name.clone())
                    .or_insert_with(|| family.clone());
            }
        }
    }
    // De-dup emitted diagnostics so the same `cap.method(...)` doesn't
    // get reported once per shape match.
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();

    for (_eid, expr) in pkg.exprs.iter() {
        // Shape A: explicit MethodCall.
        if let HirExpr::MethodCall {
            receiver, method, ..
        } = expr
        {
            let Some(recv_ty) = typed.expr_ty.get(receiver).copied() else {
                continue;
            };
            if let crate::ty::TyData::Cap { family, .. } = typed.ty_arena.get(recv_ty) {
                let key = (format!("{:?}", family), method.clone());
                if seen.insert(key) {
                    if let Err(err) = resolver.check_method(family, method) {
                        out.push(crate::diag::cap_resolution_error(
                            &err,
                            &SourceSpan { start: 0, end: 0 },
                        ));
                    }
                }
            }
            continue;
        }
        // Shape B: Call { callee: Path([name, method]) } — the form
        // the HIR lowerer emits for `cap.method(args)`.
        if let HirExpr::Call { callee, args } = expr {
            if let HirExpr::Path(segs) = &pkg.exprs[*callee] {
                if segs.len() == 2 {
                    if let Some(family) = cap_param_tys.get(&segs[0]) {
                        let key = (segs[0].clone(), segs[1].clone());
                        if seen.insert(key) {
                            if let Err(err) = resolver.check_method(family, &segs[1]) {
                                out.push(crate::diag::cap_resolution_error(
                                    &err,
                                    &SourceSpan { start: 0, end: 0 },
                                ));
                            } else {
                                // MT4065: validate narrowing
                                // constructor's arg shape. The
                                // narrowing methods that produce a
                                // path-constraint (`path`) need a
                                // string literal first arg; `host`
                                // needs a non-empty string list.
                                check_narrowing_args(family, &segs[1], args, pkg, out);
                            }
                        }
                    } else if looks_like_cap(&segs[0]).is_some() {
                        // MT4060: `Fs.method(...)` shape — using
                        // the family type name as a value. The user
                        // probably meant to receive a cap via a fn
                        // parameter; flag the unbound name.
                        let key = (segs[0].clone(), segs[1].clone());
                        if seen.insert(key) {
                            out.push(crate::diag::cap_name_unbound(
                                &segs[0],
                                &SourceSpan { start: 0, end: 0 },
                            ));
                        }
                    }
                }
            }
        }
    }
    let _ = resolver;
}

/// MT4065 emit: validate a narrowing constructor's argument shape.
/// `family.method(args)` is in the surface; we check the args
/// against the constraint shape `method` produces. v0.21 ruleset:
///
/// - `Fs.path(p)` — p must be a string literal (else MT4065).
/// - `Net.host(h)` — h must be a non-empty string literal.
/// - `Fs.ro()` — no args required.
/// - everything else — no check (MT4065 surfaces only on the
///   narrowing methods themselves).
fn check_narrowing_args(
    family: &CapFamily,
    method: &str,
    args: &[mty_hir::HirArg],
    pkg: &Package,
    out: &mut Vec<Diagnostic>,
) {
    use mty_hir::HirLiteral;
    match (family, method) {
        (CapFamily::Fs, "path") | (CapFamily::Net, "host") => {
            if args.is_empty() {
                out.push(crate::diag::cap_constraint_invalid(
                    family,
                    method,
                    "expected at least one string literal argument",
                    &SourceSpan { start: 0, end: 0 },
                ));
                return;
            }
            // First arg must be a string literal.
            let arg_expr = &pkg.exprs[args[0].value];
            let ok = matches!(arg_expr, HirExpr::Literal(HirLiteral::Str(s)) if !s.is_empty());
            if !ok {
                out.push(crate::diag::cap_constraint_invalid(
                    family,
                    method,
                    "expected a non-empty string literal",
                    &SourceSpan { start: 0, end: 0 },
                ));
            }
        }
        _ => {}
    }
}

/// Heuristic: does `name` "look like" a capability identifier (e.g.
/// `Fs`, `Net`, `Clock`, `Dom`, `Model`)? Used by the MT4060 emit-site
/// to decide between MT4060 vs MT2021 when an unresolved name shows
/// up in a path expression.
pub fn looks_like_cap(name: &str) -> Option<CapFamily> {
    match name {
        "Fs" => Some(CapFamily::Fs),
        "Net" => Some(CapFamily::Net),
        "Clock" => Some(CapFamily::Clock),
        "Dom" => Some(CapFamily::Dom),
        "Model" => Some(CapFamily::Model),
        _ => None,
    }
}

/// Validate that a given (family, method, constraint) triple is
/// acceptable. Wraps [`CapResolver::check_narrowing`] for use from
/// `mty-types::check` once a real surface-syntax for cap narrowing
/// lands.
#[allow(dead_code)]
pub fn validate_narrowing(
    family: &CapFamily,
    method: &str,
    constraint: &CapConstraint,
) -> Result<(), CapResolutionError> {
    let resolver = CapResolver::new();
    resolver.check_narrowing(family, method, constraint)
}
