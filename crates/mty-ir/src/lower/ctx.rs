//! Lowering context — owns the in-flight `Program` plus per-function
//! scratch state (block builder, local map, scope stack).

use crate::ir::*;
use mty_hir::{ExprId, FnId, Package, SourceSpan};
use mty_types::{TyArena, TyData, TyId, TypedPackage};
use std::collections::{HashMap, HashSet};

// v0.41 T1 — Lowering-time per-fn map of `Local -> TyId` (see the
// `local_tys` field on `FnBuilder` below). Used to recover the
// receiver's resolved ADT type when projecting a multi-segment
// `Path` (`p.y` is parsed as `Path(["p","y"])`, not `Field`) so the
// field-name → index lookup hits the user struct's def-map entry
// instead of the stdlib whitelist fallback.

pub struct LowerCtx<'a> {
    pub pkg: &'a Package,
    pub typed: &'a TypedPackage,
    pub prog: Program,
    /// HirFnId -> IrFnId (populated as we allocate function shells).
    pub fn_map: HashMap<FnId, IrFnId>,
    /// AgentName -> AgentIrId.
    pub agent_map: HashMap<String, AgentIrId>,
    /// FnDefId in defmap -> IrFnId (so name-resolved fn calls land on
    /// the right SIR fn).
    pub fn_def_to_sir: HashMap<u32, IrFnId>,
}

impl<'a> LowerCtx<'a> {
    pub fn new(pkg: &'a Package, typed: &'a TypedPackage) -> Self {
        Self {
            pkg,
            typed,
            prog: Program::default(),
            fn_map: HashMap::new(),
            agent_map: HashMap::new(),
            fn_def_to_sir: HashMap::new(),
        }
    }

    pub fn finish(self) -> Program {
        self.prog
    }

    /// Allocate an empty `Function` shell and return its id. Caller fills
    /// in `params`, `locals`, `blocks` later via `FnBuilder`.
    pub fn alloc_fn_shell(
        &mut self,
        name: String,
        ret_ty: IrTy,
        hir_fn: Option<FnId>,
        span: SourceSpan,
    ) -> IrFnId {
        let id = IrFnId(self.prog.fns.len() as u32);
        self.prog.fns.push(Function {
            id,
            name,
            params: vec![],
            locals: vec![LocalDecl {
                name: "_ret".into(),
                ty: ret_ty.clone(),
                mutable: true,
                source: LocalSource::Return,
            }],
            blocks: vec![],
            entry: BlockId(0),
            ret_ty,
            effects: vec![],
            hir_fn,
            span,
        });
        id
    }

    /// Resolve an HIR ExprId to its resolved Ty (via the typed-package
    /// side tables). Falls back to `Error` if absent.
    pub fn expr_ty(&self, id: ExprId) -> TyId {
        self.typed
            .expr_ty
            .get(&id)
            .copied()
            .unwrap_or(self.typed.ty_arena.error)
    }

    /// Get the underlying TyData for an expr.
    pub fn expr_tydata(&self, id: ExprId) -> &TyData {
        self.typed.ty_arena.get(self.expr_ty(id))
    }

    pub fn ty_arena(&self) -> &TyArena {
        &self.typed.ty_arena
    }
}

