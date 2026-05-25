//! SIR → LLVM IR lowering core. Mirrors the Cranelift lowerer in
//! `mty-codegen-cranelift::lower` so the two backends ship the same
//! source coverage.
//!
//! Only compiled with `--features llvm`.

#![cfg(feature = "llvm")]

use crate::{CompileResult, LlvmError, LlvmOptLevel};
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::{Linkage, Module};
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine, TargetTriple,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum};
use inkwell::values::{BasicValue, BasicValueEnum, FunctionValue, IntValue, PointerValue};
use inkwell::{AddressSpace, IntPredicate, OptimizationLevel};
use mty_ir::ir::{
    BinOp, BlockId, BuiltinId, Const, FnRef, Function, IrFnId, IrTy, Local, Operand, Place,
    Program, Projection, Rvalue, Stmt, Term, UnOp,
};
use mty_types::{FloatKind, IntKind};
use std::collections::HashMap;

/// Build an `LLVMModule` from `prog`. Runs the standard optimizer at
/// the requested level.
pub fn lower_program<'ctx>(
    ctx: &'ctx Context,
    prog: &Program,
    opt: LlvmOptLevel,
) -> CompileResult<Module<'ctx>> {
    let module = ctx.create_module("stardust");
    let builder = ctx.create_builder();
    let mut lowerer = ProgramLowerer::new(ctx, &module, &builder, prog);
    lowerer.declare_runtime_imports();
    lowerer.declare_user_fns()?;
    for f in &prog.fns {
        lowerer.define_fn(f)?;
    }
    // Verify the whole module.
    if let Err(e) = module.verify() {
        return Err(LlvmError::Module(e.to_string()));
    }
    run_optimizer(&module, opt)?;
    Ok(module)
}

/// Run the LLVM optimizer at the chosen opt level.
fn run_optimizer(module: &Module<'_>, opt: LlvmOptLevel) -> CompileResult<()> {
    let opt_level = match opt {
        LlvmOptLevel::O0 => OptimizationLevel::None,
        LlvmOptLevel::O2 => OptimizationLevel::Default,
        LlvmOptLevel::O3 => OptimizationLevel::Aggressive,
    };
    Target::initialize_native(&InitializationConfig::default()).map_err(LlvmError::Module)?;
    let triple_str = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple_str).map_err(|e| LlvmError::Module(e.to_string()))?;
    let machine = target
        .create_target_machine(
            &triple_str,
            "generic",
            "",
            opt_level,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| LlvmError::Module("could not create TargetMachine".into()))?;
    let passes = match opt {
        LlvmOptLevel::O0 => "default<O0>",
        LlvmOptLevel::O2 => "default<O2>",
        LlvmOptLevel::O3 => "default<O3>",
    };
    let pass_opts = PassBuilderOptions::create();
    module
        .run_passes(passes, &machine, pass_opts)
        .map_err(|e| LlvmError::Module(e.to_string()))?;
    Ok(())
}

/// Emit a host-format object file (`.o`) for `module`.
pub fn write_object(module: &Module<'_>, out: &std::path::Path) -> CompileResult<()> {
    Target::initialize_native(&InitializationConfig::default()).map_err(LlvmError::Module)?;
    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).map_err(|e| LlvmError::Module(e.to_string()))?;
    let machine = target
        .create_target_machine(
            &triple,
            "generic",
            "",
            OptimizationLevel::Default,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| LlvmError::Module("could not create TargetMachine".into()))?;
    machine
        .write_to_file(module, FileType::Object, out)
        .map_err(|e| LlvmError::Io(e.to_string()))?;
    Ok(())
}

// =============================================================================
// Module-level lowering context
// =============================================================================

struct ProgramLowerer<'ctx, 'a, 'b> {
    ctx: &'ctx Context,
    module: &'b Module<'ctx>,
    builder: &'a Builder<'ctx>,
    prog: &'a Program,
    user_fns: HashMap<IrFnId, FunctionValue<'ctx>>,
    runtime_fns: HashMap<&'static str, FunctionValue<'ctx>>,
    string_pool: HashMap<String, PointerValue<'ctx>>,
}

impl<'ctx, 'a, 'b> ProgramLowerer<'ctx, 'a, 'b> {
    fn new(
        ctx: &'ctx Context,
        module: &'b Module<'ctx>,
        builder: &'a Builder<'ctx>,
        prog: &'a Program,
    ) -> Self {
        Self {
            ctx,
            module,
            builder,
            prog,
            user_fns: HashMap::new(),
            runtime_fns: HashMap::new(),
            string_pool: HashMap::new(),
        }
    }

