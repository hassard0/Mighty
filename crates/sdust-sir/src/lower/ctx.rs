//! Lowering context — owns the in-flight `Program` plus per-function
//! scratch state (block builder, local map, scope stack).

use crate::sir::*;
use sdust_hir::{ExprId, FnId, Package, SourceSpan};
use sdust_types::{TyArena, TyData, TyId, TypedPackage};
use std::collections::HashMap;

pub struct LowerCtx<'a> {
    pub pkg: &'a Package,
    pub typed: &'a TypedPackage,
    pub prog: Program,
    /// HirFnId -> SirFnId (populated as we allocate function shells).
    pub fn_map: HashMap<FnId, SirFnId>,
    /// AgentName -> AgentSirId.
    pub agent_map: HashMap<String, AgentSirId>,
    /// FnDefId in defmap -> SirFnId (so name-resolved fn calls land on
    /// the right SIR fn).
    pub fn_def_to_sir: HashMap<u32, SirFnId>,
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
        ret_ty: SirTy,
        hir_fn: Option<FnId>,
        span: SourceSpan,
    ) -> SirFnId {
        let id = SirFnId(self.prog.fns.len() as u32);
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
    pub fn_id: SirFnId,
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
}

#[derive(Clone, Copy)]
pub struct LoopFrame {
    pub continue_target: BlockId,
    pub exit_target: BlockId,
    pub result_local: Local,
}

impl FnBuilder {
    pub fn new(fn_id: SirFnId, ret_ty: SirTy) -> Self {
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
        };
        s.cur = entry;
        s
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
    }

    pub fn set_term(&mut self, t: Term) {
        let blk = self.cur.0 as usize;
        self.blocks[blk].terminator = t;
    }

    pub fn new_local(
        &mut self,
        name: impl Into<String>,
        ty: SirTy,
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

    pub fn fresh_temp(&mut self, ty: SirTy) -> Local {
        self.new_local("", ty, true, LocalSource::Temp)
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
            ret_ty: SirTy::Unit, // caller fills
            effects: vec![],
            hir_fn,
            span,
        }
    }
}
