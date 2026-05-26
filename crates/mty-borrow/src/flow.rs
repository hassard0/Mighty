//! Borrow-checker walker (v0.3).
//!
//! Drives ownership/borrow/move state per Place across the typed HIR of
//! every fn body, agent state initializer, agent handler, agent method,
//! and supervisor child expression.
//!
//! ## v0.1 (slice 4) baseline
//!
//! - **Lexical regions.** A borrow's region was the innermost enclosing
//!   block (or the borrower local's scope).
//! - **Per-local state** with `Ownership::Borrowed{n}` / `BorrowedMut`.
//! - **All calls move by default**, unless the parameter type is
//!   `Ref { .. }`.
//! - **`if`/`match` joins by state intersection.**
//!
//! ## v0.3 hardening (A54/A55/A56)
//!
//! - **Field-level Place tracking** (`place::Place`): `&mut s.a` and
//!   `&s.b` are now disjoint. Conflicts detected via overlap on the
//!   `BorrowLedger`.
//! - **NLL last-use**: per-fn pre-pass computes the last program point
//!   where each local is referenced. When the walker reaches the
//!   borrower binding's last use, the corresponding borrow record is
//!   removed from the ledger (and the root-local state recomputed
//!   from the ledger remnants).
//! - **Precise MT3009**: `move *ref` (and `let x = *ref` for non-Copy)
//!   emits MT3009 with a tailored message.

