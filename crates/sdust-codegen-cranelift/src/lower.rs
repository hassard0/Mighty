//! SIR → Cranelift IR lowering core (slice 8).
//!
//! This module is reused by both the JIT and AOT paths. The driver
//! creates a Cranelift `Module` (either `JITModule` or `ObjectModule`),
//! then walks the SIR and asks this module to declare/define each fn.
//!
//! The lowerer is *intentionally conservative*: SIR shapes it doesn't
//! understand raise `CodegenError::Unsupported`, which the caller can
//! catch and fall back to the interpreter. Slice-8 covers:
//!
//! - integer / bool / float arithmetic & comparisons
//! - locals → stack slots
//! - direct fn-to-fn calls (monomorphized)
//! - `log("...")` and `print("...")` (string-literal arg) via the
//!   runtime extern table
//! - `if` / `goto` / `return` / `unreachable` terminators
//! - immediate string constants via a literal pool
//!
//! Out-of-scope for slice-8 (fall through to interpreter):
//!
//! - aggregate (struct/enum/array/tuple) construction & projection
//! - generic-typed bindings (handled by [`crate::mono`])
//! - effect calls (lower to interpreter)
//! - agent spawn / send / ask (the runtime drives those; main fn
//!   compiles, agent handlers stay on the interpreter for slice 8)
//! - capabilities / dyn dispatch / pattern matching

use crate::abi::{build_signature, cl_ty_for, host_call_conv};
use crate::error::{CodegenError, CompileResult};
use crate::runtime_imports;
use cranelift_codegen::ir::types as ct;
use cranelift_codegen::ir::{
    AbiParam, Function as ClFunction, InstBuilder, MemFlags, Signature, UserFuncName,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use sdust_sir::sir::{
    BinOp, BlockId, BuiltinId, Const, FnRef, Function, Local, Operand, Place, Program, Rvalue,
    SirFnId, SirTy, Stmt, Term, UnOp,
};
use sdust_types::IntKind;
use std::collections::HashMap;
use target_lexicon::Triple;

/// Per-module lowering context. Holds the cranelift module + per-fn
/// FuncId lookup tables. Lifetime tied to the module's lifetime.
pub struct LowerCtx<'m, M: Module> {
    pub module: &'m mut M,
    pub fn_ids: HashMap<SirFnId, FuncId>,
    pub fn_sigs: HashMap<SirFnId, Signature>,
    pub runtime_ids: HashMap<&'static str, FuncId>,
    pub string_pool: HashMap<String, DataId>,
    pub triple: Triple,
}

impl<'m, M: Module> LowerCtx<'m, M> {
    pub fn new(module: &'m mut M, triple: Triple) -> Self {
        Self {
            module,
            fn_ids: HashMap::new(),
            fn_sigs: HashMap::new(),
            runtime_ids: HashMap::new(),
            string_pool: HashMap::new(),
            triple,
        }
    }

    /// Declare every fn in `prog`. Pre-declaration lets call sites
    /// resolve forward references without a separate pass.
    pub fn declare_fns(&mut self, prog: &Program) -> CompileResult<()> {
        // Runtime imports first.
        let cc = host_call_conv(&self.triple);
        for ri in runtime_imports::RUNTIME_IMPORTS {
            let sig = ri.signature(cc);
            let id = self
                .module
                .declare_function(ri.name, Linkage::Import, &sig)
                .map_err(|e| CodegenError::Module(e.to_string()))?;
            self.runtime_ids.insert(ri.name, id);
        }
        // User fns next.
        for f in &prog.fns {
            let param_tys: Vec<_> = f
                .params
                .iter()
                .map(|p| f.locals[p.0 as usize].ty.clone())
                .collect();
            let sig = build_signature(&self.triple, &param_tys, &f.ret_ty);
            let linkage = if f.name == "main" {
                Linkage::Export
            } else {
                Linkage::Local
            };
            let id = self
                .module
                .declare_function(&f.name, linkage, &sig)
                .map_err(|e| CodegenError::Module(e.to_string()))?;
            self.fn_ids.insert(f.id, id);
            self.fn_sigs.insert(f.id, sig);
        }
        Ok(())
    }