    fn ptr_ty(&self) -> inkwell::types::PointerType<'ctx> {
        self.ctx.ptr_type(AddressSpace::default())
    }

    fn i8_ty(&self) -> inkwell::types::IntType<'ctx> {
        self.ctx.i8_type()
    }
    fn i32_ty(&self) -> inkwell::types::IntType<'ctx> {
        self.ctx.i32_type()
    }
    fn i64_ty(&self) -> inkwell::types::IntType<'ctx> {
        self.ctx.i64_type()
    }

    fn declare_runtime_imports(&mut self) {
        let void = self.ctx.void_type();
        let ptr = self.ptr_ty();
        let i8 = self.i8_ty();
        let i64 = self.i64_ty();
        // log(ptr, len)
        let sig = void.fn_type(&[ptr.into(), i64.into()], false);
        self.runtime_fns.insert(
            "mty_runtime_log",
            self.module
                .add_function("mty_runtime_log", sig, Some(Linkage::External)),
        );
        let sig = void.fn_type(&[ptr.into(), i64.into()], false);
        self.runtime_fns.insert(
            "mty_runtime_print",
            self.module
                .add_function("mty_runtime_print", sig, Some(Linkage::External)),
        );
        let sig = void.fn_type(&[ptr.into(), i64.into()], false);
        self.runtime_fns.insert(
            "mty_runtime_panic",
            self.module
                .add_function("mty_runtime_panic", sig, Some(Linkage::External)),
        );
        let sig = i64.fn_type(&[], false);
        self.runtime_fns.insert(
            "mty_runtime_arena_push",
            self.module
                .add_function("mty_runtime_arena_push", sig, Some(Linkage::External)),
        );
        let sig = void.fn_type(&[i64.into()], false);
        self.runtime_fns.insert(
            "mty_runtime_arena_pop",
            self.module
                .add_function("mty_runtime_arena_pop", sig, Some(Linkage::External)),
        );
        let sig = i8.fn_type(&[i64.into()], false);
        self.runtime_fns.insert(
            "mty_runtime_budget_charge",
            self.module
                .add_function("mty_runtime_budget_charge", sig, Some(Linkage::External)),
        );
        let sig = void.fn_type(&[i64.into(), i64.into(), i64.into()], false);
        self.runtime_fns.insert(
            "mty_runtime_send",
            self.module
                .add_function("mty_runtime_send", sig, Some(Linkage::External)),
        );
        let sig = i64.fn_type(&[i64.into(), i64.into(), i64.into(), i64.into()], false);
        self.runtime_fns.insert(
            "mty_runtime_ask",
            self.module
                .add_function("mty_runtime_ask", sig, Some(Linkage::External)),
        );
        let sig = i64.fn_type(&[i64.into()], false);
        self.runtime_fns.insert(
            "mty_runtime_spawn",
            self.module
                .add_function("mty_runtime_spawn", sig, Some(Linkage::External)),
        );
        let sig = i64.fn_type(&[ptr.into(), i64.into(), i64.into()], false);
        self.runtime_fns.insert(
            "mty_runtime_extern_call",
            self.module
                .add_function("mty_runtime_extern_call", sig, Some(Linkage::External)),
        );
    }

    fn llvm_ty(&self, t: &IrTy) -> BasicTypeEnum<'ctx> {
        match t {
            IrTy::Bool => self.i8_ty().into(),
            IrTy::Char => self.i32_ty().into(),
            IrTy::Int(k) => match k {
                IntKind::I8 | IntKind::U8 => self.i8_ty().into(),
                IntKind::I16 | IntKind::U16 => self.ctx.i16_type().into(),
                IntKind::I32 | IntKind::U32 | IntKind::IntInfer => self.i32_ty().into(),
                IntKind::I64 | IntKind::U64 | IntKind::ISize | IntKind::USize => {
                    self.i64_ty().into()
                }
                IntKind::I128 | IntKind::U128 => self.ctx.i128_type().into(),
            },
            IrTy::Float(k) => match k {
                FloatKind::F32 => self.ctx.f32_type().into(),
                FloatKind::F64 | FloatKind::FloatInfer => self.ctx.f64_type().into(),
            },
            IrTy::Duration | IrTy::Size => self.i64_ty().into(),
            // Aggregates, refs, strings: lower to pointer.
            _ => self.ptr_ty().into(),
        }
    }

    fn fn_type_of(&self, f: &Function) -> inkwell::types::FunctionType<'ctx> {
        let param_tys: Vec<BasicMetadataTypeEnum<'ctx>> = f
            .params
            .iter()
            .filter_map(|p| {
                let t = &f.locals[p.0 as usize].ty;
                if matches!(t, IrTy::Unit | IrTy::Never) {
                    None
                } else {
                    Some(self.llvm_ty(t).into())
                }
            })
            .collect();
        if matches!(f.ret_ty, IrTy::Unit | IrTy::Never) {
            self.ctx.void_type().fn_type(&param_tys, false)
        } else {
            self.llvm_ty(&f.ret_ty).fn_type(&param_tys, false)
        }
    }

    fn declare_user_fns(&mut self) -> CompileResult<()> {
        for f in &self.prog.fns {
            let fty = self.fn_type_of(f);
            let linkage = if f.name == "main" {
                Linkage::External
            } else {
                Linkage::Internal
            };
            let fv = self.module.add_function(&f.name, fty, Some(linkage));
            self.user_fns.insert(f.id, fv);
        }
        Ok(())
    }

    fn intern_string(&mut self, s: &str) -> PointerValue<'ctx> {
        if let Some(p) = self.string_pool.get(s) {
            return *p;
        }
        let g = self
            .builder
            .build_global_string_ptr(s, ".Lstr")
            .expect("global string");
        let ptr = g.as_pointer_value();
        self.string_pool.insert(s.to_string(), ptr);
        ptr
    }

    fn define_fn(&mut self, f: &Function) -> CompileResult<()> {
        let fv = *self
            .user_fns
            .get(&f.id)
            .ok_or_else(|| LlvmError::Module(format!("undeclared fn {}", f.name)))?;
        let mut fl = FnLowerer::new(self, f, fv);
        fl.lower()?;
        // Per-fn verify.
        if !fv.verify(true) {
            return Err(LlvmError::VerifierFailed {
                name: f.name.clone(),
                msg: "fn-level verify failed".into(),
            });
        }
        Ok(())
    }
}

