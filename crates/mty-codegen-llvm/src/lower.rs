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
    // v0.36 T4 — LLVM module name is internal (consumed only by our
    // own `module.verify()` and the JIT/object emitter). Renamed
    // from `"stardust"` to `"mighty"` to align with the brand.
    let module = ctx.create_module("mighty");
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
        // v0.40 T2 — typed-slot Vec storage. Header (32 bytes) is heap
        // allocated through the arena alloc so the pointer stays stable
        // across `push` growth. Matches cranelift's symbol exactly.
        let sig = i64.fn_type(&[i64.into(), i64.into(), i64.into()], false);
        self.runtime_fns.insert(
            "mty_runtime_alloc",
            self.module
                .add_function("mty_runtime_alloc", sig, Some(Linkage::External)),
        );
        // v0.42 T4 (L23 fix) — typed log/print/format runtime surface.
        // The cranelift backend declares the same symbols in
        // `crates/mty-codegen-cranelift/src/runtime_imports.rs`; both
        // backends resolve them against the same Rust impls in
        // `mty_runtime::codegen_abi`.
        let i32t = self.i32_ty();
        let f32t = self.ctx.f32_type();
        let f64t = self.ctx.f64_type();
        // log_* (newline). Each call: void(value).
        let cases_log: &[(&str, BasicMetadataTypeEnum<'ctx>)] = &[
            ("mty_runtime_log_i32", i32t.into()),
            ("mty_runtime_log_u32", i32t.into()),
            ("mty_runtime_log_u64", i64.into()),
            ("mty_runtime_log_usize", i64.into()),
            ("mty_runtime_log_f32", f32t.into()),
            ("mty_runtime_log_f64", f64t.into()),
            ("mty_runtime_log_bool", i8.into()),
            ("mty_runtime_print_i32", i32t.into()),
            ("mty_runtime_print_i64", i64.into()),
            ("mty_runtime_print_u32", i32t.into()),
            ("mty_runtime_print_u64", i64.into()),
            ("mty_runtime_print_usize", i64.into()),
            ("mty_runtime_print_f32", f32t.into()),
            ("mty_runtime_print_f64", f64t.into()),
            ("mty_runtime_print_bool", i8.into()),
        ];
        for (name, pt) in cases_log {
            let s = void.fn_type(&[*pt], false);
            self.runtime_fns.insert(
                name,
                self.module.add_function(name, s, Some(Linkage::External)),
            );
        }
        // print_sep / print_newline — void().
        for name in ["mty_runtime_print_sep", "mty_runtime_print_newline"] {
            let s = void.fn_type(&[], false);
            self.runtime_fns.insert(
                name,
                self.module.add_function(name, s, Some(Linkage::External)),
            );
        }
        // fmt_* — void(value, dst_ptr_as_i64).
        let cases_fmt: &[(&str, BasicMetadataTypeEnum<'ctx>)] = &[
            ("mty_runtime_fmt_i32", i32t.into()),
            ("mty_runtime_fmt_i64_to_slot", i64.into()),
            ("mty_runtime_fmt_u32", i32t.into()),
            ("mty_runtime_fmt_u64", i64.into()),
            ("mty_runtime_fmt_usize", i64.into()),
            ("mty_runtime_fmt_f32", f32t.into()),
            ("mty_runtime_fmt_f64", f64t.into()),
            ("mty_runtime_fmt_bool", i8.into()),
        ];
        for (name, pt) in cases_fmt {
            let s = void.fn_type(&[*pt, i64.into()], false);
            self.runtime_fns.insert(
                name,
                self.module.add_function(name, s, Some(Linkage::External)),
            );
        }
        // str_concat — void(aptr, alen, bptr, blen, dst). All i64
        // (pointers are passed as i64 to match the cranelift signature
        // and the runtime impl).
        let s = void.fn_type(
            &[i64.into(), i64.into(), i64.into(), i64.into(), i64.into()],
            false,
        );
        self.runtime_fns.insert(
            "mty_runtime_str_concat",
            self.module
                .add_function("mty_runtime_str_concat", s, Some(Linkage::External)),
        );
        // v0.45 T1 (L18 fix) — native `std.fs.*` surface. Each call
        // lowers to one of the symbols declared below; the parameter
        // shapes mirror the cranelift `runtime_imports::RUNTIME_IMPORTS`
        // entries.
        //   read* / read_dir : void(path_ptr_i64, path_len_i64, dst_i64)
        //   write* / append  : i32 (path_ptr, path_len, data_ptr, data_len)
        //   exists / mkdir / rm: i32 (path_ptr, path_len)
        //   metadata         : i32 (path_ptr, path_len, dst_slot)
        for name in [
            "mty_runtime_fs_read",
            "mty_runtime_fs_read_to_string",
            "mty_runtime_fs_read_dir",
        ] {
            let s = void.fn_type(&[i64.into(), i64.into(), i64.into()], false);
            self.runtime_fns.insert(
                name,
                self.module.add_function(name, s, Some(Linkage::External)),
            );
        }
        for name in [
            "mty_runtime_fs_write",
            "mty_runtime_fs_write_string",
            "mty_runtime_fs_append",
        ] {
            let s = i32t.fn_type(&[i64.into(), i64.into(), i64.into(), i64.into()], false);
            self.runtime_fns.insert(
                name,
                self.module.add_function(name, s, Some(Linkage::External)),
            );
        }
        for name in [
            "mty_runtime_fs_exists",
            "mty_runtime_fs_create_dir_all",
            "mty_runtime_fs_remove_file",
            "mty_runtime_fs_remove_dir_all",
        ] {
            let s = i32t.fn_type(&[i64.into(), i64.into()], false);
            self.runtime_fns.insert(
                name,
                self.module.add_function(name, s, Some(Linkage::External)),
            );
        }
        {
            let s = i32t.fn_type(&[i64.into(), i64.into(), i64.into()], false);
            self.runtime_fns.insert(
                "mty_runtime_fs_metadata",
                self.module
                    .add_function("mty_runtime_fs_metadata", s, Some(Linkage::External)),
            );
        }
        // v0.46 T4 — read_dir iterator handle ABI.
        // dir_open : (path_ptr_i64, path_len_i64) -> i64 (handle, 0 = err)
        {
            let s = i64.fn_type(&[i64.into(), i64.into()], false);
            self.runtime_fns.insert(
                "mty_runtime_fs_dir_open",
                self.module
                    .add_function("mty_runtime_fs_dir_open", s, Some(Linkage::External)),
            );
        }
        // dir_next : (handle, dst_slot_i64) -> i32
        {
            let s = i32t.fn_type(&[i64.into(), i64.into()], false);
            self.runtime_fns.insert(
                "mty_runtime_fs_dir_next",
                self.module
                    .add_function("mty_runtime_fs_dir_next", s, Some(Linkage::External)),
            );
        }
        // dir_close : (handle) -> ()
        {
            let s = void.fn_type(&[i64.into()], false);
            self.runtime_fns.insert(
                "mty_runtime_fs_dir_close",
                self.module
                    .add_function("mty_runtime_fs_dir_close", s, Some(Linkage::External)),
            );
        }
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

    fn define_fn(&mut self, f: &'a Function) -> CompileResult<()> {
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
    /// v0.40 T2 — destination SIR type of the current assignment, so
    /// `Vec.new()` can pluck `T` out of the LHS `Vec[T]` and seed the
    /// typed-slot header's `elem_size@24` word. Mirrors cranelift's
    /// `current_dest_ty`. Pushed/popped around `eval_rvalue`.
    current_dest_ty: Option<IrTy>,
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
            current_dest_ty: None,
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

        // v0.42 T1 — mirror the cranelift backend's L1/L28 fix
        // (`crates/mty-codegen-cranelift/src/lower.rs::lower_blocks`):
        // `mty_runtime_alloc` returns 0 when no arena frame is active,
        // so a plain `fn main()` doing `Vec.new()` / `String.with_capacity()`
        // would dereference null under a built native binary (LLVM
        // backend included). Auto-push an implicit arena frame at the
        // entry block of `main`. SIR's explicit `ArenaPush`/`ArenaPop`
        // pair already nests around source-level `arena {}` blocks, so
        // we only push for `main` (not every fn). The frame is
        // implicitly torn down at process exit, matching the cranelift
        // path and the JIT lifetime.
        let is_main = self.f.name == "main";
        let entry_id = self.f.entry;
        let block_ids: Vec<_> = self.f.blocks.iter().map(|b| b.id).collect();
        for id in block_ids {
            let bb = self.blocks[&id];
            self.pl.builder.position_at_end(bb);
            if is_main && id == entry_id {
                let f = self.pl.runtime_fns["mty_runtime_arena_push"];
                let _ = self.pl.builder.build_call(f, &[], "auto_arena_push");
            }
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
            Stmt::EffectInvoke { op, args, out, .. } => {
                let (path, method) = match op {
                    mty_ir::ir::EffectOp::GenericCall { path, method } => {
                        (path.clone(), method.clone())
                    }
                };
                let full_name = if path.is_empty() {
                    method.clone()
                } else {
                    format!("{}.{}", path.join("."), method)
                };
                // v0.45 T1 (L18 fix) — native `std.fs.*` dispatch.
                // Mirrors the cranelift path: every fs method routes
                // through its dedicated runtime symbol so JIT/AOT
                // outputs touch disk without an interpreter fallback.
                if is_native_fs_method_llvm(&full_name) {
                    return self.emit_fs_call_llvm(&full_name, args, out.as_ref());
                }
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
                    // v0.37 T5: pass the source SIR type so the coerce
                    // path can pick `zext` (unsigned widening) for U8 /
                    // U16 / U32 / U64 returns. Pre-fix the LLVM backend
                    // unconditionally used `build_int_cast` (signed) —
                    // same bug v0.36 T1 fixed for cranelift.
                    let src_ty = self.operand_ir_ty(op);
                    let v = self.coerce_with_src(v, want, src_ty.as_ref());
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
        // v0.40 T2 — pin the destination local's SIR type around the
        // rvalue evaluation so `Vec.new()` can read `Vec[T]` and seed
        // the typed-slot header's elem_size word.
        let local_ty = self.f.locals[place.local.0 as usize].ty.clone();
        let prev_dest = self.current_dest_ty.take();
        self.current_dest_ty = Some(local_ty);
        let v = self.eval_rvalue(rv)?;
        self.current_dest_ty = prev_dest;
        let slot = self.ensure_local(place.local);
        let want = self.pl.llvm_ty(&self.f.locals[place.local.0 as usize].ty);
        // v0.37 T5: prefer unsigned widening (`zext`) when the rvalue's
        // source SIR type is an unsigned integer. For non-Use rvalues
        // (BinOp/Cast/Call results) we don't try to thread further —
        // those paths already pick the right signedness internally.
        let src_ty = match rv {
            Rvalue::Use(op) => self.operand_ir_ty(op),
            _ => None,
        };
        let v = self.coerce_with_src(v, want, src_ty.as_ref());
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
                let sa = self.operand_ir_ty(a);
                let sb = self.operand_ir_ty(b);
                self.lower_binop_typed(*op, av, bv, sa.as_ref(), sb.as_ref())
            }
            Rvalue::UnOp(op, a) => {
                let av = self.eval_operand(a)?;
                self.lower_unop(*op, av)
            }
            Rvalue::Call { func, args } => self.lower_call(func, args),
            // v0.40 T2 — native growable `Vec` ops. The receiver
            // evaluates to the i64 header pointer produced by
            // `emit_vec_new`; `push`/`set`/`clear` mutate it in place
            // and return the *same* pointer, so the
            // `v = v.push(x)` capture-rebind threads a stable value
            // across the loop back-edge. Mirrors cranelift's
            // Rvalue::MethodCall arm.
            Rvalue::MethodCall {
                receiver,
                method,
                args,
            } => match method.as_str() {
                "push" => self.emit_vec_push(receiver, args),
                "len" => self.emit_vec_len(receiver),
                "get" => self.emit_vec_get(receiver, args),
                "set" => self.emit_vec_set(receiver, args),
                "pop" => self.emit_vec_pop(receiver),
                "clear" => self.emit_vec_clear(receiver),
                _ => Ok(self.pl.i64_ty().const_zero().into()),
            },
            Rvalue::Cast { src, ty } => {
                let v = self.eval_operand(src)?;
                let want = self.pl.llvm_ty(ty);
                let src_ty = self.operand_ir_ty(src);
                Ok(self.cast_value(v, want, src_ty.as_ref(), ty))
            }
            // v0.37 Track T3 — LLVM backend doesn't model Str's
            // (ptr,len) split; treat StrPtr as a pass-through, mirroring
            // the wasm backend. The cranelift native path handles real
            // pointer extraction.
            Rvalue::StrPtr(src) => self.eval_operand(src),
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

    /// Lower a SIR `BinOp` to an LLVM instruction sequence. Knows the
    /// SIR types of the two operands and uses them to pick:
    /// - unsigned vs signed widening (`zext` vs `sext`) when the
    ///   operands have different LLVM bit widths,
    /// - unsigned vs signed division / remainder
    ///   (`build_int_unsigned_div/rem` vs `build_int_signed_div/rem`),
    /// - unsigned vs signed comparisons (`ULT/ULE/UGT/UGE` vs
    ///   `SLT/SLE/SGT/SGE`),
    /// - logical vs arithmetic right shift (`build_right_shift` with
    ///   `sign_extend = false` vs `true`).
    ///
    /// v0.37 T5 — fix for the U8 widening bug + downstream unsigned
    /// op-semantics on the LLVM backend. Mirrors v0.36 T1's cranelift
    /// `lower_binop_typed`.
    fn lower_binop_typed(
        &mut self,
        op: BinOp,
        a: BasicValueEnum<'ctx>,
        b: BasicValueEnum<'ctx>,
        sa: Option<&IrTy>,
        sb: Option<&IrTy>,
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
        let ua = sa.is_some_and(Self::is_unsigned_int_ty);
        let ub = sb.is_some_and(Self::is_unsigned_int_ty);
        // If *either* operand is known unsigned, treat the op as
        // unsigned. (Mixing signed + unsigned is itself a typeck error
        // upstream; this fallback is just defensive.)
        let unsigned = ua || ub;
        // Integer path — widen both sides to i64, using each side's
        // own signedness for the extend choice. (The cranelift path
        // widens to the wider of the two; here we always widen to i64
        // because the existing LLVM lowering normalises arithmetic at
        // i64 — preserving that contract while threading signedness.)
        let i64t = self.pl.i64_ty();
        let a = self.coerce_with_src(a, i64t.into(), sa).into_int_value();
        let b = self.coerce_with_src(b, i64t.into(), sb).into_int_value();
        Ok(match op {
            BinOp::Add => self.pl.builder.build_int_add(a, b, "iadd").unwrap().into(),
            BinOp::Sub => self.pl.builder.build_int_sub(a, b, "isub").unwrap().into(),
            BinOp::Mul => self.pl.builder.build_int_mul(a, b, "imul").unwrap().into(),
            BinOp::Div => {
                if unsigned {
                    self.pl
                        .builder
                        .build_int_unsigned_div(a, b, "iudiv")
                        .unwrap()
                        .into()
                } else {
                    self.pl
                        .builder
                        .build_int_signed_div(a, b, "isdiv")
                        .unwrap()
                        .into()
                }
            }
            BinOp::Rem => {
                if unsigned {
                    self.pl
                        .builder
                        .build_int_unsigned_rem(a, b, "iurem")
                        .unwrap()
                        .into()
                } else {
                    self.pl
                        .builder
                        .build_int_signed_rem(a, b, "isrem")
                        .unwrap()
                        .into()
                }
            }
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
                // `sign_extend = true` → arithmetic shift (ashr);
                // `false` → logical shift (lshr). Unsigned operands
                // want lshr so the high bits zero out instead of
                // propagating the sign bit.
                .build_right_shift(a, b, !unsigned, "ishr")
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
                .build_int_compare(
                    if unsigned {
                        IntPredicate::ULT
                    } else {
                        IntPredicate::SLT
                    },
                    a,
                    b,
                    "ilt",
                )
                .unwrap()
                .into(),
            BinOp::Le => self
                .pl
                .builder
                .build_int_compare(
                    if unsigned {
                        IntPredicate::ULE
                    } else {
                        IntPredicate::SLE
                    },
                    a,
                    b,
                    "ile",
                )
                .unwrap()
                .into(),
            BinOp::Gt => self
                .pl
                .builder
                .build_int_compare(
                    if unsigned {
                        IntPredicate::UGT
                    } else {
                        IntPredicate::SGT
                    },
                    a,
                    b,
                    "igt",
                )
                .unwrap()
                .into(),
            BinOp::Ge => self
                .pl
                .builder
                .build_int_compare(
                    if unsigned {
                        IntPredicate::UGE
                    } else {
                        IntPredicate::SGE
                    },
                    a,
                    b,
                    "ige",
                )
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
                // v0.42 T4 (L23 fix) — typed log/print lowering on the
                // LLVM backend. Mirrors the cranelift `lower_call`
                // arm: dispatch on each operand's SIR type to the
                // matching `mty_runtime_log_*` / `_print_*` runtime
                // symbol. Multi-arg uses `print_*` per element + a
                // trailing `print_newline` (for `log`). Str operands
                // still go through the literal-only `string_pair` —
                // the LLVM backend doesn't model dynamic Str
                // aggregates the way cranelift does.
                let is_log = matches!(func, FnRef::Builtin(BuiltinId::Log));
                let visible: Vec<&Operand> = args
                    .iter()
                    .filter(|a| !matches!(a, Operand::Const(Const::Unit)))
                    .collect();
                if visible.is_empty() {
                    if is_log {
                        let f = self.pl.runtime_fns["mty_runtime_print_newline"];
                        let _ = self.pl.builder.build_call(f, &[], "nl");
                    }
                    return Ok(self.pl.i64_ty().const_zero().into());
                }
                let single = visible.len() == 1;
                for (i, op) in visible.iter().enumerate() {
                    if i > 0 {
                        let f = self.pl.runtime_fns["mty_runtime_print_sep"];
                        let _ = self.pl.builder.build_call(f, &[], "sep");
                    }
                    self.emit_one_log_arg_llvm(op, single, is_log)?;
                }
                if !single && is_log {
                    let f = self.pl.runtime_fns["mty_runtime_print_newline"];
                    let _ = self.pl.builder.build_call(f, &[], "nl");
                }
                Ok(self.pl.i64_ty().const_zero().into())
            }
            FnRef::Builtin(BuiltinId::Extern(name))
                if name == "Vec.new" || name == "Vec.with_capacity" =>
            {
                // v0.40 T2 — `Vec.new()` / `Vec.with_capacity(n)`
                // construct a real native growable vector header.
                // Mirrors the cranelift backend's emit_vec_new dispatch.
                self.emit_vec_new()
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
                    // v0.37 T5: capture the operand's SIR type so the
                    // coerce path picks `zext` for unsigned widening
                    // (U8 → U32/U64 fn args were wrong pre-fix).
                    let src_ty = self.operand_ir_ty(a);
                    let want_ty = if !callee_param_tys.is_empty() {
                        Some(callee_param_tys.remove(0))
                    } else {
                        None
                    };
                    let coerced = if let Some(t) = &want_ty {
                        let lt = self.pl.llvm_ty(t);
                        self.coerce_with_src(v, lt, src_ty.as_ref())
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

    /// v0.42 T4 (L23 fix) — emit a single typed `log`/`print` arg on
    /// the LLVM backend. Mirrors the cranelift `emit_one_log_arg`.
    fn emit_one_log_arg_llvm(
        &mut self,
        op: &Operand,
        single_arg: bool,
        is_log: bool,
    ) -> CompileResult<()> {
        let op_ty = self.operand_ir_ty(op);
        let use_newline_variant = single_arg && is_log;
        match op_ty.as_ref() {
            Some(IrTy::Str | IrTy::String | IrTy::Bytes) => {
                // LLVM backend keeps the literal-only string_pair —
                // dynamic Str aggregates aren't modeled here yet. We
                // still route through it so literal `log("hi")` keeps
                // working end-to-end.
                let (ptr, len) = self.string_pair(op)?;
                let sym = if use_newline_variant {
                    "mty_runtime_log"
                } else {
                    "mty_runtime_print"
                };
                let f = self.pl.runtime_fns[sym];
                let _ = self
                    .pl
                    .builder
                    .build_call(f, &[ptr.into(), len.into()], "log");
            }
            Some(IrTy::Bool) => {
                let v = self.eval_operand(op)?;
                let v_i8 = self.coerce_with_src(v, self.pl.i8_ty().into(), op_ty.as_ref());
                let sym = if use_newline_variant {
                    "mty_runtime_log_bool"
                } else {
                    "mty_runtime_print_bool"
                };
                let f = self.pl.runtime_fns[sym];
                let _ = self.pl.builder.build_call(f, &[v_i8.into()], "log");
            }
            Some(IrTy::Char) => {
                // Treat Char as U32 — surface the codepoint value.
                let v = self.eval_operand(op)?;
                let v_i32 = self.coerce_with_src(v, self.pl.i32_ty().into(), op_ty.as_ref());
                let sym = if use_newline_variant {
                    "mty_runtime_log_u32"
                } else {
                    "mty_runtime_print_u32"
                };
                let f = self.pl.runtime_fns[sym];
                let _ = self.pl.builder.build_call(f, &[v_i32.into()], "log");
            }
            Some(IrTy::Int(k)) => {
                let (sym_log, sym_print, want_ty) = match k {
                    IntKind::I8 | IntKind::I16 | IntKind::I32 | IntKind::IntInfer => (
                        "mty_runtime_log_i32",
                        "mty_runtime_print_i32",
                        self.pl.i32_ty().as_basic_type_enum(),
                    ),
                    IntKind::I64 | IntKind::ISize => (
                        "mty_runtime_log_i64",
                        "mty_runtime_print_i64",
                        self.pl.i64_ty().as_basic_type_enum(),
                    ),
                    IntKind::U8 | IntKind::U16 | IntKind::U32 => (
                        "mty_runtime_log_u32",
                        "mty_runtime_print_u32",
                        self.pl.i32_ty().as_basic_type_enum(),
                    ),
                    IntKind::U64 => (
                        "mty_runtime_log_u64",
                        "mty_runtime_print_u64",
                        self.pl.i64_ty().as_basic_type_enum(),
                    ),
                    IntKind::USize => (
                        "mty_runtime_log_usize",
                        "mty_runtime_print_usize",
                        self.pl.i64_ty().as_basic_type_enum(),
                    ),
                    IntKind::I128 | IntKind::U128 => (
                        "mty_runtime_log_i64",
                        "mty_runtime_print_i64",
                        self.pl.i64_ty().as_basic_type_enum(),
                    ),
                };
                let v = self.eval_operand(op)?;
                let v = self.coerce_with_src(v, want_ty, op_ty.as_ref());
                let sym = if use_newline_variant {
                    sym_log
                } else {
                    sym_print
                };
                let f = self.pl.runtime_fns[sym];
                let _ = self.pl.builder.build_call(f, &[v.into()], "log");
            }
            Some(IrTy::Float(k)) => {
                let (sym_log, sym_print) = match k {
                    FloatKind::F32 => ("mty_runtime_log_f32", "mty_runtime_print_f32"),
                    FloatKind::F64 | FloatKind::FloatInfer => {
                        ("mty_runtime_log_f64", "mty_runtime_print_f64")
                    }
                };
                let v = self.eval_operand(op)?;
                let sym = if use_newline_variant {
                    sym_log
                } else {
                    sym_print
                };
                let f = self.pl.runtime_fns[sym];
                let _ = self.pl.builder.build_call(f, &[v.into()], "log");
            }
            Some(IrTy::Size | IrTy::Duration) => {
                let v = self.eval_operand(op)?;
                let v = self.coerce_with_src(v, self.pl.i64_ty().into(), op_ty.as_ref());
                let sym = if use_newline_variant {
                    "mty_runtime_log_usize"
                } else {
                    "mty_runtime_print_usize"
                };
                let f = self.pl.runtime_fns[sym];
                let _ = self.pl.builder.build_call(f, &[v.into()], "log");
            }
            Some(_) | None => {
                // Best-effort: fall back to the literal string path.
                if let Ok((ptr, len)) = self.string_pair(op) {
                    let sym = if use_newline_variant {
                        "mty_runtime_log"
                    } else {
                        "mty_runtime_print"
                    };
                    let f = self.pl.runtime_fns[sym];
                    let _ = self
                        .pl
                        .builder
                        .build_call(f, &[ptr.into(), len.into()], "log");
                }
            }
        }
        Ok(())
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

    /// v0.45 T1 (L18 fix) — lower a `std.fs.*` call to its native
    /// runtime ABI symbol on the LLVM backend.
    ///
    /// Mirrors the cranelift `emit_fs_call` ABI:
    ///   - read* / read_dir : write (ptr, len, ok) into a 24-byte
    ///     `alloca`'d slot; emit the slot address into the out place.
    ///   - write* / append  : (path, len, data, len) -> i32, stored
    ///     into the out place.
    ///   - exists / mkdir / rm: (path, len) -> i32.
    ///   - metadata         : 24-byte slot {size, mtime_ms, is_file,
    ///     is_dir} + i32 success return; slot address into the out
    ///     place.
    ///
    /// LLVM backend caveat: `string_pair` only supports literal Str
    /// operands (same limitation as the typed-log path). Dynamic-Str
    /// paths surface as `LlvmError::Unsupported`; the cranelift
    /// backend (used by `mty build` by default on the IDE's
    /// development triples) handles both shapes.
    fn emit_fs_call_llvm(
        &mut self,
        full_name: &str,
        args: &[Operand],
        out: Option<&Place>,
    ) -> CompileResult<()> {
        let Some(path_op) = args.first() else {
            return Err(LlvmError::Unsupported(format!(
                "{full_name}: missing path arg"
            )));
        };
        let (path_ptr, path_len) = self.string_pair(path_op)?;
        // Convert pointers to i64 for the runtime call (the runtime
        // ABI is uniformly i64-shaped).
        let path_ptr_i64 = self
            .pl
            .builder
            .build_ptr_to_int(path_ptr, self.pl.i64_ty(), "fs_path_ptr_i64")
            .unwrap();

        let kind = LlvmFsAbiKind::for_method(full_name);
        let i64_ty = self.pl.i64_ty();
        let i32_ty = self.pl.i32_ty();

        // Helper: alloca a 24-byte struct slot, returning its i64-ified
        // address so the runtime can write fields by raw offset.
        let alloc_slot24 = |this: &mut Self, name: &str| -> (PointerValue<'ctx>, IntValue<'ctx>) {
            let arr_ty = this.pl.ctx.i8_type().array_type(24);
            let slot = this.pl.builder.build_alloca(arr_ty, name).unwrap();
            let slot_i64 = this
                .pl
                .builder
                .build_ptr_to_int(slot, this.pl.i64_ty(), &format!("{name}_i64"))
                .unwrap();
            (slot, slot_i64)
        };

        let ret_kind: LlvmFsRet<'ctx> = match kind {
            LlvmFsAbiKind::ReadStrSlot { symbol } => {
                let (slot, slot_i64) = alloc_slot24(self, "fs_read_slot");
                let f = self.pl.runtime_fns[symbol];
                let _ = self.pl.builder.build_call(
                    f,
                    &[path_ptr_i64.into(), path_len.into(), slot_i64.into()],
                    "fs_call",
                );
                LlvmFsRet::Slot(slot)
            }
            LlvmFsAbiKind::WriteI32 { symbol } => {
                let Some(data_op) = args.get(1) else {
                    return Err(LlvmError::Unsupported(format!(
                        "{full_name}: missing data arg"
                    )));
                };
                let (data_ptr, data_len) = self.string_pair(data_op)?;
                let data_ptr_i64 = self
                    .pl
                    .builder
                    .build_ptr_to_int(data_ptr, i64_ty, "fs_data_ptr_i64")
                    .unwrap();
                let f = self.pl.runtime_fns[symbol];
                let call = self
                    .pl
                    .builder
                    .build_call(
                        f,
                        &[
                            path_ptr_i64.into(),
                            path_len.into(),
                            data_ptr_i64.into(),
                            data_len.into(),
                        ],
                        "fs_call",
                    )
                    .unwrap();
                let raw = call
                    .try_as_basic_value()
                    .left()
                    .unwrap_or_else(|| i32_ty.const_zero().into());
                LlvmFsRet::I32(raw)
            }
            LlvmFsAbiKind::PathI32 { symbol } => {
                let f = self.pl.runtime_fns[symbol];
                let call = self
                    .pl
                    .builder
                    .build_call(f, &[path_ptr_i64.into(), path_len.into()], "fs_call")
                    .unwrap();
                let raw = call
                    .try_as_basic_value()
                    .left()
                    .unwrap_or_else(|| i32_ty.const_zero().into());
                LlvmFsRet::I32(raw)
            }
            LlvmFsAbiKind::MetadataSlot { symbol } => {
                let (slot, slot_i64) = alloc_slot24(self, "fs_md_slot");
                let f = self.pl.runtime_fns[symbol];
                let _ = self.pl.builder.build_call(
                    f,
                    &[path_ptr_i64.into(), path_len.into(), slot_i64.into()],
                    "fs_call",
                );
                LlvmFsRet::Slot(slot)
            }
            LlvmFsAbiKind::DirOpenHandle { symbol } => {
                let f = self.pl.runtime_fns[symbol];
                let call = self
                    .pl
                    .builder
                    .build_call(f, &[path_ptr_i64.into(), path_len.into()], "fs_call")
                    .unwrap();
                let raw = call
                    .try_as_basic_value()
                    .left()
                    .unwrap_or_else(|| i64_ty.const_zero().into());
                LlvmFsRet::I64(raw)
            }
        };

        if let Some(p) = out {
            if p.proj.is_empty() {
                let slot = self.ensure_local(p.local);
                let want = self.pl.llvm_ty(&self.f.locals[p.local.0 as usize].ty);
                match ret_kind {
                    LlvmFsRet::Slot(slot_ptr) => {
                        // Out type is an aggregate (Str-shaped or
                        // record-shaped). Mighty stores aggregate locals
                        // as pointers; write the alloca's address into
                        // the local's slot.
                        let p_i64 = self
                            .pl
                            .builder
                            .build_ptr_to_int(slot_ptr, self.pl.i64_ty(), "fs_ret_i64")
                            .unwrap();
                        let coerced = self.coerce(p_i64.into(), want);
                        let _ = self.pl.builder.build_store(slot, coerced);
                    }
                    LlvmFsRet::I32(raw) => {
                        let coerced = self.coerce(raw, want);
                        let _ = self.pl.builder.build_store(slot, coerced);
                    }
                    LlvmFsRet::I64(raw) => {
                        // v0.46 T4 — DirIter handle. Coerce to the
                        // local's declared LLVM type (usually a
                        // pointer for the opaque `DirIter` ADT).
                        let coerced = self.coerce(raw, want);
                        let _ = self.pl.builder.build_store(slot, coerced);
                    }
                }
            }
        }
        Ok(())
    }

    /// SIR → LLVM type for an [`Operand`]. Returns `None` when the
    /// operand is a constant whose declared type doesn't pin a SIR
    /// type (rare — `Const::Unit` etc.). v0.37 T5: used by the binop /
    /// coerce_with_src paths to choose signed vs unsigned widening.
    fn operand_ir_ty(&self, op: &Operand) -> Option<IrTy> {
        match op {
            Operand::Copy(p) | Operand::Move(p) => {
                let mut ty = self.f.locals[p.local.0 as usize].ty.clone();
                for proj in &p.proj {
                    match proj {
                        Projection::Field(i) => {
                            if let IrTy::Adt(id, _) = &ty {
                                if let Some(adt) = self.pl.prog.adt_by_id(*id) {
                                    if let Some(v) = adt.variants.first() {
                                        if let Some(f) = v.fields.get(*i) {
                                            ty = f.ty.clone();
                                            continue;
                                        }
                                    }
                                }
                            }
                            return None;
                        }
                        Projection::TupleIndex(i) => {
                            if let IrTy::Tuple(elems) = &ty {
                                if let Some(t) = elems.get(*i) {
                                    ty = t.clone();
                                    continue;
                                }
                            }
                            return None;
                        }
                        Projection::Deref => {
                            if let IrTy::Ref { inner, .. } = &ty {
                                ty = (**inner).clone();
                                continue;
                            }
                            return None;
                        }
                        Projection::Index(_) => {
                            if let IrTy::Array { elem, .. } = &ty {
                                ty = (**elem).clone();
                                continue;
                            }
                            return None;
                        }
                        Projection::VariantField(v, f) => {
                            if let IrTy::Adt(id, _) = &ty {
                                if let Some(adt) = self.pl.prog.adt_by_id(*id) {
                                    if let Some(var) = adt.variants.get(*v) {
                                        if let Some(field) = var.fields.get(*f) {
                                            ty = field.ty.clone();
                                            continue;
                                        }
                                    }
                                }
                            }
                            return None;
                        }
                    }
                }
                Some(ty)
            }
            Operand::Const(c) => match c {
                Const::Int(_, k) => Some(IrTy::Int(*k)),
                Const::Float(_, k) => Some(IrTy::Float(*k)),
                Const::Bool(_) => Some(IrTy::Bool),
                Const::Char(_) => Some(IrTy::Char),
                Const::Str(_) => Some(IrTy::Str),
                Const::Duration { .. } => Some(IrTy::Duration),
                Const::Size { .. } => Some(IrTy::Size),
                Const::Unit => Some(IrTy::Unit),
                _ => None,
            },
        }
    }

    /// True iff `ty` is an unsigned integer (u8/u16/u32/u64/u128/usize).
    /// Returns false for signed ints, bools, chars, and non-integer
    /// types. v0.37 T5: used to pick `zext` vs `sext` (and unsigned vs
    /// signed div/rem/cmp/shr).
    fn is_unsigned_int_ty(ty: &IrTy) -> bool {
        matches!(
            ty,
            IrTy::Int(
                IntKind::U8
                    | IntKind::U16
                    | IntKind::U32
                    | IntKind::U64
                    | IntKind::U128
                    | IntKind::USize
            )
        )
    }

    /// Variant of [`Self::coerce`] that knows the SIR type of the source
    /// value, so it can pick `zext` (unsigned widening) for unsigned
    /// sources instead of the default `sext`. v0.37 T5 — fix for the
    /// LLVM-backend U8 → wider-int arithmetic / fn-arg / return widening
    /// bug, mirroring v0.36 T1's cranelift fix.
    fn coerce_with_src(
        &mut self,
        v: BasicValueEnum<'ctx>,
        want: BasicTypeEnum<'ctx>,
        src_ty: Option<&IrTy>,
    ) -> BasicValueEnum<'ctx> {
        if v.get_type() == want {
            return v;
        }
        if let (BasicTypeEnum::IntType(_), BasicTypeEnum::IntType(it)) = (v.get_type(), want) {
            let is_signed = !src_ty.is_some_and(Self::is_unsigned_int_ty);
            return self
                .pl
                .builder
                .build_int_cast_sign_flag(v.into_int_value(), it, is_signed, "icast")
                .unwrap()
                .into();
        }
        // Non-int-to-int paths fall through to the legacy coerce.
        self.coerce(v, want)
    }

    /// v0.42 T2 — value-preserving `as Ty` lowering used by
    /// `Rvalue::Cast`. Picks the right LLVM conversion instruction for
    /// each combination of source / destination kinds rather than the
    /// bit-preserving fallback in [`Self::coerce`]:
    ///
    /// | src       | dst       | LLVM instruction                       |
    /// |-----------|-----------|----------------------------------------|
    /// | int (s)   | wider int | `sext`                                 |
    /// | int (u)   | wider int | `zext`                                 |
    /// | int       | smaller int | `trunc`                              |
    /// | int (s)   | float     | `sitofp`                               |
    /// | int (u)   | float     | `uitofp`                               |
    /// | float     | int (s)   | `llvm.fptosi.sat.iN.fM` (NaN→0, ±inf→min/max) |
    /// | float     | int (u)   | `llvm.fptoui.sat.iN.fM` (NaN→0, +inf→max, -inf→0) |
    /// | float     | wider fp  | `fpext`                                |
    /// | float     | smaller fp | `fptrunc`                             |
    ///
    /// Bool↔Int is handled at the typecheck side already (Bool stores
    /// as i8); for unknown source types we fall back to the bit-
    /// preserving `coerce_with_src` — same conservative behaviour as
    /// pre-v0.42 T2.
    fn cast_value(
        &mut self,
        v: BasicValueEnum<'ctx>,
        want: BasicTypeEnum<'ctx>,
        src_ty: Option<&IrTy>,
        dst_ty: &IrTy,
    ) -> BasicValueEnum<'ctx> {
        let have = v.get_type();
        if have == want
            && matches!(
                (src_ty, dst_ty),
                (Some(IrTy::Int(_)), IrTy::Int(_)) | (Some(IrTy::Float(_)), IrTy::Float(_))
            )
        {
            return v;
        }
        let unsigned_src = src_ty.is_some_and(Self::is_unsigned_int_ty);
        match (src_ty, dst_ty, have, want) {
            // ── Int → Float ──────────────────────────────────────────
            (
                Some(IrTy::Int(_)),
                IrTy::Float(_),
                BasicTypeEnum::IntType(_),
                BasicTypeEnum::FloatType(ft),
            ) => {
                let iv = v.into_int_value();
                if unsigned_src {
                    self.pl
                        .builder
                        .build_unsigned_int_to_float(iv, ft, "uitofp")
                        .unwrap()
                        .into()
                } else {
                    self.pl
                        .builder
                        .build_signed_int_to_float(iv, ft, "sitofp")
                        .unwrap()
                        .into()
                }
            }
            // ── Float → Int ──────────────────────────────────────────
            (
                Some(IrTy::Float(_)),
                IrTy::Int(_),
                BasicTypeEnum::FloatType(_),
                BasicTypeEnum::IntType(it),
            ) => {
                // Use the saturating LLVM intrinsic so NaN → 0 and
                // ±inf clamp to dst's min/max — matches the doc'd
                // semantics in docs/reference/casts.md and the
                // cranelift `fcvt_to_*_sat` instructions.
                let dst_unsigned = Self::is_unsigned_int_ty(dst_ty);
                let name = if dst_unsigned {
                    "llvm.fptoui.sat"
                } else {
                    "llvm.fptosi.sat"
                };
                let intrinsic = inkwell::intrinsics::Intrinsic::find(name)
                    .expect("llvm.fpto[su]i.sat intrinsic must exist");
                let fn_val = intrinsic
                    .get_declaration(&self.pl.module, &[it.into(), have])
                    .expect("intrinsic declaration");
                let call = self
                    .pl
                    .builder
                    .build_call(fn_val, &[v.into()], "fp_to_int_sat")
                    .unwrap();
                call.try_as_basic_value().left().unwrap()
            }
            // ── Float → Float ────────────────────────────────────────
            (
                Some(IrTy::Float(_)),
                IrTy::Float(_),
                BasicTypeEnum::FloatType(_),
                BasicTypeEnum::FloatType(ft),
            ) => {
                let fv = v.into_float_value();
                let src_bits: u32 = if v.get_type() == self.pl.ctx.f32_type().into() {
                    32
                } else {
                    64
                };
                let dst_bits: u32 = if ft == self.pl.ctx.f32_type() { 32 } else { 64 };
                if src_bits < dst_bits {
                    self.pl
                        .builder
                        .build_float_ext(fv, ft, "fpext")
                        .unwrap()
                        .into()
                } else if src_bits > dst_bits {
                    self.pl
                        .builder
                        .build_float_trunc(fv, ft, "fptrunc")
                        .unwrap()
                        .into()
                } else {
                    v
                }
            }
            // ── everything else — delegate to coerce_with_src for
            // Int↔Int and the bit-preserving fallback.
            _ => self.coerce_with_src(v, want, src_ty),
        }
    }

    /// Coerce a value into the wanted type; handles int<->int, float<->float
    /// and bitcast for size-equal int<->float pairs. Pointer<->int via
    /// ptrtoint/inttoptr.
    ///
    /// Note: the int-to-int path here defaults to **signed** widening
    /// via `build_int_cast` (which is just `LLVMBuildIntCast` =
    /// `LLVMBuildSExtOrBitCast` for widening). Callers that know the
    /// source is unsigned should use [`Self::coerce_with_src`] instead.
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
                // v0.40 T2: inkwell 0.5's FloatType has no
                // `get_bit_width` — derive width by direct type
                // comparison against the context's f32/f64 types.
                let fbw: u32 = if ft == self.pl.ctx.f32_type() { 32 } else { 64 };
                if it.get_bit_width() == fbw {
                    self.pl.builder.build_bit_cast(v, ft, "ibcast").unwrap()
                } else {
                    let intermediate = if fbw == 32 {
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
                let fbw: u32 = if ft == self.pl.ctx.f32_type() { 32 } else { 64 };
                if fbw == it.get_bit_width() {
                    self.pl.builder.build_bit_cast(v, it, "fbcast").unwrap()
                } else {
                    let intermediate = if fbw == 32 {
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

    // =========================================================================
    // v0.40 T2 — typed-slot Vec lowering (port of v0.39 T3 cranelift work)
    // =========================================================================
    //
    // Mirrors `mty-codegen-cranelift::lower::emit_vec_*` exactly:
    //
    // Header layout (32 bytes), heap-allocated through `mty_runtime_alloc`
    // so the pointer is stable across `push` growth:
    //   off  0 : len       (i64) — element count
    //   off  8 : cap       (i64) — capacity in elements
    //   off 16 : data      (ptr) — pointer to `cap * elem_size` bytes
    //   off 24 : elem_size (i64) — size in bytes of one element slot
    //
    // The element type is statically known at codegen time. Per-element
    // size + load/store width is chosen by `vec_elem_size_for` /
    // `vec_elem_ld_st`, mirroring the cranelift `field_load_ty` widths.
    //
    // Bounds-check on `get`/`set`: if idx >= len, call
    // `mty_runtime_panic` then emit `unreachable`. Same shape as
    // cranelift's TrapCode::user(5) variant; LLVM doesn't have a direct
    // user-trap-code equivalent, so we use `unreachable` after the
    // panic — the runtime panic exits the process before the
    // unreachable executes, and any stub that returns surfaces UB at
    // runtime (matching cranelift's terminal-block semantics).

    const VEC_LEN_OFF: u64 = 0;
    const VEC_CAP_OFF: u64 = 8;
    const VEC_DATA_OFF: u64 = 16;
    const VEC_ELEM_SIZE_OFF: u64 = 24;
    const VEC_HEADER_SIZE: u64 = 32;
    /// Marker constant for v0.39 header layout. Mirrors cranelift's
    /// `VEC_HEADER_V2`. Future migration tooling reads this to gate
    /// v1→v2 upgrades for serialized Vec values.
    #[allow(dead_code)]
    pub const VEC_HEADER_V2: u32 = 2;
    /// Default element-slot width when the element type is unknown.
    const VEC_FALLBACK_ELEM_SIZE: u64 = 8;

    /// Pull the element type `T` out of a `Vec[T]` SIR type. Returns
    /// None for non-Vec / no-generics inputs.
    fn vec_elem_ty_from(&self, t: &IrTy) -> Option<IrTy> {
        if let IrTy::Adt(id, args) = t {
            let name = self.pl.prog.adt_by_id(*id).map(|a| a.name.as_str());
            if name == Some("Vec") {
                if let Some(elem) = args.first() {
                    return Some(elem.clone());
                }
            }
        }
        None
    }

    /// Static element-slot width for a Vec element type. Mirrors
    /// `field_load_ty` widths but always yields a byte count (1/2/4/8)
    /// for scalars and the layout size for aggregates. Falls back to
    /// 8 for unresolved types.
    fn vec_elem_size_for(&self, elem_ty: &IrTy) -> u64 {
        use IrTy::*;
        match elem_ty {
            Bool => 1,
            Int(k) => match k {
                IntKind::I8 | IntKind::U8 => 1,
                IntKind::I16 | IntKind::U16 => 2,
                IntKind::I32 | IntKind::U32 | IntKind::IntInfer => 4,
                IntKind::I64 | IntKind::U64 | IntKind::ISize | IntKind::USize => 8,
                IntKind::I128 | IntKind::U128 => 16,
            },
            Float(k) => match k {
                FloatKind::F32 => 4,
                FloatKind::F64 | FloatKind::FloatInfer => 8,
            },
            Char => 4,
            Duration | Size => 8,
            Ref { .. } | RawPtr(_) | Cap { .. } | Fn { .. } => 8,
            Tuple(_) | Array { .. } | Adt(_, _) | Str | String | Bytes => {
                Self::ir_type_size(elem_ty, self.pl.prog) as u64
            }
            Error | Param(_) | Module(_) | Unit | Never | Dyn(_) => Self::VEC_FALLBACK_ELEM_SIZE,
        }
    }

    /// LLVM scalar load/store type for a Vec element type plus a
    /// sign/zero-extension flag for narrow-int reads. Returns None when
    /// the element is an aggregate (caller must memcpy).
    fn vec_elem_ld_st(&self, elem_ty: &IrTy) -> Option<(BasicTypeEnum<'ctx>, bool)> {
        use IrTy::*;
        Some(match elem_ty {
            Bool => (self.pl.i8_ty().into(), false),
            Int(k) => match k {
                IntKind::I8 => (self.pl.i8_ty().into(), true),
                IntKind::U8 => (self.pl.i8_ty().into(), false),
                IntKind::I16 => (self.pl.ctx.i16_type().into(), true),
                IntKind::U16 => (self.pl.ctx.i16_type().into(), false),
                IntKind::I32 | IntKind::IntInfer => (self.pl.i32_ty().into(), true),
                IntKind::U32 => (self.pl.i32_ty().into(), false),
                IntKind::I64 | IntKind::ISize => (self.pl.i64_ty().into(), true),
                IntKind::U64 | IntKind::USize => (self.pl.i64_ty().into(), false),
                IntKind::I128 | IntKind::U128 => return None,
            },
            Float(k) => match k {
                FloatKind::F32 => (self.pl.ctx.f32_type().into(), false),
                FloatKind::F64 | FloatKind::FloatInfer => (self.pl.ctx.f64_type().into(), false),
            },
            Char => (self.pl.i32_ty().into(), false),
            Duration | Size => (self.pl.i64_ty().into(), false),
            Ref { .. } | RawPtr(_) | Cap { .. } | Fn { .. } => (self.pl.i64_ty().into(), false),
            Error | Param(_) | Module(_) | Dyn(_) | Unit | Never => {
                (self.pl.i64_ty().into(), false)
            }
            Tuple(_) | Array { .. } | Adt(_, _) | Str | String | Bytes => return None,
        })
    }

    /// Convenience: pull `T` out of the receiver operand's local type
    /// and pick element size + LLVM load width. Falls back to the
    /// 8-byte i64 slot when the type info is missing.
    fn vec_elem_info(
        &self,
        receiver: &Operand,
    ) -> (u64, Option<(BasicTypeEnum<'ctx>, bool)>, Option<IrTy>) {
        let recv_ty = match receiver {
            Operand::Copy(p) | Operand::Move(p) => {
                Some(self.f.locals[p.local.0 as usize].ty.clone())
            }
            Operand::Const(_) => None,
        };
        // The SIR lowerer types the MethodCall result temp as
        // `IrTy::Error` (see `lower_expr` MethodCall arm in
        // mty-ir/src/lower/exprs.rs), and the receiver of the next
        // iteration's `v = v.push(x)` carries that same Error type
        // forward. So `recv_ty` is usually `Some(Error)`. Fall back
        // to `current_dest_ty` (the type of the *destination* of the
        // current assignment), which still has `Vec[T]` for the user-
        // declared `let mut v: Vec[T] = Vec.new()`.
        let elem_ty = recv_ty
            .as_ref()
            .and_then(|t| self.vec_elem_ty_from(t))
            .or_else(|| {
                self.current_dest_ty
                    .as_ref()
                    .and_then(|t| self.vec_elem_ty_from(t))
            });
        let size = elem_ty
            .as_ref()
            .map(|t| self.vec_elem_size_for(t))
            .unwrap_or(Self::VEC_FALLBACK_ELEM_SIZE);
        let lds = elem_ty.as_ref().and_then(|t| self.vec_elem_ld_st(t));
        (size, lds, elem_ty)
    }

    /// Minimal `type_size` walker — mirrors cranelift's
    /// `aggregate::type_size` for the subset Vec stores by-value. The
    /// LLVM crate doesn't pull in the cranelift layout module, so we
    /// keep a small parallel implementation here. Natural-alignment,
    /// no niche/reorder.
    fn ir_type_size(t: &IrTy, prog: &Program) -> u32 {
        use IrTy::*;
        match t {
            Bool => 1,
            Char => 4,
            Unit | Never | Module(_) | Param(_) | Error => 0,
            Int(k) => match k {
                IntKind::I8 | IntKind::U8 => 1,
                IntKind::I16 | IntKind::U16 => 2,
                IntKind::I32 | IntKind::U32 | IntKind::IntInfer => 4,
                IntKind::I64 | IntKind::U64 | IntKind::ISize | IntKind::USize => 8,
                IntKind::I128 | IntKind::U128 => 16,
            },
            Float(k) => match k {
                FloatKind::F32 => 4,
                FloatKind::F64 | FloatKind::FloatInfer => 8,
            },
            Duration | Size => 8,
            Str | String | Bytes => 16,
            Ref { .. } | RawPtr(_) | Cap { .. } | Fn { .. } => 8,
            Dyn(_) => 16,
            Tuple(elems) => {
                Self::layout_struct_size(elems.iter().map(|e| Self::ir_type_size(e, prog)))
            }
            Array { elem, len } => {
                let n = len.unwrap_or(0) as u32;
                Self::ir_type_size(elem, prog) * n
            }
            Adt(id, _args) => match prog.adt_by_id(*id) {
                Some(adt) if adt.variants.len() == 1 => {
                    let v = &adt.variants[0];
                    Self::layout_struct_size(
                        v.fields.iter().map(|f| Self::ir_type_size(&f.ty, prog)),
                    )
                }
                Some(adt) if adt.variants.is_empty() => 8,
                Some(adt) => {
                    // Enum: tag (u32) + max payload.
                    let payload = adt
                        .variants
                        .iter()
                        .map(|v| {
                            Self::layout_struct_size(
                                v.fields.iter().map(|f| Self::ir_type_size(&f.ty, prog)),
                            )
                        })
                        .max()
                        .unwrap_or(0);
                    // tag 4, then payload aligned to 8 (worst-case ptr align).
                    8u32.max(4 + payload)
                }
                None => 8,
            },
        }
    }

    /// Sequential-pack natural-alignment size (rounded up to the
    /// struct's natural alignment, which for our subset is the
    /// max-of-pow2-fields). Simpler than the cranelift variant —
    /// we only need the *size* and the v0.40 Vec workload pushes
    /// pow2-sized scalars. Aggregates of aggregates are still
    /// supported but with looser alignment guarantees than
    /// cranelift's; that's fine for the v0.40 test surface (which
    /// only exercises simple struct-of-int).
    fn layout_struct_size(sizes: impl Iterator<Item = u32>) -> u32 {
        let mut total: u32 = 0;
        let mut max_a: u32 = 1;
        for s in sizes {
            let a = s.max(1).next_power_of_two().min(8);
            max_a = max_a.max(a);
            total = (total + a - 1) & !(a - 1);
            total += s;
        }
        // round up to max-alignment
        (total + max_a - 1) & !(max_a - 1)
    }

    /// Allocate `size` bytes (align 8) from the runtime arena, returning
    /// the resulting pointer (cast from i64 → ptr).
    fn rt_alloc(&mut self, size: IntValue<'ctx>) -> PointerValue<'ctx> {
        let i64t = self.pl.i64_ty();
        let align = i64t.const_int(8, false);
        let zero = i64t.const_int(0, false);
        let f = self.pl.runtime_fns["mty_runtime_alloc"];
        let call = self
            .pl
            .builder
            .build_call(f, &[size.into(), align.into(), zero.into()], "vec_rt_alloc")
            .unwrap();
        let raw = call
            .try_as_basic_value()
            .left()
            .map(|v| v.into_int_value())
            .unwrap_or_else(|| i64t.const_zero());
        self.pl
            .builder
            .build_int_to_ptr(raw, self.pl.ptr_ty(), "vec_alloc_p")
            .unwrap()
    }

    /// Compute a typed byte-offset pointer: `base + off` as `ptr`.
    /// Uses GEP on i8 so the offset is in raw bytes.
    fn ptr_off(&mut self, base: PointerValue<'ctx>, off: u64) -> PointerValue<'ctx> {
        let i64t = self.pl.i64_ty();
        let off_v = i64t.const_int(off, false);
        let i8t = self.pl.i8_ty();
        unsafe {
            self.pl
                .builder
                .build_in_bounds_gep(i8t, base, &[off_v], "vptr_off")
                .unwrap()
        }
    }

    /// Dynamic byte-offset pointer (offset is an `IntValue`).
    fn ptr_off_dyn(&mut self, base: PointerValue<'ctx>, off: IntValue<'ctx>) -> PointerValue<'ctx> {
        let i8t = self.pl.i8_ty();
        unsafe {
            self.pl
                .builder
                .build_in_bounds_gep(i8t, base, &[off], "vptr_off_d")
                .unwrap()
        }
    }

    /// Convert a header pointer value back from an i64 (the form locals
    /// store under our `IrTy::Adt` → ptr lowering, after a Move/Copy
    /// load).
    fn header_to_ptr(&mut self, v: BasicValueEnum<'ctx>) -> PointerValue<'ctx> {
        match v {
            BasicValueEnum::PointerValue(p) => p,
            BasicValueEnum::IntValue(iv) => self
                .pl
                .builder
                .build_int_to_ptr(iv, self.pl.ptr_ty(), "i2hdr")
                .unwrap(),
            _ => self.pl.ptr_ty().const_null(),
        }
    }

    /// `Vec.new()` — allocate a zeroed header (len=0, cap=0, data=null,
    /// elem_size=T-sized). Returns a pointer as a `BasicValueEnum`.
    fn emit_vec_new(&mut self) -> CompileResult<BasicValueEnum<'ctx>> {
        let elem_size = self
            .current_dest_ty
            .clone()
            .as_ref()
            .and_then(|t| self.vec_elem_ty_from(t))
            .map(|t| self.vec_elem_size_for(&t))
            .unwrap_or(Self::VEC_FALLBACK_ELEM_SIZE);
        let i64t = self.pl.i64_ty();
        let hsize = i64t.const_int(Self::VEC_HEADER_SIZE, false);
        let hdr = self.rt_alloc(hsize);
        let zero = i64t.const_zero();
        let esz = i64t.const_int(elem_size, false);
        // len = 0
        let len_p = self.ptr_off(hdr, Self::VEC_LEN_OFF);
        self.pl.builder.build_store(len_p, zero).unwrap();
        // cap = 0
        let cap_p = self.ptr_off(hdr, Self::VEC_CAP_OFF);
        self.pl.builder.build_store(cap_p, zero).unwrap();
        // data = null
        let data_p = self.ptr_off(hdr, Self::VEC_DATA_OFF);
        self.pl
            .builder
            .build_store(data_p, self.pl.ptr_ty().const_null())
            .unwrap();
        // elem_size = esz
        let esz_p = self.ptr_off(hdr, Self::VEC_ELEM_SIZE_OFF);
        self.pl.builder.build_store(esz_p, esz).unwrap();
        Ok(hdr.into())
    }

    /// Evaluate a Vec receiver operand to its header pointer.
    fn vec_header(&mut self, receiver: &Operand) -> CompileResult<PointerValue<'ctx>> {
        let v = self.eval_operand(receiver)?;
        Ok(self.header_to_ptr(v))
    }

    /// Load `len` field.
    fn vec_load_len(&mut self, hdr: PointerValue<'ctx>) -> IntValue<'ctx> {
        let p = self.ptr_off(hdr, Self::VEC_LEN_OFF);
        let i64t = self.pl.i64_ty();
        self.pl
            .builder
            .build_load(i64t, p, "vlen")
            .unwrap()
            .into_int_value()
    }

    /// Load `cap` field.
    fn vec_load_cap(&mut self, hdr: PointerValue<'ctx>) -> IntValue<'ctx> {
        let p = self.ptr_off(hdr, Self::VEC_CAP_OFF);
        let i64t = self.pl.i64_ty();
        self.pl
            .builder
            .build_load(i64t, p, "vcap")
            .unwrap()
            .into_int_value()
    }

    /// Load `data` field as a pointer.
    fn vec_load_data(&mut self, hdr: PointerValue<'ctx>) -> PointerValue<'ctx> {
        let p = self.ptr_off(hdr, Self::VEC_DATA_OFF);
        let pt = self.pl.ptr_ty();
        self.pl
            .builder
            .build_load(pt, p, "vdata")
            .unwrap()
            .into_pointer_value()
    }

    /// Store one element value into a Vec data slot. Mirrors
    /// `cranelift_lower::vec_store_elem`.
    fn vec_store_elem(
        &mut self,
        slot: PointerValue<'ctx>,
        val: BasicValueEnum<'ctx>,
        elem_size: u64,
        lds: Option<(BasicTypeEnum<'ctx>, bool)>,
        _elem_ty: Option<&IrTy>,
    ) {
        if let Some((cty, _signed)) = lds {
            // Scalar slot: coerce the source to the slot's LLVM type
            // then store.
            let narrowed = self.coerce(val, cty);
            self.pl.builder.build_store(slot, narrowed).unwrap();
            return;
        }
        // Fallback: i64-word store, matching v0.38 / cranelift fallback.
        if elem_size == Self::VEC_FALLBACK_ELEM_SIZE {
            let narrowed = self.coerce(val, self.pl.i64_ty().into());
            self.pl.builder.build_store(slot, narrowed).unwrap();
            return;
        }
        // Aggregate slot: val is the source aggregate's address.
        // Copy elem_size bytes via build_memcpy.
        let src_ptr = self.header_to_ptr(val);
        let size_v = self.pl.i64_ty().const_int(elem_size, false);
        let _ = self.pl.builder.build_memcpy(slot, 1, src_ptr, 1, size_v);
    }

    /// Load one element value from a Vec data slot. Mirrors
    /// `cranelift_lower::vec_load_elem`.
    fn vec_load_elem(
        &mut self,
        slot: PointerValue<'ctx>,
        elem_size: u64,
        lds: Option<(BasicTypeEnum<'ctx>, bool)>,
    ) -> BasicValueEnum<'ctx> {
        if let Some((cty, signed)) = lds {
            let raw = self.pl.builder.build_load(cty, slot, "velem").unwrap();
            // Sign- or zero-extend narrow ints up to i64 for downstream.
            if let BasicTypeEnum::IntType(it) = cty {
                let bw = it.get_bit_width();
                if bw < 64 {
                    let i64t = self.pl.i64_ty();
                    let iv = raw.into_int_value();
                    return self
                        .pl
                        .builder
                        .build_int_cast_sign_flag(iv, i64t, signed, "vext")
                        .unwrap()
                        .into();
                }
            }
            return raw;
        }
        // Fallback (unknown elem ty): load an i64 word.
        if elem_size == Self::VEC_FALLBACK_ELEM_SIZE {
            return self
                .pl
                .builder
                .build_load(self.pl.i64_ty(), slot, "velem_i64")
                .unwrap();
        }
        // Aggregate: hand back the slot pointer.
        slot.into()
    }

    /// Bounds-check: if `idx >= len`, call `mty_runtime_panic` and
    /// emit `unreachable`. The panic stub aborts the process; the
    /// `unreachable` keeps the OOB block terminal for the LLVM
    /// verifier.
    fn vec_bounds_check(&mut self, idx: IntValue<'ctx>, len: IntValue<'ctx>) {
        let oob = self.pl.ctx.append_basic_block(self.fv, "vec_oob");
        let ok = self.pl.ctx.append_basic_block(self.fv, "vec_ok");
        let is_oob = self
            .pl
            .builder
            .build_int_compare(IntPredicate::UGE, idx, len, "is_oob")
            .unwrap();
        self.pl
            .builder
            .build_conditional_branch(is_oob, oob, ok)
            .unwrap();
        self.pl.builder.position_at_end(oob);
        let nptr = self.pl.intern_string("Vec index out of bounds");
        let nlen = self.pl.i64_ty().const_int(23, false);
        let f = self.pl.runtime_fns["mty_runtime_panic"];
        let _ = self
            .pl
            .builder
            .build_call(f, &[nptr.into(), nlen.into()], "vpanic");
        self.pl.builder.build_unreachable().unwrap();
        self.pl.builder.position_at_end(ok);
    }

    /// `v.push(x)` — ensure capacity (growing if `len == cap`), store
    /// the element at `data[len]`, bump `len`, return the (unchanged)
    /// header pointer.
    fn emit_vec_push(
        &mut self,
        receiver: &Operand,
        args: &[Operand],
    ) -> CompileResult<BasicValueEnum<'ctx>> {
        let (elem_size, lds, elem_ty) = self.vec_elem_info(receiver);
        let hdr = self.vec_header(receiver)?;
        let len = self.vec_load_len(hdr);
        let cap = self.vec_load_cap(hdr);

        let grow_bb = self.pl.ctx.append_basic_block(self.fv, "vec_grow");
        let cont_bb = self.pl.ctx.append_basic_block(self.fv, "vec_cont");
        let need_grow = self
            .pl
            .builder
            .build_int_compare(IntPredicate::EQ, len, cap, "need_grow")
            .unwrap();
        self.pl
            .builder
            .build_conditional_branch(need_grow, grow_bb, cont_bb)
            .unwrap();

        // --- grow_bb ---
        self.pl.builder.position_at_end(grow_bb);
        let i64t = self.pl.i64_ty();
        let two = i64t.const_int(2, false);
        let two_cap = self.pl.builder.build_int_mul(cap, two, "twocap").unwrap();
        let four = i64t.const_int(4, false);
        let small = self
            .pl
            .builder
            .build_int_compare(IntPredicate::ULT, two_cap, four, "cap_small")
            .unwrap();
        let new_cap = self
            .pl
            .builder
            .build_select(small, four, two_cap, "new_cap")
            .unwrap()
            .into_int_value();
        let esz = i64t.const_int(elem_size, false);
        let new_bytes = self
            .pl
            .builder
            .build_int_mul(new_cap, esz, "new_bytes")
            .unwrap();
        let new_data = self.rt_alloc(new_bytes);
        let old_data = self.vec_load_data(hdr);
        let copy_bytes = self
            .pl
            .builder
            .build_int_mul(len, esz, "copy_bytes")
            .unwrap();
        // memcpy live prefix from old_data → new_data. Align 1 keeps
        // it safe for non-pow2-of-8 element sizes (Vec[U8] etc.).
        let _ = self
            .pl
            .builder
            .build_memcpy(new_data, 1, old_data, 1, copy_bytes);
        let cap_p = self.ptr_off(hdr, Self::VEC_CAP_OFF);
        self.pl.builder.build_store(cap_p, new_cap).unwrap();
        let data_p = self.ptr_off(hdr, Self::VEC_DATA_OFF);
        self.pl.builder.build_store(data_p, new_data).unwrap();
        self.pl.builder.build_unconditional_branch(cont_bb).unwrap();

        // --- cont_bb ---
        self.pl.builder.position_at_end(cont_bb);
        let data = self.vec_load_data(hdr);
        let raw = if let Some(a) = args.first() {
            self.eval_operand(a)?
        } else {
            self.pl.i64_ty().const_zero().into()
        };
        let esz2 = i64t.const_int(elem_size, false);
        let byte_off = self.pl.builder.build_int_mul(len, esz2, "boff").unwrap();
        let slot = self.ptr_off_dyn(data, byte_off);
        self.vec_store_elem(slot, raw, elem_size, lds, elem_ty.as_ref());
        let one = i64t.const_int(1, false);
        let new_len = self.pl.builder.build_int_add(len, one, "new_len").unwrap();
        let len_p = self.ptr_off(hdr, Self::VEC_LEN_OFF);
        self.pl.builder.build_store(len_p, new_len).unwrap();
        Ok(hdr.into())
    }

    /// `v.len()` — load element count.
    fn emit_vec_len(&mut self, receiver: &Operand) -> CompileResult<BasicValueEnum<'ctx>> {
        let hdr = self.vec_header(receiver)?;
        Ok(self.vec_load_len(hdr).into())
    }

    /// `v.get(i)` — bounds-checked element load with sign/zero extend.
    fn emit_vec_get(
        &mut self,
        receiver: &Operand,
        args: &[Operand],
    ) -> CompileResult<BasicValueEnum<'ctx>> {
        let (elem_size, lds, _elem_ty) = self.vec_elem_info(receiver);
        let hdr = self.vec_header(receiver)?;
        let len = self.vec_load_len(hdr);
        let data = self.vec_load_data(hdr);
        let idx = if let Some(a) = args.first() {
            let raw = self.eval_operand(a)?;
            self.coerce(raw, self.pl.i64_ty().into()).into_int_value()
        } else {
            self.pl.i64_ty().const_zero()
        };
        self.vec_bounds_check(idx, len);
        let esz = self.pl.i64_ty().const_int(elem_size, false);
        let byte_off = self.pl.builder.build_int_mul(idx, esz, "gboff").unwrap();
        let slot = self.ptr_off_dyn(data, byte_off);
        Ok(self.vec_load_elem(slot, elem_size, lds))
    }

    /// `v.set(i, x)` — bounds-checked element store.
    fn emit_vec_set(
        &mut self,
        receiver: &Operand,
        args: &[Operand],
    ) -> CompileResult<BasicValueEnum<'ctx>> {
        let (elem_size, lds, elem_ty) = self.vec_elem_info(receiver);
        let hdr = self.vec_header(receiver)?;
        let len = self.vec_load_len(hdr);
        let data = self.vec_load_data(hdr);
        let idx = if let Some(a) = args.first() {
            let raw = self.eval_operand(a)?;
            self.coerce(raw, self.pl.i64_ty().into()).into_int_value()
        } else {
            self.pl.i64_ty().const_zero()
        };
        self.vec_bounds_check(idx, len);
        let val = if let Some(a) = args.get(1) {
            self.eval_operand(a)?
        } else {
            self.pl.i64_ty().const_zero().into()
        };
        let esz = self.pl.i64_ty().const_int(elem_size, false);
        let byte_off = self.pl.builder.build_int_mul(idx, esz, "sboff").unwrap();
        let slot = self.ptr_off_dyn(data, byte_off);
        self.vec_store_elem(slot, val, elem_size, lds, elem_ty.as_ref());
        Ok(hdr.into())
    }

    /// `v.pop()` — saturating decrement of len; returns previously-last
    /// element (or 0 on empty). Two-arm shape (empty / non-empty) so
    /// the load doesn't dereference null when `data == null`.
    fn emit_vec_pop(&mut self, receiver: &Operand) -> CompileResult<BasicValueEnum<'ctx>> {
        let (elem_size, lds, _elem_ty) = self.vec_elem_info(receiver);
        let hdr = self.vec_header(receiver)?;
        let len = self.vec_load_len(hdr);
        let i64t = self.pl.i64_ty();
        let zero = i64t.const_zero();

        let empty_bb = self.pl.ctx.append_basic_block(self.fv, "pop_empty");
        let pop_bb = self.pl.ctx.append_basic_block(self.fv, "pop_load");
        let join_bb = self.pl.ctx.append_basic_block(self.fv, "pop_join");

        // result slot — allocate a stack slot for the result so we can
        // store from both arms without phi gymnastics.
        let res_slot = self.pl.builder.build_alloca(i64t, "pop_res").unwrap();

        let is_empty = self
            .pl
            .builder
            .build_int_compare(IntPredicate::EQ, len, zero, "is_empty")
            .unwrap();
        self.pl
            .builder
            .build_conditional_branch(is_empty, empty_bb, pop_bb)
            .unwrap();

        // --- empty_bb ---
        self.pl.builder.position_at_end(empty_bb);
        self.pl.builder.build_store(res_slot, zero).unwrap();
        self.pl.builder.build_unconditional_branch(join_bb).unwrap();

        // --- pop_bb ---
        self.pl.builder.position_at_end(pop_bb);
        let one = i64t.const_int(1, false);
        let new_len = self.pl.builder.build_int_sub(len, one, "new_len").unwrap();
        let len_p = self.ptr_off(hdr, Self::VEC_LEN_OFF);
        self.pl.builder.build_store(len_p, new_len).unwrap();
        let data = self.vec_load_data(hdr);
        let esz = i64t.const_int(elem_size, false);
        let byte_off = self
            .pl
            .builder
            .build_int_mul(new_len, esz, "pboff")
            .unwrap();
        let slot = self.ptr_off_dyn(data, byte_off);
        let elem = self.vec_load_elem(slot, elem_size, lds);
        let elem_i64 = self.coerce(elem, i64t.into());
        self.pl.builder.build_store(res_slot, elem_i64).unwrap();
        self.pl.builder.build_unconditional_branch(join_bb).unwrap();

        // --- join ---
        self.pl.builder.position_at_end(join_bb);
        let v = self.pl.builder.build_load(i64t, res_slot, "pop_v").unwrap();
        Ok(v)
    }

    /// `v.clear()` — reset len to 0 (keeps the allocation).
    fn emit_vec_clear(&mut self, receiver: &Operand) -> CompileResult<BasicValueEnum<'ctx>> {
        let hdr = self.vec_header(receiver)?;
        let zero = self.pl.i64_ty().const_zero();
        let p = self.ptr_off(hdr, Self::VEC_LEN_OFF);
        self.pl.builder.build_store(p, zero).unwrap();
        Ok(hdr.into())
    }
}

// =============================================================================
// v0.45 T1 — native std.fs.* support on the LLVM backend (L18 fix)
// =============================================================================

/// Recognise the `std.fs.*` methods that the LLVM backend now lowers
/// natively. Kept in sync with the cranelift `is_native_fs_method` so
/// both backends ship identical source coverage. Accepts both
/// `std.fs.*` and bare `fs.*` shapes (the latter from a `use std.fs`
/// import).
fn is_native_fs_method_llvm(full_name: &str) -> bool {
    let bare = full_name.strip_prefix("std.").unwrap_or(full_name);
    matches!(
        bare,
        "fs.read"
            | "fs.read_file"
            | "fs.read_to_string"
            | "fs.read_dir"
            | "fs.list_dir"
            | "fs.read_dir_lines"
            | "fs.write"
            | "fs.write_file"
            | "fs.write_string"
            | "fs.append"
            | "fs.exists"
            | "fs.metadata"
            | "fs.stat"
            | "fs.create_dir_all"
            | "fs.remove_file"
            | "fs.remove_dir_all"
    )
}

#[derive(Debug, Clone, Copy)]
enum LlvmFsAbiKind {
    ReadStrSlot {
        symbol: &'static str,
    },
    WriteI32 {
        symbol: &'static str,
    },
    PathI32 {
        symbol: &'static str,
    },
    MetadataSlot {
        symbol: &'static str,
    },
    /// v0.46 T4 — `read_dir` returns an i64 DirIter handle through
    /// `mty_runtime_fs_dir_open`. The LLVM backend's projection /
    /// method-dispatch story is more limited than cranelift's, but
    /// the bare open-call still flows correctly so source code can
    /// at least drive the open through the LLVM lane without
    /// hitting `Unsupported`.
    DirOpenHandle {
        symbol: &'static str,
    },
}

impl LlvmFsAbiKind {
    fn for_method(full_name: &str) -> Self {
        let bare = full_name.strip_prefix("std.").unwrap_or(full_name);
        match bare {
            "fs.read" | "fs.read_file" => LlvmFsAbiKind::ReadStrSlot {
                symbol: "mty_runtime_fs_read",
            },
            "fs.read_to_string" => LlvmFsAbiKind::ReadStrSlot {
                symbol: "mty_runtime_fs_read_to_string",
            },
            "fs.read_dir" | "fs.list_dir" => LlvmFsAbiKind::DirOpenHandle {
                symbol: "mty_runtime_fs_dir_open",
            },
            "fs.read_dir_lines" => LlvmFsAbiKind::ReadStrSlot {
                symbol: "mty_runtime_fs_read_dir",
            },
            "fs.write" | "fs.write_file" => LlvmFsAbiKind::WriteI32 {
                symbol: "mty_runtime_fs_write",
            },
            "fs.write_string" => LlvmFsAbiKind::WriteI32 {
                symbol: "mty_runtime_fs_write_string",
            },
            "fs.append" => LlvmFsAbiKind::WriteI32 {
                symbol: "mty_runtime_fs_append",
            },
            "fs.exists" => LlvmFsAbiKind::PathI32 {
                symbol: "mty_runtime_fs_exists",
            },
            "fs.create_dir_all" => LlvmFsAbiKind::PathI32 {
                symbol: "mty_runtime_fs_create_dir_all",
            },
            "fs.remove_file" => LlvmFsAbiKind::PathI32 {
                symbol: "mty_runtime_fs_remove_file",
            },
            "fs.remove_dir_all" => LlvmFsAbiKind::PathI32 {
                symbol: "mty_runtime_fs_remove_dir_all",
            },
            "fs.metadata" | "fs.stat" => LlvmFsAbiKind::MetadataSlot {
                symbol: "mty_runtime_fs_metadata",
            },
            _ => unreachable!("LlvmFsAbiKind::for_method called on non-fs method {full_name}"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum LlvmFsRet<'ctx> {
    Slot(PointerValue<'ctx>),
    I32(BasicValueEnum<'ctx>),
    /// v0.46 T4 — `read_dir` returns an opaque i64 DirIter handle
    /// from `mty_runtime_fs_dir_open`. Stored straight into the
    /// out local through the standard scalar coerce path.
    I64(BasicValueEnum<'ctx>),
}