    /// Intern a string literal as a module data symbol; returns the
    /// DataId so the lowerer can emit a `symbol_value` reference.
    pub fn intern_string(&mut self, s: &str) -> CompileResult<DataId> {
        if let Some(id) = self.string_pool.get(s) {
            return Ok(*id);
        }
        let name = format!(".Lstr_{}", self.string_pool.len());
        let id = self
            .module
            .declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| CodegenError::Module(e.to_string()))?;
        let mut desc = DataDescription::new();
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0); // null-terminate so C-side prints are easy
        desc.define(bytes.into_boxed_slice());
        self.module
            .define_data(id, &desc)
            .map_err(|e| CodegenError::Module(e.to_string()))?;
        self.string_pool.insert(s.to_string(), id);
        Ok(id)
    }

    /// Define a single SIR fn into the cranelift module.
    pub fn define_fn(&mut self, prog: &Program, f: &Function) -> CompileResult<()> {
        let func_id = *self
            .fn_ids
            .get(&f.id)
            .ok_or_else(|| CodegenError::Module(format!("undeclared fn {}", f.name)))?;
        let sig = self.fn_sigs.get(&f.id).cloned().ok_or_else(|| {
            CodegenError::Module(format!("missing signature for fn {}", f.name))
        })?;

        let mut clf = ClFunction::with_name_signature(UserFuncName::user(0, f.id.0), sig);
        let mut ctx = FunctionBuilderContext::new();
        lower_one(self, prog, f, &mut clf, &mut ctx)?;

        let mut mctx = self.module.make_context();
        mctx.func = clf;
        self.module
            .define_function(func_id, &mut mctx)
            .map_err(|e| CodegenError::VerifierFailed {
                name: f.name.clone(),
                msg: e.to_string(),
            })?;
        self.module.clear_context(&mut mctx);
        Ok(())
    }
}

/// Helper that orchestrates the per-fn lowering so the
/// `FunctionBuilder` borrow naturally drops before we move out.
fn lower_one<M: Module>(
    mod_ctx: &mut LowerCtx<'_, M>,
    prog: &Program,
    f: &Function,
    clf: &mut ClFunction,
    ctx: &mut FunctionBuilderContext,
) -> CompileResult<()> {
    let mut b = FunctionBuilder::new(clf, ctx);
    {
        let mut fl = FnLower::new(mod_ctx, prog, f, &mut b)?;
        fl.lower_blocks()?;
    }
    b.seal_all_blocks();
    b.finalize();
    Ok(())
}

/// Per-function builder. Holds the Variable map (one per SIR Local)
/// and the BlockId→cranelift Block map.
///
/// The four lifetimes are deliberately split: `'a` for the lowering
/// context (long-lived), `'b` for the cranelift `FunctionBuilder`
/// borrow (re-borrowed within define_fn), `'m` for the module
/// reference inside the LowerCtx, and `'p` for the program/function
/// borrow. Keeping them distinct avoids the "cannot reborrow b" trap
/// when both `FnLower` and `define_fn`'s caller want to touch `b`.
pub struct FnLower<'short, 'long, 'a, 'm, 'p, M: Module> {
    pub mod_ctx: &'a mut LowerCtx<'m, M>,
    pub prog: &'p Program,
    pub f: &'p Function,
    /// `'short` is the lifetime of the &mut borrow on the builder; it
    /// must be SHORTER than `'long` (the builder's own lifetime
    /// parameter, which describes how long it can hold its own
    /// internal references). Keeping them distinct lets the outer
    /// driver re-borrow `b` after `FnLower` drops.
    pub b: &'short mut FunctionBuilder<'long>,
    pub vars: HashMap<Local, Variable>,
    pub blocks: HashMap<BlockId, cranelift_codegen::ir::Block>,
}

impl<'short, 'long, 'a, 'm, 'p, M: Module> FnLower<'short, 'long, 'a, 'm, 'p, M> {
    fn new(
        mod_ctx: &'a mut LowerCtx<'m, M>,
        prog: &'p Program,
        f: &'p Function,
        b: &'short mut FunctionBuilder<'long>,
    ) -> CompileResult<Self> {
        Ok(Self {
            mod_ctx,
            prog,
            f,
            b,
            vars: HashMap::new(),
            blocks: HashMap::new(),
        })
    }

    fn ensure_block(&mut self, id: BlockId) -> cranelift_codegen::ir::Block {
        if let Some(b) = self.blocks.get(&id) {
            return *b;
        }
        let blk = self.b.create_block();
        self.blocks.insert(id, blk);
        blk
    }

    fn ensure_var(&mut self, l: Local) -> Variable {
        if let Some(v) = self.vars.get(&l) {
            return *v;
        }
        let ty = cl_ty_for(&self.f.locals[l.0 as usize].ty);
        let var = self.b.declare_var(ty);
        self.vars.insert(l, var);
        var
    }

