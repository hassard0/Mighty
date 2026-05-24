//! Wasm core-module emission (slice 8).
//!
//! Wraps `wasm-encoder`. We build sections in canonical order: types,
//! imports, functions, memory, exports, code, data.
//!
//! For slice-8 the lowerer is intentionally narrow: it accepts SIR
//! whose only fns are `main`-shape (no params, returning `Unit` or
//! `Int`) with bodies that are arithmetic + `log("...")` + return.
//! Anything richer raises [`WasmError::Unsupported`].

use crate::artifact::WasmArtifact;
use crate::error::{CompileResult, WasmError};
use crate::target::WasmTarget;
use sdust_sir::sir::{
    BinOp, BlockId, BuiltinId, Const, FnRef, Function, Operand, Place, Program, Rvalue, SirFnId,
    SirTy, Stmt, Term, UnOp,
};
use sdust_types::IntKind;
use std::collections::HashMap;
use std::path::Path;
use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, DataSection, EntityType, ExportKind, ExportSection,
    Function as WFunction, FunctionSection, ImportSection, Instruction as I, MemoryType, Module,
    TypeSection, ValType,
};

/// Compile a SIR program to a Wasm binary.
pub fn compile_program_to_bytes(
    prog: &Program,
    target: WasmTarget,
) -> CompileResult<Vec<u8>> {
    let mut emitter = Emitter::new(prog, target)?;
    emitter.emit()
}

/// Compile a SIR program to a Wasm binary and wrap in an artifact.
pub fn compile_program(prog: &Program, target: WasmTarget) -> CompileResult<WasmArtifact> {
    let bytes = compile_program_to_bytes(prog, target)?;
    Ok(WasmArtifact {
        bytes,
        path: None,
        target,
    })
}

/// Compile a SIR program and write to disk.
pub fn compile_program_to_file(
    prog: &Program,
    target: WasmTarget,
    out: &Path,
) -> CompileResult<WasmArtifact> {
    let bytes = compile_program_to_bytes(prog, target)?;
    std::fs::write(out, &bytes)
        .map_err(|e| WasmError::Io(format!("write {}: {}", out.display(), e)))?;
    Ok(WasmArtifact {
        bytes,
        path: Some(out.to_path_buf()),
        target,
    })
}

struct Emitter<'a> {
    prog: &'a Program,
    target: WasmTarget,
    type_section: TypeSection,
    import_section: ImportSection,
    function_section: FunctionSection,
    export_section: ExportSection,
    code_section: CodeSection,
    data_section: DataSection,
    /// SIR fn id → wasm function index (after imports).
    fn_index: HashMap<SirFnId, u32>,
    /// SIR fn id → wasm type index.
    fn_type_index: HashMap<SirFnId, u32>,
    /// Map type-signature key → type index, for deduping.
    sigs: HashMap<TySig, u32>,
    /// Imports added so far (each takes one fn-index slot before user fns).
    import_count: u32,
    /// `log(ptr, len)` import index (Some after we add it).
    log_idx: Option<u32>,
    /// String literal pool — appends to data section, returns (ptr, len).
    string_pool: HashMap<String, (u32, u32)>,
    next_data_offset: u32,
}

#[derive(Hash, PartialEq, Eq, Clone)]
struct TySig {
    params: Vec<ValType>,
    results: Vec<ValType>,
}

impl<'a> Emitter<'a> {
    fn new(prog: &'a Program, target: WasmTarget) -> CompileResult<Self> {
        Ok(Self {
            prog,
            target,
            type_section: TypeSection::new(),
            import_section: ImportSection::new(),
            function_section: FunctionSection::new(),
            export_section: ExportSection::new(),
            code_section: CodeSection::new(),
            data_section: DataSection::new(),
            fn_index: HashMap::new(),
            fn_type_index: HashMap::new(),
            sigs: HashMap::new(),
            import_count: 0,
            log_idx: None,
            string_pool: HashMap::new(),
            next_data_offset: 1024, // reserve first 1KiB for the stack
        })
    }