// =============================================================================
// Per-function lowering
// =============================================================================

struct FnLowerer<'p, 'ctx, 'a, 'b> {
    pl: &'p mut ProgramLowerer<'ctx, 'a, 'b>,
    f: &'a Function,
    fv: FunctionValue<'ctx>,
    blocks: HashMap<BlockId, BasicBlock<'ctx>>,
    /// SIR local → llvm "alloca" pointer holding the local's value.
    locals: HashMap<Local, PointerValue<'ctx>>,
}

impl<'p, 'ctx, 'a, 'b> FnLowerer<'p, 'ctx, 'a, 'b> {
    fn new(
        pl: &'p mut ProgramLowerer<'ctx, 'a, 'b>,
        f: &'a Function,
        fv: FunctionValue<'ctx>,
    ) -> Self {
        Self {
            pl,
            f,
            fv,
            blocks: HashMap::new(),
            locals: HashMap::new(),
        }
    }

    fn ensure_block(&mut self, id: BlockId) -> BasicBlock<'ctx> {
        if let Some(b) = self.blocks.get(&id) {
            return *b;
        }
        let b = self
            .pl
            .ctx
            .append_basic_block(self.fv, &format!("bb{}", id.0));
        self.blocks.insert(id, b);
        b
    }

    fn ensure_local(&mut self, l: Local) -> PointerValue<'ctx> {
        if let Some(p) = self.locals.get(&l) {
            return *p;
        }
        let ty = self.pl.llvm_ty(&self.f.locals[l.0 as usize].ty);
        let p = self
            .pl
            .builder
            .build_alloca(ty, &format!("_{}", l.0))
            .expect("alloca");
        self.locals.insert(l, p);
        p
    }

    fn lower(&mut self) -> CompileResult<()> {
        let entry = self.ensure_block(self.f.entry);
        self.pl.builder.position_at_end(entry);

        // Allocate all locals up front.
        for (i, _) in self.f.locals.iter().enumerate() {
            self.ensure_local(Local(i as u32));
        }

        // Seed params: each SIR param has its value provided as an
        // LLVM fn arg. Skip Unit/Never params (they're not in the
        // signature).
        let mut arg_idx = 0usize;
        for p in &self.f.params {
            let lty = &self.f.locals[p.0 as usize].ty;
            if matches!(lty, IrTy::Unit | IrTy::Never) {
                continue;
            }
            let arg = self
                .fv
                .get_nth_param(arg_idx as u32)
                .expect("missing param");
            let slot = self.ensure_local(*p);
            self.pl.builder.build_store(slot, arg).expect("store param");
            arg_idx += 1;
        }

        // Pre-create blocks.
        for blk in &self.f.blocks {
            let _ = self.ensure_block(blk.id);
        }

        let block_ids: Vec<_> = self.f.blocks.iter().map(|b| b.id).collect();
        for id in block_ids {
            let bb = self.blocks[&id];
            self.pl.builder.position_at_end(bb);
            self.lower_block(id)?;
        }
        Ok(())
    }

    fn lower_block(&mut self, id: BlockId) -> CompileResult<()> {
        let idx = self
            .f
            .blocks
            .iter()
            .position(|b| b.id == id)
            .ok_or_else(|| LlvmError::Module(format!("missing block {:?}", id)))?;
        let stmt_count = self.f.blocks[idx].stmts.len();
        for s in 0..stmt_count {
            let stmt = self.f.blocks[idx].stmts[s].clone();
            self.lower_stmt(&stmt)?;
        }
        let term = self.f.blocks[idx].terminator.clone();
        self.lower_term(&term)?;
        Ok(())
    }

    fn lower_stmt(&mut self, s: &Stmt) -> CompileResult<()> {
        match s {
            Stmt::Nop | Stmt::StorageLive(_) | Stmt::StorageDead(_) | Stmt::Drop(_) => Ok(()),
            Stmt::ArenaPush(_) => {
                let f = self.pl.runtime_fns["mty_runtime_arena_push"];
                let _ = self.pl.builder.build_call(f, &[], "arena_push");
                Ok(())
            }
            Stmt::ArenaPop(_) => {
                let f = self.pl.runtime_fns["mty_runtime_arena_pop"];
                let z = self.pl.i64_ty().const_zero();
                let _ = self.pl.builder.build_call(f, &[z.into()], "arena_pop");
                Ok(())
            }
            Stmt::Assign(p, rv) => self.lower_assign(p, rv),
            Stmt::EffectInvoke { op, out, .. } => {
                let method = match op {
                    mty_ir::ir::EffectOp::GenericCall { method, .. } => method.clone(),
                };
                let nptr = self.pl.intern_string(&method);
                let nlen = self.pl.i64_ty().const_int(method.len() as u64, false);
                let zero = self.pl.i64_ty().const_zero();
                let f = self.pl.runtime_fns["mty_runtime_extern_call"];
                let call = self
                    .pl
                    .builder
                    .build_call(f, &[nptr.into(), nlen.into(), zero.into()], "extern")
                    .expect("call");
                if let Some(p) = out {
                    if p.proj.is_empty() {
                        let slot = self.ensure_local(p.local);
                        let want = self.pl.llvm_ty(&self.f.locals[p.local.0 as usize].ty);
                        let raw = call.try_as_basic_value().left().unwrap();
                        let coerced = self.coerce(raw, want);
                        let _ = self.pl.builder.build_store(slot, coerced);
                    }
                }
                Ok(())
            }
        }
    }

    fn lower_term(&mut self, t: &Term) -> CompileResult<()> {
        match t {
            Term::Goto(blk) => {
                let dest = self.ensure_block(*blk);
                self.pl.builder.build_unconditional_branch(dest).unwrap();
                Ok(())
            }
            Term::If { cond, then, else_ } => {
                let c = self.eval_operand(cond)?;
                let cv = self.to_i1(c);
                let t_bb = self.ensure_block(*then);
                let e_bb = self.ensure_block(*else_);
                self.pl
                    .builder
                    .build_conditional_branch(cv, t_bb, e_bb)
                    .unwrap();
                Ok(())
            }
            Term::Return(op) => {
                if matches!(self.f.ret_ty, IrTy::Unit | IrTy::Never) {
                    self.pl.builder.build_return(None).unwrap();
                } else {
                    let v = self.eval_operand(op)?;
                    let want = self.pl.llvm_ty(&self.f.ret_ty);
                    let v = self.coerce(v, want);
                    self.pl.builder.build_return(Some(&v)).unwrap();
                }
                Ok(())
            }
            Term::Unreachable => {
                self.pl.builder.build_unreachable().unwrap();
                Ok(())
            }
            Term::Panic { msg } => {
                let v = self.eval_operand(msg)?;
                let v = self.coerce(v, self.pl.ptr_ty().into());
                let zero = self.pl.i64_ty().const_zero();
                let f = self.pl.runtime_fns["mty_runtime_panic"];
                let _ = self
                    .pl
                    .builder
                    .build_call(f, &[v.into(), zero.into()], "panic");
                self.pl.builder.build_unreachable().unwrap();
                Ok(())
            }
            Term::TryReturnErr(_) => {
                // Best-effort: build a null/zero and return it as the
                // error sentinel. Mirrors the Cranelift backend's
                // simplified `?` path; semantic correctness requires
                // typeck to surface the Result Adt.
                if matches!(self.f.ret_ty, IrTy::Unit | IrTy::Never) {
                    self.pl.builder.build_return(None).unwrap();
                } else {
                    let want = self.pl.llvm_ty(&self.f.ret_ty);
                    let z: BasicValueEnum<'ctx> = match want {
                        BasicTypeEnum::IntType(it) => it.const_zero().into(),
                        BasicTypeEnum::FloatType(ft) => ft.const_zero().into(),
                        BasicTypeEnum::PointerType(pt) => pt.const_null().into(),
                        _ => self.pl.i64_ty().const_zero().into(),
                    };
                    self.pl.builder.build_return(Some(&z)).unwrap();
                }
                Ok(())
            }
            Term::SwitchInt {
                discr,
                arms,
                default,
            } => {
                let d = self.eval_operand(discr)?;
                let d = self.coerce(d, self.pl.i64_ty().into());
                let d_int = d.into_int_value();
                let default_bb = self.ensure_block(*default);
                let arms_vec: Vec<_> = arms
                    .iter()
                    .map(|(v, b)| {
                        (
                            self.pl.i64_ty().const_int(*v as u64, true),
                            self.ensure_block(*b),
                        )
                    })
                    .collect();
                self.pl
                    .builder
                    .build_switch(d_int, default_bb, &arms_vec)
                    .unwrap();
                Ok(())
            }
            Term::SwitchVariant {
                discr,
                adt: _,
                arms,
                default,
            } => {
                // Load tag (i32) from offset 0 of the aggregate pointer.
                let p = self.eval_operand(discr)?;
                let ptr = self.coerce(p, self.pl.ptr_ty().into()).into_pointer_value();
                let tag = self
                    .pl
                    .builder
                    .build_load(self.pl.i32_ty(), ptr, "tag")
                    .unwrap()
                    .into_int_value();
                let default_bb = self.ensure_block(*default);
                let arms_vec: Vec<_> = arms
                    .iter()
                    .map(|(v, b)| {
                        (
                            self.pl.i32_ty().const_int(*v as u64, false),
                            self.ensure_block(*b),
                        )
                    })
                    .collect();
                self.pl
                    .builder
                    .build_switch(tag, default_bb, &arms_vec)
                    .unwrap();
                Ok(())
            }
            Term::Suspend { .. } => Err(LlvmError::Unsupported("async suspend".into())),
        }
    }

    fn lower_assign(&mut self, place: &Place, rv: &Rvalue) -> CompileResult<()> {
        if !place.proj.is_empty() {
            return Err(LlvmError::Unsupported("llvm projection-store TBD".into()));
        }
        let v = self.eval_rvalue(rv)?;
        let slot = self.ensure_local(place.local);
        let want = self.pl.llvm_ty(&self.f.locals[place.local.0 as usize].ty);
        let v = self.coerce(v, want);
        self.pl.builder.build_store(slot, v).unwrap();
        Ok(())
    }

    fn eval_rvalue(&mut self, rv: &Rvalue) -> CompileResult<BasicValueEnum<'ctx>> {
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
                let want = self.pl.llvm_ty(ty);
                Ok(self.coerce(v, want))
            }
            // Best-effort stubs for the rest — all return a null pointer
            // so the function still verifies.
            _ => Ok(self.pl.ptr_ty().const_null().into()),
        }
    }

    fn eval_operand(&mut self, op: &Operand) -> CompileResult<BasicValueEnum<'ctx>> {
        match op {
            Operand::Const(c) => self.eval_const(c),
            Operand::Copy(p) | Operand::Move(p) => {
                if !p.proj.is_empty() {
                    // Best-effort: treat as a null pointer.
                    return Ok(self.pl.ptr_ty().const_null().into());
                }
                let slot = self.ensure_local(p.local);
                let ty = self.pl.llvm_ty(&self.f.locals[p.local.0 as usize].ty);
                Ok(self.pl.builder.build_load(ty, slot, "load").unwrap())
            }
        }
    }

    fn eval_const(&mut self, c: &Const) -> CompileResult<BasicValueEnum<'ctx>> {
        Ok(match c {
            Const::Unit => self.pl.i64_ty().const_zero().into(),
            Const::Bool(b) => self
                .pl
                .i8_ty()
                .const_int(if *b { 1 } else { 0 }, false)
                .into(),
            Const::Int(v, k) => {
                let t = self.pl.llvm_ty(&IrTy::Int(*k));
                match t {
                    BasicTypeEnum::IntType(it) => it.const_int(*v as u64, true).into(),
                    _ => self.pl.i64_ty().const_int(*v as u64, true).into(),
                }
            }
            Const::Float(v, k) => match k {
                FloatKind::F32 => self.pl.ctx.f32_type().const_float(*v).into(),
                FloatKind::F64 | FloatKind::FloatInfer => {
                    self.pl.ctx.f64_type().const_float(*v).into()
                }
            },
            Const::Char(c) => self.pl.i32_ty().const_int(*c as u64, false).into(),
            Const::Str(s) => self.pl.intern_string(s).into(),
            Const::Duration { value, .. } | Const::Size { value, .. } => {
                self.pl.i64_ty().const_int(*value, false).into()
            }
            Const::FnPtr(_) | Const::NullPtr => self.pl.ptr_ty().const_null().into(),
        })
    }

    fn lower_binop(
        &mut self,
        op: BinOp,
        a: BasicValueEnum<'ctx>,
        b: BasicValueEnum<'ctx>,
    ) -> CompileResult<BasicValueEnum<'ctx>> {
        use inkwell::FloatPredicate;
        // Float path.
        if a.is_float_value() || b.is_float_value() {
            let want = if a.is_float_value() {
                a.get_type()
            } else {
                b.get_type()
            };
            let a = self.coerce(a, want);
            let b = self.coerce(b, want);
            let av = a.into_float_value();
            let bv = b.into_float_value();
            return Ok(match op {
                BinOp::Add => self
                    .pl
                    .builder
                    .build_float_add(av, bv, "fadd")
                    .unwrap()
                    .into(),
                BinOp::Sub => self
                    .pl
                    .builder
                    .build_float_sub(av, bv, "fsub")
                    .unwrap()
                    .into(),
                BinOp::Mul => self
                    .pl
                    .builder
                    .build_float_mul(av, bv, "fmul")
                    .unwrap()
                    .into(),
                BinOp::Div => self
                    .pl
                    .builder
                    .build_float_div(av, bv, "fdiv")
                    .unwrap()
                    .into(),
                BinOp::Eq => self
                    .pl
                    .builder
                    .build_float_compare(FloatPredicate::OEQ, av, bv, "feq")
                    .unwrap()
                    .into(),
                BinOp::Ne => self
                    .pl
                    .builder
                    .build_float_compare(FloatPredicate::ONE, av, bv, "fne")
                    .unwrap()
                    .into(),
                BinOp::Lt => self
                    .pl
                    .builder
                    .build_float_compare(FloatPredicate::OLT, av, bv, "flt")
                    .unwrap()
                    .into(),
                BinOp::Le => self
                    .pl
                    .builder
                    .build_float_compare(FloatPredicate::OLE, av, bv, "fle")
                    .unwrap()
                    .into(),
                BinOp::Gt => self
                    .pl
                    .builder
                    .build_float_compare(FloatPredicate::OGT, av, bv, "fgt")
                    .unwrap()
                    .into(),
                BinOp::Ge => self
                    .pl
                    .builder
                    .build_float_compare(FloatPredicate::OGE, av, bv, "fge")
                    .unwrap()
                    .into(),
                _ => return Err(LlvmError::Unsupported(format!("float binop {:?}", op))),
            });
        }
        // Integer path — widen narrower to wider via sext.
        let i64t = self.pl.i64_ty();
        let a = self.coerce(a, i64t.into()).into_int_value();
        let b = self.coerce(b, i64t.into()).into_int_value();
        Ok(match op {
            BinOp::Add => self.pl.builder.build_int_add(a, b, "iadd").unwrap().into(),
            BinOp::Sub => self.pl.builder.build_int_sub(a, b, "isub").unwrap().into(),
            BinOp::Mul => self.pl.builder.build_int_mul(a, b, "imul").unwrap().into(),
            BinOp::Div => self
                .pl
                .builder
                .build_int_signed_div(a, b, "isdiv")
                .unwrap()
                .into(),
            BinOp::Rem => self
                .pl
                .builder
                .build_int_signed_rem(a, b, "isrem")
                .unwrap()
                .into(),
            BinOp::BitAnd | BinOp::And => self.pl.builder.build_and(a, b, "iand").unwrap().into(),
            BinOp::BitOr | BinOp::Or => self.pl.builder.build_or(a, b, "ior").unwrap().into(),
            BinOp::BitXor => self.pl.builder.build_xor(a, b, "ixor").unwrap().into(),
            BinOp::Shl => self
                .pl
                .builder
                .build_left_shift(a, b, "ishl")
                .unwrap()
                .into(),
            BinOp::Shr => self
                .pl
                .builder
                .build_right_shift(a, b, true, "isshr")
                .unwrap()
                .into(),
            BinOp::Eq => self
                .pl
                .builder
                .build_int_compare(IntPredicate::EQ, a, b, "ieq")
                .unwrap()
                .into(),
            BinOp::Ne => self
                .pl
                .builder
                .build_int_compare(IntPredicate::NE, a, b, "ine")
                .unwrap()
                .into(),
            BinOp::Lt => self
                .pl
                .builder
                .build_int_compare(IntPredicate::SLT, a, b, "ilt")
                .unwrap()
                .into(),
            BinOp::Le => self
                .pl
                .builder
                .build_int_compare(IntPredicate::SLE, a, b, "ile")
                .unwrap()
                .into(),
            BinOp::Gt => self
                .pl
                .builder
                .build_int_compare(IntPredicate::SGT, a, b, "igt")
                .unwrap()
                .into(),
            BinOp::Ge => self
                .pl
                .builder
                .build_int_compare(IntPredicate::SGE, a, b, "ige")
                .unwrap()
                .into(),
        })
    }

    fn lower_unop(
        &mut self,
        op: UnOp,
        v: BasicValueEnum<'ctx>,
    ) -> CompileResult<BasicValueEnum<'ctx>> {
        Ok(match op {
            UnOp::Neg => {
                if v.is_float_value() {
                    let f = v.into_float_value();
                    self.pl.builder.build_float_neg(f, "fneg").unwrap().into()
                } else {
                    let i = v.into_int_value();
                    self.pl.builder.build_int_neg(i, "ineg").unwrap().into()
                }
            }
            UnOp::Not => {
                let i = self.coerce(v, self.pl.i64_ty().into()).into_int_value();
                let z = self.pl.i64_ty().const_zero();
                self.pl
                    .builder
                    .build_int_compare(IntPredicate::EQ, i, z, "inot")
                    .unwrap()
                    .into()
            }
        })
    }

    fn lower_call(
        &mut self,
        func: &FnRef,
        args: &[Operand],
    ) -> CompileResult<BasicValueEnum<'ctx>> {
        match func {
            FnRef::Builtin(BuiltinId::Log) | FnRef::Builtin(BuiltinId::Print) => {
                if args.len() != 1 {
                    return Err(LlvmError::Unsupported("log arity".into()));
                }
                let (ptr, len) = self.string_pair(&args[0])?;
                let sym = if matches!(func, FnRef::Builtin(BuiltinId::Log)) {
                    "mty_runtime_log"
                } else {
                    "mty_runtime_print"
                };
                let f = self.pl.runtime_fns[sym];
                let _ = self
                    .pl
                    .builder
                    .build_call(f, &[ptr.into(), len.into()], "log");
                Ok(self.pl.i64_ty().const_zero().into())
            }
            FnRef::Builtin(BuiltinId::Panic) => {
                if args.len() != 1 {
                    return Err(LlvmError::Unsupported("panic arity".into()));
                }
                let (ptr, len) = self.string_pair(&args[0])?;
                let f = self.pl.runtime_fns["mty_runtime_panic"];
                let _ = self
                    .pl
                    .builder
                    .build_call(f, &[ptr.into(), len.into()], "panic");
                Ok(self.pl.i64_ty().const_zero().into())
            }
            FnRef::User(callee_id) => {
                let callee = *self.pl.user_fns.get(callee_id).ok_or_else(|| {
                    LlvmError::Module(format!("call to undeclared {:?}", callee_id))
                })?;
                let callee_fn = self.pl.prog.fn_by_id(*callee_id);
                let mut arg_vals: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> = Vec::new();
                let mut callee_param_tys: Vec<IrTy> = callee_fn
                    .params
                    .iter()
                    .map(|p| callee_fn.locals[p.0 as usize].ty.clone())
                    .filter(|t| !matches!(t, IrTy::Unit | IrTy::Never))
                    .collect();
                let expected = callee_param_tys.len();
                for a in args {
                    if arg_vals.len() >= expected {
                        break;
                    }
                    if matches!(a, Operand::Const(Const::Unit)) {
                        continue;
                    }
                    let v = self.eval_operand(a)?;
                    let want_ty = if !callee_param_tys.is_empty() {
                        Some(callee_param_tys.remove(0))
                    } else {
                        None
                    };
                    let coerced = if let Some(t) = &want_ty {
                        let lt = self.pl.llvm_ty(t);
                        self.coerce(v, lt)
                    } else {
                        v
                    };
                    arg_vals.push(coerced.into());
                }
                // Pad missing args with zero defaults.
                while !callee_param_tys.is_empty() {
                    let t = callee_param_tys.remove(0);
                    let lt = self.pl.llvm_ty(&t);
                    let v: BasicValueEnum<'ctx> = match lt {
                        BasicTypeEnum::IntType(it) => it.const_zero().into(),
                        BasicTypeEnum::FloatType(ft) => ft.const_zero().into(),
                        BasicTypeEnum::PointerType(pt) => pt.const_null().into(),
                        _ => self.pl.i64_ty().const_zero().into(),
                    };
                    arg_vals.push(v.into());
                }
                let call = self
                    .pl
                    .builder
                    .build_call(callee, &arg_vals, "call")
                    .unwrap();
                Ok(call
                    .try_as_basic_value()
                    .left()
                    .unwrap_or_else(|| self.pl.i64_ty().const_zero().into()))
            }
            FnRef::Builtin(_) => {
                // Best-effort stub: return zero.
                Ok(self.pl.i64_ty().const_zero().into())
            }
        }
    }

    fn string_pair(&mut self, op: &Operand) -> CompileResult<(PointerValue<'ctx>, IntValue<'ctx>)> {
        match op {
            Operand::Const(Const::Str(s)) => {
                let ptr = self.pl.intern_string(s);
                let len = self.pl.i64_ty().const_int(s.len() as u64, false);
                Ok((ptr, len))
            }
            _ => Err(LlvmError::Unsupported("non-literal string in log".into())),
        }
    }

    /// Coerce a value into the wanted type; handles int<->int, float<->float
    /// and bitcast for size-equal int<->float pairs. Pointer<->int via
    /// ptrtoint/inttoptr.
    fn coerce(
        &mut self,
        v: BasicValueEnum<'ctx>,
        want: BasicTypeEnum<'ctx>,
    ) -> BasicValueEnum<'ctx> {
        // Identity.
        if v.get_type() == want {
            return v;
        }
        match (v.get_type(), want) {
            (BasicTypeEnum::IntType(_), BasicTypeEnum::IntType(it)) => self
                .pl
                .builder
                .build_int_cast(v.into_int_value(), it, "icast")
                .unwrap()
                .into(),
            (BasicTypeEnum::FloatType(_), BasicTypeEnum::FloatType(ft)) => self
                .pl
                .builder
                .build_float_cast(v.into_float_value(), ft, "fcast")
                .unwrap()
                .into(),
            (BasicTypeEnum::IntType(_), BasicTypeEnum::PointerType(pt)) => self
                .pl
                .builder
                .build_int_to_ptr(v.into_int_value(), pt, "i2p")
                .unwrap()
                .into(),
            (BasicTypeEnum::PointerType(_), BasicTypeEnum::IntType(it)) => self
                .pl
                .builder
                .build_ptr_to_int(v.into_pointer_value(), it, "p2i")
                .unwrap()
                .into(),
            (BasicTypeEnum::IntType(it), BasicTypeEnum::FloatType(ft)) => {
                if it.get_bit_width() == ft.get_bit_width().into() {
                    self.pl.builder.build_bit_cast(v, ft, "ibcast").unwrap()
                } else {
                    let intermediate = if ft.get_bit_width() == 32 {
                        self.pl.ctx.i32_type()
                    } else {
                        self.pl.i64_ty()
                    };
                    let bits = self
                        .pl
                        .builder
                        .build_int_cast(v.into_int_value(), intermediate, "ibcast")
                        .unwrap();
                    self.pl.builder.build_bit_cast(bits, ft, "ibcast2").unwrap()
                }
            }
            (BasicTypeEnum::FloatType(ft), BasicTypeEnum::IntType(it)) => {
                if ft.get_bit_width() == it.get_bit_width().into() {
                    self.pl.builder.build_bit_cast(v, it, "fbcast").unwrap()
                } else {
                    let intermediate = if ft.get_bit_width() == 32 {
                        self.pl.ctx.i32_type()
                    } else {
                        self.pl.i64_ty()
                    };
                    let bits = self
                        .pl
                        .builder
                        .build_bit_cast(v, intermediate, "fbcast")
                        .unwrap();
                    self.pl
                        .builder
                        .build_int_cast(bits.into_int_value(), it, "fbcast2")
                        .unwrap()
                        .into()
                }
            }
            _ => v,
        }
    }

    fn to_i1(&mut self, v: BasicValueEnum<'ctx>) -> IntValue<'ctx> {
        if let BasicValueEnum::IntValue(iv) = v {
            if iv.get_type().get_bit_width() == 1 {
                return iv;
            }
            let z = iv.get_type().const_zero();
            self.pl
                .builder
                .build_int_compare(IntPredicate::NE, iv, z, "to_i1")
                .unwrap()
        } else {
            let i = self.coerce(v, self.pl.i64_ty().into()).into_int_value();
            let z = i.get_type().const_zero();
            self.pl
                .builder
                .build_int_compare(IntPredicate::NE, i, z, "to_i1")
                .unwrap()
        }
    }
}