    /// Seal all blocks and finalize the FunctionBuilder. Called once at
    /// the end of lowering so the cranelift builder's borrow naturally
    /// expires when this FnLower drops.
    fn finalize_builder(&mut self) {
        self.b.seal_all_blocks();
    }

    fn lower_blocks(&mut self) -> CompileResult<()> {
        // Create entry block & seed param values.
        let entry = self.ensure_block(self.f.entry);
        self.b.append_block_params_for_function_params(entry);
        self.b.switch_to_block(entry);

        // Seed variable values for each param (in declaration order).
        let entry_params: Vec<_> = self.b.block_params(entry).to_vec();
        for (idx, local) in self.f.params.iter().enumerate() {
            let var = self.ensure_var(*local);
            if let Some(val) = entry_params.get(idx) {
                self.b.def_var(var, *val);
            }
        }

        // Pre-create all blocks so terminators can target them.
        for blk in &self.f.blocks {
            let _ = self.ensure_block(blk.id);
        }

        // Lower each block.
        // We need to clone block ids first to avoid double-borrow.
        let block_ids: Vec<_> = self.f.blocks.iter().map(|b| b.id).collect();
        for id in block_ids {
            let cl_blk = self.blocks[&id];
            self.b.switch_to_block(cl_blk);
            self.lower_one_block(id)?;
        }
        Ok(())
    }

    fn lower_one_block(&mut self, id: BlockId) -> CompileResult<()> {
        // We index into self.f.blocks; the SIR block is borrowed by
        // index to avoid a clone.
        let idx = self
            .f
            .blocks
            .iter()
            .position(|b| b.id == id)
            .ok_or_else(|| CodegenError::Module(format!("missing block {:?}", id)))?;
        // Lower statements.
        let stmt_count = self.f.blocks[idx].stmts.len();
        for s in 0..stmt_count {
            let stmt = self.f.blocks[idx].stmts[s].clone();
            self.lower_stmt(&stmt)?;
        }
        let term = self.f.blocks[idx].terminator.clone();
        self.lower_term(&term)?;
        Ok(())
    }

    fn lower_stmt(&mut self, stmt: &Stmt) -> CompileResult<()> {
        match stmt {
            Stmt::Nop | Stmt::StorageLive(_) | Stmt::StorageDead(_) | Stmt::Drop(_) => Ok(()),
            Stmt::ArenaPush(_) => {
                self.call_rt_no_args("stardust_runtime_arena_push", Some(ct::I64))?;
                Ok(())
            }
            Stmt::ArenaPop(_) => {
                let zero = self.b.ins().iconst(ct::I64, 0);
                self.call_rt("stardust_runtime_arena_pop", &[zero], None)?;
                Ok(())
            }
            Stmt::Assign(place, rv) => self.lower_assign(place, rv),
            Stmt::EffectInvoke { .. } => Err(CodegenError::Unsupported(
                "effect invoke at native lowering".into(),
            )),
        }
    }