/// Builder for a single function body. Owns the block array under
/// construction; the caller hands it back when the body is done.
pub struct FnBuilder {
    pub fn_id: IrFnId,
    pub locals: Vec<LocalDecl>,
    pub params: Vec<Local>,
    pub blocks: Vec<Block>,
    /// Currently-being-built block (index into `blocks`).
    pub cur: BlockId,
    /// HIR local name -> SIR Local. Mirrors slice-4 borrow-walker's
    /// name-based map (shadowing keeps the latest binding).
    pub locals_by_name: HashMap<String, Local>,
    /// Arena scope counter, for synthesizing fresh ArenaIds.
    pub next_arena: u32,
    /// v0.5: stack of `(continue_target, exit_target, result_local)` for
    /// the currently-enclosing loops. `result_local` holds the loop's
    /// value (written by `break <value>`); for `loop`/`for`/`while` it
    /// is always allocated so `lower_loop` can return it. Empty when
    /// not inside any loop — bare `break`/`continue` in that case is a
    /// no-op (the borrow checker / type checker have already reported
    /// the misuse).
    pub loop_stack: Vec<LoopFrame>,
    /// v0.22: source span attached to every `Stmt` and `Term` emitted
    /// while this is set. Updated by lowering helpers before they call
    /// `push_stmt` / `set_term`. Persisted into the per-fn `FnSpanTable`
    /// at `finish()` time and merged into `Program::span_table`.
    pub cur_span: SourceSpan,
    /// v0.22: collected stmt + terminator spans, parallel to
    /// `blocks[block_idx].stmts` and `blocks[block_idx].terminator`.
    /// Populated lazily as `push_stmt` / `set_term` are called.
    pub spans: FnSpanTable,
    /// v0.25 — set of [`Local`]s known to hold a `std.web.Canvas`
    /// handle. Populated when a let-binding or temp is initialized
    /// from `std.web.Canvas.new(...)` (or moves a previously-marked
    /// canvas local). The MethodCall lowerer consults this set to
    /// decide whether `canvas.fill_rect(...)` should route to the
    /// first-class `BuiltinId::CanvasOp(...)` builtin (which the
    /// wasm32-web emitter lowers to a `mty:web/canvas@0.1` import
    /// call) or to the generic `Rvalue::MethodCall` fallback. Closes
    /// the v0.23 → v0.24 unfinished business documented in
    /// `dev/history/notes/DEMO06_CANVAS_DIRECT_V0_24_NOTES.md` §A.
    ///
    /// Tracked as a per-fn set because canvas handles are scoped to a
    /// single function body (lets don't escape, and v0.24 doesn't yet
    /// model agent fields with non-primitive types). Per-fn keeps the
    /// detection cheap and avoids cross-fn aliasing footguns.
    pub canvas_locals: HashSet<Local>,
    /// v0.41 T1 — Map `Local -> TyId` for bindings whose HIR-resolved
    /// type is known at lower-time. Populated by `bind_pat_assign`
    /// from the let-statement's `init_ty` and by parameter lowering.
    /// Consulted by `resolve_path` / `resolve_field_index` so user
    /// struct field projections resolve to the correct field index
    /// instead of falling back to 0. See L15 in
    /// `mighty-ide/docs/mighty-language-lessons.md`.
    pub local_tys: HashMap<Local, TyId>,
}

#[derive(Clone, Copy)]
pub struct LoopFrame {
    pub continue_target: BlockId,
    pub exit_target: BlockId,
    pub result_local: Local,
}

impl FnBuilder {
    pub fn new(fn_id: IrFnId, ret_ty: IrTy) -> Self {
        let entry = BlockId(0);
        let mut s = Self {
            fn_id,
            locals: vec![LocalDecl {
                name: "_ret".into(),
                ty: ret_ty,
                mutable: true,
                source: LocalSource::Return,
            }],
            params: vec![],
            blocks: vec![Block {
                id: entry,
                stmts: vec![],
                terminator: Term::Unreachable,
            }],
            cur: entry,
            locals_by_name: HashMap::new(),
            next_arena: 0,
            loop_stack: Vec::new(),
            cur_span: SourceSpan { start: 0, end: 0 },
            spans: FnSpanTable::new(),
            canvas_locals: HashSet::new(),
            local_tys: HashMap::new(),
        };
        s.cur = entry;
        s
    }

    /// v0.22: set the "current span" used for subsequent `push_stmt`
    /// / `set_term` calls. Lowering helpers call this immediately
    /// before lowering an HIR expression or stmt so the span flows
    /// transparently into the emitted MtyIR.
    pub fn set_cur_span(&mut self, span: SourceSpan) {
        self.cur_span = span;
    }

    /// v0.22: borrow + restore helper. Use when lowering a child
    /// expression whose span should temporarily override the current
    /// span, but the parent's span must be restored when we resume
    /// emitting after the child returns.
    pub fn with_span<R>(&mut self, span: SourceSpan, f: impl FnOnce(&mut Self) -> R) -> R {
        let prev = self.cur_span.clone();
        self.cur_span = span;
        let r = f(self);
        self.cur_span = prev;
        r
    }

    /// v0.22: lookup an HIR expression's span via the package and
    /// install it as the current span. Returns the previous span so
    /// callers can restore.
    pub fn enter_expr_span(&mut self, pkg: &Package, eid: ExprId) -> SourceSpan {
        let prev = self.cur_span.clone();
        let sp = expr_span(pkg, eid);
        self.cur_span = sp;
        prev
    }

    pub fn push_loop(&mut self, frame: LoopFrame) {
        self.loop_stack.push(frame);
    }
    pub fn pop_loop(&mut self) {
        self.loop_stack.pop();
    }
    pub fn current_loop(&self) -> Option<LoopFrame> {
        self.loop_stack.last().copied()
    }

    pub fn new_block(&mut self) -> BlockId {
        let id = BlockId(self.blocks.len() as u32);
        self.blocks.push(Block {
            id,
            stmts: vec![],
            terminator: Term::Unreachable,
        });
        id
    }

    pub fn switch_to(&mut self, b: BlockId) {
        self.cur = b;
    }