use crate::arena_region::ArenaCounter;
use crate::copy::is_copy;
use crate::diag;
use crate::drop_plan::{DropEntry, DropPlan};
use crate::nll::{compute_last_use, LastUseMap, ProgramPoint};
use crate::place::{Place, Proj};
use crate::sendable::is_sendable;
use crate::state::{
    join_states, ArenaRegionId, BorrowKind, BorrowLedger, BorrowRecord, LocalState, Ownership,
    ScopeFrame,
};
use mty_diagnostics::Diagnostic;
use mty_hir::*;
use mty_types::{TyData, TyId, TypedPackage};
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
        let Some((fid, _)) = pkg.fns.iter().nth(fid_idx) else {
            continue;
        };
        let hir_fn = &pkg.fns[fid];
        let Some(body) = hir_fn.body else { continue };
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
    let (last_use, _max_pt) = compute_last_use(pkg, body);
    let mut bcx = BorrowCx::new(typed, pkg, diagnostics, drop_plan);
    bcx.last_use = last_use;
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
    /// v0.3 (A54): live borrow ledger keyed by Place.
    ledger: BorrowLedger,
    /// v0.3 (A55): pre-computed last-use point per local.
    last_use: LastUseMap,
    /// v0.3 (A55): monotone program-point counter; advanced in
    /// lock-step with `nll::Pre`.
    current_point: u32,
    /// v0.3 (A55): the borrower binding being established by the
    /// enclosing `let`. Set in `walk_stmt(HirStmt::Let)` for the
    /// duration of the init walk, so a `Borrow { .. }` can stamp the
    /// new ledger record with its owner.
    pending_borrower: Option<String>,
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
            ledger: BorrowLedger::default(),
            last_use: LastUseMap::default(),
            current_point: 0,
            pending_borrower: None,
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
        // v0.12 (Gap C / MT3007 emit-site): before decaying borrows
        // held by departing locals, scan the ledger for records whose
        // *owner* (root local) is bound in this departing frame BUT
        // whose *borrower* is bound in an outer frame. That shape is
        // "borrow outlives owner" — the value is going out of scope
        // while a borrow handle still exists in a wider region.
        // Pre-v0.12 we silently dropped both the owner and the now-
        // dangling borrower; v0.12 surfaces MT3007 first.
        let departing: std::collections::HashSet<&String> = frame.locals.iter().collect();
        for r in &self.ledger.records {
            if departing.contains(&r.place.root) {
                let borrower_outer = match &r.borrower {
                    Some(b) => !departing.contains(b),
                    None => false,
                };
                if borrower_outer {
                    self.diag
                        .push(diag::borrow_outlives_owner(&r.place.root, &r.at));
                }
            }
        }
        // End of scope: emit drop intents and decay borrows held by
        // departing locals.
        let removed = self.ledger.decay_borrowers(frame.locals.iter());
        for r in removed {
            self.recompute_root_state(&r.place.root);
        }
        // Also remove any records rooted at a departing local (the owner
        // is gone — the record is dangling). This is the v0.12 sweep
        // counterpart to the MT3007 emit above.
        let departing_owned: Vec<String> = frame.locals.clone();
        self.ledger
            .records
            .retain(|r| !departing_owned.iter().any(|n| n == &r.place.root));
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

    /// v0.5 (loop back-edge analysis): run `body_walker` against the loop's
    /// body until the per-local ledger state converges, conservatively
    /// merged with each prior iteration's snapshot. Bounded at 16
    /// iterations so a divergent body still terminates; if convergence
    /// hasn't happened by then we fall back to a final pass and exit.
    ///
    /// The borrow ledger is monotonic in the join (`join_ledgers` keeps
    /// records present in either branch), so successive iterations only
    /// add records; convergence is when the ledger record count stays
    /// the same.
    fn loop_fixed_point(&mut self, mut body_walker: impl FnMut(&mut Self)) {
        let pre_locals = self.locals.clone();
        let pre_ledger = self.ledger.clone();
        let mut prev_records = pre_ledger.records.len();
        for _i in 0..16 {
            body_walker(self);
            // Conservatively join post-body state with pre-body baseline so
            // borrows opened mid-body and conditionally dropped before the
            // back-edge still constrain the next iteration.
            self.locals = join_states(pre_locals.clone(), &self.locals);
            self.ledger = join_ledgers(&pre_ledger, &self.ledger);
            if self.ledger.records.len() == prev_records {
                return;
            }
            prev_records = self.ledger.records.len();
        }
        // Final pass after the cap: the conservative join above already
        // covered the back-edge state, so just emit any borrow-conflict
        // diagnostics the body would raise on a final iteration.
        body_walker(self);
        self.locals = join_states(pre_locals, &self.locals);
        self.ledger = join_ledgers(&pre_ledger, &self.ledger);
    }

    fn walk_stmt(&mut self, stmt: &HirStmt) {
        match stmt {
            HirStmt::Let {
                pat,
                ty: _,
                init,
                mutable,
            } => {
                // v0.3 (A55): if the let binds a single name, capture it
                // as the pending borrower so any borrow expression in
                // `init` can stamp the ledger record.
                let bind_name = pattern_single_name(self.pkg, *pat);
                let init_ty = match init {
                    Some(e) => {
                        let prev = self.pending_borrower.take();
                        self.pending_borrower.clone_from(&bind_name);
                        let _ = self.walk_expr(*e, Position::Use);
                        self.pending_borrower = prev;
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

    /// Try to interpret an expression as a `Place`. Returns `None` for
    /// expressions that aren't places (literals, calls, etc.).
    ///
    /// Multi-segment `Path(["s", "a"])` is treated as `s` (local) then
    /// successive `Field(a)` projections — the lowering pass folds
    /// dotted paths into one `Path` node, so the borrow checker has to
    /// unfold them when the first segment is a local.
    fn expr_as_place(&self, eid: ExprId) -> Option<Place> {
        let expr = self.pkg.exprs[eid].clone();
        match expr {
            HirExpr::Path(segs) => self.segs_to_place(&segs),
            HirExpr::PathGeneric { segments, .. } => self.segs_to_place(&segments),
            HirExpr::Field { receiver, name } => {
                let rp = self.expr_as_place(receiver)?;
                let mut p = rp;
                p.projs.push(Proj::Field(name));
                // v0.3: truncate at depth 1 (A54 note).
                Some(p.truncate_for_v0_3())
            }
            HirExpr::Unary {
                op: UnOp::Deref,
                rhs,
            } => {
                let rp = self.expr_as_place(rhs)?;
                let mut p = rp;
                p.projs.push(Proj::Deref);
                Some(p.truncate_for_v0_3())
            }
            HirExpr::Index { receiver, .. } => {
                let rp = self.expr_as_place(receiver)?;
                let mut p = rp;
                p.projs.push(Proj::Index);
                Some(p.truncate_for_v0_3())
            }
            _ => None,
        }
    }

    /// Turn a multi-segment path into a Place, IF the first segment is
    /// a local. `["s"]` → Place(s). `["s", "a"]` → Place(s.a). Longer
    /// paths are truncated to depth 1 per A54.
    fn segs_to_place(&self, segs: &[String]) -> Option<Place> {
        if segs.is_empty() {
            return None;
        }
        if !self.locals.contains_key(&segs[0]) {
            return None;
        }
        let mut p = Place::root(segs[0].clone());
        for s in &segs[1..] {
            p.projs.push(Proj::Field(s.clone()));
        }
        Some(p.truncate_for_v0_3())
    }

    /// Walk an expression in the given position. Returns the local-name
    /// root if the expression is a simple path to a local (useful for
    /// arena escape detection).
    fn walk_expr(&mut self, eid: ExprId, pos: Position) -> Option<String> {
        let expr = self.pkg.exprs[eid].clone();
        let span = SourceSpan { start: 0, end: 0 };
        match expr {
            HirExpr::Path(segs) => {
                if segs.is_empty() {
                    return None;
                }
                let name = segs[0].clone();
                self.advance_point_and_record_use(&name);
                if segs.len() == 1 && self.locals.contains_key(&name) {
                    match pos {
                        Position::Move => self.do_move(&name, &span),
                        Position::BorrowShared => self.do_borrow_shared(&name, &span),
                        Position::BorrowMut => self.do_borrow_mut(&name, &span),
                        Position::Use => self.do_use(&name, &span),
                        Position::AssignTarget => self.do_assign(&name, &span),
                    }
                    self.maybe_decay_after_use(&name);
                    return Some(name);
                }
                // Multi-segment path (`s.a` etc.) — if the root is a
                // local AND the position is a borrow, this is a
                // FIELD-LEVEL borrow that should have been handled by
                // the Borrow case via expr_as_place. In Use/Move/
                // AssignTarget positions, conservatively treat the
                // read as a `do_use` of the root local so the move /
                // uninit checks still fire.
                if self.locals.contains_key(&name) {
                    match pos {
                        Position::Use | Position::BorrowShared | Position::BorrowMut => {
                            self.do_use(&name, &span);
                        }
                        Position::Move => {
                            // Moving out of `s.a` falls through to a
                            // whole-local move on `s` (v0.3 doesn't do
                            // partial-move tracking). Safe but loose.
                            self.do_move(&name, &span);
                        }
                        Position::AssignTarget => {
                            // `s.a = ...` requires `s` to be mut; the
                            // assignment doesn't change `s`'s overall
                            // Ownership state, so just check mutability.
                            if let Some(local) = self.locals.get(&name) {
                                if !local.mutable {
                                    self.diag.push(diag::assign_to_immut_local(&name, &span));
                                }
                            }
                        }
                    }
                }
                self.maybe_decay_after_use(&name);
                if segs.len() == 1 {
                    Some(name)
                } else {
                    None
                }
            }
            HirExpr::PathGeneric { segments, .. } => {
                if segments.is_empty() {
                    return None;
                }
                let name = segments[0].clone();
                self.advance_point_and_record_use(&name);
                if segments.len() == 1 && self.locals.contains_key(&name) {
                    if pos == Position::Use {
                        self.do_use(&name, &span);
                    }
                    self.maybe_decay_after_use(&name);
                    return Some(name);
                }
                self.maybe_decay_after_use(&name);
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
                let is_assign = matches!(
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
                );
                if is_assign {
                    let _ = self.walk_expr(lhs, Position::AssignTarget);
                } else {
                    let _ = self.walk_expr(lhs, Position::Use);
                }
                // v0.12 (Gap C): for a plain `x = &y` assignment, the
                // borrower is the LHS path's root local. Set
                // `pending_borrower` so the borrow record stamps the
                // ledger with the right owner — this is what enables
                // MT3007 detection at scope-end for assignments
                // (previously only `let` bindings set pending_borrower).
                let bind_name = if matches!(op, BinOp::Assign) {
                    lhs_root_name(self.pkg, lhs)
                } else {
                    None
                };
                if bind_name.is_some() {
                    let prev = self.pending_borrower.take();
                    self.pending_borrower.clone_from(&bind_name);
                    let _ = self.walk_expr(rhs, Position::Use);
                    self.pending_borrower = prev;
                } else {
                    let _ = self.walk_expr(rhs, Position::Use);
                }
                None
            }
            HirExpr::Unary { op, rhs } => {
                // v0.3 (A56): `let x = *ref` (Position::Use of a Deref of
                // a non-Copy ref) is also an MT3009. The explicit Move
                // case is handled in HirExpr::Move.
                if matches!(op, UnOp::Deref) && pos == Position::Use {
                    self.check_deref_move(rhs, &span);
                }
                let _ = self.walk_expr(rhs, Position::Use);
                None
            }
            HirExpr::Borrow { mutable, inner } => {
                // v0.3 (A54): try to compute a Place for the borrow.
                // If we get one, emit a Place-aware borrow event;
                // otherwise fall back to the old whole-expression walk.
                let place = self.expr_as_place(inner);
                match place {
                    Some(p) => {
                        let kind = if mutable {
                            BorrowKind::Mut
                        } else {
                            BorrowKind::Shared
                        };
                        self.try_place_borrow(&p, kind, &span);
                        // We still need to advance the program point for
                        // the path read inside the place (so the pre-pass
                        // numbering stays in sync). Walk children in Use
                        // position purely for point-counting; the actual
                        // state change is suppressed by special-casing
                        // when a place was extracted.
                        self.walk_for_points_only(inner);
                    }
                    None => {
                        let inner_pos = if mutable {
                            Position::BorrowMut
                        } else {
                            Position::BorrowShared
                        };
                        let _ = self.walk_expr(inner, inner_pos);
                    }
                }
                None
            }
            HirExpr::Move(inner) => {
                // v0.3 (A56): `move *ref` of a non-Copy ref => MT3009.
                let inner_expr = self.pkg.exprs[inner].clone();
                if let HirExpr::Unary {
                    op: UnOp::Deref,
                    rhs,
                } = inner_expr
                {
                    self.check_deref_move(rhs, &span);
                    let _ = self.walk_expr(rhs, Position::Use);
                    return None;
                }
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
                let ledger_snap = self.ledger.clone();
                self.walk_block(then);
                let after_then = self.locals.clone();
                let after_then_ledger = self.ledger.clone();
                self.locals.clone_from(&snapshot);
                self.ledger = ledger_snap;
                if let Some(e) = else_ {
                    let _ = self.walk_expr(e, Position::Use);
                }
                self.locals = join_states(self.locals.clone(), &after_then);
                // Ledger join: keep records that exist in EITHER branch
                // (conservative — over-restricts so a borrow held on one
                // arm conflicts with a borrow taken after the join).
                self.ledger = join_ledgers(&self.ledger, &after_then_ledger);
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
                let ledger_snap = self.ledger.clone();
                self.push_frame(None);
                self.bind_pattern(pat, scrut_ty);
                self.walk_block(then);
                self.pop_frame();
                let after_then = self.locals.clone();
                let after_then_ledger = self.ledger.clone();
                self.locals.clone_from(&snapshot);
                self.ledger = ledger_snap;
                if let Some(e) = else_ {
                    let _ = self.walk_expr(e, Position::Use);
                }
                self.locals = join_states(self.locals.clone(), &after_then);
                self.ledger = join_ledgers(&self.ledger, &after_then_ledger);
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
                let mut joined_ledger: Option<BorrowLedger> = None;
                let base = self.locals.clone();
                let base_ledger = self.ledger.clone();
                for arm in &arms {
                    self.locals.clone_from(&base);
                    self.ledger = base_ledger.clone();
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
                    joined_ledger = Some(match joined_ledger {
                        Some(j) => join_ledgers(&j, &self.ledger),
                        None => self.ledger.clone(),
                    });
                }
                self.locals = joined.unwrap_or(base);
                self.ledger = joined_ledger.unwrap_or(base_ledger);
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
                // v0.5 (loop back-edge): run the body until the per-local
                // ledger converges (or a 16-iteration safety cap fires).
                // Patterns are re-bound on each iteration.
                self.loop_fixed_point(|this| {
                    this.push_frame(None);
                    this.bind_pattern(pat, elem_ty);
                    this.walk_block(body);
                    this.pop_frame();
                });
                None
            }
            HirExpr::While { cond, body } => {
                let _ = self.walk_expr(cond, Position::Use);
                self.loop_fixed_point(|this| {
                    this.walk_block(body);
                });
                None
            }
            HirExpr::Loop { body } => {
                self.loop_fixed_point(|this| {
                    this.walk_block(body);
                });
                None
            }
            HirExpr::Return(inner) => {
                if let Some(e) = inner {
                    // Return moves the value out.
                    let _ = self.walk_expr(e, Position::Move);
                }
                None
            }
            HirExpr::Break(inner) => {
                if let Some(e) = inner {
                    // `break value` moves the value out of the loop.
                    let _ = self.walk_expr(e, Position::Move);
                }
                None
            }
            HirExpr::Continue => None,
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
                // emit MT3010.
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
                // permanently moved here — closures in Mighty are
                // affine wrt their captures but slice 4 doesn't enforce).
                // v0.3: snapshot the ledger too so borrows taken
                // inside the lambda body don't leak out.
                let snapshot = self.locals.clone();
                let ledger_snap = self.ledger.clone();
                self.push_frame(None);
                for p in &params {
                    let ty = self.typed.ty_arena.unit;
                    self.bind_local(p.name.clone(), ty, p.span.clone(), false);
                }
                self.walk_block(body);
                self.pop_frame();
                self.locals = snapshot;
                self.ledger = ledger_snap;
                None
            }
            HirExpr::Run(inner) => self.walk_expr(inner, pos),
            HirExpr::Error => None,
        }
    }

    /// Advance the program point counter and record this name's use.
    fn advance_point_and_record_use(&mut self, _name: &str) {
        self.current_point += 1;
    }

    /// v0.3 (A55): after a Path use, check if `name`'s last-use point
    /// has been reached. If yes, decay any borrows held by `name`
    /// (this is the NLL deactivation step).
    fn maybe_decay_after_use(&mut self, name: &str) {
        let pt_just_used = ProgramPoint(self.current_point.saturating_sub(1));
        if let Some(last) = self.last_use.get(name) {
            if pt_just_used >= last {
                let removed = self.ledger.decay_borrower(name);
                for r in removed {
                    self.recompute_root_state(&r.place.root);
                }
            }
        }
    }

    /// Recompute the root local's `Borrowed*` state from the ledger
    /// after a borrow record was removed. Owned/Moved/Uninit are not
    /// touched.
    fn recompute_root_state(&mut self, root: &str) {
        let mut shared = 0u32;
        let mut has_mut = false;
        for r in &self.ledger.records {
            if r.place.root == root {
                match r.kind {
                    BorrowKind::Shared => shared += 1,
                    BorrowKind::Mut => has_mut = true,
                }
            }
        }
        if let Some(s) = self.locals.get_mut(root) {
            match &s.state {
                Ownership::Owned | Ownership::Borrowed { .. } | Ownership::BorrowedMut => {
                    s.state = if has_mut {
                        Ownership::BorrowedMut
                    } else if shared > 0 {
                        Ownership::Borrowed { count: shared }
                    } else {
                        Ownership::Owned
                    };
                }
                _ => {}
            }
        }
    }

    /// v0.3 (A54): take a `Place` borrow. Detects field-level overlap
    /// via the ledger. For root-level borrows we also keep the legacy
    /// per-local `Ownership::Borrowed*` state in sync so existing diag
    /// pathways still fire.
    fn try_place_borrow(&mut self, place: &Place, kind: BorrowKind, span: &SourceSpan) {
        // Mutability check: borrow_mut of a place rooted at an immutable
        // local needs the legacy MT3013 check on the root.
        if kind == BorrowKind::Mut {
            if let Some(s) = self.locals.get(&place.root) {
                if !s.mutable {
                    self.diag
                        .push(diag::mut_borrow_of_immut_local(&place.root, span));
                    // continue: still record the borrow so further checks
                    // see the state.
                }
            }
        }

        // Move/uninit check on the root.
        if let Some(s) = self.locals.get(&place.root) {
            match &s.state {
                Ownership::Moved { at } => {
                    self.diag
                        .push(diag::borrow_after_move(&place.root, span, at));
                    return;
                }
                Ownership::Uninit => {
                    self.diag
                        .push(diag::use_of_uninitialized(&place.root, span));
                    return;
                }
                _ => {}
            }
        }

        // Conflict scan in the ledger.
        let is_root_borrow = place.projs.is_empty();
        let pretty = format!("{}", place);
        let conflict_count = self.ledger.conflicts_with(place).count();
        if conflict_count > 0 {
            // Pick the worst conflict to classify the error.
            let mut any_mut_existing = false;
            let mut any_shared_existing = false;
            for c in self.ledger.conflicts_with(place) {
                match c.kind {
                    BorrowKind::Mut => any_mut_existing = true,
                    BorrowKind::Shared => any_shared_existing = true,
                }
            }
            match (kind, any_mut_existing, any_shared_existing) {
                (BorrowKind::Mut, true, _) => {
                    // existing &mut + new &mut => MT3006
                    if is_root_borrow {
                        self.diag.push(diag::two_mut_borrows(&place.root, span));
                    } else {
                        self.diag.push(diag::two_mut_borrows_place(&pretty, span));
                    }
                }
                (BorrowKind::Mut, false, true) => {
                    // existing & + new &mut => MT3004
                    if is_root_borrow {
                        self.diag
                            .push(diag::mut_borrow_while_shared(&place.root, span));
                    } else {
                        self.diag
                            .push(diag::mut_borrow_while_shared_place(&pretty, span));
                    }
                }
                (BorrowKind::Shared, true, _) => {
                    // existing &mut + new & => MT3005
                    if is_root_borrow {
                        self.diag
                            .push(diag::shared_borrow_while_mut(&place.root, span));
                    } else {
                        self.diag
                            .push(diag::shared_borrow_while_mut_place(&pretty, span));
                    }
                }
                (BorrowKind::Shared, false, true) => {
                    // shared + shared overlap is fine.
                }
                _ => {}
            }
        }

        // Record the borrow.
        self.ledger.push(BorrowRecord {
            place: place.clone(),
            kind,
            borrower: self.pending_borrower.clone(),
            at: span.clone(),
        });

        // Sync root-local state for backwards-compat (existing tests
        // depend on these state values).
        if let Some(s) = self.locals.get_mut(&place.root) {
            match (&s.state, kind) {
                (Ownership::Owned, BorrowKind::Shared) => {
                    s.state = Ownership::Borrowed { count: 1 };
                }
                (Ownership::Borrowed { count }, BorrowKind::Shared) => {
                    s.state = Ownership::Borrowed { count: count + 1 };
                }
                (Ownership::Owned, BorrowKind::Mut) => {
                    s.state = Ownership::BorrowedMut;
                }
                _ => { /* already in a borrow state; leave as-is */ }
            }
        }
    }

    /// Walk an expression solely to advance program-point counts for
    /// any Path uses inside it (so the main walker stays in sync with
    /// the pre-pass). State is NOT updated.
    fn walk_for_points_only(&mut self, eid: ExprId) {
        let expr = self.pkg.exprs[eid].clone();
        match expr {
            HirExpr::Path(segs) if !segs.is_empty() => {
                self.advance_point_and_record_use(&segs[0]);
                self.maybe_decay_after_use(&segs[0]);
            }
            HirExpr::PathGeneric { segments, .. } if !segments.is_empty() => {
                self.advance_point_and_record_use(&segments[0]);
                self.maybe_decay_after_use(&segments[0]);
            }
            HirExpr::Field { receiver, .. } => self.walk_for_points_only(receiver),
            HirExpr::Unary { rhs, .. } => self.walk_for_points_only(rhs),
            HirExpr::Index { receiver, idx } => {
                self.walk_for_points_only(receiver);
                self.walk_for_points_only(idx);
            }
            _ => { /* not a place sub-shape; ignore */ }
        }
    }

    /// v0.3 (A56): MT3009 detector for `*ref` where ref's underlying
    /// type is non-Copy. Called on the inner `rhs` of `Unary{Deref}`.
    fn check_deref_move(&mut self, ref_expr: ExprId, span: &SourceSpan) {
        let ref_ty = self.typed.expr_ty.get(&ref_expr).copied();
        let Some(rty) = ref_ty else { return };
        let inner_ty = match self.typed.ty_arena.get(rty) {
            TyData::Ref { inner, .. } => *inner,
            _ => return, // not a reference; codepath unreachable on well-typed source
        };
        if is_copy(inner_ty, &self.typed.ty_arena, &self.typed.def_map) {
            return;
        }
        // Non-Copy: MT3009. Name the ref expression for the user.
        let pretty = self.expr_pretty_name(ref_expr);
        self.diag.push(diag::move_out_of_ref_named(&pretty, span));
    }

    fn expr_pretty_name(&self, eid: ExprId) -> String {
        let expr = self.pkg.exprs[eid].clone();
        match expr {
            HirExpr::Path(segs) => segs.join("."),
            HirExpr::PathGeneric { segments, .. } => segments.join("."),
            HirExpr::Field { receiver, name } => {
                format!("{}.{}", self.expr_pretty_name(receiver), name)
            }
            _ => "<ref>".into(),
        }
    }

    fn do_use(&mut self, name: &str, span: &SourceSpan) {
        let Some(state) = self.locals.get_mut(name) else {
            return;
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
        // v0.12 (Gap C / MT3002 emit-site): scan the ledger for live
        // borrows of any subplace of `name` BEFORE inspecting the
        // root-local state. Field-level borrows (`&x.a`) also update
        // the root's `Borrowed*` state for legacy reasons, which would
        // otherwise route this to MT3008. Distinguishing the
        // subplace-borrow shape (MT3002) from the whole-value-borrow
        // shape (MT3008) gives users a more precise diagnostic.
        let has_subplace_borrow = self
            .ledger
            .records
            .iter()
            .any(|r| r.place.root == name && !r.place.projs.is_empty());
        let has_root_borrow = self
            .ledger
            .records
            .iter()
            .any(|r| r.place.root == name && r.place.projs.is_empty());
        let Some(state) = self.locals.get_mut(name) else {
            return;
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
                // Prefer MT3002 (move out of borrowed value) when the
                // surviving borrow is on a subplace only; reserve
                // MT3008 (cannot move a borrowed value) for the root-
                // level borrow case where the entire value is borrowed.
                if has_subplace_borrow && !has_root_borrow {
                    self.diag.push(diag::move_out_of_borrow(name, span));
                } else {
                    self.diag.push(diag::cannot_move_borrowed(name, span));
                }
            }
            Ownership::Owned => {
                if has_subplace_borrow {
                    self.diag.push(diag::move_out_of_borrow(name, span));
                }
                if !was_copy {
                    state.state = Ownership::Moved { at: span.clone() };
                }
            }
        }
    }

    fn do_borrow_shared(&mut self, name: &str, span: &SourceSpan) {
        // v0.3: if the local's type is already a reference, this is
        // a reborrow (e.g. passing `r: &T` as `&T` arg). Reborrow at
        // the borrow-check level is a no-op on the source local — we
        // only need to read it. This eliminates a slice-4 false-flag
        // where MT3013 would fire on immutable `&mut T` bindings.
        if self.local_is_ref_typed(name) {
            self.do_use(name, span);
            return;
        }
        let p = Place::root(name);
        self.try_place_borrow(&p, BorrowKind::Shared, span);
    }

    fn do_borrow_mut(&mut self, name: &str, span: &SourceSpan) {
        if self.local_is_ref_typed(name) {
            self.do_use(name, span);
            return;
        }
        let p = Place::root(name);
        self.try_place_borrow(&p, BorrowKind::Mut, span);
    }

    fn local_is_ref_typed(&self, name: &str) -> bool {
        match self.locals.get(name) {
            Some(s) => matches!(self.typed.ty_arena.get(s.ty), TyData::Ref { .. }),
            None => false,
        }
    }

    fn do_assign(&mut self, name: &str, span: &SourceSpan) {
        let Some(state) = self.locals.get_mut(name) else {
            return;
        };
        if !state.mutable {
            self.diag.push(diag::assign_to_immut_local(name, span));
        }
        // Assignment re-initialises the binding (Uninit→Owned, Moved→Owned).
        state.state = Ownership::Owned;
    }
}

/// v0.12 (Gap C / MT3007): if `eid` is a single-segment path expression
/// (e.g. `x` in `x = &y`), return its root identifier as a `String`.
/// Used to set `pending_borrower` on plain assignment so that the
/// resulting borrow record stamps the LHS local as borrower — enabling
/// scope-end MT3007 detection.
fn lhs_root_name(pkg: &Package, eid: ExprId) -> Option<String> {
    match &pkg.exprs[eid] {
        HirExpr::Path(segs) if segs.len() == 1 => Some(segs[0].clone()),
        _ => None,
    }
}

/// Pattern: extract the single binding name if `pat` is a simple
/// `Binding { name, sub: None }`. Used by NLL last-use to associate a
/// borrower binding with the borrow it creates.
fn pattern_single_name(pkg: &Package, pid: PatId) -> Option<String> {
    match &pkg.pats[pid] {
        HirPat::Binding { name, sub: None } => Some(name.clone()),
        _ => None,
    }
}

/// Conservative ledger join: keep all records present in EITHER branch.
/// De-duplicate by (place, kind, borrower).
fn join_ledgers(a: &BorrowLedger, b: &BorrowLedger) -> BorrowLedger {
    let mut out = BorrowLedger::default();
    for r in a.records.iter().chain(b.records.iter()) {
        let dup = out
            .records
            .iter()
            .any(|x| x.place == r.place && x.kind == r.kind && x.borrower == r.borrower);
        if !dup {
            out.records.push(r.clone());
        }
    }
    out
}
