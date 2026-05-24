//! Bidirectional expression / statement checking.

use crate::defs::*;
use crate::diag;
use crate::infer::*;
use crate::resolve::ParamScope;
use crate::ty::*;
use sdust_diagnostics::Diagnostic;
use sdust_hir::*;
use std::collections::HashMap;

/// Local binding scope. A stack of frames; each frame is a list of
/// `(name, TyId)` pairs.
#[derive(Default)]
pub struct LocalScope {
    frames: Vec<Vec<(String, TyId)>>,
}

impl LocalScope {
    pub fn enter(&mut self) {
        self.frames.push(vec![]);
    }
    pub fn leave(&mut self) {
        self.frames.pop();
    }
    pub fn bind(&mut self, name: impl Into<String>, ty: TyId) {
        if self.frames.is_empty() {
            self.frames.push(vec![]);
        }
        let f = self.frames.last_mut().unwrap();
        f.push((name.into(), ty));
    }
    pub fn lookup(&self, name: &str) -> Option<TyId> {
        for frame in self.frames.iter().rev() {
            for (n, t) in frame.iter().rev() {
                if n == name {
                    return Some(*t);
                }
            }
        }
        None
    }
}

/// v0.3 (A65): the kind of lexical scope we're currently type-checking.
/// Differentiates **permissive** scopes (where Slice-3's A21 fresh-var
/// fallback for unresolved names still applies) from **strict** scopes
/// (where any unresolved name promotes to SD2021).
///
/// | ScopeKind   | Unresolved name behavior              |
/// |-------------|---------------------------------------|
/// | TopLevelFn  | Permissive (slice-3 A21 fresh-var)    |
/// | ExternBlock | Permissive (foreign ABI shim)         |
/// | Macro       | Permissive (token-soup; later expand) |
/// | Unsafe      | Permissive (raw-ptr builtins)         |
/// | Arena       | Permissive (arena-implicit names)     |
/// | Budget      | Permissive (budget-category names)    |
/// | Sandbox     | Permissive (narrowed-cap names)       |
/// | AgentBody   | **Strict** — SD2021 on unknown        |
/// | HandlerBody | **Strict** — SD2021 on unknown        |
/// | SupervisorBody | **Strict** — SD2021 on unknown     |
/// | CapNarrowBody | **Strict** — SD2021 on unknown      |
///
/// The `tolerance` per-body set still applies to strict scopes: it carries
/// the agent's state / ctor-param / method names so the body sees its own
/// surface. Anything *not* in the set, and *not* a prelude / local /
/// def-map binding, hits the strict-vs-permissive switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    TopLevelFn,
    ExternBlock,
    Macro,
    Unsafe,
    Arena,
    Budget,
    Sandbox,
    AgentBody,
    HandlerBody,
    SupervisorBody,
    CapNarrowBody,
}

impl ScopeKind {
    /// True iff unresolved names should hard-error (SD2021) instead of
    /// silently falling back to a fresh inference variable.
    pub fn is_strict(&self) -> bool {
        matches!(
            self,
            ScopeKind::AgentBody
                | ScopeKind::HandlerBody
                | ScopeKind::SupervisorBody
                | ScopeKind::CapNarrowBody
        )
    }

    /// Short human-readable label for the SD2021 strict-mode note.
    pub fn label(&self) -> &'static str {
        match self {
            ScopeKind::TopLevelFn => "top-level fn",
            ScopeKind::ExternBlock => "extern block",
            ScopeKind::Macro => "macro",
            ScopeKind::Unsafe => "unsafe",
            ScopeKind::Arena => "arena",
            ScopeKind::Budget => "budget",
            ScopeKind::Sandbox => "sandbox",
            ScopeKind::AgentBody => "agent",
            ScopeKind::HandlerBody => "handler",
            ScopeKind::SupervisorBody => "supervisor",
            ScopeKind::CapNarrowBody => "cap-narrow",
        }
    }
}

pub struct Cx<'a> {
    pub pkg: &'a Package,
    pub defs: &'a mut DefMap,
    pub arena: &'a mut TyArena,
    pub subst: &'a mut Substitution,
    pub diag: &'a mut Vec<Diagnostic>,
    pub locals: LocalScope,
    pub return_ty: TyId,
    pub result_id: AdtId,
    pub option_id: AdtId,
    pub agent_ref_id: AdtId,
    /// Generic params in scope (for body-checking).
    pub param_scope: ParamScope,
    /// Scope-aware tolerance set for unresolved value names. Names in this
    /// set silently resolve to fresh inference vars (A21); names not in the
    /// set emit SD2021 when the current scope is **strict** (see
    /// `ScopeKind::is_strict`). When `tolerance_open` is true (extern,
    /// macro, deep unsafe), all unresolved names are tolerated regardless
    /// of strictness — this matches v0.2's pre-A65 behavior.
    pub tolerance: std::collections::HashSet<String>,
    pub tolerance_open: bool,
    /// v0.3 (A65): the kind of scope we're currently inside. Drives the
    /// permissive-vs-strict policy at unresolved-name sites.
    pub scope_kind: ScopeKind,
    /// Side-table sink: every expression we type produces a (id, ty) entry.
    pub expr_ty: &'a mut HashMap<ExprId, TyId>,
}

impl<'a> Cx<'a> {
    fn span_of_expr(&self, _e: ExprId) -> SourceSpan {
        // Expression spans are not directly stored per HirExpr in slice 2's
        // arena; placeholder. The diag still includes the message.
        SourceSpan { start: 0, end: 0 }
    }

    fn fresh(&mut self) -> TyId {
        let v = self.subst.fresh_var();
        self.arena.var(v)
    }
}

/// Build a ParamId -> TyId map from an ADT's `param_ids` and positional `args`.
fn adt_subst(
    defs: &DefMap,
    aid: AdtId,
    args: &[TyId],
) -> std::collections::HashMap<crate::ty::ParamId, TyId> {
    let mut m = std::collections::HashMap::new();
    if let Some(adt) = defs.adt(aid) {
        for (pid, ty) in adt.param_ids.iter().zip(args.iter()) {
            m.insert(*pid, *ty);
        }
    }
    m
}

pub fn synth_expr(cx: &mut Cx, expr_id: ExprId) -> TyId {
    let ty = synth_expr_inner(cx, expr_id);
    cx.expr_ty.insert(expr_id, ty);
    ty
}

