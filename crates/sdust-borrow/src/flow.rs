//! Linear, lexical borrow-checker walker.
//!
//! Drives ownership/borrow/move state per local across the typed HIR of
//! every fn body, agent state initializer, agent handler, agent method,
//! and supervisor child expression.
//!
//! Slice 4 simplifications:
//! - **Lexical regions only.** A borrow's region is the innermost
//!   enclosing block. End-of-block decays all borrows.
//! - **All calls are moving by default**, unless the parameter type is
//!   `Ref { .. }` (in which case the arg is borrowed for the duration of
//!   the call expression).
//! - **`if`/`match` joins by state intersection** — see `state::join_states`.
//! - **Spans are approximate.** We only have per-fn / per-handler spans
//!   in the HIR; slice 4 reports diagnostics with the best-available
//!   span. Tightening span fidelity is post-v0.1.

use crate::arena_region::ArenaCounter;
use crate::copy::is_copy;
use crate::diag;
use crate::drop_plan::{DropEntry, DropPlan};
use crate::sendable::is_sendable;
use crate::state::{join_states, ArenaRegionId, LocalState, Ownership, ScopeFrame};
use sdust_diagnostics::Diagnostic;
use sdust_hir::*;
use sdust_types::{TyData, TyId, TypedPackage};
use std::collections::HashMap;

/// Borrow-check entry point. Takes the typed package alongside the HIR
/// package (whose arenas the typed-package's expr_ty map indexes into).
pub fn run(typed: &TypedPackage, pkg: &Package) -> Vec<Diagnostic> {
    let mut diagnostics: Vec<Diagnostic> = vec![];
    let mut drop_plan = DropPlan::default();

    // Build hir_fn → agent owner map (so agent-method bodies use agent
    // tolerance — only relevant for diagnostic spans).
    let mut hir_fn_to_agent: HashMap<FnId, AgentId> = HashMap::new();
    for item_id in &pkg.top_level {
        if let Item::Agent(aid) = &pkg.items[*item_id] {
            let agent = &pkg.agents[*aid];
            for mfid in &agent.methods {
                hir_fn_to_agent.insert(*mfid, *aid);
            }
        }
    }

    // 1) Top-level fns + agent methods (all flattened through `pkg.fns`).
    for fid_idx in 0..pkg.fns.len() {
        let fid = match pkg.fns.iter().nth(fid_idx) {
            Some((id, _)) => id,
            None => continue,
        };
        let hir_fn = &pkg.fns[fid];
        let body = match hir_fn.body {
            Some(b) => b,
            None => continue,
        };
        check_fn_body(
            typed,
            pkg,
            fid,
            hir_fn,
            body,
            &mut diagnostics,
            &mut drop_plan,
        );
    }

    // 2) Agent state initializers + handlers — methods are already in pkg.fns.
    for item_id in &pkg.top_level {
        if let Item::Agent(aid) = &pkg.items[*item_id] {
            let agent = &pkg.agents[*aid];
            // State init exprs: each is a stand-alone expression; we
            // treat it as a single block-tail-style use.
            for state in &agent.state {
                if let Some(init) = state.init {
                    let mut bcx = BorrowCx::new(typed, pkg, &mut diagnostics, &mut drop_plan);
                    bcx.push_frame(None);
                    let _ = bcx.walk_expr(init, Position::Use);
                    bcx.pop_frame();
                }
            }
            // Handlers.
            for handler in &agent.handlers {
                let mut bcx = BorrowCx::new(typed, pkg, &mut diagnostics, &mut drop_plan);
                bcx.push_frame(None);
                // Handler params: slice 4 best-effort binding. We don't
                // have per-handler param types in TypedPackage (they're
                // local to the type checker's protocol-resolution step).
                // Bind as Unit/opaque, which trips no copy/sendable rule.
                for pname in &handler.params {
                    bcx.bind_local(
                        pname.clone(),
                        typed.ty_arena.unit,
                        handler.span.clone(),
                        false,
                    );
                }
                bcx.walk_block(handler.body);
                bcx.pop_frame();
            }
        }
        // Supervisor children expressions don't introduce locals.
        if let Item::Supervisor(sid) = &pkg.items[*item_id] {
            let sup = &pkg.supervisors[*sid];
            for (_, child_expr) in &sup.children {
                let mut bcx = BorrowCx::new(typed, pkg, &mut diagnostics, &mut drop_plan);
                bcx.push_frame(None);
                let _ = bcx.walk_expr(*child_expr, Position::Use);
                bcx.pop_frame();
            }
        }
    }

    let _ = drop_plan;
    diagnostics
}