    pub fn push_stmt(&mut self, s: Stmt) {
        let blk = self.cur.0 as usize;
        self.blocks[blk].stmts.push(s);
        // v0.22: record the current span at the position of the
        // just-pushed stmt. Manually-built Programs that bypass the
        // builder never see this side-table, so the cranelift backend's
        // fallback synthetic-spread keeps working for them.
        let stmt_idx = self.blocks[blk].stmts.len() - 1;
        self.spans
            .set_stmt_span(self.cur.0, stmt_idx, self.cur_span.clone());
    }

    pub fn set_term(&mut self, t: Term) {
        let blk = self.cur.0 as usize;
        self.blocks[blk].terminator = t;
        // v0.22: record the current span as the terminator's span.
        self.spans
            .set_terminator_span(self.cur.0, self.cur_span.clone());
    }

    pub fn new_local(
        &mut self,
        name: impl Into<String>,
        ty: IrTy,
        mutable: bool,
        src: LocalSource,
    ) -> Local {
        let name = name.into();
        let id = Local(self.locals.len() as u32);
        self.locals.push(LocalDecl {
            name: name.clone(),
            ty,
            mutable,
            source: src,
        });
        if !name.is_empty() {
            self.locals_by_name.insert(name, id);
        }
        id
    }

    pub fn fresh_temp(&mut self, ty: IrTy) -> Local {
        self.new_local("", ty, true, LocalSource::Temp)
    }

    /// v0.25 — mark a [`Local`] as holding a `std.web.Canvas` handle.
    /// Idempotent; safe to call on already-marked locals (e.g. when a
    /// canvas local is re-bound to itself).
    pub fn mark_canvas_local(&mut self, l: Local) {
        self.canvas_locals.insert(l);
    }

    /// v0.25 — true iff `l` was previously marked via
    /// [`Self::mark_canvas_local`]. Used by the MethodCall lowerer
    /// (`crates/mty-ir/src/lower/exprs.rs`) to decide whether
    /// `canvas.fill_rect(...)` routes to `BuiltinId::CanvasOp` or
    /// to the generic `Rvalue::MethodCall` fallback.
    pub fn is_canvas_local(&self, l: Local) -> bool {
        self.canvas_locals.contains(&l)
    }

    /// v0.41 T1 — Record the HIR-resolved type for a local. Caller
    /// should pass the binding's `TyId` if known (otherwise skip the
    /// call). Idempotent: re-binds overwrite.
    pub fn set_local_ty(&mut self, l: Local, ty: TyId) {
        self.local_tys.insert(l, ty);
    }

    /// v0.41 T1 — Look up the HIR-resolved type for a local. Returns
    /// `None` if no type was recorded (the lowerer should then fall
    /// back to the legacy permissive behaviour).
    pub fn local_ty(&self, l: Local) -> Option<TyId> {
        self.local_tys.get(&l).copied()
    }

    pub fn fresh_arena(&mut self) -> ArenaId {
        let a = ArenaId(self.next_arena);
        self.next_arena += 1;
        a
    }

    pub fn current_block(&self) -> BlockId {
        self.cur
    }

    pub fn finish(self, hir_fn: Option<FnId>, span: SourceSpan) -> Function {
        Function {
            id: self.fn_id,
            name: String::new(), // caller fills
            params: self.params,
            locals: self.locals,
            blocks: self.blocks,
            entry: BlockId(0),
            ret_ty: IrTy::Unit, // caller fills
            effects: vec![],
            hir_fn,
            span,
        }
    }

    /// v0.22: finish + return the collected span table separately. Used
    /// by the IR lowerer so it can merge the table into `Program::span_table`
    /// alongside installing the Function.
    pub fn finish_with_spans(
        self,
        hir_fn: Option<FnId>,
        span: SourceSpan,
    ) -> (Function, FnSpanTable) {
        let fn_id = self.fn_id;
        let params = self.params;
        let locals = self.locals;
        let blocks = self.blocks;
        let spans = self.spans;
        let func = Function {
            id: fn_id,
            name: String::new(),
            params,
            locals,
            blocks,
            entry: BlockId(0),
            ret_ty: IrTy::Unit,
            effects: vec![],
            hir_fn,
            span,
        };
        (func, spans)
    }
}

/// v0.22 Coverage Closure (workspace-build unblock): minimal stub for
/// the span-table work-in-progress in this file. HIR does not yet
/// expose per-expression spans (only HirFn/HirAgent etc. carry one);
/// returning a zero span keeps the type-checker / borrow-checker
/// coverage closure unblocked while the broader span-table effort
/// completes. Replace with a real lookup once `mty_hir::Package`
/// surfaces an `exprs_spans` arena.
fn expr_span(_pkg: &Package, _eid: ExprId) -> SourceSpan {
    SourceSpan { start: 0, end: 0 }
}