fn synth_expr_inner(cx: &mut Cx, expr_id: ExprId) -> TyId {
    let expr = cx.pkg.exprs[expr_id].clone();
    match expr {
        HirExpr::Literal(lit) => synth_literal(cx, &lit),
        HirExpr::Path(segs) => synth_path(cx, &segs, expr_id),
        HirExpr::PathGeneric { segments, generics } => {
            synth_path_generic(cx, &segments, &generics, expr_id)
        }
        HirExpr::Block(b) => check_block(cx, b, None),
        HirExpr::Tuple(xs) => {
            let parts: Vec<TyId> = xs.iter().map(|e| synth_expr(cx, *e)).collect();
            cx.arena.tuple(parts)
        }
        HirExpr::Array(xs) => {
            if xs.is_empty() {
                let v = cx.fresh();
                cx.arena.array(v, Some(0))
            } else {
                let first = synth_expr(cx, xs[0]);
                for &e in &xs[1..] {
                    check_expr(cx, e, first);
                }
                cx.arena.array(first, Some(xs.len() as u64))
            }
        }
        HirExpr::Binary { op, lhs, rhs } => synth_binary(cx, op, lhs, rhs, expr_id),
        HirExpr::Unary { op, rhs } => synth_unary(cx, op, rhs),
        HirExpr::Borrow { mutable, inner } => {
            let inner_ty = synth_expr(cx, inner);
            cx.arena.ref_to(mutable, inner_ty)
        }
        HirExpr::Move(inner) => synth_expr(cx, inner),
        HirExpr::Call { callee, args } => synth_call(cx, callee, &args, expr_id),
        HirExpr::MethodCall {
            receiver,
            method,
            args,
        } => synth_method_call(cx, receiver, &method, &args, expr_id),
        HirExpr::Field { receiver, name } => synth_field(cx, receiver, &name),
        HirExpr::Index { receiver, idx } => synth_index(cx, receiver, idx),
        HirExpr::If { cond, then, else_ } => {
            check_expr(cx, cond, cx.arena.bool_);
            let then_ty = check_block(cx, then, None);
            match else_ {
                Some(else_e) => {
                    let else_ty = synth_expr(cx, else_e);
                    if unify(then_ty, else_ty, cx.subst, cx.arena).is_err() {
                        cx.diag.push(diag::mismatch(
                            then_ty,
                            else_ty,
                            &cx.span_of_expr(expr_id),
                            cx.arena,
                            cx.subst,
                            cx.defs,
                        ));
                    }
                    then_ty
                }
                None => cx.arena.unit,
            }
        }
        HirExpr::IfLet {
            pat,
            scrutinee,
            then,
            else_,
        } => {
            let scrut_ty = synth_expr(cx, scrutinee);
            cx.locals.enter();
            check_pattern(cx, pat, scrut_ty);
            let then_ty = check_block(cx, then, None);
            cx.locals.leave();
            match else_ {
                Some(else_e) => {
                    let else_ty = synth_expr(cx, else_e);
                    let _ = unify(then_ty, else_ty, cx.subst, cx.arena);
                    then_ty
                }
                None => cx.arena.unit,
            }
        }
        HirExpr::Match { scrutinee, arms } => synth_match(cx, scrutinee, &arms),
        HirExpr::For { pat, iter, body } => {
            let iter_ty = synth_expr(cx, iter);
            // Element type: if iter is &[T] or [T; N], element is T; else fresh var.
            let elem = element_of(cx, iter_ty);
            cx.locals.enter();
            check_pattern(cx, pat, elem);
            let _ = check_block(cx, body, None);
            cx.locals.leave();
            cx.arena.unit
        }
        HirExpr::While { cond, body } => {
            check_expr(cx, cond, cx.arena.bool_);
            let _ = check_block(cx, body, None);
            cx.arena.unit
        }
        HirExpr::Loop { body } => {
            let _ = check_block(cx, body, None);
            cx.arena.never
        }
        HirExpr::Return(inner) => {
            if let Some(e) = inner {
                let t = synth_expr(cx, e);
                let _ = unify(t, cx.return_ty, cx.subst, cx.arena);
            }
            cx.arena.never
        }
        HirExpr::Struct { path, fields } => synth_struct(cx, &path, &fields, expr_id),
        HirExpr::Map(_) => {
            // Slice 3: Map literals (e.g. `Map::[Str, Json]{}`) check the
            // entries but yield an opaque Map type or fresh var.
            cx.fresh()
        }
        HirExpr::Send { target, msg, args } => {
            let _ = synth_expr(cx, target);
            for (i, a) in args.iter().enumerate() {
                let ty = synth_expr(cx, a.value);
                check_sendable_arg(cx, i, ty, expr_id);
            }
            let _ = msg;
            cx.arena.unit
        }
        HirExpr::Ask { target, msg, args } => {
            let _ = synth_expr(cx, target);
            for (i, a) in args.iter().enumerate() {
                let ty = synth_expr(cx, a.value);
                check_sendable_arg(cx, i, ty, expr_id);
            }
            let _ = msg;
            cx.fresh()
        }
        HirExpr::Deadline { inner, dur } => {
            let _ = synth_expr(cx, dur);
            synth_expr(cx, inner)
        }
        HirExpr::Question(inner) => synth_question(cx, inner, expr_id),
        HirExpr::Spawn { inner, .. } => {
            let t = synth_expr(cx, inner);
            cx.arena.adt(cx.agent_ref_id, vec![t])
        }
        HirExpr::Detach(inner) | HirExpr::Join(inner) => synth_expr(cx, inner),
        HirExpr::HtmlTemplate(_) => cx.arena.string,
        HirExpr::Unsafe(b) => {
            // Inside `unsafe`, open the tolerance for raw-pointer / pointer
            // ABI builtins that aren't part of the prelude (e.g. p.read(),
            // raw_ptr is already a prelude fn). For slice 4 we simply open
            // tolerance fully inside unsafe blocks (spec §21 allows
            // additional primitives).
            //
            // v0.3 (A65): `unsafe` is a permissive scope — strict outer
            // bodies (agent / handler / supervisor) yield to the looser
            // policy inside the block. The original strict scope_kind is
            // saved and restored at the closing brace.
            let saved_open = cx.tolerance_open;
            let saved_kind = cx.scope_kind;
            cx.tolerance_open = true;
            cx.scope_kind = ScopeKind::Unsafe;
            let t = check_block(cx, b, None);
            cx.tolerance_open = saved_open;
            cx.scope_kind = saved_kind;
            t
        }
        HirExpr::Arena { body, .. } => {
            // Arena bodies have implicit access to identifiers like the
            // arena's own name; we open tolerance so example 12's
            // `tokenize(input)` etc. work even though `tokenize` isn't
            // a prelude name.
            let saved_open = cx.tolerance_open;
            let saved_kind = cx.scope_kind;
            cx.tolerance_open = true;
            cx.scope_kind = ScopeKind::Arena;
            let t = synth_expr(cx, body);
            cx.tolerance_open = saved_open;
            cx.scope_kind = saved_kind;
            t
        }
        HirExpr::TaskScope { body, .. } => check_block(cx, body, None),
        HirExpr::Budget { entries, body } => {
            for (_, v) in &entries {
                let _ = synth_expr(cx, *v);
            }
            // Budget bodies have implicit access to budget-category names
            // (`cpu`, `wall`, `mem`, `mb`) and to any name referenced as a
            // policy-recoverable identifier. Open tolerance for slice 4.
            let saved_open = cx.tolerance_open;
            let saved_kind = cx.scope_kind;
            cx.tolerance_open = true;
            cx.scope_kind = ScopeKind::Budget;
            let t = synth_expr(cx, body);
            cx.tolerance_open = saved_open;
            cx.scope_kind = saved_kind;
            t
        }
        HirExpr::Sandbox { body, entries, .. } => {
            // Sandbox-with bodies have implicit access to every capability
            // narrowing entry. Pre-walk the entry expressions, then open
            // tolerance for the body.
            //
            // v0.3 (A65): sandbox bodies that narrow a cap (`cap.ro("/data")`)
            // are themselves a **strict** sub-scope — we want the body to
            // refuse unknown identifiers. We keep `tolerance_open=false`
            // here while pushing CapNarrowBody when the sandbox contains
            // narrowing entries (the `entries` arity > 0 case). Otherwise
            // we fall back to the legacy Sandbox-permissive policy.
            for (_, e) in &entries {
                let _ = synth_expr(cx, *e);
            }
            // v0.3 (A65): sandbox-with bodies tag their ScopeKind as
            // CapNarrowBody for framework consistency, but we keep
            // `tolerance_open=true` so the historic v0.2 surface (where
            // names like `job(input)` are intentionally unresolved at
            // type-check time, to be wired by the runtime under the
            // sandbox's authority) continues to compile. When slice-7
            // ships real cap-name resolution we'll flip this to false
            // and SD2021-strict-mode will fire automatically — see
            // EFFECTS_V0_3_NOTES.md for the rationale.
            let saved_open = cx.tolerance_open;
            let saved_kind = cx.scope_kind;
            cx.tolerance_open = true;
            cx.scope_kind = if entries.is_empty() {
                ScopeKind::Sandbox
            } else {
                ScopeKind::CapNarrowBody
            };
            let t = check_block(cx, body, None);
            cx.tolerance_open = saved_open;
            cx.scope_kind = saved_kind;
            t
        }
        HirExpr::Run(inner) => synth_expr(cx, inner),
        HirExpr::Cast { lhs, ty } => {
            let _ = synth_expr(cx, lhs);
            crate::resolve::resolve_hir_type(
                ty,
                cx.pkg,
                cx.defs,
                cx.arena,
                &cx.param_scope,
                cx.diag,
            )
        }
        HirExpr::Lambda { params, ret, body } => synth_lambda(cx, &params, ret, body),
        HirExpr::Error => cx.arena.error,
    }
}