fn check_fn_body(
    typed: &TypedPackage,
    pkg: &Package,
    fid: FnId,
    hir_fn: &HirFn,
    body: BlockId,
    diagnostics: &mut Vec<Diagnostic>,
    drop_plan: &mut DropPlan,
) {
    let mut bcx = BorrowCx::new(typed, pkg, diagnostics, drop_plan);
    bcx.push_frame(None);
    // Bind params from the typed side table.
    if let Some(params) = typed.fn_params.get(&fid) {
        for (name, ty) in params {
            bcx.bind_local(name.clone(), *ty, hir_fn.span.clone(), false);
        }
    }
    bcx.walk_block(body);
    bcx.pop_frame();
}

/// Position in which an expression is being evaluated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Position {
    /// Plain rvalue / read context.
    Use,
    /// Operand of `move` — must move.
    Move,
    /// Operand of `&` — borrow shared.
    BorrowShared,
    /// Operand of `&mut` — borrow mut.
    BorrowMut,
    /// L-value of assignment — must be a place.
    AssignTarget,
}

struct BorrowCx<'a> {
    typed: &'a TypedPackage,
    pkg: &'a Package,
    locals: HashMap<String, LocalState>,
    scopes: Vec<ScopeFrame>,
    arenas: ArenaCounter,
    diag: &'a mut Vec<Diagnostic>,
    drop_plan: &'a mut DropPlan,
}

impl<'a> BorrowCx<'a> {
    fn new(
        typed: &'a TypedPackage,
        pkg: &'a Package,
        diag: &'a mut Vec<Diagnostic>,
        drop_plan: &'a mut DropPlan,
    ) -> Self {
        Self {
            typed,
            pkg,
            locals: HashMap::new(),
            scopes: vec![],
            arenas: ArenaCounter::default(),
            diag,
            drop_plan,
        }
    }

    fn push_frame(&mut self, region: Option<ArenaRegionId>) {
        self.scopes.push(ScopeFrame {
            locals: vec![],
            arena_region: region,
        });
    }

    fn pop_frame(&mut self) {
        let frame = self.scopes.pop().expect("scope underflow");
        // End of scope: emit drop intents and remove locals.
        for name in &frame.locals {
            if let Some(state) = self.locals.get(name) {
                if matches!(state.state, Ownership::Owned) && !state.is_copy {
                    self.drop_plan.entries.push(DropEntry {
                        local_name: name.clone(),
                        span: state.declared_at.clone(),
                    });
                }
            }
            self.locals.remove(name);
        }
    }

    fn current_arena(&self) -> Option<ArenaRegionId> {
        self.scopes.iter().rev().find_map(|f| f.arena_region)
    }

    fn bind_local(&mut self, name: String, ty: TyId, span: SourceSpan, mutable: bool) {
        let is_copy_ty = is_copy(ty, &self.typed.ty_arena, &self.typed.def_map);
        let region = self.current_arena();
        let state = LocalState {
            name: name.clone(),
            ty,
            state: Ownership::Owned,
            declared_at: span,
            mutable,
            is_copy: is_copy_ty,
            arena_region: region,
        };
        self.locals.insert(name.clone(), state);
        if let Some(f) = self.scopes.last_mut() {
            f.locals.push(name);
        }
    }