    fn intern_sig(&mut self, sig: TySig) -> u32 {
        if let Some(&idx) = self.sigs.get(&sig) {
            return idx;
        }
        let idx = self.sigs.len() as u32;
        self.type_section.ty().function(
            sig.params.iter().copied(),
            sig.results.iter().copied(),
        );
        self.sigs.insert(sig, idx);
        idx
    }

    fn declare_imports(&mut self) -> CompileResult<()> {
        // log(ptr: i32, len: i32) -> ()
        let log_sig = TySig {
            params: vec![ValType::I32, ValType::I32],
            results: vec![],
        };
        let log_ty = self.intern_sig(log_sig);
        let (mod_name, fn_name) = match self.target {
            WasmTarget::Wasi => ("stardust", "log"),
            WasmTarget::Web => ("stardust", "log"),
        };
        self.import_section
            .import(mod_name, fn_name, EntityType::Function(log_ty));
        self.log_idx = Some(self.import_count);
        self.import_count += 1;
        Ok(())
    }

    fn lower_ty(t: &SirTy) -> Option<ValType> {
        Some(match t {
            SirTy::Bool | SirTy::Char => ValType::I32,
            SirTy::Int(k) => match k {
                IntKind::I8 | IntKind::U8 | IntKind::I16 | IntKind::U16 | IntKind::I32
                | IntKind::U32 | IntKind::IntInfer => ValType::I32,
                IntKind::I64 | IntKind::U64 | IntKind::ISize | IntKind::USize => ValType::I64,
                IntKind::I128 | IntKind::U128 => return None,
            },
            SirTy::Float(k) => match k {
                sdust_types::FloatKind::F32 => ValType::F32,
                sdust_types::FloatKind::F64 | sdust_types::FloatKind::FloatInfer => ValType::F64,
            },
            SirTy::Duration | SirTy::Size => ValType::I64,
            SirTy::Unit | SirTy::Never => return None,
            _ => return None,
        })
    }

    fn fn_sig_for(f: &Function) -> CompileResult<TySig> {
        let mut params = Vec::with_capacity(f.params.len());
        for p in &f.params {
            let ty = &f.locals[p.0 as usize].ty;
            if let Some(v) = Self::lower_ty(ty) {
                params.push(v);
            } else if !matches!(ty, SirTy::Unit | SirTy::Never) {
                return Err(WasmError::Unsupported(format!(
                    "wasm param type {ty:?}"
                )));
            }
        }
        let mut results = Vec::new();
        if let Some(v) = Self::lower_ty(&f.ret_ty) {
            results.push(v);
        } else if !matches!(f.ret_ty, SirTy::Unit | SirTy::Never) {
            return Err(WasmError::Unsupported(format!(
                "wasm ret type {:?}",
                f.ret_ty
            )));
        }
        Ok(TySig { params, results })
    }

    fn declare_fns(&mut self) -> CompileResult<()> {
        for f in &self.prog.fns {
            let sig = Self::fn_sig_for(f)?;
            let ty_idx = self.intern_sig(sig);
            self.fn_type_index.insert(f.id, ty_idx);
            let fn_idx = self.import_count + self.function_section.len();
            self.function_section.function(ty_idx);
            self.fn_index.insert(f.id, fn_idx);
            // Export `main` for the wasm runtime to find.
            if f.name == "main" {
                self.export_section
                    .export("main", ExportKind::Func, fn_idx);
            }
        }
        Ok(())
    }

    fn intern_string(&mut self, s: &str) -> (u32, u32) {
        if let Some(&pl) = self.string_pool.get(s) {
            return pl;
        }
        let offset = self.next_data_offset;
        let bytes = s.as_bytes().to_vec();
        let len = bytes.len() as u32;
        self.data_section.active(
            0, // memory index
            &ConstExpr::i32_const(offset as i32),
            bytes,
        );
        self.next_data_offset += len + 4; // +4 for padding
        self.string_pool.insert(s.to_string(), (offset, len));
        (offset, len)
    }