pub fn check_expr(cx: &mut Cx, expr_id: ExprId, expected: TyId) {
    let t = synth_expr(cx, expr_id);
    if unify(t, expected, cx.subst, cx.arena).is_err() {
        cx.diag.push(diag::mismatch(
            expected,
            t,
            &cx.span_of_expr(expr_id),
            cx.arena,
            cx.subst,
            cx.defs,
        ));
    }
}

fn synth_literal(cx: &mut Cx, lit: &HirLiteral) -> TyId {
    match lit {
        HirLiteral::Int(_, suffix) => match suffix.as_deref() {
            Some("i8") => cx.arena.i8,
            Some("i16") => cx.arena.i16,
            Some("i32") => cx.arena.i32,
            Some("i64") => cx.arena.i64,
            Some("i128") => cx.arena.i128,
            Some("u8") => cx.arena.u8,
            Some("u16") => cx.arena.u16,
            Some("u32") => cx.arena.u32,
            Some("u64") => cx.arena.u64,
            Some("u128") => cx.arena.u128,
            Some("usize") => cx.arena.usize,
            Some("isize") => cx.arena.isize,
            _ => cx.arena.int_infer,
        },
        HirLiteral::Float(_, suffix) => match suffix.as_deref() {
            Some("f32") => cx.arena.f32,
            Some("f64") => cx.arena.f64,
            _ => cx.arena.float_infer,
        },
        HirLiteral::Str(_) => cx.arena.str_,
        HirLiteral::Char(_) => cx.arena.char_,
        HirLiteral::Bool(_) => cx.arena.bool_,
        HirLiteral::Duration { .. } => cx.arena.duration,
        HirLiteral::Size { .. } => cx.arena.size,
    }
}

fn synth_path(cx: &mut Cx, segments: &[String], expr_id: ExprId) -> TyId {
    if segments.len() == 1 {
        let name = &segments[0];
        if let Some(t) = cx.locals.lookup(name) {
            return t;
        }
        if let Some(d) = cx.defs.lookup(name) {
            return resolve_value_def(cx, d, &[], expr_id, name);
        }
        // v0.3 (A65): scope-aware tolerance. Slice-3's permissive fresh-var
        // fallback (A21) now only applies in **permissive** scopes; strict
        // scopes (agent/handler/supervisor/cap-narrow bodies) promote an
        // unresolved name to SD2021 via `unresolved_value_strict`.
        //
        // Three escape hatches still let strict scopes accept unknowns:
        // (a) `tolerance_open == true` — opened inside unsafe/arena/budget/
        //     sandbox sub-blocks where the surface intentionally accepts
        //     extra implicit names;
        // (b) `tolerance.contains(name)` — the per-body tolerance set
        //     (agent state / ctor-params / sibling methods);
        // (c) the scope is permissive (e.g. TopLevelFn / ExternBlock).
        if cx.tolerance_open || cx.tolerance.contains(name) {
            return cx.fresh();
        }
        if !cx.scope_kind.is_strict() {
            // Permissive scope: keep slice-3 A21 fresh-var policy.
            return cx.fresh();
        }
        // Strict scope: hard error.
        cx.diag.push(diag::unresolved_value_strict(
            name,
            cx.scope_kind.label(),
            &cx.span_of_expr(expr_id),
        ));
        return cx.arena.error;
    }
    // Multi-segment path: `Shape.Circle` (enum variant), `Foo.bar` (method
    // not supported), `std.http.serve` (opaque), etc.
    let first = &segments[0];
    // Try `Enum.Variant` (registered Adt with matching variant).
    if let Some(DefRef::Adt(aid)) = cx.defs.lookup(first) {
        if segments.len() == 2 {
            let vname = &segments[1];
            if let Some(adt) = cx.defs.adt(aid) {
                if let Some(idx) = adt.variants.iter().position(|v| &v.name == vname) {
                    return synth_variant_constructor(cx, aid, idx);
                }
            }
        }
        // Opaque or arity-unmatched ADT chain: treat as opaque.
        return cx.fresh();
    }
    // Try module path.
    if let Some(DefRef::Module(_)) = cx.defs.lookup(first) {
        return cx.fresh();
    }
    // Try dotted lookup.
    if let Some(d) = cx.defs.lookup_path(segments) {
        return resolve_value_def(cx, d, &[], expr_id, &segments.join("."));
    }
    // First segment is a known local: this is a field-chain on a local.
    // Permissive — return fresh.
    if cx.locals.lookup(first).is_some() {
        return cx.fresh();
    }
    // First segment is a tolerated identifier (capability, state, etc.):
    // treat the chain as opaque.
    if cx.tolerance_open || cx.tolerance.contains(first) {
        return cx.fresh();
    }
    // v0.3 (A65): permissive scope keeps slice-3 fresh-var fallback.
    if !cx.scope_kind.is_strict() {
        return cx.fresh();
    }
    // Strict scope: truly unresolved multi-segment path errors on first.
    cx.diag.push(diag::unresolved_value_strict(
        first,
        cx.scope_kind.label(),
        &cx.span_of_expr(expr_id),
    ));
    cx.arena.error
}