    fn walk_block(&mut self, bid: BlockId) {
        self.push_frame(None);
        let block = self.pkg.blocks[bid].clone();
        for stmt in &block.stmts {
            self.walk_stmt(stmt);
        }
        if let Some(tail) = block.tail {
            let _ = self.walk_expr(tail, Position::Use);
        }
        self.pop_frame();
    }

    fn walk_stmt(&mut self, stmt: &HirStmt) {
        match stmt {
            HirStmt::Let {
                pat,
                ty: _,
                init,
                mutable,
            } => {
                let init_ty = match init {
                    Some(e) => {
                        let _ = self.walk_expr(*e, Position::Use);
                        self.typed
                            .expr_ty
                            .get(e)
                            .copied()
                            .unwrap_or(self.typed.ty_arena.unit)
                    }
                    None => self.typed.ty_arena.unit,
                };
                self.bind_pattern_mut(*pat, init_ty, *mutable);
            }
            HirStmt::Expr(e) => {
                let _ = self.walk_expr(*e, Position::Use);
            }
        }
    }

    /// Bind pattern locals using `scrut_ty` as the scrutinee type. Slice-4
    /// MVP destructures tuples and unwraps `Ref` patterns; struct/enum
    /// field types fall back to Unit (field-ty lookup for patterns isn't
    /// in the side table yet).
    fn bind_pattern(&mut self, pid: PatId, scrut_ty: TyId) {
        let pat = self.pkg.pats[pid].clone();
        self.bind_pattern_inner_mut(&pat, scrut_ty, false);
    }

    fn bind_pattern_mut(&mut self, pid: PatId, scrut_ty: TyId, mutable: bool) {
        let pat = self.pkg.pats[pid].clone();
        self.bind_pattern_inner_mut(&pat, scrut_ty, mutable);
    }

    fn bind_pattern_inner_mut(&mut self, pat: &HirPat, scrut_ty: TyId, mutable: bool) {
        match pat {
            HirPat::Binding { name, sub } => {
                self.bind_local(
                    name.clone(),
                    scrut_ty,
                    SourceSpan { start: 0, end: 0 },
                    mutable,
                );
                if let Some(s) = sub {
                    let sub_pat = self.pkg.pats[*s].clone();
                    self.bind_pattern_inner_mut(&sub_pat, scrut_ty, mutable);
                }
            }
            HirPat::Tuple(xs) => {
                let parts: Vec<TyId> = match self.typed.ty_arena.get(scrut_ty) {
                    TyData::Tuple(ps) => ps.clone(),
                    _ => vec![self.typed.ty_arena.unit; xs.len()],
                };
                for (i, s) in xs.iter().enumerate() {
                    let sub_pat = self.pkg.pats[*s].clone();
                    let t = parts.get(i).copied().unwrap_or(self.typed.ty_arena.unit);
                    self.bind_pattern_inner_mut(&sub_pat, t, mutable);
                }
            }
            HirPat::Struct { fields, .. } => {
                let ut = self.typed.ty_arena.unit;
                for (fname, sub) in fields {
                    match sub {
                        Some(s) => {
                            let sp = self.pkg.pats[*s].clone();
                            self.bind_pattern_inner_mut(&sp, ut, mutable);
                        }
                        None => {
                            self.bind_local(
                                fname.clone(),
                                ut,
                                SourceSpan { start: 0, end: 0 },
                                mutable,
                            );
                        }
                    }
                }
            }
            HirPat::Enum { args, .. } => {
                let ut = self.typed.ty_arena.unit;
                for s in args {
                    let sp = self.pkg.pats[*s].clone();
                    self.bind_pattern_inner_mut(&sp, ut, mutable);
                }
            }
            HirPat::Ref { inner, .. } => {
                let inner_ty = match self.typed.ty_arena.get(scrut_ty) {
                    TyData::Ref { inner: i, .. } => *i,
                    _ => self.typed.ty_arena.unit,
                };
                let sp = self.pkg.pats[*inner].clone();
                self.bind_pattern_inner_mut(&sp, inner_ty, mutable);
            }
            HirPat::Range { .. } | HirPat::Wildcard | HirPat::Literal(_) => {}
        }
    }