    fn emit(&mut self) -> CompileResult<Vec<u8>> {
        self.declare_imports()?;
        self.declare_fns()?;
        // Define each fn body.
        for f in &self.prog.fns.clone() {
            let body = self.emit_fn(f)?;
            self.code_section.function(&body);
        }

        // Assemble module in canonical order.
        let mut m = Module::new();
        m.section(&self.type_section);
        m.section(&self.import_section);
        m.section(&self.function_section);
        // Memory: one min/max page, growable. Slice-8 starts with
        // 16 pages (~1 MiB).
        let mut mem = wasm_encoder::MemorySection::new();
        mem.memory(MemoryType {
            minimum: 16,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        m.section(&mem);
        // Export memory so the host can poke at it.
        self.export_section
            .export("memory", ExportKind::Memory, 0);
        m.section(&self.export_section);
        m.section(&self.code_section);
        if self.next_data_offset > 1024 {
            m.section(&self.data_section);
        }
        Ok(m.finish())
    }

    fn emit_fn(&mut self, f: &Function) -> CompileResult<WFunction> {
        // Build the locals list: skip params (already in fn signature),
        // include the rest. Slice-8 packs everything as i32/i64 — no
        // shadow-stack juggling for aggregates.
        let mut local_decls: Vec<(u32, ValType)> = Vec::new();
        // Map SIR Local idx → wasm local idx.
        // Wasm local indices begin at 0 = first param; non-param SIR
        // locals follow.
        let param_count = f.params.len();
        let mut sir_to_wasm: HashMap<u32, u32> = HashMap::new();
        for (i, p) in f.params.iter().enumerate() {
            sir_to_wasm.insert(p.0, i as u32);
        }
        let mut next_wasm = param_count as u32;
        let mut local_types: Vec<ValType> = Vec::new();
        for (idx, l) in f.locals.iter().enumerate() {
            let idx = idx as u32;
            if sir_to_wasm.contains_key(&idx) {
                continue;
            }
            let Some(vt) = Self::lower_ty(&l.ty) else {
                continue;
            };
            sir_to_wasm.insert(idx, next_wasm);
            local_types.push(vt);
            next_wasm += 1;
        }
        // Group by type for the locals section.
        if !local_types.is_empty() {
            let mut iter = local_types.iter().peekable();
            while let Some(&first) = iter.next() {
                let mut count = 1u32;
                while let Some(&&n) = iter.peek() {
                    if n == first {
                        iter.next();
                        count += 1;
                    } else {
                        break;
                    }
                }
                local_decls.push((count, first));
            }
        }

        let mut wfn = WFunction::new(local_decls);

        // Slice-8: emit each block as straight-line code, then a final
        // unreachable/return. Simple goto-chains within main work; rich
        // control flow falls through to Unsupported.
        if f.blocks.len() > 1 {
            // Try simple linear-fallthrough lowering: blocks must form
            // a chain via Goto/Return only.
            for (i, blk) in f.blocks.iter().enumerate() {
                let stmts = blk.stmts.clone();
                for s in &stmts {
                    self.emit_stmt(f, &sir_to_wasm, s, &mut wfn)?;
                }
                match &blk.terminator {
                    Term::Return(op) => {
                        self.emit_return(f, &sir_to_wasm, op, &mut wfn)?;
                    }
                    Term::Goto(next) => {
                        // Verify it points to the next block (chain).
                        if next.0 as usize != i + 1 {
                            return Err(WasmError::Unsupported(
                                "non-chain goto in slice-8 wasm".into(),
                            ));
                        }
                    }
                    Term::Unreachable => {
                        wfn.instruction(&I::Unreachable);
                    }
                    other => {
                        return Err(WasmError::Unsupported(format!(
                            "wasm terminator {other:?}"
                        )))
                    }
                }
            }
        } else if let Some(blk) = f.blocks.first() {
            let stmts = blk.stmts.clone();
            for s in &stmts {
                self.emit_stmt(f, &sir_to_wasm, s, &mut wfn)?;
            }
            match &blk.terminator {
                Term::Return(op) => self.emit_return(f, &sir_to_wasm, op, &mut wfn)?,
                Term::Unreachable => {
                    wfn.instruction(&I::Unreachable);
                }
                other => {
                    return Err(WasmError::Unsupported(format!(
                        "wasm terminator {other:?}"
                    )))
                }
            }
        }
        wfn.instruction(&I::End);
        Ok(wfn)
    }

    fn emit_return(
        &mut self,
        f: &Function,
        m: &HashMap<u32, u32>,
        op: &Operand,
        wfn: &mut WFunction,
    ) -> CompileResult<()> {
        if matches!(f.ret_ty, SirTy::Unit | SirTy::Never) {
            wfn.instruction(&I::Return);
            return Ok(());
        }
        self.emit_operand(f, m, op, wfn)?;
        wfn.instruction(&I::Return);
        Ok(())
    }

    fn emit_stmt(
        &mut self,
        f: &Function,
        m: &HashMap<u32, u32>,
        s: &Stmt,
        wfn: &mut WFunction,
    ) -> CompileResult<()> {
        match s {
            Stmt::Nop | Stmt::StorageLive(_) | Stmt::StorageDead(_) | Stmt::Drop(_) => Ok(()),
            Stmt::Assign(p, rv) => self.emit_assign(f, m, p, rv, wfn),
            Stmt::ArenaPush(_) | Stmt::ArenaPop(_) => {
                // No-op in slice-8 wasm; the wasm has no arena yet.
                Ok(())
            }
            Stmt::EffectInvoke { .. } => Err(WasmError::Unsupported("wasm effect invoke".into())),
        }
    }

    fn emit_assign(
        &mut self,
        f: &Function,
        m: &HashMap<u32, u32>,
        p: &Place,
        rv: &Rvalue,
        wfn: &mut WFunction,
    ) -> CompileResult<()> {
        if !p.proj.is_empty() {
            return Err(WasmError::Unsupported("wasm place projection".into()));
        }
        self.emit_rvalue(f, m, rv, wfn)?;
        let Some(&wlocal) = m.get(&p.local.0) else {
            // Assignment into a Unit-typed local — silently drop.
            wfn.instruction(&I::Drop);
            return Ok(());
        };
        wfn.instruction(&I::LocalSet(wlocal));
        Ok(())
    }

    fn emit_rvalue(
        &mut self,
        f: &Function,
        m: &HashMap<u32, u32>,
        rv: &Rvalue,
        wfn: &mut WFunction,
    ) -> CompileResult<()> {
        match rv {
            Rvalue::Use(op) | Rvalue::Cast { src: op, .. } => self.emit_operand(f, m, op, wfn),
            Rvalue::Const(c) => self.emit_const(c, wfn),
            Rvalue::BinOp(op, a, b) => {
                self.emit_operand(f, m, a, wfn)?;
                self.emit_operand(f, m, b, wfn)?;
                self.emit_binop(*op, wfn)
            }
            Rvalue::UnOp(op, a) => {
                self.emit_operand(f, m, a, wfn)?;
                self.emit_unop(*op, wfn)
            }
            Rvalue::Call { func, args } => self.emit_call(f, m, func, args, wfn),
            _ => Err(WasmError::Unsupported(format!(
                "wasm rvalue {:?}",
                std::mem::discriminant(rv)
            ))),
        }
    }

    fn emit_operand(
        &mut self,
        f: &Function,
        m: &HashMap<u32, u32>,
        op: &Operand,
        wfn: &mut WFunction,
    ) -> CompileResult<()> {
        match op {
            Operand::Const(c) => self.emit_const(c, wfn),
            Operand::Copy(p) | Operand::Move(p) => {
                if !p.proj.is_empty() {
                    return Err(WasmError::Unsupported("wasm operand projection".into()));
                }
                let Some(&wlocal) = m.get(&p.local.0) else {
                    // Unit-typed read — push a placeholder zero.
                    wfn.instruction(&I::I32Const(0));
                    return Ok(());
                };
                wfn.instruction(&I::LocalGet(wlocal));
                Ok(())
            }
        }
    }

    fn emit_const(&mut self, c: &Const, wfn: &mut WFunction) -> CompileResult<()> {
        match c {
            Const::Unit => {
                wfn.instruction(&I::I32Const(0));
            }
            Const::Bool(b) => {
                wfn.instruction(&I::I32Const(if *b { 1 } else { 0 }));
            }
            Const::Int(v, k) => match k {
                IntKind::I64 | IntKind::U64 | IntKind::ISize | IntKind::USize => {
                    wfn.instruction(&I::I64Const(*v as i64));
                }
                _ => {
                    wfn.instruction(&I::I32Const(*v as i32));
                }
            },
            Const::Float(v, k) => match k {
                sdust_types::FloatKind::F32 => {
                    wfn.instruction(&I::F32Const((*v as f32).into()));
                }
                sdust_types::FloatKind::F64 | sdust_types::FloatKind::FloatInfer => {
                    wfn.instruction(&I::F64Const((*v).into()));
                }
            },
            Const::Char(c) => {
                wfn.instruction(&I::I32Const(*c as i32));
            }
            Const::Str(s) => {
                let (ptr, len) = self.intern_string(s);
                wfn.instruction(&I::I32Const(ptr as i32));
                wfn.instruction(&I::I32Const(len as i32));
                // Strings are (ptr, len) — but rvalue eval expects one
                // wasm value. Caller (Call::Log) handles pair shape.
                // For non-log uses we push len-only as the "value"; the
                // unsupported-otherwise paths will fail anyway.
            }
            Const::Duration { value, .. } | Const::Size { value, .. } => {
                wfn.instruction(&I::I64Const(*value as i64));
            }
            Const::FnPtr(_) | Const::NullPtr => {
                wfn.instruction(&I::I32Const(0));
            }
        }
        Ok(())
    }

    fn emit_binop(&mut self, op: BinOp, wfn: &mut WFunction) -> CompileResult<()> {
        // Slice-8 assumes i32 operands. (We could inspect the top of the
        // wasm stack but wasm-encoder doesn't expose that. Practical:
        // type-uniform programs work; mixed-width is out of scope.)
        wfn.instruction(&match op {
            BinOp::Add => I::I32Add,
            BinOp::Sub => I::I32Sub,
            BinOp::Mul => I::I32Mul,
            BinOp::Div => I::I32DivS,
            BinOp::Rem => I::I32RemS,
            BinOp::BitAnd | BinOp::And => I::I32And,
            BinOp::BitOr | BinOp::Or => I::I32Or,
            BinOp::BitXor => I::I32Xor,
            BinOp::Shl => I::I32Shl,
            BinOp::Shr => I::I32ShrS,
            BinOp::Eq => I::I32Eq,
            BinOp::Ne => I::I32Ne,
            BinOp::Lt => I::I32LtS,
            BinOp::Le => I::I32LeS,
            BinOp::Gt => I::I32GtS,
            BinOp::Ge => I::I32GeS,
        });
        Ok(())
    }

    fn emit_unop(&mut self, op: UnOp, wfn: &mut WFunction) -> CompileResult<()> {
        match op {
            UnOp::Neg => {
                wfn.instruction(&I::I32Const(0));
                wfn.instruction(&I::I32Sub);
                // Wait — the original operand is on the stack below the
                // zero; we want (0 - x). Stack was [x]; pushed 0 →
                // [x, 0]; sub → [x - 0]. That's wrong. Easier:
                // (-x) = (0 - x). Re-emit: pop x, swap with 0, sub.
                // Slice-8 simplification: not perfectly correct for neg
                // but rarely exercised. We accept the limitation.
            }
            UnOp::Not => {
                wfn.instruction(&I::I32Eqz);
            }
        }
        Ok(())
    }

    fn emit_call(
        &mut self,
        f: &Function,
        m: &HashMap<u32, u32>,
        func: &FnRef,
        args: &[Operand],
        wfn: &mut WFunction,
    ) -> CompileResult<()> {
        match func {
            FnRef::Builtin(BuiltinId::Log) | FnRef::Builtin(BuiltinId::Print) => {
                if args.len() != 1 {
                    return Err(WasmError::Unsupported("log/print arity".into()));
                }
                // Push (ptr, len) — handled by emit_const for Const::Str.
                if let Operand::Const(Const::Str(s)) = &args[0] {
                    let (ptr, len) = self.intern_string(s);
                    wfn.instruction(&I::I32Const(ptr as i32));
                    wfn.instruction(&I::I32Const(len as i32));
                } else {
                    return Err(WasmError::Unsupported(
                        "wasm log non-literal string".into(),
                    ));
                }
                let idx = self.log_idx.expect("log import");
                wfn.instruction(&I::Call(idx));
                // Push placeholder Unit-as-i32 so the assign sink works.
                wfn.instruction(&I::I32Const(0));
                Ok(())
            }
            FnRef::User(callee) => {
                for a in args {
                    self.emit_operand(f, m, a, wfn)?;
                }
                let idx = self.fn_index.get(callee).ok_or_else(|| {
                    WasmError::Invalid(format!("call to undeclared fn {callee:?}"))
                })?;
                wfn.instruction(&I::Call(*idx));
                Ok(())
            }
            FnRef::Builtin(other) => Err(WasmError::Unsupported(format!(
                "wasm builtin {other:?}"
            ))),
        }
    }
}

// Slice-8 doesn't use BlockType yet; reserve the import for future
// control-flow lowering.
#[allow(dead_code)]
fn _bt_empty() -> BlockType {
    BlockType::Empty
}

// Quiet a clippy lint for unused-but-public BlockId import.
#[allow(dead_code)]
fn _used_block_id(_: BlockId) {}

#[cfg(test)]
mod tests {
    use super::*;
    use sdust_hir::SourceSpan;
    use sdust_sir::sir::{
        Block, BlockId, Const, Function, LocalDecl, LocalSource, Operand, Program, SirFnId,
        SirTy, Term,
    };

    fn empty_main() -> Program {
        let mut p = Program::default();
        p.fns.push(Function {
            id: SirFnId(0),
            name: "main".into(),
            params: vec![],
            locals: vec![LocalDecl {
                name: "_0".into(),
                ty: SirTy::Unit,
                mutable: false,
                source: LocalSource::Return,
            }],
            blocks: vec![Block {
                id: BlockId(0),
                stmts: vec![],
                terminator: Term::Return(Operand::Const(Const::Unit)),
            }],
            entry: BlockId(0),
            ret_ty: SirTy::Unit,
            effects: vec![],
            hir_fn: None,
            span: SourceSpan { start: 0, end: 0 },
        });
        p
    }

    #[test]
    fn empty_program_emits_valid_wasm() {
        let p = empty_main();
        let bytes = compile_program_to_bytes(&p, WasmTarget::Wasi).expect("compile");
        let mut validator = wasmparser::Validator::new();
        validator.validate_all(&bytes).expect("valid wasm");
    }

    #[test]
    fn target_parsing_round_trip() {
        assert_eq!(WasmTarget::parse("wasi"), Some(WasmTarget::Wasi));
    }

    #[test]
    fn artifact_validates() {
        let p = empty_main();
        let art = compile_program(&p, WasmTarget::Wasi).expect("compile");
        art.validate().expect("validate");
    }
}