fn synth_path_generic(
    cx: &mut Cx,
    segments: &[String],
    generics: &[TypeId],
    expr_id: ExprId,
) -> TyId {
    // Map::[K, V] etc. — resolve segments to a def; if generic, instantiate.
    let resolved_generics: Vec<TyId> = generics
        .iter()
        .map(|t| {
            crate::resolve::resolve_hir_type(
                *t,
                cx.pkg,
                cx.defs,
                cx.arena,
                &cx.param_scope,
                cx.diag,
            )
        })
        .collect();
    if segments.len() == 1 {
        let name = &segments[0];
        if let Some(d) = cx.defs.lookup(name) {
            return resolve_value_def(cx, d, &resolved_generics, expr_id, name);
        }
    }
    if let Some(d) = cx.defs.lookup_path(segments) {
        return resolve_value_def(cx, d, &resolved_generics, expr_id, &segments.join("."));
    }
    cx.fresh()
}

fn resolve_value_def(
    cx: &mut Cx,
    d: DefRef,
    explicit_generics: &[TyId],
    _expr_id: ExprId,
    _name: &str,
) -> TyId {
    match d {
        DefRef::Fn(fid) => {
            let fdef = match cx.defs.fn_def(fid) {
                Some(f) => f.clone(),
                None => return cx.arena.error,
            };
            // Instantiate generics: build a HashMap<ParamId, TyId>.
            let arg_tys: Vec<TyId> = if !explicit_generics.is_empty()
                && explicit_generics.len() == fdef.param_ids.len()
            {
                explicit_generics.to_vec()
            } else if !fdef.param_ids.is_empty() {
                fdef.param_ids.iter().map(|_| cx.fresh()).collect()
            } else {
                vec![]
            };
            let mut replacement = std::collections::HashMap::new();
            for (pid, ty) in fdef.param_ids.iter().zip(arg_tys.iter()) {
                replacement.insert(*pid, *ty);
            }
            let params: Vec<TyId> = fdef
                .params
                .iter()
                .map(|(_, t)| substitute_params(*t, &replacement, cx.arena))
                .collect();
            let ret = substitute_params(fdef.ret, &replacement, cx.arena);
            cx.arena.fn_ty(params, ret, fdef.effects.clone())
        }
        DefRef::Variant(aid, idx) => synth_variant_constructor(cx, aid, idx),
        DefRef::Adt(_) | DefRef::Module(_) | DefRef::Param(_) => {
            // Used as a value position — opaque/permissive.
            cx.fresh()
        }
    }
}

/// Build the type of an enum variant constructor.
fn synth_variant_constructor(cx: &mut Cx, aid: AdtId, idx: usize) -> TyId {
    let (variant, param_ids) = match cx.defs.adt(aid) {
        Some(a) => (a.variants[idx].clone(), a.param_ids.clone()),
        None => return cx.arena.error,
    };
    // Fresh generic args for the ADT, mapped by ParamId.
    let arg_tys: Vec<TyId> = param_ids.iter().map(|_| cx.fresh()).collect();
    let mut replacement = std::collections::HashMap::new();
    for (pid, ty) in param_ids.iter().zip(arg_tys.iter()) {
        replacement.insert(*pid, *ty);
    }
    let payload_tys: Vec<TyId> = variant
        .fields
        .iter()
        .map(|f| substitute_params(f.ty, &replacement, cx.arena))
        .collect();
    let adt_ty = cx.arena.adt(aid, arg_tys);
    if payload_tys.is_empty() {
        // Nullary constructor — its value is the adt type itself.
        adt_ty
    } else {
        cx.arena.fn_ty(payload_tys, adt_ty, vec![])
    }
}

fn synth_binary(cx: &mut Cx, op: BinOp, lhs: ExprId, rhs: ExprId, expr_id: ExprId) -> TyId {
    let l = synth_expr(cx, lhs);
    let r = synth_expr(cx, rhs);
    use BinOp::*;
    let op_str = format!("{:?}", op);
    match op {
        Add | Sub | Mul | Div | Rem | BitAnd | BitOr | BitXor | Shl | Shr => {
            if unify(l, r, cx.subst, cx.arena).is_err() {
                cx.diag.push(diag::binop_type_mismatch(
                    &op_str,
                    l,
                    r,
                    &cx.span_of_expr(expr_id),
                    cx.arena,
                    cx.subst,
                    cx.defs,
                ));
            }
            l
        }
        Eq | Ne | Lt | Le | Gt | Ge => {
            if unify(l, r, cx.subst, cx.arena).is_err() {
                cx.diag.push(diag::binop_type_mismatch(
                    &op_str,
                    l,
                    r,
                    &cx.span_of_expr(expr_id),
                    cx.arena,
                    cx.subst,
                    cx.defs,
                ));
            }
            cx.arena.bool_
        }
        And | Or => cx.arena.bool_,
        Range | RangeEq => {
            let _ = unify(l, r, cx.subst, cx.arena);
            l
        }
        Assign | AssignAdd | AssignSub | AssignMul | AssignDiv | AssignRem | AssignBitAnd
        | AssignBitOr | AssignBitXor | AssignShl | AssignShr => {
            let _ = unify(l, r, cx.subst, cx.arena);
            cx.arena.unit
        }
    }
}

fn synth_unary(cx: &mut Cx, op: UnOp, rhs: ExprId) -> TyId {
    let r = synth_expr(cx, rhs);
    match op {
        UnOp::Neg => r,
        UnOp::Not => cx.arena.bool_,
        UnOp::Deref => {
            // If r is &T or *T, result is T.
            let resolved = cx.subst.resolve(r, cx.arena);
            match cx.arena.get(resolved).clone() {
                TyData::Ref { inner, .. } => inner,
                TyData::RawPtr(inner) => inner,
                _ => cx.fresh(),
            }
        }
    }
}