    /// Walk an expression in the given position. Returns nothing; state
    /// updates accumulate on `self.locals`. Returns the local-name root if
    /// the expression is a simple path to a local (useful for arena escape
    /// detection).
    fn walk_expr(&mut self, eid: ExprId, pos: Position) -> Option<String> {
        let expr = self.pkg.exprs[eid].clone();
        let span = SourceSpan { start: 0, end: 0 };
        match expr {
            HirExpr::Path(segs) => {
                if segs.len() == 1 {
                    let name = segs[0].clone();
                    if self.locals.contains_key(&name) {
                        match pos {
                            Position::Move => self.do_move(&name, &span),
                            Position::BorrowShared => self.do_borrow_shared(&name, &span),
                            Position::BorrowMut => self.do_borrow_mut(&name, &span),
                            Position::Use => self.do_use(&name, &span),
                            Position::AssignTarget => self.do_assign(&name, &span),
                        }
                        return Some(name);
                    }
                }
                None
            }
            HirExpr::PathGeneric { segments, .. } => {
                if segments.len() == 1 && self.locals.contains_key(&segments[0]) {
                    let name = segments[0].clone();
                    if pos == Position::Use {
                        self.do_use(&name, &span);
                    }
                    return Some(name);
                }
                None
            }
            HirExpr::Literal(_) => None,
            HirExpr::Block(b) => {
                self.walk_block(b);
                None
            }
            HirExpr::Tuple(xs) => {
                for e in xs {
                    let _ = self.walk_expr(e, Position::Use);
                }
                None
            }
            HirExpr::Array(xs) => {
                for e in xs {
                    let _ = self.walk_expr(e, Position::Use);
                }
                None
            }
            HirExpr::Binary { op, lhs, rhs } => {
                if matches!(
                    op,
                    BinOp::Assign
                        | BinOp::AssignAdd
                        | BinOp::AssignSub
                        | BinOp::AssignMul
                        | BinOp::AssignDiv
                        | BinOp::AssignRem
                        | BinOp::AssignBitAnd
                        | BinOp::AssignBitOr
                        | BinOp::AssignBitXor
                        | BinOp::AssignShl
                        | BinOp::AssignShr
                ) {
                    let _ = self.walk_expr(lhs, Position::AssignTarget);
                } else {
                    let _ = self.walk_expr(lhs, Position::Use);
                }
                let _ = self.walk_expr(rhs, Position::Use);
                None
            }
            HirExpr::Unary { op: _, rhs } => {
                // Slice 4 treats all unary operands as plain Use position.
                let _ = self.walk_expr(rhs, Position::Use);
                None
            }
            HirExpr::Borrow { mutable, inner } => {
                let p = if mutable {
                    Position::BorrowMut
                } else {
                    Position::BorrowShared
                };
                let _ = self.walk_expr(inner, p);
                None
            }
            HirExpr::Move(inner) => {
                let _ = self.walk_expr(inner, Position::Move);
                None
            }
            HirExpr::Call { callee, args } => {
                let _ = self.walk_expr(callee, Position::Use);
                // Look up callee's resolved Fn type for parameter shapes.
                let callee_ty = self.typed.expr_ty.get(&callee).copied();
                let param_tys: Vec<TyId> = match callee_ty {
                    Some(t) => {
                        let resolved = t;
                        if let TyData::Fn { params, .. } = self.typed.ty_arena.get(resolved).clone()
                        {
                            params
                        } else {
                            vec![]
                        }
                    }
                    None => vec![],
                };
                for (i, arg) in args.iter().enumerate() {
                    let pos = match param_tys.get(i) {
                        Some(pt) => match self.typed.ty_arena.get(*pt) {
                            TyData::Ref { mutable: true, .. } => Position::BorrowMut,
                            TyData::Ref { mutable: false, .. } => Position::BorrowShared,
                            _ => Position::Use,
                        },
                        None => Position::Use,
                    };
                    let _ = self.walk_expr(arg.value, pos);
                }
                None
            }
            HirExpr::MethodCall { receiver, args, .. } => {
                // Receiver is borrowed shared for the duration (slice 4).
                let _ = self.walk_expr(receiver, Position::BorrowShared);
                for arg in args {
                    let _ = self.walk_expr(arg.value, Position::Use);
                }
                None
            }
            HirExpr::Field { receiver, .. } => {
                let _ = self.walk_expr(receiver, Position::Use);
                None
            }
            HirExpr::Index { receiver, idx } => {
                let _ = self.walk_expr(receiver, Position::Use);
                let _ = self.walk_expr(idx, Position::Use);
                None
            }
            HirExpr::If { cond, then, else_ } => {
                let _ = self.walk_expr(cond, Position::Use);
                let snapshot = self.locals.clone();
                self.walk_block(then);
                let after_then = self.locals.clone();
                self.locals = snapshot.clone();
                if let Some(e) = else_ {
                    let _ = self.walk_expr(e, Position::Use);
                }
                self.locals = join_states(self.locals.clone(), &after_then);
                None
            }
            HirExpr::IfLet {
                pat,
                scrutinee,
                then,
                else_,
            } => {
                let _ = self.walk_expr(scrutinee, Position::Use);
                let scrut_ty = self
                    .typed
                    .expr_ty
                    .get(&scrutinee)
                    .copied()
                    .unwrap_or(self.typed.ty_arena.unit);
                let snapshot = self.locals.clone();
                self.push_frame(None);
                self.bind_pattern(pat, scrut_ty);
                self.walk_block(then);
                self.pop_frame();
                let after_then = self.locals.clone();
                self.locals = snapshot.clone();
                if let Some(e) = else_ {
                    let _ = self.walk_expr(e, Position::Use);
                }
                self.locals = join_states(self.locals.clone(), &after_then);
                None
            }
            HirExpr::Match { scrutinee, arms } => {
                let _ = self.walk_expr(scrutinee, Position::Use);
                let scrut_ty = self
                    .typed
                    .expr_ty
                    .get(&scrutinee)
                    .copied()
                    .unwrap_or(self.typed.ty_arena.unit);
                let mut joined: Option<HashMap<String, LocalState>> = None;
                let base = self.locals.clone();
                for arm in &arms {
                    self.locals = base.clone();
                    self.push_frame(None);
                    self.bind_pattern(arm.pat, scrut_ty);
                    if let Some(g) = arm.guard {
                        let _ = self.walk_expr(g, Position::Use);
                    }
                    let _ = self.walk_expr(arm.body, Position::Use);
                    self.pop_frame();
                    joined = Some(match joined {
                        Some(j) => join_states(j, &self.locals),
                        None => self.locals.clone(),
                    });
                }
                self.locals = joined.unwrap_or(base);
                None
            }
            HirExpr::For { pat, iter, body } => {
                let _ = self.walk_expr(iter, Position::Use);
                let iter_ty = self
                    .typed
                    .expr_ty
                    .get(&iter)
                    .copied()
                    .unwrap_or(self.typed.ty_arena.unit);
                let elem_ty = match self.typed.ty_arena.get(iter_ty) {
                    TyData::Array { elem, .. } => *elem,
                    TyData::Ref { inner, .. } => match self.typed.ty_arena.get(*inner) {
                        TyData::Array { elem, .. } => *elem,
                        _ => self.typed.ty_arena.unit,
                    },
                    _ => self.typed.ty_arena.unit,
                };
                self.push_frame(None);
                self.bind_pattern(pat, elem_ty);
                self.walk_block(body);
                self.pop_frame();
                None
            }
            HirExpr::While { cond, body } => {
                let _ = self.walk_expr(cond, Position::Use);
                self.walk_block(body);
                None
            }
            HirExpr::Loop { body } => {
                self.walk_block(body);
                None
            }
            HirExpr::Return(inner) => {
                if let Some(e) = inner {
                    // Return moves the value out.
                    let _ = self.walk_expr(e, Position::Move);
                }
                None
            }
            HirExpr::Struct { fields, .. } => {
                for (_, e) in fields {
                    let _ = self.walk_expr(e, Position::Use);
                }
                None
            }
            HirExpr::Map(entries) => {
                for (k, v) in entries {
                    let _ = self.walk_expr(k, Position::Use);
                    let _ = self.walk_expr(v, Position::Use);
                }
                None
            }
            HirExpr::Send { target, args, .. } | HirExpr::Ask { target, args, .. } => {
                let _ = self.walk_expr(target, Position::Use);
                // Each arg's resolved type must be Sendable.
                for arg in &args {
                    let arg_name = self.walk_expr(arg.value, Position::Move);
                    if let Some(arg_ty) = self.typed.expr_ty.get(&arg.value).copied() {
                        if !is_sendable(arg_ty, &self.typed.ty_arena, &self.typed.def_map) {
                            let pretty = arg_name.unwrap_or_else(|| "<arg>".into());
                            self.diag
                                .push(diag::non_sendable_message_arg(&pretty, &span));
                        }
                    }
                }
                None
            }
            HirExpr::Deadline { inner, dur } => {
                let _ = self.walk_expr(dur, Position::Use);
                self.walk_expr(inner, pos)
            }
            HirExpr::Question(inner) => self.walk_expr(inner, pos),
            HirExpr::Spawn { inner, .. } => {
                let _ = self.walk_expr(inner, Position::Use);
                None
            }
            HirExpr::Detach(inner) | HirExpr::Join(inner) => {
                let _ = self.walk_expr(inner, Position::Use);
                None
            }
            HirExpr::HtmlTemplate(_) => None,
            HirExpr::Unsafe(b) => {
                self.walk_block(b);
                None
            }
            HirExpr::Arena { body, .. } => {
                let region = self.arenas.fresh();
                self.push_frame(Some(region));
                // If body is a Block, walk its stmts then check tail
                // while the arena frame still has its locals visible.
                let body_expr = self.pkg.exprs[body].clone();
                let tail_name = match body_expr {
                    HirExpr::Block(bid) => {
                        let block = self.pkg.blocks[bid].clone();
                        for stmt in &block.stmts {
                            self.walk_stmt(stmt);
                        }
                        if let Some(tail) = block.tail {
                            self.walk_expr(tail, Position::Use)
                        } else {
                            None
                        }
                    }
                    _ => self.walk_expr(body, Position::Use),
                };
                // Arena escape: if the body's tail directly resolves to a
                // local owned in the active arena region and is not Copy,
                // emit SD3010.
                if let Some(name) = tail_name {
                    if let Some(state) = self.locals.get(&name) {
                        if state.arena_region == Some(region) && !state.is_copy {
                            self.diag.push(diag::arena_escape(&name, &span));
                        }
                    }
                }
                self.pop_frame();
                None
            }
            HirExpr::TaskScope { body, .. } => {
                self.walk_block(body);
                None
            }
            HirExpr::Budget { entries, body } => {
                for (_, e) in entries {
                    let _ = self.walk_expr(e, Position::Use);
                }
                let _ = self.walk_expr(body, Position::Use);
                None
            }
            HirExpr::Sandbox { body, entries, .. } => {
                for (_, e) in entries {
                    let _ = self.walk_expr(e, Position::Use);
                }
                self.walk_block(body);
                None
            }
            HirExpr::Cast { lhs, .. } => {
                let _ = self.walk_expr(lhs, Position::Use);
                None
            }
            HirExpr::Lambda { params, body, .. } => {
                // Lambdas open a fresh local scope. Slice-4 simplification:
                // we walk the body in a fresh BorrowCx state (locals
                // snapshot-and-restore so captured outer locals aren't
                // permanently moved here — closures in Stardust are
                // affine wrt their captures but slice 4 doesn't enforce).
                let snapshot = self.locals.clone();
                self.push_frame(None);
                for p in &params {
                    let ty = self.typed.ty_arena.unit;
                    self.bind_local(p.name.clone(), ty, p.span.clone(), false);
                }
                self.walk_block(body);
                self.pop_frame();
                self.locals = snapshot;
                None
            }
            HirExpr::Run(inner) => self.walk_expr(inner, pos),
            HirExpr::Error => None,
        }
    }

