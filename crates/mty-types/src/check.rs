//! Bidirectional expression / statement checking.

use crate::defs::*;
use crate::diag;
use crate::infer::*;
use crate::resolve::ParamScope;
use crate::ty::*;
use mty_diagnostics::Diagnostic;
use mty_hir::*;
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
/// (where any unresolved name promotes to MT2021).
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
/// | AgentBody   | **Strict** — MT2021 on unknown        |
/// | HandlerBody | **Strict** — MT2021 on unknown        |
/// | SupervisorBody | **Strict** — MT2021 on unknown     |
/// | CapNarrowBody | **Strict** — MT2021 on unknown      |
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
    /// True iff unresolved names should hard-error (MT2021) instead of
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

    /// Short human-readable label for the MT2021 strict-mode note.
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
    /// set emit MT2021 when the current scope is **strict** (see
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
    /// v0.37 Track T3 — FFI call-site coercion sinks. The call-site
    /// checker inserts `arg.value` exprIds here when the corresponding
    /// extern-c param requires a `Str → *U8` or `&local → *T` coercion.
    /// The IR lowerer reads these to emit the right shape (load Str
    /// ptr-half, take place address) instead of the default copy.
    pub coerce_str_to_ptr: &'a mut std::collections::HashSet<ExprId>,
    pub coerce_addr_of: &'a mut std::collections::HashSet<ExprId>,
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
            // v0.12 (Gap B / MT2025 emit-site): `&literal` / `&(a + b)` /
            // `&fn_call()` are not place expressions. Pre-v0.12 the
            // synth path silently typed these as `&T`. We now fire
            // MT2025 for the non-place shapes while keeping the legacy
            // type so the rest of the body still type-checks.
            if !is_place_expr(&cx.pkg.exprs[inner]) {
                cx.diag
                    .push(diag::cannot_take_ref(&cx.span_of_expr(expr_id)));
            }
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
                        // v0.22 Coverage Closure (MT2018 emit-site): the
                        // two branches of an `if/else` produce
                        // incompatible types. Pre-v0.22 this funnelled
                        // through MT2001 (generic type mismatch); we now
                        // surface MT2018 at the `if`-expression span so
                        // `mty explain MT2018` text points at the join
                        // site.
                        cx.diag.push(diag::if_branch_mismatch(
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
        HirExpr::WhileLet {
            pat,
            scrutinee,
            body,
        } => {
            // v0.29 Track D: `while let pat = scrutinee { body }`.
            // Mirrors `if let` typing: synth the scrutinee, check the
            // pattern against that type to introduce its bindings, then
            // check the body. The whole expression has type `unit`
            // (just like plain `while`).
            let scrut_ty = synth_expr(cx, scrutinee);
            cx.locals.enter();
            check_pattern(cx, pat, scrut_ty);
            let _ = check_block(cx, body, None);
            cx.locals.leave();
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
        HirExpr::Break(inner) => {
            // `break <value>` contributes the value's type to the enclosing
            // `loop`'s result type. v0.5 doesn't yet unify those across
            // breaks (loops still synth to `never`), but we still synth the
            // inner expression so its type info lands in the side table.
            if let Some(e) = inner {
                let _ = synth_expr(cx, e);
            }
            cx.arena.never
        }
        HirExpr::Continue => cx.arena.never,
        HirExpr::Struct { path, fields } => synth_struct(cx, &path, &fields, expr_id),
        HirExpr::Map(_) => {
            // Slice 3: Map literals (e.g. `Map::[Str, Json]{}`) check the
            // entries but yield an opaque Map type or fresh var.
            cx.fresh()
        }
        HirExpr::Send { target, msg, args } => {
            let target_ty = synth_expr(cx, target);
            for (i, a) in args.iter().enumerate() {
                let ty = synth_expr(cx, a.value);
                check_sendable_arg(cx, i, ty, expr_id);
            }
            // v0.29 Track C: lower the bang-send result type to the
            // protocol message's declared reply (or Unit if undeclared).
            // Pre-v0.29 this returned `cx.arena.unit` unconditionally,
            // forcing call sites like `let r: Str = agent ! Review(s)`
            // to either force a type error or thread the reply through
            // a `format!` stand-in (see v0.27 demo 08 workaround).
            resolve_message_reply_ty(cx, target_ty, &msg, &args, expr_id)
        }
        HirExpr::Ask { target, msg, args } => {
            let target_ty = synth_expr(cx, target);
            for (i, a) in args.iter().enumerate() {
                let ty = synth_expr(cx, a.value);
                check_sendable_arg(cx, i, ty, expr_id);
            }
            // v0.29 Track C: same lowering as `Send` — `?Msg(args)` and
            // `!Msg(args)` both surface the protocol's declared reply.
            // Pre-v0.29 `Ask` synthesised a fresh inference var, which
            // unified with anything but never pinned to the declared
            // reply at the call site.
            resolve_message_reply_ty(cx, target_ty, &msg, &args, expr_id)
        }
        HirExpr::Deadline { inner, dur } => {
            let _ = synth_expr(cx, dur);
            synth_expr(cx, inner)
        }
        HirExpr::Question(inner) => synth_question(cx, inner, expr_id),
        HirExpr::Spawn { inner, .. } => {
            // v0.29 Track C: when the spawned expression is a direct
            // call to an agent constructor (`spawn Foo(...)`), pin the
            // AgentRef's parameter to the agent's ADT so downstream
            // bang-send / ask sites can resolve the message reply type
            // via `agent_protocols`. Pre-v0.29 the call returned a
            // fresh inference var, leaving `AgentRef[?N]` opaque.
            let inner_expr = cx.pkg.exprs[inner].clone();
            let agent_adt: Option<AdtId> = match &inner_expr {
                HirExpr::Call { callee, args: _ } => match &cx.pkg.exprs[*callee] {
                    HirExpr::Path(segs) if segs.len() == 1 => match cx.defs.lookup(&segs[0]) {
                        Some(DefRef::Adt(aid)) if cx.defs.agent_protocols.contains_key(&aid) => {
                            Some(aid)
                        }
                        _ => None,
                    },
                    _ => None,
                },
                HirExpr::Path(segs) if segs.len() == 1 => match cx.defs.lookup(&segs[0]) {
                    Some(DefRef::Adt(aid)) if cx.defs.agent_protocols.contains_key(&aid) => {
                        Some(aid)
                    }
                    _ => None,
                },
                _ => None,
            };
            let t = synth_expr(cx, inner);
            let inner_ty = match agent_adt {
                Some(aid) => cx.arena.adt(aid, vec![]),
                None => t,
            };
            cx.arena.adt(cx.agent_ref_id, vec![inner_ty])
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
            // and MT2021-strict-mode will fire automatically — see
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
            let lhs_ty = synth_expr(cx, lhs);
            let target = crate::resolve::resolve_hir_type(
                ty,
                cx.pkg,
                cx.defs,
                cx.arena,
                &cx.param_scope,
                cx.diag,
            );
            // v0.37 T2 (MT2027 emit-site): `expr as Ty` is only valid for
            // a fixed set of scalar conversions. Anything else (e.g.
            // `Str as I32`, `Bool as Str`, tuple/array/Adt casts) has no
            // defined scalar lowering and would silently fall through to
            // `IrTy::Error` in the back-end, so we reject it here.
            // We deliberately keep `Error` / `Var` / `Param` permissive so
            // upstream errors don't cascade into MT2027.
            if !is_valid_cast(cx, lhs_ty, target) {
                cx.diag.push(diag::invalid_cast(
                    lhs_ty,
                    target,
                    &cx.span_of_expr(expr_id),
                    cx.arena,
                    cx.subst,
                    cx.defs,
                ));
            }
            target
        }
        HirExpr::Lambda { params, ret, body } => synth_lambda(cx, &params, ret, body),
        HirExpr::Error => cx.arena.error,
    }
}

/// v0.37 T2 (MT2027): is `src as dst` a recognised scalar conversion?
///
/// Accepted shapes:
///   - int  ↔ int   (widen / narrow / sign change)
///   - int  ↔ float (truncate / round)
///   - float↔ float (widen / narrow)
///   - bool → int   (false→0, true→1)
///   - char → int (codepoint), int → char (the back-end already permits
///     U8/U32 → Char in the cranelift coerce path; keep parity here)
///
/// Anything else (Str, Bytes, Tuple, Array, Adt, Ref, Fn) is rejected.
/// `Error`/`Var`/`Param` are *permitted* on either side so upstream
/// errors don't cascade.
fn is_valid_cast(cx: &mut Cx, src: TyId, dst: TyId) -> bool {
    let s = cx.subst.resolve(src, cx.arena);
    let d = cx.subst.resolve(dst, cx.arena);
    let sd = cx.arena.get(s).clone();
    let dd = cx.arena.get(d).clone();
    // Permissive on poisoned / unresolved sides so we don't cascade
    // MT2027 onto cases the user already knows about.
    if matches!(sd, TyData::Error | TyData::Var(_) | TyData::Param(_))
        || matches!(dd, TyData::Error | TyData::Var(_) | TyData::Param(_))
    {
        return true;
    }
    // Trivial identity: int as same-int / float as same-float.
    if s == d {
        return true;
    }
    let s_is_int = matches!(sd, TyData::Int(_));
    let s_is_float = matches!(sd, TyData::Float(_));
    let s_is_bool = matches!(sd, TyData::Bool);
    let s_is_char = matches!(sd, TyData::Char);
    let d_is_int = matches!(dd, TyData::Int(_));
    let d_is_float = matches!(dd, TyData::Float(_));
    let d_is_char = matches!(dd, TyData::Char);
    if (s_is_int || s_is_float) && (d_is_int || d_is_float) {
        return true;
    }
    if s_is_bool && d_is_int {
        return true;
    }
    if s_is_char && d_is_int {
        return true;
    }
    if s_is_int && d_is_char {
        return true;
    }
    false
}

/// v0.12 (Gap B / MT2025): true iff `e` is a "place" (l-value) — the only
/// shape where `&e` / `&mut e` is meaningful. Mirrors the borrow checker's
/// `expr_as_place` test on the syntactic side. Conservative: when in
/// doubt we treat as a place to avoid false-positive MT2025 firings.
fn is_place_expr(e: &HirExpr) -> bool {
    match e {
        HirExpr::Path(_)
        | HirExpr::PathGeneric { .. }
        | HirExpr::Field { .. }
        | HirExpr::Index { .. }
        // `&*r` is fine (reborrow).
        | HirExpr::Unary { op: UnOp::Deref, .. }
        // Borrow of a borrow / move is itself a place-ish wrapper.
        | HirExpr::Borrow { .. }
        | HirExpr::Move(_) => true,
        // Block/If/Match/IfLet can produce a place when the tail is a
        // place; conservatively treat as place to avoid false positives.
        HirExpr::Block(_)
        | HirExpr::If { .. }
        | HirExpr::IfLet { .. }
        | HirExpr::Match { .. } => true,
        // Literals / arithmetic / structural ctors / calls produce values,
        // not places.
        HirExpr::Literal(_)
        | HirExpr::Binary { .. }
        | HirExpr::Call { .. }
        | HirExpr::MethodCall { .. }
        | HirExpr::Tuple(_)
        | HirExpr::Array(_)
        | HirExpr::Struct { .. }
        | HirExpr::Map(_)
        | HirExpr::Lambda { .. }
        | HirExpr::HtmlTemplate(_)
        | HirExpr::Cast { .. } => false,
        // Everything else (Spawn, Send, Ask, Question, Loop, Break, ...)
        // either yields Unit/never or has non-place semantics; conservatively
        // treat as place to avoid false positives.
        _ => true,
    }
}

pub fn check_expr(cx: &mut Cx, expr_id: ExprId, expected: TyId) {
    // v0.12 (Gap B / MT2024 emit-site): if the expression is a lambda
    // and the expected type is a fn type with a different arity, emit a
    // precise MT2024 BEFORE falling through to the generic synth/unify
    // path (which would have surfaced this as MT2001 type-mismatch).
    if let HirExpr::Lambda { params, .. } = &cx.pkg.exprs[expr_id] {
        let exp_resolved = cx.subst.resolve(expected, cx.arena);
        if let TyData::Fn {
            params: exp_params, ..
        } = cx.arena.get(exp_resolved).clone()
        {
            if params.len() != exp_params.len() {
                cx.diag.push(diag::lambda_arity_mismatch(
                    exp_params.len(),
                    params.len(),
                    &cx.span_of_expr(expr_id),
                ));
            }
        }
    }
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
        // unresolved name to MT2021 via `unresolved_value_strict`.
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
        // v0.27 Track B (carve-out d): the name resolves to a handler-safe
        // std.* opaque ADT (e.g. `Working`, `VectorStore`, `AnthropicClient`).
        // Strict scope accepts the use, since the underlying ADT is
        // effect-bearing but those effects are already tracked through the
        // fn's `!{...}` clause. User-defined opaque ADTs are NOT in this
        // set and continue to hit MT2021. See
        // `dev/history/notes/OPAQUE_ADT_WASM_V0_27_NOTES.md`.
        if cx.defs.is_handler_safe_name(name) {
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
                // v0.12 (Gap B / MT2009 emit-site): the first segment
                // resolves to a known ADT (Enum kind only) but the
                // second segment is not one of its declared variants.
                // We skip Opaque ADTs (prelude shims like `SearchErr`
                // declared with no variants) because their constructor
                // surface is intentionally unknown to the type checker.
                if adt.kind == AdtKind::Enum {
                    let adt_name = adt.name.clone();
                    cx.diag.push(diag::unknown_variant(
                        vname,
                        &adt_name,
                        &cx.span_of_expr(expr_id),
                    ));
                    return cx.arena.error;
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
    // v0.27 Track B: handler-safe std.* opaque ADT chain (e.g.
    // `std.memory.Working.new()` — when the IDE / fmt drops `std.memory`
    // and leaves the chain rooted at `Working`). Same carve-out as the
    // single-segment case above.
    if cx.defs.is_handler_safe_name(first) {
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

/// v0.37 Track T3 — attempt one of the three FFI coercions at an
/// extern-c call site. Returns `true` iff a coercion fired (the caller
/// should skip the regular `check_expr` step). Returns `false` when no
/// coercion applies — the regular path takes over.
///
/// Coercion rules:
///
/// | Arg expr           | Arg type | Param type         | Effect                                  |
/// |--------------------|----------|--------------------|-----------------------------------------|
/// | (any)              | `Str`    | `*U8` (RawPtr U8)  | Record in `coerce_str_to_ptr` for IR.   |
/// | `&inner`           | (any)    | `*T`               | Synth `inner: T`; record `coerce_addr_of`. |
/// | `&mut inner`       | (any)    | `*T`               | Synth `inner: T`; record `coerce_addr_of`. |
///
/// In Mighty today there is only one raw-pointer flavour (`RawPtr(T)` —
/// the typed-side parser maps both `*T` and `*mut T` onto the same
/// `TyData::RawPtr(T)` because mutability lives on the *referent*'s
/// borrow node, not on the pointer type). That keeps the typeck side
/// simple: we accept both `&` and `&mut` borrow exprs in any `*T` slot,
/// and the borrow checker's mutable-vs-shared accounting still happens
/// on the inner `HirExpr::Borrow` node so the usual exclusive-borrow
/// rules apply.
fn try_extern_c_coercion(cx: &mut Cx, arg: ExprId, expected: TyId) -> bool {
    let expected_resolved = cx.subst.resolve(expected, cx.arena);
    let expected_data = cx.arena.get(expected_resolved).clone();
    // At the syntax level `*T` and `&T` share the `TYPE_BORROW` CST
    // node — slice-1's intentional simplification. Both resolve to
    // `TyData::Ref { .. }` at typeck time. We treat either flavour
    // as the FFI pointer slot here; raw-pointer-typed prelude builtins
    // (`raw_ptr`, `null`) use the legacy `TyData::RawPtr` path, so we
    // also accept that for forward-compat.
    let (TyData::Ref {
        inner: pointee_ty, ..
    }
    | TyData::RawPtr(pointee_ty)) = expected_data
    else {
        return false;
    };
    let pointee_resolved = cx.subst.resolve(pointee_ty, cx.arena);
    let pointee_data = cx.arena.get(pointee_resolved).clone();

    // Coercion 2/3: borrow expr at any *T slot.
    if let HirExpr::Borrow { inner, .. } = cx.pkg.exprs[arg].clone() {
        // Synth the inner; it should match the pointee type (or be
        // permissively compatible via unify). We don't hard-fail on
        // pointee mismatch — the regular path would have emitted a
        // diagnostic via `check_expr`, and the borrow site is rarely
        // the right place to surface a pointee mismatch.
        let inner_ty = synth_expr(cx, inner);
        let _ = crate::infer::unify(inner_ty, pointee_resolved, cx.subst, cx.arena);
        // Record the borrow expr itself in the address-of table so
        // IR lowering knows to skip the temp + Ref dance and lower
        // straight to a place address.
        cx.coerce_addr_of.insert(arg);
        // The outer expr's recorded type is the parameter (the FFI
        // `*T`), not the borrow's logical `&T` type, so downstream
        // SIR lowering sees a uniform pointer slot.
        cx.expr_ty.insert(arg, expected_resolved);
        return true;
    }

    // Coercion 1: Str → *U8.
    if matches!(pointee_data, TyData::Int(IntKind::U8)) {
        let arg_ty = synth_expr(cx, arg);
        let arg_resolved = cx.subst.resolve(arg_ty, cx.arena);
        if matches!(cx.arena.get(arg_resolved), TyData::Str) {
            cx.coerce_str_to_ptr.insert(arg);
            // Overwrite the arg's expr type to the param type so the
            // borrow checker / effect walker see a uniform `*U8` slot.
            cx.expr_ty.insert(arg, expected_resolved);
            return true;
        }
    }
    false
}

/// v0.37 Track T3 — does the call's callee expr resolve to an extern-c fn?
/// Returns `Some(true)` iff the callee path resolves to a `FnDef` whose
/// `extern_abi == Some("c")`. Returns `None` if we can't tell (the callee
/// isn't a plain path or doesn't resolve to a fn).
///
/// Only the simple single-segment-path shape is recognised, which covers
/// every `extern c { fn foo(...) }` call site today. Dotted-path / module-
/// qualified extern calls (`mod::foo()`) fall through to the regular
/// (non-coercing) path — those don't exist in current Mighty surface.
fn callee_is_extern_c(cx: &Cx, callee: ExprId) -> bool {
    let HirExpr::Path(segments) = &cx.pkg.exprs[callee] else {
        return false;
    };
    if segments.len() != 1 {
        return false;
    }
    let name = &segments[0];
    if let Some(DefRef::Fn(fdid)) = cx.defs.lookup(name) {
        if let Some(f) = cx.defs.fn_def(fdid) {
            return f.extern_abi.as_deref() == Some("c");
        }
    }
    false
}

fn synth_call(cx: &mut Cx, callee: ExprId, args: &[HirArg], expr_id: ExprId) -> TyId {
    // v0.37 T6 — variadic extern fns (`extern c fn printf(fmt, ...) -> I32;`)
    // need to accept any number of args beyond the declared prefix. The
    // checker only knows about variadicness via `FnDef.is_variadic`, so
    // we peek the callee expression: if it's a direct `Path` (or a
    // `PathGeneric`) resolving to a single `DefRef::Fn(fid)` whose
    // `FnDef.is_variadic` is `true`, run the variadic call path instead
    // of emitting MT2005 (WRONG_ARG_COUNT) for the extra args.
    let is_variadic_callee = match &cx.pkg.exprs[callee] {
        HirExpr::Path(segs) if segs.len() == 1 => cx
            .defs
            .lookup(&segs[0])
            .and_then(|d| match d {
                crate::defs::DefRef::Fn(fid) => cx.defs.fn_def(fid).map(|f| f.is_variadic),
                _ => None,
            })
            .unwrap_or(false),
        HirExpr::PathGeneric { segments, .. } if segments.len() == 1 => cx
            .defs
            .lookup(&segments[0])
            .and_then(|d| match d {
                crate::defs::DefRef::Fn(fid) => cx.defs.fn_def(fid).map(|f| f.is_variadic),
                _ => None,
            })
            .unwrap_or(false),
        _ => false,
    };
    let callee_ty = synth_expr(cx, callee);
    let callee_resolved = cx.subst.resolve(callee_ty, cx.arena);
    let data = cx.arena.get(callee_resolved).clone();
    // v0.37 Track T3 — detect extern-c callee for FFI coercion.
    let is_extern_c = callee_is_extern_c(cx, callee);
    match data {
        TyData::Fn { params, ret, .. } => {
            let fixed = params.len();
            // Arity check: variadic fns require at least the fixed-arity
            // prefix; non-variadic fns require exact match (legacy
            // MT2005 emit).
            let arity_ok = if is_variadic_callee {
                args.len() >= fixed
            } else {
                args.len() == fixed
            };
            if !arity_ok {
                cx.diag.push(diag::wrong_arg_count(
                    fixed,
                    args.len(),
                    &cx.span_of_expr(expr_id),
                ));
            }
            for (i, arg) in args.iter().enumerate() {
                let expected = params.get(i).copied().unwrap_or_else(|| cx.fresh());
                // v0.37 Track T3 — at extern-c call sites, allow three
                // coercions that the regular unifier rejects:
                //
                //   (1) Str → *U8 / *const U8       — pass ptr-half of the
                //                                     Mighty Str aggregate.
                //   (2) &local → *T                 — take address of place.
                //   (3) &mut local → *mut T         — same, mutable.
                //
                // For (2) and (3) the arg expr is a `HirExpr::Borrow {
                // mutable, inner }`. We synth the inner's type, check it
                // unifies with the pointee, and record the arg in
                // `coerce_addr_of` so IR lowering emits a place-address
                // load instead of an aggregate copy.
                //
                // For (1) we synth the arg's type, check it equals Str,
                // and record in `coerce_str_to_ptr`. The Str literal is
                // already null-terminated by `intern_string`, so no
                // separate "to C string" buffer is needed in the default
                // path. The `#[ffi_nul_ok]` faster-path attribute is a
                // v0.38 follow-up — the default in v0.37 is the safe
                // (null-terminated) layout.
                if is_extern_c && try_extern_c_coercion(cx, arg.value, expected) {
                    // Coercion succeeded — no further check_expr needed
                    // (we already validated the inner type matches the
                    // pointee). check_cap_subsumption still runs below
                    // for caps-in-extern-args, but FFI sites don't pass
                    // capability values today, so this is effectively a
                    // no-op.
                    continue;
                }
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
    //    in scope (MT4020 ambiguous / MT4021 not found).
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
        // If no inherent and trait_candidates.len() > 1: MT4020.
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
/// triggers MT3011.
/// v0.29 Track C: resolve the return type of a typed bang-send / ask
/// call (`agent ! Msg(args)` / `agent ? Msg(args)`).
///
/// Walks: `target_ty` → resolve through the substitution → if it's the
/// prelude `AgentRef[Adt(agent, _)]`, look up the agent's declared
/// protocols and the per-`(proto, msg)` reply type.
///
/// Resolution table (first match wins):
/// 1. Target resolves to `AgentRef[Adt(agent_adt, _)]`, agent has a
///    protocol that declares `msg` with `-> ReturnTy` → that TyId.
/// 2. Target resolves to `AgentRef[...]`, agent declares `msg` but
///    with no `-> ReturnTy` → `Unit` (declared default).
/// 3. Target resolves to `Adt(agent_adt, _)` directly (no AgentRef
///    wrap — happens inside the agent's own handler body where `self`
///    is the agent type) → same lookup as (1) / (2).
/// 4. No agent / protocol info available (external protocol, fresh var
///    target, error type, arity mismatch) → fresh inference var so
///    call sites that bind the result keep type-checking without a
///    hard mismatch.
///
/// Arity-mismatched calls (declared params vs supplied args differ in
/// count) still resolve to the declared reply — the arity check itself
/// is a v0.30+ follow-up; the v0.29 mandate is **return-type** lowering.
fn resolve_message_reply_ty(
    cx: &mut Cx,
    target_ty: TyId,
    msg: &str,
    _args: &[HirArg],
    _expr_id: ExprId,
) -> TyId {
    let resolved = cx.subst.resolve(target_ty, cx.arena);
    let agent_adt = agent_adt_from_target(cx, resolved);
    let Some(agent_adt) = agent_adt else {
        return cx.fresh();
    };
    let Some(proto_names) = cx.defs.agent_protocols.get(&agent_adt).cloned() else {
        return cx.fresh();
    };
    for pname in &proto_names {
        let key = (pname.clone(), msg.to_string());
        if let Some(reply_ty) = cx.defs.protocol_msg_reply.get(&key).copied() {
            return reply_ty;
        }
        // Protocol declares the message but with no `-> ReturnTy`
        // — the surface default is Unit.
        if let Some(names) = cx.defs.protocol_msg_names.get(pname) {
            if names.iter().any(|n| n == msg) {
                return cx.arena.unit;
            }
        }
    }
    // The protocol set is known, but no protocol declares `msg`.
    // Diagnostic (MT2026) already fires from the handler-side check;
    // for the call site we fall back to a fresh var so the rest of
    // the expression still type-checks.
    cx.fresh()
}

/// v0.29 Track C: pull the agent ADT id out of a resolved `Send`/`Ask`
/// target type. Handles both the `AgentRef[Adt(agent, _)]` wrap (the
/// usual case — `spawn Agent(...)` synthesises that shape) and the
/// bare `Adt(agent, _)` case (e.g. `self ! Msg(...)` inside an agent
/// method, where `self` is already the agent type).
fn agent_adt_from_target(cx: &Cx, target: TyId) -> Option<AdtId> {
    match cx.arena.get(target).clone() {
        TyData::Adt(adt_id, args) if adt_id == cx.agent_ref_id => {
            // Drill one level into AgentRef[T]. Resolve the inner arg
            // through the substitution so we see past inference vars.
            let inner = *args.first()?;
            let inner_resolved = cx.subst.resolve(inner, cx.arena);
            match cx.arena.get(inner_resolved) {
                TyData::Adt(inner_adt, _) => Some(*inner_adt),
                _ => None,
            }
        }
        TyData::Adt(adt_id, _) => Some(adt_id),
        _ => None,
    }
}

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
/// as narrow as the parameter's. If not, emit MT4010 capability_too_broad.
fn check_cap_subsumption(cx: &mut Cx, arg_expr: ExprId, param_ty: TyId, expr_id: ExprId) {
    let Some(arg_ty) = cx.expr_ty.get(&arg_expr).copied() else {
        return;
    };
    let arg_resolved = cx.subst.resolve(arg_ty, cx.arena);
    let param_resolved = cx.subst.resolve(param_ty, cx.arena);
    let TyData::Cap {
        family: af,
        constraint: ac,
    } = cx.arena.get(arg_resolved).clone()
    else {
        return;
    };
    let TyData::Cap {
        family: pf,
        constraint: pc,
    } = cx.arena.get(param_resolved).clone()
    else {
        return;
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

fn synth_expr_through(_cx: &mut Cx, _recv: ExprId, ty: TyId) -> TyId {
    // Helper that exists for clarity in the deref path; no-op for now.
    ty
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
    // v0.22 Coverage Closure (MT2016 emit-site): a match arm AFTER an
    // unconditional arm (wildcard / plain binding with no guard) can
    // never fire. Fire MT2016 as a warning on each subsequent arm.
    let mut saw_unconditional_arm = false;
    for (idx, arm) in arms.iter().enumerate() {
        if saw_unconditional_arm {
            cx.diag
                .push(diag::unreachable_match_arm(&cx.span_of_expr(arm.body)));
        }
        cx.locals.enter();
        check_pattern(cx, arm.pat, scrut_ty);
        if let Some(g) = arm.guard {
            check_expr(cx, g, cx.arena.bool_);
        }
        let arm_ty = synth_expr(cx, arm.body);
        let _ = unify(arm_ty, result, cx.subst, cx.arena);
        cx.locals.leave();
        // A pattern is unconditional iff it's a wildcard or plain binding
        // (no sub-pattern) AND the arm has no guard. Anything else may
        // refuse to match, so following arms remain reachable.
        let _ = idx;
        if arm.guard.is_none() && is_unconditional_pattern(cx, arm.pat) {
            saw_unconditional_arm = true;
        }
    }
    // v0.22 Coverage Closure (MT2015 emit-site): when the scrutinee
    // resolves to a known enum ADT, exhaustiveness requires either an
    // unconditional arm OR every variant covered by an Enum-pattern.
    // Conservative: skip the check for non-enum scrutinees (the v0.20
    // analyser handled some via runtime traps; MT2015 only owns the
    // statically-decidable enum shape).
    if !saw_unconditional_arm {
        let resolved = cx.subst.resolve(scrut_ty, cx.arena);
        if let TyData::Adt(aid, _) = cx.arena.get(resolved).clone() {
            if let Some(adt) = cx.defs.adt(aid) {
                if adt.kind == AdtKind::Enum {
                    let variants: Vec<String> =
                        adt.variants.iter().map(|v| v.name.clone()).collect();
                    let mut covered: std::collections::HashSet<String> = Default::default();
                    for arm in arms {
                        if arm.guard.is_some() {
                            continue;
                        }
                        collect_covered_variants(cx, arm.pat, &mut covered);
                    }
                    let missing: Vec<String> = variants
                        .iter()
                        .filter(|v| !covered.contains(*v))
                        .cloned()
                        .collect();
                    if !missing.is_empty() {
                        cx.diag.push(diag::non_exhaustive_match(
                            &cx.span_of_expr(scrutinee),
                            &missing,
                        ));
                    }
                }
            }
        }
    }
    result
}

/// v0.22 Coverage Closure helper: a pattern is "unconditional" iff it
/// matches every possible scrutinee. v0.22 conservative set:
/// `_`, plain `Binding { sub: None }`, and `Ref { inner }` over an
/// unconditional inner.
fn is_unconditional_pattern(cx: &Cx, pid: PatId) -> bool {
    match &cx.pkg.pats[pid] {
        HirPat::Wildcard => true,
        HirPat::Binding { sub: None, .. } => true,
        HirPat::Binding { sub: Some(s), .. } => is_unconditional_pattern(cx, *s),
        HirPat::Ref { inner, .. } => is_unconditional_pattern(cx, *inner),
        _ => false,
    }
}

/// v0.22 Coverage Closure helper: collect the variant names this pattern
/// "covers" so the MT2015 exhaustiveness check can compute the missing
/// set. Skips guarded arms (callers filter those out).
fn collect_covered_variants(cx: &Cx, pid: PatId, out: &mut std::collections::HashSet<String>) {
    match &cx.pkg.pats[pid] {
        HirPat::Enum { path, .. } => {
            if let Some(name) = path.last() {
                out.insert(name.clone());
            }
        }
        // A binding sub-pattern lifts the inner pattern's coverage.
        HirPat::Binding { sub: Some(s), .. } => collect_covered_variants(cx, *s, out),
        HirPat::Ref { inner, .. } => collect_covered_variants(cx, *inner, out),
        _ => {}
    }
}

fn synth_struct(
    cx: &mut Cx,
    path: &[String],
    fields: &[(String, ExprId)],
    expr_id: ExprId,
) -> TyId {
    // Resolve path to an ADT (struct).
    let name = path.last().cloned().unwrap_or_default();
    let Some(DefRef::Adt(aid)) = cx.defs.lookup(&name) else {
        // Permissive: synth fields, return fresh.
        for (_, e) in fields {
            let _ = synth_expr(cx, *e);
        }
        return cx.fresh();
    };
    let adt = match cx.defs.adt(aid) {
        Some(a) => a.clone(),
        None => return cx.arena.error,
    };
    if adt.kind != AdtKind::Struct {
        // v0.12 (Gap B / MT2022 emit-site): struct literal applied to a
        // non-struct ADT. We restrict the fire to **Enum** ADTs — opaque
        // / prelude shim ADTs (Page, Url, etc.) keep the legacy
        // permissive treatment so v0.x examples that use `Page {}` as
        // an empty placeholder still compile. AdtKind::Opaque is the
        // catch-all bucket for shim types.
        if adt.kind == AdtKind::Enum {
            cx.diag
                .push(diag::not_a_struct(&adt.name, &cx.span_of_expr(expr_id)));
        }
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

/// v0.22 Coverage Closure: pub re-export of `check_stmt` so the
/// items.rs custom function-body path (MT2019 emit) can drive
/// statement checking directly without losing access through the
/// private helper.
pub fn check_stmt_pub(cx: &mut Cx, stmt: &HirStmt) {
    check_stmt(cx, stmt)
}

fn check_stmt(cx: &mut Cx, stmt: &HirStmt) {
    match stmt {
        HirStmt::Let {
            pat,
            ty,
            init,
            mutable,
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
            // v0.14 (Gap B / MT2003 emit-site): when a let-binding has no
            // type annotation AND the initializer's synthesised type is an
            // empty container whose element/payload type is a free
            // inference variable, fire MT2003 ("cannot infer type").
            // Pre-v0.14 the type checker silently kept `[T?]` open and
            // defaulted later to the v0.x error sentinel — which downstream
            // codegen would surface as a less helpful trap. This catches
            // the common shape `let xs = []` / `let xs: _ = []` early.
            //
            // v0.14 integrator carve-out: `let mut xs = []` is a legitimate
            // idiom — subsequent assignments (`xs = xs.push(v)` etc.) will
            // unify the element type. Skip the eager emit for `mut`
            // bindings; the existing late default-to-Error path still
            // catches truly never-constrained mutable slots.
            //
            // Spec ref: §7.2 (inference) of v1.0-RC2 / KNOWN_ISSUES #11.
            if declared.is_none() && !*mutable {
                if let Some(e) = init {
                    if is_cannot_infer_shape(cx, *e, init_ty) {
                        let pat_name =
                            pattern_first_binding_name(cx, *pat).unwrap_or_else(|| "_".into());
                        cx.diag.push(diag::cannot_infer(
                            &cx.span_of_expr(*e),
                            format!("binding `{}`", pat_name),
                        ));
                    }
                }
            }
            check_pattern(cx, *pat, init_ty);
        }
        HirStmt::Expr(e) => {
            let _ = synth_expr(cx, *e);
        }
    }
}

/// v0.14 (MT2003): walks the init expression to find the well-defined
/// "can't infer" shapes the type checker should surface immediately:
///
/// - `[]` empty array literal — element type is a free `Var`.
///
/// Returns `true` when the binding type cannot be inferred from local
/// information. Further shapes (empty map, `Default()`, generic
/// constructor with no args) can land later; the v1.x emit-landing plan
/// in KNOWN_ISSUES #11 calls this "trait-iterator + collect chain".
fn is_cannot_infer_shape(cx: &Cx, expr_id: ExprId, ty: TyId) -> bool {
    match &cx.pkg.exprs[expr_id] {
        HirExpr::Array(xs) if xs.is_empty() => {
            let resolved = cx.subst.resolve(ty, cx.arena);
            match cx.arena.get(resolved) {
                TyData::Array { elem, .. } => {
                    let elem_r = cx.subst.resolve(*elem, cx.arena);
                    matches!(cx.arena.get(elem_r), TyData::Var(_))
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// Pull the first binding name out of `pat` for diagnostic phrasing.
/// Returns `None` for wildcards / literal-only patterns.
fn pattern_first_binding_name(cx: &Cx, pat_id: PatId) -> Option<String> {
    match &cx.pkg.pats[pat_id] {
        HirPat::Binding { name, .. } => Some(name.clone()),
        HirPat::Tuple(xs) => xs.iter().find_map(|p| pattern_first_binding_name(cx, *p)),
        HirPat::Ref { inner, .. } => pattern_first_binding_name(cx, *inner),
        _ => None,
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