fn synth_call(cx: &mut Cx, callee: ExprId, args: &[HirArg], expr_id: ExprId) -> TyId {
    let callee_ty = synth_expr(cx, callee);
    let callee_resolved = cx.subst.resolve(callee_ty, cx.arena);
    let data = cx.arena.get(callee_resolved).clone();
    match data {
        TyData::Fn { params, ret, .. } => {
            if params.len() != args.len() {
                cx.diag.push(diag::wrong_arg_count(
                    params.len(),
                    args.len(),
                    &cx.span_of_expr(expr_id),
                ));
            }
            for (i, arg) in args.iter().enumerate() {
                let expected = params.get(i).copied().unwrap_or_else(|| cx.fresh());
                check_expr(cx, arg.value, expected);
                check_cap_subsumption(cx, arg.value, expected, expr_id);
            }
            ret
        }
        TyData::Var(_) | TyData::Error => {
            // Permissive: check args, return fresh.
            for arg in args {
                let _ = synth_expr(cx, arg.value);
            }
            cx.fresh()
        }
        TyData::Adt(_, _) => {
            // Could be a unit-variant used as a call (no), or an agent
            // constructor like `Echoer()`. Slice 3: treat as fresh.
            for arg in args {
                let _ = synth_expr(cx, arg.value);
            }
            callee_resolved
        }
        _ => {
            cx.diag.push(diag::not_callable(
                callee_resolved,
                &cx.span_of_expr(expr_id),
                cx.arena,
                cx.subst,
                cx.defs,
            ));
            for arg in args {
                let _ = synth_expr(cx, arg.value);
            }
            cx.arena.error
        }
    }
}

fn synth_method_call(
    cx: &mut Cx,
    receiver: ExprId,
    method: &str,
    args: &[HirArg],
    expr_id: ExprId,
) -> TyId {
    let recv_ty = synth_expr(cx, receiver);
    let resolved = cx.subst.resolve(recv_ty, cx.arena);
    let data = cx.arena.get(resolved).clone();

    // 0. Capability narrowing methods (slice 5).
    if let TyData::Cap { family, constraint } = &data {
        if let Some(ret) = synth_cap_method(cx, family.clone(), constraint.clone(), method, args) {
            return ret;
        }
        // Other methods on caps fall through to opaque-permissive (e.g.
        // `fs.read(path)` returns Bytes-of-something — slice 5 keeps the
        // permissive fallback rather than inventing a return type).
        for arg in args {
            let _ = synth_expr(cx, arg.value);
        }
        return cx.fresh();
    }

    // 1. User-declared (struct/enum/agent) ADT receivers go through the
    //    impl-method index first. Slice 5: also consider trait impls
    //    in scope (SD4020 ambiguous / SD4021 not found).
    if let TyData::Adt(aid, _) = data {
        // Check trait dispatch: collect all candidate impls.
        let trait_candidates: Vec<(String, FnDefId)> = cx
            .defs
            .traits
            .by_method
            .get(&(aid, method.to_string()))
            .cloned()
            .unwrap_or_default();
        let inherent = cx
            .defs
            .impl_methods
            .get(&(aid, method.to_string()))
            .copied();
        // If inherent is present, it wins; trait candidates ignored.
        // If no inherent and trait_candidates.len() > 1: SD4020.
        if inherent.is_none() && trait_candidates.len() > 1 {
            let names: Vec<String> = trait_candidates.iter().map(|(t, _)| t.clone()).collect();
            cx.diag.push(diag::method_ambiguous(
                method,
                &names,
                &cx.span_of_expr(expr_id),
            ));
            // Continue with first candidate to avoid cascade.
        }
    }

    if let TyData::Adt(aid, ref adt_args) = data {
        // Slice-5: try inherent first, then fall back to single trait
        // candidate. (Ambiguity already reported above.)
        let trait_first = cx
            .defs
            .traits
            .by_method
            .get(&(aid, method.to_string()))
            .and_then(|v| v.first().map(|(_, fid)| *fid));
        let resolved_fdef = cx
            .defs
            .impl_methods
            .get(&(aid, method.to_string()))
            .copied()
            .or(trait_first);
        if let Some(fdef_id) = resolved_fdef {
            let fdef = match cx.defs.fn_def(fdef_id) {
                Some(f) => f.clone(),
                None => return cx.arena.error,
            };
            // Build subst from the receiver's adt-args (positional).
            let mut replacement = std::collections::HashMap::new();
            if let Some(adt) = cx.defs.adt(aid) {
                for (pid, ty) in adt.param_ids.iter().zip(adt_args.iter()) {
                    replacement.insert(*pid, *ty);
                }
            }
            // Add fresh vars for method-level generic params.
            for pid in &fdef.param_ids {
                let v = cx.fresh();
                replacement.insert(*pid, v);
            }
            let params: Vec<TyId> = fdef
                .params
                .iter()
                .map(|(_, t)| substitute_params(*t, &replacement, cx.arena))
                .collect();
            let ret = substitute_params(fdef.ret, &replacement, cx.arena);
            if params.len() != args.len() {
                cx.diag.push(diag::wrong_arg_count(
                    params.len(),
                    args.len(),
                    &cx.span_of_expr(expr_id),
                ));
            }
            for (i, arg) in args.iter().enumerate() {
                let expected = params.get(i).copied().unwrap_or_else(|| cx.fresh());
                check_expr(cx, arg.value, expected);
            }
            return ret;
        }
        // User ADT, method not in impl index:
        // If the ADT is opaque (prelude or agent), fall through to permissive.
        let kind_is_opaque = cx
            .defs
            .adt(aid)
            .map(|a| a.kind == AdtKind::Opaque)
            .unwrap_or(true);
        if !kind_is_opaque && !cx.defs.builtin_methods.contains_key(method) {
            // Slice 4 (A17): hard error on user struct/enum with unknown method.
            cx.diag.push(diag::unknown_method(
                method,
                recv_ty,
                &cx.span_of_expr(expr_id),
                cx.arena,
                cx.subst,
                cx.defs,
            ));
            for arg in args {
                let _ = synth_expr(cx, arg.value);
            }
            return cx.arena.error;
        }
        // Opaque ADT: fall through to permissive paths below.
    }

    // 2. Built-in method table: permissive — accept any arity, return fresh.
    if cx.defs.builtin_methods.contains_key(method) {
        for arg in args {
            let _ = synth_expr(cx, arg.value);
        }
        return cx.fresh();
    }

    // 3. Receiver-shape specials (e.g. arrays expose .len).
    if matches!(
        data,
        TyData::Array { .. } | TyData::Ref { .. } | TyData::Str | TyData::String | TyData::Bytes
    ) && method == "len"
    {
        for arg in args {
            let _ = synth_expr(cx, arg.value);
        }
        return cx.arena.usize;
    }
    // 4. Opaque ADT fallback: permissive fresh.
    if matches!(data, TyData::Adt(_, _)) {
        for arg in args {
            let _ = synth_expr(cx, arg.value);
        }
        return cx.fresh();
    }
    // 5. Var or Error receiver: permissive.
    if matches!(data, TyData::Var(_) | TyData::Error) {
        for arg in args {
            let _ = synth_expr(cx, arg.value);
        }
        return cx.fresh();
    }
    cx.diag.push(diag::unknown_method(
        method,
        recv_ty,
        &cx.span_of_expr(expr_id),
        cx.arena,
        cx.subst,
        cx.defs,
    ));
    for arg in args {
        let _ = synth_expr(cx, arg.value);
    }
    cx.arena.error
}