    fn do_use(&mut self, name: &str, span: &SourceSpan) {
        let state = match self.locals.get_mut(name) {
            Some(s) => s,
            None => return,
        };
        match state.state.clone() {
            Ownership::Moved { at } => {
                self.diag.push(diag::use_after_move(name, span, &at));
            }
            Ownership::Uninit => {
                self.diag.push(diag::use_of_uninitialized(name, span));
            }
            Ownership::Owned | Ownership::Borrowed { .. } | Ownership::BorrowedMut => {
                // Plain read of a borrowed value is fine: the read goes
                // through the borrow, not through the owner. We model
                // this as a non-state-mutating read.
            }
        }
    }

    fn do_move(&mut self, name: &str, span: &SourceSpan) {
        let state = match self.locals.get_mut(name) {
            Some(s) => s,
            None => return,
        };
        let was_copy = state.is_copy;
        match state.state.clone() {
            Ownership::Moved { at } => {
                self.diag.push(diag::use_after_move(name, span, &at));
            }
            Ownership::Uninit => {
                self.diag.push(diag::use_of_uninitialized(name, span));
            }
            Ownership::Borrowed { .. } | Ownership::BorrowedMut => {
                self.diag.push(diag::cannot_move_borrowed(name, span));
            }
            Ownership::Owned => {
                if !was_copy {
                    state.state = Ownership::Moved { at: span.clone() };
                }
            }
        }
    }