    fn lower_term(&mut self, term: &Term) -> CompileResult<()> {
        match term {
            Term::Goto(blk) => {
                let target = self.ensure_block(*blk);
                self.b.ins().jump(target, &[]);
                Ok(())
            }
            Term::If { cond, then, else_ } => {
                let c = self.eval_operand(cond)?;
                let t = self.ensure_block(*then);
                let e = self.ensure_block(*else_);
                self.b.ins().brif(c, t, &[], e, &[]);
                Ok(())
            }
            Term::Return(op) => {
                if matches!(self.f.ret_ty, SirTy::Unit | SirTy::Never) {
                    self.b.ins().return_(&[]);
                } else {
                    let v = self.eval_operand(op)?;
                    self.b.ins().return_(&[v]);
                }
                Ok(())
            }
            Term::Unreachable => {
                self.b.ins().trap(cranelift_codegen::ir::TrapCode::user(1).unwrap());
                Ok(())
            }
            Term::Panic { msg } => {
                let v = self.eval_operand(msg)?;
                let zero = self.b.ins().iconst(ct::I64, 0);
                self.call_rt("stardust_runtime_panic", &[v, zero], None)?;
                self.b.ins().trap(cranelift_codegen::ir::TrapCode::user(2).unwrap());
                Ok(())
            }
            Term::TryReturnErr(_) => Err(CodegenError::Unsupported("? propagation".into())),
            Term::SwitchInt { discr, arms, default } => {
                let disc = self.eval_operand(discr)?;
                let mut else_block = self.ensure_block(*default);
                // Lower as a chain of brifs (small switch).
                for (val, target) in arms {
                    let next = self.b.create_block();
                    let lit = self.b.ins().iconst(ct::I64, *val as i64);
                    let cmp = self
                        .b
                        .ins()
                        .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, disc, lit);
                    let tgt = self.ensure_block(*target);
                    self.b.ins().brif(cmp, tgt, &[], next, &[]);
                    self.b.switch_to_block(next);
                    self.b.seal_block(next);
                    else_block = next;
                }
                let default_block = self.ensure_block(*default);
                self.b.ins().jump(default_block, &[]);
                let _ = else_block;
                Ok(())
            }
            Term::SwitchVariant { .. } => Err(CodegenError::Unsupported("switch variant".into())),
            Term::Suspend { .. } => Err(CodegenError::Unsupported("async suspend".into())),
        }
    }

    fn lower_assign(&mut self, place: &Place, rv: &Rvalue) -> CompileResult<()> {
        // Slice-8 only supports flat local writes (no projections).
        if !place.proj.is_empty() {
            return Err(CodegenError::Unsupported(
                "place projection at native lowering".into(),
            ));
        }
        let val = self.eval_rvalue(rv)?;
        let var = self.ensure_var(place.local);
        // Type-coerce: if rvalue produced a different type, coerce.
        let want = cl_ty_for(&self.f.locals[place.local.0 as usize].ty);
        let val = self.coerce_to(val, want);
        self.b.def_var(var, val);
        Ok(())
    }

    fn coerce_to(
        &mut self,
        val: cranelift_codegen::ir::Value,
        want: cranelift_codegen::ir::Type,
    ) -> cranelift_codegen::ir::Value {
        let have = self.b.func.dfg.value_type(val);
        if have == want {
            return val;
        }
        // Integer widening / narrowing only — slice-8 doesn't need
        // float<->int yet.
        if have.is_int() && want.is_int() {
            if have.bits() < want.bits() {
                return self.b.ins().sextend(want, val);
            }
            if have.bits() > want.bits() {
                return self.b.ins().ireduce(want, val);
            }
        }
        // Fall through with original; cranelift may still verify ok
        // for compatible types.
        val
    }

    fn eval_rvalue(&mut self, rv: &Rvalue) -> CompileResult<cranelift_codegen::ir::Value> {
        match rv {
            Rvalue::Use(op) => self.eval_operand(op),
            Rvalue::Const(c) => self.eval_const(c),
            Rvalue::BinOp(op, a, b) => {
                let av = self.eval_operand(a)?;
                let bv = self.eval_operand(b)?;
                self.lower_binop(*op, av, bv)
            }
            Rvalue::UnOp(op, a) => {
                let av = self.eval_operand(a)?;
                self.lower_unop(*op, av)
            }
            Rvalue::Call { func, args } => self.lower_call(func, args),
            Rvalue::Cast { src, ty } => {
                let v = self.eval_operand(src)?;
                let want = cl_ty_for(ty);
                Ok(self.coerce_to(v, want))
            }
            Rvalue::Ref { .. }
            | Rvalue::Deref(_)
            | Rvalue::AdtInit { .. }
            | Rvalue::TupleInit(_)
            | Rvalue::ArrayInit(_)
            | Rvalue::FieldRead { .. }
            | Rvalue::TupleRead { .. }
            | Rvalue::IndexRead { .. }
            | Rvalue::MethodCall { .. }
            | Rvalue::AgentSpawn { .. }
            | Rvalue::Send { .. }
            | Rvalue::Ask { .. }
            | Rvalue::CapValue { .. } => Err(CodegenError::Unsupported(format!(
                "rvalue {:?} at native lowering",
                std::mem::discriminant(rv)
            ))),
        }
    }

    fn eval_const(&mut self, c: &Const) -> CompileResult<cranelift_codegen::ir::Value> {
        Ok(match c {
            Const::Unit => self.b.ins().iconst(ct::I64, 0),
            Const::Bool(b) => self.b.ins().iconst(ct::I8, if *b { 1 } else { 0 }),
            Const::Int(v, k) => {
                let t = cl_ty_for(&SirTy::Int(*k));
                self.b.ins().iconst(t, *v as i64)
            }
            Const::Float(v, k) => match k {
                sdust_types::FloatKind::F32 => self.b.ins().f32const(*v as f32),
                sdust_types::FloatKind::F64 | sdust_types::FloatKind::FloatInfer => {
                    self.b.ins().f64const(*v)
                }
            },
            Const::Char(c) => self.b.ins().iconst(ct::I32, *c as i64),
            Const::Str(s) => {
                let id = self.mod_ctx.intern_string(s)?;
                let gv = self
                    .mod_ctx
                    .module
                    .declare_data_in_func(id, self.b.func);
                self.b.ins().symbol_value(ct::I64, gv)
            }
            Const::Duration { value, .. } | Const::Size { value, .. } => {
                self.b.ins().iconst(ct::I64, *value as i64)
            }
            Const::FnPtr(_) => {
                return Err(CodegenError::Unsupported("fn-pointer const".into()))
            }
            Const::NullPtr => self.b.ins().iconst(ct::I64, 0),
        })
    }

    fn eval_operand(&mut self, op: &Operand) -> CompileResult<cranelift_codegen::ir::Value> {
        match op {
            Operand::Copy(p) | Operand::Move(p) => {
                if !p.proj.is_empty() {
                    return Err(CodegenError::Unsupported("operand projection".into()));
                }
                let var = self.ensure_var(p.local);
                Ok(self.b.use_var(var))
            }
            Operand::Const(c) => self.eval_const(c),
        }
    }

    fn lower_binop(
        &mut self,
        op: BinOp,
        a: cranelift_codegen::ir::Value,
        b: cranelift_codegen::ir::Value,
    ) -> CompileResult<cranelift_codegen::ir::Value> {
        use cranelift_codegen::ir::condcodes::IntCC::*;
        // Promote both to the wider type.
        let ta = self.b.func.dfg.value_type(a);
        let tb = self.b.func.dfg.value_type(b);
        let (a, b) = if ta.bits() == tb.bits() {
            (a, b)
        } else if ta.bits() < tb.bits() {
            (self.b.ins().sextend(tb, a), b)
        } else {
            (a, self.b.ins().sextend(ta, b))
        };
        Ok(match op {
            BinOp::Add => self.b.ins().iadd(a, b),
            BinOp::Sub => self.b.ins().isub(a, b),
            BinOp::Mul => self.b.ins().imul(a, b),
            BinOp::Div => self.b.ins().sdiv(a, b),
            BinOp::Rem => self.b.ins().srem(a, b),
            BinOp::BitAnd | BinOp::And => self.b.ins().band(a, b),
            BinOp::BitOr | BinOp::Or => self.b.ins().bor(a, b),
            BinOp::BitXor => self.b.ins().bxor(a, b),
            BinOp::Shl => self.b.ins().ishl(a, b),
            BinOp::Shr => self.b.ins().sshr(a, b),
            BinOp::Eq => self.b.ins().icmp(Equal, a, b),
            BinOp::Ne => self.b.ins().icmp(NotEqual, a, b),
            BinOp::Lt => self.b.ins().icmp(SignedLessThan, a, b),
            BinOp::Le => self.b.ins().icmp(SignedLessThanOrEqual, a, b),
            BinOp::Gt => self.b.ins().icmp(SignedGreaterThan, a, b),
            BinOp::Ge => self.b.ins().icmp(SignedGreaterThanOrEqual, a, b),
        })
    }

    fn lower_unop(
        &mut self,
        op: UnOp,
        v: cranelift_codegen::ir::Value,
    ) -> CompileResult<cranelift_codegen::ir::Value> {
        Ok(match op {
            UnOp::Neg => self.b.ins().ineg(v),
            UnOp::Not => {
                // Logical not: cmp against zero.
                let vt = self.b.func.dfg.value_type(v);
                let z = self.b.ins().iconst(vt, 0);
                self.b
                    .ins()
                    .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, v, z)
            }
        })
    }

    fn lower_call(
        &mut self,
        func: &FnRef,
        args: &[Operand],
    ) -> CompileResult<cranelift_codegen::ir::Value> {
        match func {
            FnRef::Builtin(BuiltinId::Log) | FnRef::Builtin(BuiltinId::Print) => {
                // Args are (str). Slice-8 expects exactly one Operand
                // that is either a Const::Str or a local of Str type.
                if args.len() != 1 {
                    return Err(CodegenError::Unsupported(format!(
                        "log/print arity {}",
                        args.len()
                    )));
                }
                // Get (ptr, len) pair.
                let (ptr, len) = self.string_pair(&args[0])?;
                let sym = if matches!(func, FnRef::Builtin(BuiltinId::Log)) {
                    "stardust_runtime_log"
                } else {
                    "stardust_runtime_print"
                };
                self.call_rt(sym, &[ptr, len], None)?;
                Ok(self.b.ins().iconst(ct::I64, 0))
            }
            FnRef::Builtin(BuiltinId::Panic) => {
                if args.len() != 1 {
                    return Err(CodegenError::Unsupported("panic arity".into()));
                }
                let (ptr, len) = self.string_pair(&args[0])?;
                self.call_rt("stardust_runtime_panic", &[ptr, len], None)?;
                Ok(self.b.ins().iconst(ct::I64, 0))
            }
            FnRef::User(callee_id) => {
                let func_id = *self.mod_ctx.fn_ids.get(callee_id).ok_or_else(|| {
                    CodegenError::Module(format!("call to undeclared fn {:?}", callee_id))
                })?;
                let func_ref = self
                    .mod_ctx
                    .module
                    .declare_func_in_func(func_id, self.b.func);
                let mut arg_vals = Vec::with_capacity(args.len());
                for a in args {
                    arg_vals.push(self.eval_operand(a)?);
                }
                let call = self.b.ins().call(func_ref, &arg_vals);
                let results = self.b.inst_results(call).to_vec();
                Ok(results
                    .first()
                    .copied()
                    .unwrap_or_else(|| self.b.ins().iconst(ct::I64, 0)))
            }
            FnRef::Builtin(_) => Err(CodegenError::Unsupported(format!(
                "builtin {:?} at native lowering",
                func
            ))),
        }
    }

    /// Extract the (ptr, len) pair for a string operand. Slice-8 only
    /// supports `Const::Str` literals here — locals carrying String
    /// values aren't yet representable.
    fn string_pair(
        &mut self,
        op: &Operand,
    ) -> CompileResult<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value)> {
        match op {
            Operand::Const(Const::Str(s)) => {
                let id = self.mod_ctx.intern_string(s)?;
                let gv = self
                    .mod_ctx
                    .module
                    .declare_data_in_func(id, self.b.func);
                let ptr = self.b.ins().symbol_value(ct::I64, gv);
                let len = self.b.ins().iconst(ct::I64, s.len() as i64);
                Ok((ptr, len))
            }
            _ => Err(CodegenError::Unsupported(
                "non-literal string in log/print".into(),
            )),
        }
    }

    fn call_rt(
        &mut self,
        name: &'static str,
        args: &[cranelift_codegen::ir::Value],
        _ret_ty: Option<cranelift_codegen::ir::Type>,
    ) -> CompileResult<Option<cranelift_codegen::ir::Value>> {
        let fid = *self
            .mod_ctx
            .runtime_ids
            .get(name)
            .ok_or_else(|| CodegenError::MissingImport(name.into()))?;
        let fref = self.mod_ctx.module.declare_func_in_func(fid, self.b.func);
        let call = self.b.ins().call(fref, args);
        Ok(self.b.inst_results(call).first().copied())
    }

    fn call_rt_no_args(
        &mut self,
        name: &'static str,
        _ret_ty: Option<cranelift_codegen::ir::Type>,
    ) -> CompileResult<Option<cranelift_codegen::ir::Value>> {
        self.call_rt(name, &[], _ret_ty)
    }
}

/// Construct cranelift codegen flags suitable for slice-8.
/// `is_pic` controls whether the resulting code is position-independent.
pub fn default_flags(is_pic: bool) -> cranelift_codegen::settings::Flags {
    let mut b = settings::builder();
    let _ = b.set("opt_level", "speed");
    let _ = b.set(
        "is_pic",
        if is_pic { "true" } else { "false" },
    );
    settings::Flags::new(b)
}

/// Pre-declare every fn that an empty cranelift `Sentinel` will need.
/// Helper used by the integration tests.
pub fn empty_main_sig(triple: &Triple) -> Signature {
    let mut s = Signature::new(host_call_conv(triple));
    s.returns.push(AbiParam::new(ct::I64));
    s
}

// Re-exports for convenience.
pub use crate::abi::host_call_conv as cc_for_triple;

// MemFlags helper for future loads/stores (slice-8 currently unused).
#[allow(dead_code)]
fn trusted_mem_flags() -> MemFlags {
    MemFlags::trusted()
}