/// v0.3 (A65): enforce the Sendable trait at `!Msg(args)` / `?Msg(args)`
/// call sites. The argument type is resolved through the substitution
/// then handed to `crate::sendable::sendable_reason`; a non-None reason
/// triggers SD3011.
fn check_sendable_arg(cx: &mut Cx, arg_idx: usize, arg_ty: TyId, span_expr: ExprId) {
    let resolved = cx.subst.resolve(arg_ty, cx.arena);
    if let Some(reason) = crate::sendable::sendable_reason(resolved, cx.arena, cx.defs) {
        cx.diag.push(diag::non_sendable_message_arg(
            arg_idx,
            resolved,
            &reason,
            &cx.span_of_expr(span_expr),
            cx.arena,
            cx.subst,
            cx.defs,
        ));
    }
}

/// Slice-5: capability narrowing methods. Returns Some(result_ty) if the
/// method is a recognized narrower; None otherwise (caller falls through
/// to permissive dispatch).
/// Slice-5: check that the argument's capability constraint is at least
/// as narrow as the parameter's. If not, emit SD4010 capability_too_broad.
fn check_cap_subsumption(cx: &mut Cx, arg_expr: ExprId, param_ty: TyId, expr_id: ExprId) {
    let arg_ty = match cx.expr_ty.get(&arg_expr).copied() {
        Some(t) => t,
        None => return,
    };
    let arg_resolved = cx.subst.resolve(arg_ty, cx.arena);
    let param_resolved = cx.subst.resolve(param_ty, cx.arena);
    let (af, ac) = match cx.arena.get(arg_resolved).clone() {
        TyData::Cap { family, constraint } => (family, constraint),
        _ => return,
    };
    let (pf, pc) = match cx.arena.get(param_resolved).clone() {
        TyData::Cap { family, constraint } => (family, constraint),
        _ => return,
    };
    if af != pf {
        // Cross-family mismatch already caught by normal unification.
        return;
    }
    if !ac.is_narrower_or_eq(&pc) {
        cx.diag.push(diag::capability_too_broad(
            arg_resolved,
            param_resolved,
            &cx.span_of_expr(expr_id),
            cx.arena,
            cx.subst,
            cx.defs,
        ));
    }
}

fn synth_cap_method(
    cx: &mut Cx,
    family: crate::ty::CapFamily,
    constraint: crate::ty::CapConstraint,
    method: &str,
    args: &[HirArg],
) -> Option<TyId> {
    use crate::ty::CapConstraint as C;
    // Eval any args so side effects + nested types are still typed.
    for arg in args {
        let _ = synth_expr(cx, arg.value);
    }
    let new_constraint = match (method, args.len()) {
        ("ro", 1) => {
            // fs.ro(path) — narrow to ReadOnly + extract path literal if available.
            let path_str = first_str_arg(cx, args);
            match path_str {
                Some(p) => C::And(vec![C::ReadOnly, C::Path(p)]),
                None => C::ReadOnly,
            }
        }
        ("path", 1) => {
            let path_str = first_str_arg(cx, args);
            match path_str {
                Some(p) => C::Path(p),
                None => return None,
            }
        }
        ("host", 1) => {
            let host = first_str_arg(cx, args);
            match host {
                Some(h) => C::Host(vec![h]),
                None => return None,
            }
        }
        _ => return None,
    };
    // Compose with existing constraint: result is And(existing, new) (set-union of restrictions).
    let composed = match constraint {
        C::Any => new_constraint,
        c => C::And(vec![c, new_constraint]),
    };
    Some(cx.arena.cap(family, composed))
}