    fn do_borrow_shared(&mut self, name: &str, span: &SourceSpan) {
        let state = match self.locals.get_mut(name) {
            Some(s) => s,
            None => return,
        };
        match state.state.clone() {
            Ownership::Moved { at } => {
                self.diag.push(diag::borrow_after_move(name, span, &at));
            }
            Ownership::Uninit => {
                self.diag.push(diag::use_of_uninitialized(name, span));
            }
            Ownership::BorrowedMut => {
                self.diag.push(diag::shared_borrow_while_mut(name, span));
            }
            Ownership::Owned => {
                state.state = Ownership::Borrowed { count: 1 };
            }
            Ownership::Borrowed { count } => {
                state.state = Ownership::Borrowed { count: count + 1 };
            }
        }
    }

    fn do_borrow_mut(&mut self, name: &str, span: &SourceSpan) {
        let state = match self.locals.get_mut(name) {
            Some(s) => s,
            None => return,
        };
        match state.state.clone() {
            Ownership::Moved { at } => {
                self.diag.push(diag::borrow_after_move(name, span, &at));
            }
            Ownership::Uninit => {
                self.diag.push(diag::use_of_uninitialized(name, span));
            }
            Ownership::Borrowed { .. } => {
                self.diag.push(diag::mut_borrow_while_shared(name, span));
            }
            Ownership::BorrowedMut => {
                self.diag.push(diag::two_mut_borrows(name, span));
            }
            Ownership::Owned => {
                if !state.mutable {
                    self.diag.push(diag::mut_borrow_of_immut_local(name, span));
                }
                state.state = Ownership::BorrowedMut;
            }
        }
    }

    fn do_assign(&mut self, name: &str, span: &SourceSpan) {
        let state = match self.locals.get_mut(name) {
            Some(s) => s,
            None => return,
        };
        if !state.mutable {
            self.diag.push(diag::assign_to_immut_local(name, span));
        }
        // Assignment re-initialises the binding (Uninit→Owned, Moved→Owned).
        state.state = Ownership::Owned;
    }
}