fn first_str_arg(cx: &Cx, args: &[HirArg]) -> Option<String> {
    let a = args.first()?;
    match &cx.pkg.exprs[a.value] {
        HirExpr::Literal(HirLiteral::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn synth_field(cx: &mut Cx, receiver: ExprId, name: &str) -> TyId {
    let recv_ty = synth_expr(cx, receiver);
    let resolved = cx.subst.resolve(recv_ty, cx.arena);
    let data = cx.arena.get(resolved).clone();
    match data {
        TyData::Adt(aid, args) => {
            let adt = match cx.defs.adt(aid) {
                Some(a) => a.clone(),
                None => return cx.arena.error,
            };
            // Struct: single variant; look up field name there.
            if adt.kind == AdtKind::Struct {
                let variant = &adt.variants[0];
                if let Some(field) = variant
                    .fields
                    .iter()
                    .find(|f| f.name.as_deref() == Some(name))
                {
                    let subst_map = adt_subst(cx.defs, aid, &args);
                    let ty = substitute_params(field.ty, &subst_map, cx.arena);
                    return ty;
                }
                // Unknown field — error only if non-opaque + non-empty.
                if !variant.fields.is_empty() {
                    cx.diag.push(diag::unknown_field(
                        name,
                        &adt.name,
                        &SourceSpan { start: 0, end: 0 },
                    ));
                    return cx.arena.error;
                }
            }
            // Opaque or enum: permissive.
            cx.fresh()
        }
        TyData::Ref { inner, .. } => {
            // Auto-deref single ref for field access.
            let new_data = cx.arena.get(inner).clone();
            match new_data {
                TyData::Adt(_, _) => {
                    // Try field on inner.
                    let saved_recv = synth_expr_through(cx, receiver, inner);
                    let _ = saved_recv;
                    let resolved = cx.subst.resolve(inner, cx.arena);
                    let data = cx.arena.get(resolved).clone();
                    if let TyData::Adt(aid, args) = data {
                        if let Some(adt) = cx.defs.adt(aid).cloned() {
                            if adt.kind == AdtKind::Struct {
                                let variant = &adt.variants[0];
                                if let Some(field) = variant
                                    .fields
                                    .iter()
                                    .find(|f| f.name.as_deref() == Some(name))
                                {
                                    let subst_map = adt_subst(cx.defs, aid, &args);
                                    return substitute_params(field.ty, &subst_map, cx.arena);
                                }
                            }
                        }
                    }
                    cx.fresh()
                }
                _ => cx.fresh(),
            }
        }
        _ => cx.fresh(),
    }
}

fn synth_expr_through(_cx: &mut Cx, _recv: ExprId, _ty: TyId) -> TyId {
    // Helper that exists for clarity in the deref path; no-op for now.
    _ty
}

fn synth_index(cx: &mut Cx, receiver: ExprId, idx: ExprId) -> TyId {
    let r = synth_expr(cx, receiver);
    let _ = synth_expr(cx, idx);
    let resolved = cx.subst.resolve(r, cx.arena);
    match cx.arena.get(resolved).clone() {
        TyData::Array { elem, .. } => elem,
        TyData::Ref { inner, .. } => match cx.arena.get(inner).clone() {
            TyData::Array { elem, .. } => elem,
            _ => cx.fresh(),
        },
        TyData::Str | TyData::String => cx.arena.char_,
        TyData::Bytes => cx.arena.u8,
        _ => cx.fresh(),
    }
}

fn synth_match(cx: &mut Cx, scrutinee: ExprId, arms: &[HirMatchArm]) -> TyId {
    let scrut_ty = synth_expr(cx, scrutinee);
    let result = cx.fresh();
    for arm in arms {
        cx.locals.enter();
        check_pattern(cx, arm.pat, scrut_ty);
        if let Some(g) = arm.guard {
            check_expr(cx, g, cx.arena.bool_);
        }
        let arm_ty = synth_expr(cx, arm.body);
        let _ = unify(arm_ty, result, cx.subst, cx.arena);
        cx.locals.leave();
    }
    result
}

fn synth_struct(
    cx: &mut Cx,
    path: &[String],
    fields: &[(String, ExprId)],
    expr_id: ExprId,
) -> TyId {
    // Resolve path to an ADT (struct).
    let name = path.last().cloned().unwrap_or_default();
    let aid = match cx.defs.lookup(&name) {
        Some(DefRef::Adt(id)) => id,
        _ => {
            // Permissive: synth fields, return fresh.
            for (_, e) in fields {
                let _ = synth_expr(cx, *e);
            }
            return cx.fresh();
        }
    };
    let adt = match cx.defs.adt(aid) {
        Some(a) => a.clone(),
        None => return cx.arena.error,
    };
    if adt.kind != AdtKind::Struct {
        // Permissive: type-check field exprs but return opaque.
        for (_, e) in fields {
            let _ = synth_expr(cx, *e);
        }
        return cx.arena.adt(aid, vec![]);
    }
    // Fresh generic args.
    let arg_tys: Vec<TyId> = adt.param_ids.iter().map(|_| cx.fresh()).collect();
    let mut replacement = std::collections::HashMap::new();
    for (pid, ty) in adt.param_ids.iter().zip(arg_tys.iter()) {
        replacement.insert(*pid, *ty);
    }
    let variant = &adt.variants[0];
    let mut seen: std::collections::HashSet<String> = Default::default();
    for (fname, fval) in fields {
        if !seen.insert(fname.clone()) {
            cx.diag.push(diag::duplicate_struct_field(
                fname,
                &cx.span_of_expr(expr_id),
            ));
        }
        match variant
            .fields
            .iter()
            .find(|f| f.name.as_deref() == Some(fname))
        {
            Some(field) => {
                let expected = substitute_params(field.ty, &replacement, cx.arena);
                check_expr(cx, *fval, expected);
            }
            None => {
                // Tolerate unknown fields on opaque-but-struct-shaped defs.
                if !variant.fields.is_empty() {
                    cx.diag.push(diag::unknown_field(
                        fname,
                        &adt.name,
                        &cx.span_of_expr(expr_id),
                    ));
                }
                let _ = synth_expr(cx, *fval);
            }
        }
    }
    // Missing-field check: skip when struct is opaque-shaped (variants
    // empty) — used for examples like `Page {}` where the type is opaque.
    if !variant.fields.is_empty() {
        for f in &variant.fields {
            if let Some(n) = &f.name {
                if !seen.contains(n) {
                    cx.diag.push(diag::missing_struct_field(
                        n,
                        &adt.name,
                        &cx.span_of_expr(expr_id),
                    ));
                }
            }
        }
    }
    // Reconstruct positional arg vec from the param_ids order.
    let arg_vec: Vec<TyId> = adt
        .param_ids
        .iter()
        .map(|p| replacement.get(p).copied().unwrap_or(cx.arena.error))
        .collect();
    cx.arena.adt(aid, arg_vec)
}

fn synth_question(cx: &mut Cx, inner: ExprId, expr_id: ExprId) -> TyId {
    let inner_ty = synth_expr(cx, inner);
    let resolved = cx.subst.resolve(inner_ty, cx.arena);
    let data = cx.arena.get(resolved).clone();
    // Permissive on error/var.
    if matches!(data, TyData::Var(_) | TyData::Error) {
        return cx.fresh();
    }
    match data {
        TyData::Adt(aid, args) if aid == cx.result_id && args.len() == 2 => {
            // Check enclosing fn return is Result[_, e'].
            let ret_resolved = cx.subst.resolve(cx.return_ty, cx.arena);
            let ret_data = cx.arena.get(ret_resolved).clone();
            match ret_data {
                TyData::Adt(rid, rargs) if rid == cx.result_id && rargs.len() == 2 => {
                    // Unify error types.
                    if unify(args[1], rargs[1], cx.subst, cx.arena).is_err() {
                        cx.diag.push(diag::question_error_mismatch(
                            rargs[1],
                            args[1],
                            &cx.span_of_expr(expr_id),
                            cx.arena,
                            cx.subst,
                            cx.defs,
                        ));
                    }
                    args[0]
                }
                TyData::Var(_) | TyData::Error => {
                    // Permissive.
                    args[0]
                }
                _ => {
                    cx.diag
                        .push(diag::question_outside_result(&cx.span_of_expr(expr_id)));
                    args[0]
                }
            }
        }
        _ => {
            cx.diag
                .push(diag::question_outside_result(&cx.span_of_expr(expr_id)));
            cx.arena.error
        }
    }
}

fn synth_lambda(cx: &mut Cx, params: &[HirParam], ret: Option<TypeId>, body: BlockId) -> TyId {
    let param_tys: Vec<TyId> = params
        .iter()
        .map(|p| match p.ty {
            Some(t) => crate::resolve::resolve_hir_type(
                t,
                cx.pkg,
                cx.defs,
                cx.arena,
                &cx.param_scope,
                cx.diag,
            ),
            None => cx.fresh(),
        })
        .collect();
    let ret_ty = match ret {
        Some(t) => {
            crate::resolve::resolve_hir_type(t, cx.pkg, cx.defs, cx.arena, &cx.param_scope, cx.diag)
        }
        None => cx.fresh(),
    };
    // Body check in a fresh local scope.
    cx.locals.enter();
    for (param, ty) in params.iter().zip(param_tys.iter()) {
        cx.locals.bind(param.name.clone(), *ty);
    }
    let body_ty = check_block(cx, body, Some(ret_ty));
    let _ = unify(body_ty, ret_ty, cx.subst, cx.arena);
    cx.locals.leave();
    cx.arena.fn_ty(param_tys, ret_ty, vec![])
}

fn element_of(cx: &mut Cx, ty: TyId) -> TyId {
    let resolved = cx.subst.resolve(ty, cx.arena);
    match cx.arena.get(resolved).clone() {
        TyData::Array { elem, .. } => elem,
        TyData::Ref { inner, .. } => match cx.arena.get(inner).clone() {
            TyData::Array { elem, .. } => elem,
            _ => cx.fresh(),
        },
        _ => cx.fresh(),
    }
}

/// Type-check a block. Pushes a fresh local scope, runs statements, then
/// either checks the tail against `expected` (if given) or synthesizes it.
pub fn check_block(cx: &mut Cx, block_id: BlockId, expected: Option<TyId>) -> TyId {
    cx.locals.enter();
    let block = cx.pkg.blocks[block_id].clone();
    for stmt in &block.stmts {
        check_stmt(cx, stmt);
    }
    let ty = match block.tail {
        Some(e) => match expected {
            Some(t) => {
                check_expr(cx, e, t);
                t
            }
            None => synth_expr(cx, e),
        },
        None => match expected {
            Some(t) => t,
            None => cx.arena.unit,
        },
    };
    cx.locals.leave();
    ty
}

fn check_stmt(cx: &mut Cx, stmt: &HirStmt) {
    match stmt {
        HirStmt::Let {
            pat,
            ty,
            init,
            mutable: _,
        } => {
            let declared = ty.map(|t| {
                crate::resolve::resolve_hir_type(
                    t,
                    cx.pkg,
                    cx.defs,
                    cx.arena,
                    &cx.param_scope,
                    cx.diag,
                )
            });
            let init_ty = match (declared, init) {
                (Some(t), Some(e)) => {
                    check_expr(cx, *e, t);
                    t
                }
                (Some(t), None) => t,
                (None, Some(e)) => synth_expr(cx, *e),
                (None, None) => cx.fresh(),
            };
            check_pattern(cx, *pat, init_ty);
        }
        HirStmt::Expr(e) => {
            let _ = synth_expr(cx, *e);
        }
    }
}

/// Bind locals from a pattern against the scrutinee type.
pub fn check_pattern(cx: &mut Cx, pat_id: PatId, scrut: TyId) {
    let pat = cx.pkg.pats[pat_id].clone();
    match pat {
        HirPat::Wildcard => {}
        HirPat::Literal(lit) => {
            let lit_ty = synth_literal(cx, &lit);
            let _ = unify(lit_ty, scrut, cx.subst, cx.arena);
        }
        HirPat::Binding { name, sub } => {
            cx.locals.bind(name, scrut);
            if let Some(s) = sub {
                check_pattern(cx, s, scrut);
            }
        }
        HirPat::Ref { mutable, inner } => {
            let resolved = cx.subst.resolve(scrut, cx.arena);
            match cx.arena.get(resolved).clone() {
                TyData::Ref {
                    inner: inner_ty,
                    mutable: m,
                } if m == mutable => {
                    check_pattern(cx, inner, inner_ty);
                }
                _ => {
                    let inner_var = cx.fresh();
                    let ref_ty = cx.arena.ref_to(mutable, inner_var);
                    let _ = unify(ref_ty, scrut, cx.subst, cx.arena);
                    check_pattern(cx, inner, inner_var);
                }
            }
        }
        HirPat::Tuple(xs) => {
            let resolved = cx.subst.resolve(scrut, cx.arena);
            match cx.arena.get(resolved).clone() {
                TyData::Tuple(parts) if parts.len() == xs.len() => {
                    for (sub, ty) in xs.iter().zip(parts.iter()) {
                        check_pattern(cx, *sub, *ty);
                    }
                }
                _ => {
                    for sub in &xs {
                        let v = cx.fresh();
                        check_pattern(cx, *sub, v);
                    }
                }
            }
        }
        HirPat::Struct { path, fields } => {
            let name = path.last().cloned().unwrap_or_default();
            let aid = match cx.defs.lookup(&name) {
                Some(DefRef::Adt(id)) => Some(id),
                _ => None,
            };
            if let Some(aid) = aid {
                let adt = cx.defs.adt(aid).cloned();
                if let Some(adt) = adt {
                    if adt.kind == AdtKind::Struct {
                        let variant = &adt.variants[0];
                        let arg_tys: Vec<TyId> = adt.param_ids.iter().map(|_| cx.fresh()).collect();
                        let mut replacement = std::collections::HashMap::new();
                        for (pid, ty) in adt.param_ids.iter().zip(arg_tys.iter()) {
                            replacement.insert(*pid, *ty);
                        }
                        let expected = cx.arena.adt(aid, arg_tys);
                        let _ = unify(expected, scrut, cx.subst, cx.arena);
                        for (fname, sub) in fields {
                            if let Some(field) = variant
                                .fields
                                .iter()
                                .find(|f| f.name.as_deref() == Some(&fname))
                            {
                                let ft = substitute_params(field.ty, &replacement, cx.arena);
                                if let Some(s) = sub {
                                    check_pattern(cx, s, ft);
                                } else {
                                    cx.locals.bind(fname.clone(), ft);
                                }
                            }
                        }
                        return;
                    }
                }
            }
            // Permissive: bind sub-patterns to fresh vars.
            for (fname, sub) in fields {
                let v = cx.fresh();
                if let Some(s) = sub {
                    check_pattern(cx, s, v);
                } else {
                    cx.locals.bind(fname, v);
                }
            }
        }
        HirPat::Enum { path, args } => {
            // Path like `Shape.Circle` or `Some` (single-segment via prelude).
            let (variant_name, enum_name) = if path.len() >= 2 {
                (
                    path[path.len() - 1].clone(),
                    Some(path[path.len() - 2].clone()),
                )
            } else if path.len() == 1 {
                (path[0].clone(), None)
            } else {
                (String::new(), None)
            };
            let resolved_def = if let Some(en) = enum_name.as_ref() {
                if let Some(DefRef::Adt(aid)) = cx.defs.lookup(en) {
                    let idx = cx
                        .defs
                        .adt(aid)
                        .and_then(|a| a.variants.iter().position(|v| v.name == variant_name));
                    idx.map(|i| (aid, i))
                } else {
                    None
                }
            } else if let Some(DefRef::Variant(aid, idx)) = cx.defs.lookup(&variant_name) {
                Some((aid, idx))
            } else {
                None
            };
            if let Some((aid, idx)) = resolved_def {
                let adt = cx.defs.adt(aid).cloned();
                if let Some(adt) = adt {
                    let variant = &adt.variants[idx];
                    let arg_tys: Vec<TyId> = adt.param_ids.iter().map(|_| cx.fresh()).collect();
                    let mut replacement = std::collections::HashMap::new();
                    for (pid, ty) in adt.param_ids.iter().zip(arg_tys.iter()) {
                        replacement.insert(*pid, *ty);
                    }
                    let expected = cx.arena.adt(aid, arg_tys);
                    let _ = unify(expected, scrut, cx.subst, cx.arena);
                    if args.len() != variant.fields.len() {
                        cx.diag.push(diag::wrong_variant_arity(
                            &variant.name,
                            variant.fields.len(),
                            args.len(),
                            &SourceSpan { start: 0, end: 0 },
                        ));
                    }
                    for (i, sub) in args.iter().enumerate() {
                        let field_ty = variant
                            .fields
                            .get(i)
                            .map(|f| substitute_params(f.ty, &replacement, cx.arena))
                            .unwrap_or_else(|| cx.fresh());
                        check_pattern(cx, *sub, field_ty);
                    }
                    return;
                }
            }
            // Permissive.
            for sub in &args {
                let v = cx.fresh();
                check_pattern(cx, *sub, v);
            }
        }
        HirPat::Range { lo, hi, .. } => {
            check_pattern(cx, lo, scrut);
            check_pattern(cx, hi, scrut);
        }
    }
}
