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
use crate::aggregate::{
    field_load_ty, is_aggregate, slot_size, struct_field_offset, tuple_offset, type_align,
    type_size, variant_field_offset, TAG_OFFSET, TAG_SIZE,
};
use crate::error::{CodegenError, CompileResult};
use crate::runtime_imports;
use cranelift_codegen::ir::types as ct;
use cranelift_codegen::ir::{
    AbiParam, Function as ClFunction, InstBuilder, MemFlags, Signature, SourceLoc, StackSlotData,
    StackSlotKind, UserFuncName,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use mty_ir::ir::{
    BinOp, BlockId, BuiltinId, Const, FnRef, Function, IrFnId, IrTy, Local, Operand, Place,
    Program, Projection, Rvalue, Stmt, Term, UnOp,
};
#[allow(unused_imports)]
use mty_types::IntKind;
use std::collections::HashMap;
use target_lexicon::Triple;

/// Spread (block_idx, stmt_idx_in_block) across the function's source
/// byte range so each statement gets a unique offset inside the
/// fn span. v0.21 used this exclusively because MtyIR statements
/// didn't yet carry their own `SourceSpan`; v0.22 (`lower_one_block`)
/// now prefers real spans from `Program::span_table` and falls back to
/// this synthetic spread only for manually-constructed Functions
/// (mono-specialized fns, the JIT bootstrap stub, hand-built tests).
fn synthesize_stmt_offset(
    fn_start: u32,
    fn_end: u32,
    block_idx: usize,
    stmt_idx: usize,
    stmts_in_block: usize,
) -> u32 {
    let span = (fn_end.saturating_sub(fn_start)).max(1);
    let denom = ((block_idx + 1) as u64) * (stmts_in_block.max(1) as u64);
    let num = (block_idx as u64) * (stmts_in_block.max(1) as u64) + (stmt_idx as u64);
    let frac = (num.saturating_mul(span as u64) / denom.max(1)) as u32;
    fn_start.saturating_add(frac.min(span.saturating_sub(1)))
}

/// v0.21: per-instruction `MachSrcLoc` plumbing output.
///
/// For each fn we lower, we hand cranelift a sequence of synthetic
/// `SourceLoc` values (one per MtyIR statement / terminator). After
/// `Module::define_function` finishes, we read back the per-instruction
/// `MachSrcLoc` map from `compiled_code().buffer.get_srclocs_sorted()`
/// and store one [`FnSrcLocMap`] per fn here. The DWARF builder then
/// converts each `(code_offset, src_idx)` into a [`mty_debuginfo::LineRow`]
/// with `is_stmt = true` on the first row of each statement and
/// `end_sequence = true` on the last row.
#[derive(Debug, Clone, Default)]
pub struct FnSrcLocMap {
    /// Size of the compiled function in bytes (== high_pc - low_pc).
    pub code_size: u32,
    /// Source-byte-offsets we handed to cranelift, indexed by
    /// `SourceLoc.bits()`. Index 0 is reserved for the fn's own span
    /// start (used when no per-stmt loc is set).
    pub stmt_byte_offsets: Vec<u32>,
    /// `(code_offset_within_fn, byte_offset_in_source)` rows, sorted
    /// by code_offset, deduped on (code_offset, byte_offset).
    pub rows: Vec<(u32, u32)>,
    /// Per-local slot offsets observed during lowering. Indexed by
    /// the MtyIR `Local` id (== `f.locals` index). `None` for locals
    /// we didn't materialise into a stack slot.
    pub local_slot_offsets: HashMap<u32, i32>,
}

/// Per-module lowering context. Holds the cranelift module + per-fn
/// FuncId lookup tables. Lifetime tied to the module's lifetime.
pub struct LowerCtx<'m, M: Module> {
    pub module: &'m mut M,
    pub fn_ids: HashMap<IrFnId, FuncId>,
    pub fn_sigs: HashMap<IrFnId, Signature>,
    pub runtime_ids: HashMap<&'static str, FuncId>,
    pub string_pool: HashMap<String, DataId>,
    pub triple: Triple,
    /// v0.21: per-fn `MachSrcLoc` debug info captured during
    /// `define_fn`. Populated only when [`Self::capture_debug_info`]
    /// is true; the AOT object-debug path enables it.
    pub fn_debug: HashMap<IrFnId, FnSrcLocMap>,
    /// v0.21: when true, `define_fn` instruments per-statement
    /// SourceLoc values and reads back the `MachSrcLoc` map.
    pub capture_debug_info: bool,
}

impl<'m, M: Module> LowerCtx<'m, M> {
    pub fn new(module: &'m mut M, triple: Triple) -> Self {
        Self {
            module,
            // v0.8: pre-size the maps to avoid rehashes on programs
            // with > ~30 fns (most real programs). Capacity numbers
            // are conservative — a 1 KLOC Mighty file averages ~100
            // fns + ~30 runtime imports.
            fn_ids: HashMap::with_capacity(128),
            fn_sigs: HashMap::with_capacity(128),
            runtime_ids: HashMap::with_capacity(runtime_imports::RUNTIME_IMPORTS.len()),
            string_pool: HashMap::with_capacity(64),
            triple,
            fn_debug: HashMap::with_capacity(128),
            capture_debug_info: false,
        }
    }

    /// Enable v0.21 `MachSrcLoc` plumbing for subsequent `define_fn`
    /// calls. The AOT object-debug path flips this on; the JIT path
    /// leaves it off (no DWARF emitted there).
    pub fn enable_debug_capture(&mut self) {
        self.capture_debug_info = true;
    }

    /// Declare every fn in `prog`. Pre-declaration lets call sites
    /// resolve forward references without a separate pass.
    ///
    /// v0.8: pre-builds the parameter-type vector and shares the
    /// build_signature cost across all fns. Pre-sized HashMaps reduce
    /// rehash overhead on large programs.
    pub fn declare_fns(&mut self, prog: &Program) -> CompileResult<()> {
        // Runtime imports first. The signatures depend only on the
        // host call conv, so a stdlib metadata cache would help —
        // they're already cheap (one Vec<AbiParam> per import) and
        // run once per compile, but pre-sizing the map matters for
        // larger programs where every import lookup is on the hot
        // path of subsequent codegen.
        let cc = host_call_conv(&self.triple);
        self.runtime_ids
            .reserve(runtime_imports::RUNTIME_IMPORTS.len());
        for ri in runtime_imports::RUNTIME_IMPORTS {
            let sig = ri.signature(cc);
            let id = self
                .module
                .declare_function(ri.name, Linkage::Import, &sig)
                .map_err(|e| CodegenError::Module(e.to_string()))?;
            self.runtime_ids.insert(ri.name, id);
        }
        // User fns next. Pre-size both maps for the known fn count to
        // avoid rehashing as we walk a large program.
        self.fn_ids.reserve(prog.fns.len());
        self.fn_sigs.reserve(prog.fns.len());
        // Reuse a scratch Vec across fns; the previous code allocated
        // a fresh Vec per fn.
        let mut param_tys: Vec<mty_ir::ir::IrTy> = Vec::with_capacity(8);
        for f in &prog.fns {
            param_tys.clear();
            param_tys.extend(f.params.iter().map(|p| f.locals[p.0 as usize].ty.clone()));
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
        let sig =
            self.fn_sigs.get(&f.id).cloned().ok_or_else(|| {
                CodegenError::Module(format!("missing signature for fn {}", f.name))
            })?;

        let mut clf = ClFunction::with_name_signature(UserFuncName::user(0, f.id.0), sig);
        let mut ctx = FunctionBuilderContext::new();
        let capture_debug = self.capture_debug_info;
        let mut debug = if capture_debug {
            Some(FnSrcLocMap::default())
        } else {
            None
        };
        lower_one(self, prog, f, &mut clf, &mut ctx, debug.as_mut())?;

        let mut mctx = self.module.make_context();
        let func_display = format!("{}", clf.display());
        // Debug: dump CLIF to a directory if MTY_DUMP_CLIF=<dir> is set.
        // Useful for producing vanilla-Cranelift reproducers of upstream
        // egraph/codegen bugs we can't easily isolate from .mty source.
        if let Ok(dir) = std::env::var("MTY_DUMP_CLIF") {
            let path = std::path::PathBuf::from(&dir).join(format!("{}.clif", f.name));
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(&path, &func_display);
        }
        mctx.func = clf;
        if let Err(e) = self.module.define_function(func_id, &mut mctx) {
            return Err(CodegenError::VerifierFailed {
                name: f.name.clone(),
                msg: format!("{e:?}\n--- IR ---\n{func_display}"),
            });
        }
        if let Some(d) = debug.as_mut() {
            // Pull the per-instruction MachSrcLoc map plus the
            // compiled code size. cranelift gives us sorted-by-start
            // `(start, end, SourceLoc)` triples; we collapse to one
            // entry per unique (code_offset, src_idx) pair.
            if let Some(cc) = mctx.compiled_code() {
                d.code_size = cc.buffer.total_size();
                let srclocs = cc.buffer.get_srclocs_sorted();
                let mut seen: std::collections::HashSet<(u32, u32)> =
                    std::collections::HashSet::with_capacity(srclocs.len());
                for sl in srclocs {
                    if sl.loc.is_default() {
                        // cranelift emits default-loc entries for the
                        // prologue + any instruction without a srcloc
                        // call. Skip them so the line table stays
                        // tight against real source positions.
                        continue;
                    }
                    let key = (sl.start, sl.loc.bits());
                    if seen.insert(key) {
                        d.rows.push(key);
                    }
                }
                // The buffer is already sorted by start, but defensive
                // sort costs nothing relative to the rest of compile.
                d.rows.sort_by_key(|(off, _)| *off);
            }
            self.fn_debug.insert(f.id, std::mem::take(d));
        }
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
    debug: Option<&mut FnSrcLocMap>,
) -> CompileResult<()> {
    let mut b = FunctionBuilder::new(clf, ctx);
    {
        let mut fl = FnLower::new(mod_ctx, prog, f, &mut b, debug)?;
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
pub struct FnLower<'short, 'long, 'a, 'm, 'p, 'd, M: Module> {
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
    /// For aggregate locals: the StackSlot backing the value. The
    /// `Variable` holds the *address* (i64) of that slot.
    pub agg_slots: HashMap<Local, cranelift_codegen::ir::StackSlot>,
    /// v0.21: optional debug-info sink. When `Some`, every statement /
    /// terminator we lower is annotated with a unique `SourceLoc`
    /// whose `bits()` index into `debug.stmt_byte_offsets` →
    /// source-byte-offset. Cranelift records these on every emitted
    /// machine instruction; the `define_fn` post-pass reads them back
    /// out via `MachBufferFinalized::get_srclocs_sorted()`.
    pub debug: Option<&'d mut FnSrcLocMap>,
}

impl<'short, 'long, 'a, 'm, 'p, 'd, M: Module> FnLower<'short, 'long, 'a, 'm, 'p, 'd, M> {
    fn new(
        mod_ctx: &'a mut LowerCtx<'m, M>,
        prog: &'p Program,
        f: &'p Function,
        b: &'short mut FunctionBuilder<'long>,
        debug: Option<&'d mut FnSrcLocMap>,
    ) -> CompileResult<Self> {
        Ok(Self {
            mod_ctx,
            prog,
            f,
            b,
            vars: HashMap::new(),
            blocks: HashMap::new(),
            agg_slots: HashMap::new(),
            debug,
        })
    }

    /// v0.21: record `byte_offset` as a fresh `SourceLoc` and tell
    /// cranelift to attach it to every subsequent emitted instruction
    /// until the next call. Returns the synthetic loc index.
    fn note_stmt_loc(&mut self, byte_offset: u32) {
        let Some(d) = self.debug.as_deref_mut() else {
            return;
        };
        let idx = d.stmt_byte_offsets.len() as u32;
        if idx == u32::MAX {
            return;
        }
        d.stmt_byte_offsets.push(byte_offset);
        self.b.set_srcloc(SourceLoc::new(idx));
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
        let lty = self.f.locals[l.0 as usize].ty.clone();
        // Aggregates: declare an i64 var holding the address of the
        // value. Stack slots are materialised lazily by `agg_slot_addr`
        // when we need to *construct* a value (AdtInit/TupleInit).
        // For aggregate params and aggregate locals fed by Copy/Move
        // of another address, the Variable holds the existing address.
        let ty = if is_aggregate(&lty) {
            ct::I64
        } else {
            cl_ty_for(&lty)
        };
        let var = self.b.declare_var(ty);
        self.vars.insert(l, var);
        var
    }

    /// Allocate (or fetch) the stack slot backing aggregate local `l`,
    /// and return its address. Used when we need a stable buffer to
    /// write field bytes into.
    fn agg_slot_addr(&mut self, l: Local) -> CompileResult<cranelift_codegen::ir::Value> {
        let _ = self.ensure_var(l);
        let lty = self.f.locals[l.0 as usize].ty.clone();
        if !is_aggregate(&lty) {
            return Err(CodegenError::Unsupported(format!(
                "agg_slot_addr on non-aggregate {l:?}"
            )));
        }
        let slot = match self.agg_slots.get(&l).copied() {
            Some(s) => s,
            None => {
                let size = type_size(&lty, &self.prog.adts);
                let slot_sz = slot_size(size);
                let align = type_align(&lty, &self.prog.adts).max(8);
                let log2_align = (align.next_power_of_two().trailing_zeros()).min(16) as u8;
                let slot = self.b.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    slot_sz,
                    log2_align,
                ));
                self.agg_slots.insert(l, slot);
                slot
            }
        };
        Ok(self.b.ins().stack_addr(ct::I64, slot, 0))
    }

    /// For an aggregate local, return its address. For locals that
    /// already have a backing value (param, or previously assigned),
    /// returns the value of the Variable. For uninitialised locals,
    /// returns the address of a freshly-allocated slot.
    fn agg_addr(&mut self, l: Local) -> CompileResult<cranelift_codegen::ir::Value> {
        let var = self.ensure_var(l);
        // If we've never written to this var, def_var hasn't been
        // called — but cranelift requires every use_var to follow at
        // least one def_var. Lazily allocate a slot and seed the var
        // in that case.
        // Easier: always seed with a slot address if no slot exists yet.
        if !self.agg_slots.contains_key(&l) {
            // If this local is a *parameter*, the entry-block seeding
            // already def_var'd it with the caller-supplied address.
            // Don't overwrite that.
            let is_param = self.f.params.contains(&l);
            if !is_param {
                let addr = self.agg_slot_addr(l)?;
                self.b.def_var(var, addr);
            }
        }
        Ok(self.b.use_var(var))
    }

    /// Materialise the address of a *place* (local + projections).
    /// Returns (base_addr, terminal_type).
    fn place_addr(&mut self, place: &Place) -> CompileResult<(cranelift_codegen::ir::Value, IrTy)> {
        let local_ty = self.f.locals[place.local.0 as usize].ty.clone();
        let mut cur_addr = if is_aggregate(&local_ty) {
            self.agg_addr(place.local)?
        } else if place.proj.iter().any(|p| matches!(p, Projection::Deref)) {
            // Scalar with deref projection: treat the local's value as
            // a pointer.
            let var = self.ensure_var(place.local);
            self.b.use_var(var)
        } else {
            // Scalar-with-non-deref projection: typeck mismatch where
            // the local was poisoned to {error}/scalar but the IR
            // accesses fields. Treat the local's value as an i64
            // address (typical SIR shape for Error-typed locals
            // returned by an aggregate-shaped call).
            let var = self.ensure_var(place.local);
            let v = self.b.use_var(var);
            self.coerce_to(v, ct::I64)
        };
        let mut cur_ty = local_ty;
        for proj in &place.proj {
            match proj {
                Projection::Field(idx) => {
                    match &cur_ty {
                        IrTy::Adt(id, _) => {
                            let adt = self
                                .prog
                                .adt_by_id(*id)
                                .ok_or_else(|| {
                                    CodegenError::Module(format!("missing adt {:?}", id))
                                })?
                                .clone();
                            let (off, _l) = struct_field_offset(&adt, *idx, &self.prog.adts)
                                .ok_or_else(|| {
                                    CodegenError::Module(format!(
                                        "bad field {} in {}",
                                        idx, adt.name
                                    ))
                                })?;
                            cur_addr = self.b.ins().iadd_imm(cur_addr, off as i64);
                            cur_ty = adt.variants[0].fields[*idx].ty.clone();
                        }
                        _ => {
                            // Best-effort: assume natural packing of i64
                            // fields. Codegen will load i64 from
                            // off = idx*8; tolerates SIR type poisoning.
                            let off: i64 = (*idx as i64) * 8;
                            cur_addr = self.b.ins().iadd_imm(cur_addr, off);
                            cur_ty = IrTy::Int(IntKind::I64);
                        }
                    }
                }
                Projection::TupleIndex(idx) => {
                    let elems = match &cur_ty {
                        IrTy::Tuple(elems) => elems.clone(),
                        _ => {
                            return Err(CodegenError::Unsupported("tuple proj on non-tuple".into()))
                        }
                    };
                    let (off, _l) = tuple_offset(&elems, *idx, &self.prog.adts)
                        .ok_or_else(|| CodegenError::Module(format!("bad tuple idx {}", idx)))?;
                    cur_addr = self.b.ins().iadd_imm(cur_addr, off as i64);
                    cur_ty = elems[*idx].clone();
                }
                Projection::VariantField(variant, field) => {
                    match &cur_ty {
                        IrTy::Adt(id, _) => {
                            let adt = self
                                .prog
                                .adt_by_id(*id)
                                .ok_or_else(|| {
                                    CodegenError::Module(format!("missing adt {:?}", id))
                                })?
                                .clone();
                            let (off, _l) =
                                variant_field_offset(&adt, *variant, *field, &self.prog.adts)
                                    .ok_or_else(|| {
                                        CodegenError::Module(format!(
                                            "bad variant.field {}.{} in {}",
                                            variant, field, adt.name
                                        ))
                                    })?;
                            cur_addr = self.b.ins().iadd_imm(cur_addr, off as i64);
                            cur_ty = adt.variants[*variant].fields[*field].ty.clone();
                        }
                        _ => {
                            // Best-effort: assume Result-style layout
                            // (tag(4) + pad to 8, fields at 8 + i*8).
                            let off: i64 = 8 + (*field as i64) * 8;
                            cur_addr = self.b.ins().iadd_imm(cur_addr, off);
                            // Don't know the field type; default i64.
                            cur_ty = IrTy::Int(IntKind::I64);
                            let _ = variant;
                        }
                    }
                }
                Projection::Deref => {
                    // Load the pointer through `cur_addr`, then continue
                    // from the loaded value as the new base.
                    cur_addr = self.b.ins().load(
                        ct::I64,
                        cranelift_codegen::ir::MemFlags::trusted(),
                        cur_addr,
                        0,
                    );
                    // Unwrap one layer of Ref/RawPtr.
                    cur_ty = match cur_ty {
                        IrTy::Ref { inner, .. } | IrTy::RawPtr(inner) => *inner,
                        other => other,
                    };
                }
                Projection::Index(_) => {
                    return Err(CodegenError::Unsupported("array index projection".into()));
                }
            }
        }
        Ok((cur_addr, cur_ty))
    }

    /// Compute the SIR type a `Place` resolves to (walks projections).
    fn place_type(&self, place: &Place) -> IrTy {
        let mut cur = self.f.locals[place.local.0 as usize].ty.clone();
        for proj in &place.proj {
            cur = match proj {
                Projection::Field(idx) => {
                    if let IrTy::Adt(id, _) = &cur {
                        if let Some(a) = self.prog.adt_by_id(*id) {
                            a.variants[0]
                                .fields
                                .get(*idx)
                                .map(|f| f.ty.clone())
                                .unwrap_or(IrTy::Error)
                        } else {
                            IrTy::Error
                        }
                    } else {
                        IrTy::Error
                    }
                }
                Projection::TupleIndex(idx) => {
                    if let IrTy::Tuple(es) = &cur {
                        es.get(*idx).cloned().unwrap_or(IrTy::Error)
                    } else {
                        IrTy::Error
                    }
                }
                Projection::VariantField(v, f_idx) => {
                    if let IrTy::Adt(id, _) = &cur {
                        if let Some(a) = self.prog.adt_by_id(*id) {
                            a.variants
                                .get(*v)
                                .and_then(|var| var.fields.get(*f_idx))
                                .map(|fld| fld.ty.clone())
                                .unwrap_or(IrTy::Error)
                        } else {
                            IrTy::Error
                        }
                    } else {
                        IrTy::Error
                    }
                }
                Projection::Deref => match cur {
                    IrTy::Ref { inner, .. } | IrTy::RawPtr(inner) => *inner,
                    other => other,
                },
                Projection::Index(_) => match cur {
                    IrTy::Array { elem, .. } => *elem,
                    other => other,
                },
            };
        }
        cur
    }

    /// Emit a scalar load from `addr+0` for SIR type `ty`. Used by the
    /// aggregate-read path.
    fn load_scalar(
        &mut self,
        addr: cranelift_codegen::ir::Value,
        ty: &IrTy,
    ) -> CompileResult<cranelift_codegen::ir::Value> {
        let cl_ty = field_load_ty(ty).ok_or_else(|| {
            CodegenError::Unsupported(format!("load of non-scalar field type {:?}", ty))
        })?;
        Ok(self
            .b
            .ins()
            .load(cl_ty, cranelift_codegen::ir::MemFlags::trusted(), addr, 0))
    }

    /// Store a scalar `val` (already typed appropriately) to `addr+0`.
    fn store_scalar(
        &mut self,
        addr: cranelift_codegen::ir::Value,
        val: cranelift_codegen::ir::Value,
        ty: &IrTy,
    ) -> CompileResult<()> {
        // Coerce the value to the field's natural width before storing.
        let want = field_load_ty(ty).ok_or_else(|| {
            CodegenError::Unsupported(format!("store of non-scalar field type {:?}", ty))
        })?;
        let val = self.coerce_to(val, want);
        self.b
            .ins()
            .store(cranelift_codegen::ir::MemFlags::trusted(), val, addr, 0);
        Ok(())
    }

    /// memcpy a block of `size` bytes from `src` to `dst`. Used when
    /// copying an aggregate value between locals.
    fn memcpy_bytes(
        &mut self,
        dst: cranelift_codegen::ir::Value,
        src: cranelift_codegen::ir::Value,
        size: u32,
    ) {
        // Lower as a byte-by-byte i64/i32/i16/i8 chain. Small fixed-size
        // aggregates rarely exceed 64 bytes, so this is fine.
        let mut off: u32 = 0;
        let mut remaining = size;
        while remaining >= 8 {
            let v = self.b.ins().load(
                ct::I64,
                cranelift_codegen::ir::MemFlags::trusted(),
                src,
                off as i32,
            );
            self.b.ins().store(
                cranelift_codegen::ir::MemFlags::trusted(),
                v,
                dst,
                off as i32,
            );
            off += 8;
            remaining -= 8;
        }
        while remaining >= 4 {
            let v = self.b.ins().load(
                ct::I32,
                cranelift_codegen::ir::MemFlags::trusted(),
                src,
                off as i32,
            );
            self.b.ins().store(
                cranelift_codegen::ir::MemFlags::trusted(),
                v,
                dst,
                off as i32,
            );
            off += 4;
            remaining -= 4;
        }
        while remaining >= 2 {
            let v = self.b.ins().load(
                ct::I16,
                cranelift_codegen::ir::MemFlags::trusted(),
                src,
                off as i32,
            );
            self.b.ins().store(
                cranelift_codegen::ir::MemFlags::trusted(),
                v,
                dst,
                off as i32,
            );
            off += 2;
            remaining -= 2;
        }
        while remaining >= 1 {
            let v = self.b.ins().load(
                ct::I8,
                cranelift_codegen::ir::MemFlags::trusted(),
                src,
                off as i32,
            );
            self.b.ins().store(
                cranelift_codegen::ir::MemFlags::trusted(),
                v,
                dst,
                off as i32,
            );
            off += 1;
            remaining -= 1;
        }
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
        // v0.22: prefer the real per-Stmt / per-Term `SourceSpan` from
        // `Program::span_table` when populated (HIR-lowered Functions).
        // Manually-constructed Functions (mono, tests, synthesised
        // wrappers) leave the side-table empty; for those we fall back
        // to the v0.21 synthetic-spread that walks the fn's source
        // range so each stmt still gets a distinct, monotonic offset.
        let fn_start = self.f.span.start;
        let fn_end = self.f.span.end.max(fn_start + 1);
        let stmt_count = self.f.blocks[idx].stmts.len();
        let stmts_in_block = stmt_count + 1;
        let fn_table = self.prog.span_table.get(&self.f.id);
        for s in 0..stmt_count {
            let offset = match fn_table.and_then(|t| t.stmt_span(idx as u32, s)) {
                Some(span) if span.start != 0 || span.end != 0 => span.start,
                _ => synthesize_stmt_offset(fn_start, fn_end, idx, s, stmts_in_block),
            };
            self.note_stmt_loc(offset);
            let stmt = self.f.blocks[idx].stmts[s].clone();
            self.lower_stmt(&stmt)?;
        }
        // Terminator gets its own loc — important because terminators
        // are typically branch instructions, and stepping behavior in
        // gdb/lldb relies on each branch having a source position.
        let term_offset = match fn_table.and_then(|t| t.terminator_span(idx as u32)) {
            Some(span) if span.start != 0 || span.end != 0 => span.start,
            _ => synthesize_stmt_offset(fn_start, fn_end, idx, stmt_count, stmts_in_block),
        };
        self.note_stmt_loc(term_offset);
        let term = self.f.blocks[idx].terminator.clone();
        self.lower_term(&term)?;
        Ok(())
    }

    fn lower_stmt(&mut self, stmt: &Stmt) -> CompileResult<()> {
        match stmt {
            Stmt::Nop | Stmt::StorageLive(_) | Stmt::StorageDead(_) | Stmt::Drop(_) => Ok(()),
            Stmt::ArenaPush(_) => {
                self.call_rt_no_args("mty_runtime_arena_push", Some(ct::I64))?;
                Ok(())
            }
            Stmt::ArenaPop(_) => {
                let zero = self.b.ins().iconst(ct::I64, 0);
                self.call_rt("mty_runtime_arena_pop", &[zero], None)?;
                Ok(())
            }
            Stmt::Assign(place, rv) => self.lower_assign(place, rv),
            Stmt::EffectInvoke { op, out, .. } => {
                // Slice-8 stub: route through extern_call with the
                // method name. The runtime stub returns 0 — the
                // compiled program continues with a zero-default value.
                let method = match op {
                    mty_ir::ir::EffectOp::GenericCall { path: _, method } => method.clone(),
                };
                let id = self.mod_ctx.intern_string(&method)?;
                let gv = self.mod_ctx.module.declare_data_in_func(id, self.b.func);
                let nptr = self.b.ins().symbol_value(ct::I64, gv);
                let nlen = self.b.ins().iconst(ct::I64, method.len() as i64);
                let nargs = self.b.ins().iconst(ct::I64, 0);
                let r = self
                    .call_rt("mty_runtime_extern_call", &[nptr, nlen, nargs], None)?
                    .unwrap_or_else(|| self.b.ins().iconst(ct::I64, 0));
                if let Some(p) = out {
                    if !p.proj.is_empty() {
                        let (addr, ty) = self.place_addr(p)?;
                        self.store_scalar(addr, r, &ty)?;
                    } else {
                        let local_ty = self.f.locals[p.local.0 as usize].ty.clone();
                        let var = self.ensure_var(p.local);
                        let want = if is_aggregate(&local_ty) {
                            ct::I64
                        } else {
                            cl_ty_for(&local_ty)
                        };
                        let v = self.coerce_to(r, want);
                        self.b.def_var(var, v);
                    }
                }
                Ok(())
            }
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
                if matches!(self.f.ret_ty, IrTy::Unit | IrTy::Never) {
                    self.b.ins().return_(&[]);
                } else {
                    let v = self.eval_operand(op)?;
                    // Coerce return value to the function's declared
                    // return type (the signature was built from ret_ty).
                    let want = if is_aggregate(&self.f.ret_ty) {
                        ct::I64
                    } else {
                        cl_ty_for(&self.f.ret_ty)
                    };
                    // v0.36 T1: pass the source SIR type so the coerce
                    // path can pick `uextend` for unsigned widening
                    // (was always sign-extending, breaking U8 returns).
                    let src_ty = self.operand_ir_ty(op);
                    let v = self.coerce_to_with_src(v, want, src_ty.as_ref());
                    self.b.ins().return_(&[v]);
                }
                Ok(())
            }
            Term::Unreachable => {
                self.b
                    .ins()
                    .trap(cranelift_codegen::ir::TrapCode::user(1).unwrap());
                Ok(())
            }
            Term::Panic { msg } => {
                let v = self.eval_operand(msg)?;
                let zero = self.b.ins().iconst(ct::I64, 0);
                self.call_rt("mty_runtime_panic", &[v, zero], None)?;
                self.b
                    .ins()
                    .trap(cranelift_codegen::ir::TrapCode::user(2).unwrap());
                Ok(())
            }
            Term::TryReturnErr(payload) => {
                // Lower `?`: construct Result::Err(payload) in a fresh
                // stack slot and return its address. The fn's return
                // type should be an ADT with at least 2 variants
                // (Ok=0, Err=1 by convention). If we can't see it as
                // an ADT (typeck didn't propagate), we surface
                // Unsupported so the caller falls back.
                let ret_ty = self.f.ret_ty.clone();
                let adt = match &ret_ty {
                    IrTy::Adt(id, _) => self.prog.adt_by_id(*id).cloned(),
                    _ => None,
                };
                // Allocate a fresh stack slot for the result.
                let size = type_size(&ret_ty, &self.prog.adts).max(8);
                let align = type_align(&ret_ty, &self.prog.adts).max(8);
                let log2_align = (align.next_power_of_two().trailing_zeros()).min(16) as u8;
                let slot = self.b.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    slot_size(size),
                    log2_align,
                ));
                let addr = self.b.ins().stack_addr(ct::I64, slot, 0);
                // Write tag = 1 (Err) if we have an enum.
                let is_enum = adt.as_ref().map(|a| a.variants.len() > 1).unwrap_or(true);
                if is_enum {
                    let tag = self.b.ins().iconst(ct::I32, 1);
                    self.b.ins().store(
                        cranelift_codegen::ir::MemFlags::trusted(),
                        tag,
                        addr,
                        TAG_OFFSET as i32,
                    );
                }
                // Payload offset: per variant_field_offset, variant=1
                // field=0. If we don't have an ADT, fall back to the
                // post-tag aligned slot.
                let payload_off = if let Some(a) = &adt {
                    variant_field_offset(a, 1, 0, &self.prog.adts)
                        .map(|(o, _)| o)
                        .unwrap_or(TAG_SIZE)
                } else {
                    TAG_SIZE
                };
                let payload_addr = self.b.ins().iadd_imm(addr, payload_off as i64);
                // Evaluate the payload operand. If the source ADT was
                // an aggregate at proj-walk, we get its address; copy.
                let v = self.eval_operand(payload)?;
                // Try to determine the payload ty from `payload`.
                let payload_ty = match payload {
                    Operand::Copy(p) | Operand::Move(p) => self.place_type(p),
                    Operand::Const(_) => IrTy::Int(IntKind::I64),
                };
                if is_aggregate(&payload_ty) {
                    let sz = type_size(&payload_ty, &self.prog.adts);
                    self.memcpy_bytes(payload_addr, v, sz);
                } else {
                    self.store_scalar(payload_addr, v, &payload_ty)?;
                }
                // Return addr — or unit if the fn signature has no
                // return slot.
                if matches!(self.f.ret_ty, IrTy::Unit | IrTy::Never) {
                    self.b.ins().return_(&[]);
                } else {
                    let want = if is_aggregate(&self.f.ret_ty) {
                        ct::I64
                    } else {
                        cl_ty_for(&self.f.ret_ty)
                    };
                    let v = self.coerce_to(addr, want);
                    self.b.ins().return_(&[v]);
                }
                Ok(())
            }
            Term::SwitchInt {
                discr,
                arms,
                default,
            } => {
                let disc = self.eval_operand(discr)?;
                let mut else_block = self.ensure_block(*default);
                // Lower as a chain of brifs (small switch).
                for (val, target) in arms {
                    let next = self.b.create_block();
                    let lit = self.b.ins().iconst(ct::I64, *val as i64);
                    let cmp = self.b.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                        disc,
                        lit,
                    );
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
            Term::SwitchVariant {
                discr,
                adt: _adt_id,
                arms,
                default,
            } => {
                // Get the aggregate's address, load the i32 tag, then
                // emit a chain of brif comparisons. Scalar-typed locals
                // that carry an i64 address (typeck-mismatched Error
                // locals) are accepted: their value is used directly.
                let addr = match discr {
                    Operand::Copy(p) | Operand::Move(p) => {
                        if p.proj.is_empty() && !is_aggregate(&self.f.locals[p.local.0 as usize].ty)
                        {
                            let var = self.ensure_var(p.local);
                            let v = self.b.use_var(var);
                            self.coerce_to(v, ct::I64)
                        } else {
                            let (a, _) = self.place_addr(p)?;
                            a
                        }
                    }
                    Operand::Const(_) => {
                        return Err(CodegenError::Unsupported(
                            "switch_variant on const discriminant".into(),
                        ))
                    }
                };
                let tag = self.b.ins().load(
                    ct::I32,
                    cranelift_codegen::ir::MemFlags::trusted(),
                    addr,
                    TAG_OFFSET as i32,
                );
                for (val, target) in arms {
                    let next = self.b.create_block();
                    let lit = self.b.ins().iconst(ct::I32, *val as i64);
                    let cmp =
                        self.b
                            .ins()
                            .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, tag, lit);
                    let tgt = self.ensure_block(*target);
                    self.b.ins().brif(cmp, tgt, &[], next, &[]);
                    self.b.switch_to_block(next);
                    self.b.seal_block(next);
                }
                let default_block = self.ensure_block(*default);
                self.b.ins().jump(default_block, &[]);
                Ok(())
            }
            Term::Suspend { .. } => Err(CodegenError::Unsupported("async suspend".into())),
        }
    }

    fn lower_assign(&mut self, place: &Place, rv: &Rvalue) -> CompileResult<()> {
        // Aggregate-constructing rvalues write directly into the place
        // (which must address an aggregate local). Scalar rvalues
        // produce a value we coerce + def_var.
        let local_ty = self.f.locals[place.local.0 as usize].ty.clone();
        let agg_target = is_aggregate(&local_ty) && place.proj.is_empty();
        match rv {
            Rvalue::AdtInit {
                adt,
                variant,
                fields,
            } if agg_target => {
                let _ = self.ensure_var(place.local);
                let addr = self.agg_slot_addr(place.local)?;
                self.emit_adt_init(addr, *adt, *variant, fields)?;
                // Seed the local's var with the slot address.
                let var = self.vars[&place.local];
                self.b.def_var(var, addr);
                return Ok(());
            }
            Rvalue::TupleInit(elems) if agg_target => {
                let _ = self.ensure_var(place.local);
                let addr = self.agg_slot_addr(place.local)?;
                let elem_tys = match &local_ty {
                    IrTy::Tuple(es) => es.clone(),
                    _ => return Err(CodegenError::Unsupported("non-tuple TupleInit".into())),
                };
                for (i, el) in elems.iter().enumerate() {
                    let (off, _l) = tuple_offset(&elem_tys, i, &self.prog.adts)
                        .ok_or_else(|| CodegenError::Module(format!("bad tuple init idx {}", i)))?;
                    let field_addr = self.b.ins().iadd_imm(addr, off as i64);
                    let v = self.eval_operand(el)?;
                    self.store_scalar(field_addr, v, &elem_tys[i])?;
                }
                let var = self.vars[&place.local];
                self.b.def_var(var, addr);
                return Ok(());
            }
            _ => {}
        }
        if !place.proj.is_empty() {
            // Store through projection.
            let (addr, ty) = self.place_addr(place)?;
            let v = self.eval_rvalue(rv)?;
            self.store_scalar(addr, v, &ty)?;
            return Ok(());
        }
        // Aggregate-typed locals receiving a *whole-aggregate* Use/Copy
        // → memcpy into a fresh slot. Only triggers when both sides are
        // aggregate AND the source has no projections (otherwise the
        // source might be a scalar field of an aggregate, which falls
        // through to the scalar def_var path below).
        if agg_target {
            if let Rvalue::Use(Operand::Copy(src) | Operand::Move(src)) = rv {
                let src_ty = self.place_type(src);
                if is_aggregate(&src_ty) && src.proj.is_empty() {
                    let (src_addr, _src_ty2) = self.place_addr(src)?;
                    let _ = self.ensure_var(place.local);
                    let dst_addr = self.agg_slot_addr(place.local)?;
                    let dst_size = type_size(&local_ty, &self.prog.adts);
                    let src_size = type_size(&src_ty, &self.prog.adts);
                    let size = dst_size.min(src_size).max(1);
                    self.memcpy_bytes(dst_addr, src_addr, size);
                    let var = self.vars[&place.local];
                    self.b.def_var(var, dst_addr);
                    return Ok(());
                }
            }
        }
        let val = self.eval_rvalue(rv)?;
        let var = self.ensure_var(place.local);
        let want = if is_aggregate(&local_ty) {
            ct::I64
        } else {
            cl_ty_for(&local_ty)
        };
        // v0.36 T1: prefer `uextend` for Rvalue::Use(unsigned) → wider
        // local. For other rvalue shapes (BinOp/Cast/Call) we delegate
        // to the typed coerce path with the local's own type — that
        // covers BinOp results (which already widen internally) and
        // Cast (which knows its own src type via eval_rvalue).
        let src_ty = match rv {
            Rvalue::Use(op) => self.operand_ir_ty(op),
            _ => None,
        };
        let val = self.coerce_to_with_src(val, want, src_ty.as_ref());
        self.b.def_var(var, val);
        Ok(())
    }

    /// Initialise an ADT value at `addr`. Writes the tag (if enum),
    /// then each field at its computed offset.
    fn emit_adt_init(
        &mut self,
        addr: cranelift_codegen::ir::Value,
        adt_id: mty_types::AdtId,
        variant: usize,
        fields: &[Operand],
    ) -> CompileResult<()> {
        let adt = self
            .prog
            .adt_by_id(adt_id)
            .ok_or_else(|| CodegenError::Module(format!("undeclared adt {:?}", adt_id)))?
            .clone();
        // Enum: write tag at offset 0.
        if adt.variants.len() > 1 {
            let tag_addr = self.b.ins().iadd_imm(addr, TAG_OFFSET as i64);
            let tagv = self.b.ins().iconst(ct::I32, variant as i64);
            self.b.ins().store(
                cranelift_codegen::ir::MemFlags::trusted(),
                tagv,
                tag_addr,
                0,
            );
            let _ = TAG_SIZE;
        }
        // Fields: per-variant offsets.
        for (i, op) in fields.iter().enumerate() {
            let (off, _l) =
                variant_field_offset(&adt, variant, i, &self.prog.adts).ok_or_else(|| {
                    CodegenError::Module(format!(
                        "bad init field {}.{} in {}",
                        variant, i, adt.name
                    ))
                })?;
            let field_addr = self.b.ins().iadd_imm(addr, off as i64);
            let field_ty = &adt.variants[variant].fields[i].ty;
            let v = self.eval_operand(op)?;
            // If the field type is itself aggregate, the operand must be
            // an aggregate-pointer and we memcpy.
            if is_aggregate(field_ty) {
                let size = type_size(field_ty, &self.prog.adts);
                self.memcpy_bytes(field_addr, v, size);
            } else {
                self.store_scalar(field_addr, v, field_ty)?;
            }
        }
        Ok(())
    }

    /// SIR → Cranelift IR type for an [`Operand`]. Returns `None` when
    /// the operand is a constant whose declared type doesn't pin a SIR
    /// type (rare — `Const::Unit` etc.). v0.36 T1: used by the binop /
    /// coerce_to paths to choose signed vs unsigned widening.
    fn operand_ir_ty(&self, op: &Operand) -> Option<IrTy> {
        match op {
            Operand::Copy(p) | Operand::Move(p) => {
                let mut ty = self.f.locals[p.local.0 as usize].ty.clone();
                for proj in &p.proj {
                    match proj {
                        Projection::Field(i) => {
                            if let IrTy::Adt(id, _) = &ty {
                                if let Some(adt) = self.prog.adt_by_id(*id) {
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
                                if let Some(adt) = self.prog.adt_by_id(*id) {
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
    /// types. v0.36 T1: used to pick `uextend` vs `sextend`.
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

    /// Variant of [`Self::coerce_to`] that knows the SIR type of the
    /// source value, so it can pick `uextend` for unsigned widening
    /// instead of the default `sextend`. v0.36 T1 — fix for the U8 →
    /// wider-int arithmetic / fn-arg / return widening bug.
    fn coerce_to_with_src(
        &mut self,
        val: cranelift_codegen::ir::Value,
        want: cranelift_codegen::ir::Type,
        src_ty: Option<&IrTy>,
    ) -> cranelift_codegen::ir::Value {
        let have = self.b.func.dfg.value_type(val);
        if have == want {
            return val;
        }
        if have.is_int() && want.is_int() {
            if have.bits() < want.bits() {
                let unsigned = src_ty.is_some_and(Self::is_unsigned_int_ty);
                if unsigned {
                    return self.b.ins().uextend(want, val);
                }
                return self.b.ins().sextend(want, val);
            }
            if have.bits() > want.bits() {
                return self.b.ins().ireduce(want, val);
            }
        }
        self.coerce_to(val, want)
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
        if have.is_int() && want.is_int() {
            if have.bits() < want.bits() {
                return self.b.ins().sextend(want, val);
            }
            if have.bits() > want.bits() {
                return self.b.ins().ireduce(want, val);
            }
        }
        // Float ↔ float widen/narrow.
        if have.is_float() && want.is_float() {
            if have.bits() < want.bits() {
                return self.b.ins().fpromote(want, val);
            }
            if have.bits() > want.bits() {
                return self.b.ins().fdemote(want, val);
            }
        }
        // Float ↔ int via bitcast (preserve bits). This loses semantic
        // meaning but unblocks SIR-typeck mismatches where a float
        // value flows into a local typeck-marked as int (typically
        // when an opaque-ADT-wrapped primitive's bound binding is
        // mis-resolved). The bytes are preserved so subsequent reads
        // through projection still get the right scalar.
        if have.is_int() && want.is_float() && have.bits() == want.bits() {
            return self
                .b
                .ins()
                .bitcast(want, cranelift_codegen::ir::MemFlags::new(), val);
        }
        if have.is_float() && want.is_int() && have.bits() == want.bits() {
            return self
                .b
                .ins()
                .bitcast(want, cranelift_codegen::ir::MemFlags::new(), val);
        }
        // Different-bit-width float/int: extend or reduce to bit-width,
        // then bitcast. Don't try too hard — slice 8 codegen returns
        // Unsupported for the rare cases.
        if have.is_float() && want.is_int() {
            // round-trip through bits
            let intermediate = if have.bits() == 32 { ct::I32 } else { ct::I64 };
            let bits =
                self.b
                    .ins()
                    .bitcast(intermediate, cranelift_codegen::ir::MemFlags::new(), val);
            return self.coerce_to(bits, want);
        }
        if have.is_int() && want.is_float() {
            let intermediate = if want.bits() == 32 { ct::I32 } else { ct::I64 };
            let bits = self.coerce_to(val, intermediate);
            return self
                .b
                .ins()
                .bitcast(want, cranelift_codegen::ir::MemFlags::new(), bits);
        }
        val
    }

    fn eval_rvalue(&mut self, rv: &Rvalue) -> CompileResult<cranelift_codegen::ir::Value> {
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
            Rvalue::Cast { src, ty } => {
                let v = self.eval_operand(src)?;
                let want = cl_ty_for(ty);
                let src_ty = self.operand_ir_ty(src);
                Ok(self.coerce_to_with_src(v, want, src_ty.as_ref()))
            }
            Rvalue::FieldRead { receiver, field } => {
                let mut p = receiver.clone();
                p.proj.push(Projection::Field(*field));
                let (addr, ty) = self.place_addr(&p)?;
                if is_aggregate(&ty) {
                    Ok(addr)
                } else {
                    self.load_scalar(addr, &ty)
                }
            }
            Rvalue::TupleRead { receiver, idx } => {
                let mut p = receiver.clone();
                p.proj.push(Projection::TupleIndex(*idx));
                let (addr, ty) = self.place_addr(&p)?;
                if is_aggregate(&ty) {
                    Ok(addr)
                } else {
                    self.load_scalar(addr, &ty)
                }
            }
            Rvalue::Ref { place, .. } => {
                // Take a pointer to the place; for aggregate locals
                // that's just the slot address.
                let (addr, _) = self.place_addr(place)?;
                Ok(addr)
            }
            Rvalue::Deref(op) => {
                // Load a single pointer-width value from the operand.
                let v = self.eval_operand(op)?;
                Ok(self
                    .b
                    .ins()
                    .load(ct::I64, cranelift_codegen::ir::MemFlags::trusted(), v, 0))
            }
            Rvalue::AdtInit {
                adt,
                variant,
                fields,
            } => {
                // Materialise into a fresh stack slot, return its addr.
                let adt_ref = self
                    .prog
                    .adt_by_id(*adt)
                    .ok_or_else(|| CodegenError::Module(format!("undeclared adt {:?}", adt)))?
                    .clone();
                let sty = IrTy::Adt(*adt, vec![]);
                let size = type_size(&sty, &self.prog.adts).max(8);
                let align = type_align(&sty, &self.prog.adts).max(8);
                let log2_align = (align.next_power_of_two().trailing_zeros()).min(16) as u8;
                let slot = self.b.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    slot_size(size),
                    log2_align,
                ));
                let addr = self.b.ins().stack_addr(ct::I64, slot, 0);
                self.emit_adt_init(addr, adt_ref.adt, *variant, fields)?;
                Ok(addr)
            }
            Rvalue::TupleInit(elems) => {
                let tys: Vec<IrTy> = elems.iter().map(|_| IrTy::Int(IntKind::I64)).collect();
                let sty = IrTy::Tuple(tys.clone());
                let size = type_size(&sty, &self.prog.adts).max(8);
                let align = type_align(&sty, &self.prog.adts).max(8);
                let log2_align = (align.next_power_of_two().trailing_zeros()).min(16) as u8;
                let slot = self.b.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    slot_size(size),
                    log2_align,
                ));
                let addr = self.b.ins().stack_addr(ct::I64, slot, 0);
                for (i, op) in elems.iter().enumerate() {
                    let (off, _l) = tuple_offset(&tys, i, &self.prog.adts)
                        .ok_or_else(|| CodegenError::Module(format!("bad tuple init {}", i)))?;
                    let field_addr = self.b.ins().iadd_imm(addr, off as i64);
                    let v = self.eval_operand(op)?;
                    self.store_scalar(field_addr, v, &tys[i])?;
                }
                Ok(addr)
            }
            Rvalue::ArrayInit(_) => Err(CodegenError::Unsupported(
                "array literal at native lowering".into(),
            )),
            Rvalue::IndexRead { receiver, index } => {
                // Slice-8 best effort: receiver address + (index * 8)
                // load. Only correct when the underlying type is an
                // array of i64-sized elements; otherwise returns
                // garbage but doesn't crash the build.
                let base_raw = {
                    let var = self.ensure_var(receiver.local);
                    self.b.use_var(var)
                };
                let base = self.coerce_to(base_raw, ct::I64);
                let idx = self.eval_operand(index)?;
                let idx_i64 = self.coerce_to(idx, ct::I64);
                let off = self.b.ins().imul_imm(idx_i64, 8);
                let addr = self.b.ins().iadd(base, off);
                Ok(self
                    .b
                    .ins()
                    .load(ct::I64, cranelift_codegen::ir::MemFlags::trusted(), addr, 0))
            }
            Rvalue::MethodCall { method, .. } => {
                // Slice-8 stub: route through the extern bridge as a
                // last resort. Real method-dispatch lowering needs
                // trait resolution from the typechecker.
                let nstr = method.clone();
                let id = self.mod_ctx.intern_string(&nstr)?;
                let gv = self.mod_ctx.module.declare_data_in_func(id, self.b.func);
                let nptr = self.b.ins().symbol_value(ct::I64, gv);
                let nlen = self.b.ins().iconst(ct::I64, nstr.len() as i64);
                let nargs = self.b.ins().iconst(ct::I64, 0);
                let r = self.call_rt("mty_runtime_extern_call", &[nptr, nlen, nargs], None)?;
                Ok(r.unwrap_or_else(|| self.b.ins().iconst(ct::I64, 0)))
            }
            Rvalue::AgentSpawn { agent, .. } => {
                // Route through runtime: mty_runtime_spawn(agent_id) -> i64
                let aid = self.b.ins().iconst(ct::I64, agent.0 as i64);
                let r = self.call_rt("mty_runtime_spawn", &[aid], None)?;
                Ok(r.unwrap_or_else(|| self.b.ins().iconst(ct::I64, 0)))
            }
            Rvalue::Send {
                target,
                msg: _,
                args: _,
            } => {
                // Route through runtime: mty_runtime_send(target, msg, payload)
                let t = self.eval_operand(target)?;
                let m = self.b.ins().iconst(ct::I64, 0);
                let p = self.b.ins().iconst(ct::I64, 0);
                self.call_rt("mty_runtime_send", &[t, m, p], None)?;
                Ok(self.b.ins().iconst(ct::I64, 0))
            }
            Rvalue::Ask {
                target,
                msg: _,
                args: _,
                deadline_ms,
            } => {
                let t = self.eval_operand(target)?;
                let m = self.b.ins().iconst(ct::I64, 0);
                let p = self.b.ins().iconst(ct::I64, 0);
                let d = self
                    .b
                    .ins()
                    .iconst(ct::I64, deadline_ms.unwrap_or(0) as i64);
                let r = self.call_rt("mty_runtime_ask", &[t, m, p, d], None)?;
                Ok(r.unwrap_or_else(|| self.b.ins().iconst(ct::I64, 0)))
            }
            Rvalue::CapValue { .. } => {
                // Slice-8 stub: opaque capability value as null pointer.
                Ok(self.b.ins().iconst(ct::I64, 0))
            }
        }
    }

    fn eval_const(&mut self, c: &Const) -> CompileResult<cranelift_codegen::ir::Value> {
        Ok(match c {
            Const::Unit => self.b.ins().iconst(ct::I64, 0),
            Const::Bool(b) => self.b.ins().iconst(ct::I8, if *b { 1 } else { 0 }),
            Const::Int(v, k) => {
                let t = cl_ty_for(&IrTy::Int(*k));
                self.b.ins().iconst(t, *v as i64)
            }
            Const::Float(v, k) => match k {
                mty_types::FloatKind::F32 => self.b.ins().f32const(*v as f32),
                mty_types::FloatKind::F64 | mty_types::FloatKind::FloatInfer => {
                    self.b.ins().f64const(*v)
                }
            },
            Const::Char(c) => self.b.ins().iconst(ct::I32, *c as i64),
            Const::Str(s) => {
                // v0.36 T1: strings are aggregates (ptr, len) — emit a
                // fresh 16-byte stack slot, write ptr at +0 and len at
                // +8, return the slot address. This lets dynamic-log
                // (`log(local_str)`) read both halves from a stable
                // address, while still cheaply representing literal
                // strings.
                let id = self.mod_ctx.intern_string(s)?;
                let gv = self.mod_ctx.module.declare_data_in_func(id, self.b.func);
                let ptr = self.b.ins().symbol_value(ct::I64, gv);
                let len = self.b.ins().iconst(ct::I64, s.len() as i64);
                let slot = self.b.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    16,
                    3, // log2(8) = 3
                ));
                let addr = self.b.ins().stack_addr(ct::I64, slot, 0);
                self.b
                    .ins()
                    .store(cranelift_codegen::ir::MemFlags::trusted(), ptr, addr, 0);
                self.b
                    .ins()
                    .store(cranelift_codegen::ir::MemFlags::trusted(), len, addr, 8);
                addr
            }
            Const::Duration { value, .. } | Const::Size { value, .. } => {
                self.b.ins().iconst(ct::I64, *value as i64)
            }
            Const::FnPtr(_) => return Err(CodegenError::Unsupported("fn-pointer const".into())),
            Const::NullPtr => self.b.ins().iconst(ct::I64, 0),
        })
    }

    fn eval_operand(&mut self, op: &Operand) -> CompileResult<cranelift_codegen::ir::Value> {
        match op {
            Operand::Copy(p) | Operand::Move(p) => {
                if p.proj.is_empty() {
                    let var = self.ensure_var(p.local);
                    return Ok(self.b.use_var(var));
                }
                // Projection through aggregate: walk to the target and
                // load the scalar (or return the address if aggregate).
                let (addr, ty) = self.place_addr(p)?;
                if is_aggregate(&ty) {
                    Ok(addr)
                } else {
                    self.load_scalar(addr, &ty)
                }
            }
            Operand::Const(c) => self.eval_const(c),
        }
    }

    /// Lower a SIR `BinOp` to a Cranelift instruction sequence. Knows
    /// the SIR types of the two operands and uses them to pick:
    /// - unsigned vs signed widening (`uextend` vs `sextend`) when the
    ///   operands have different cranelift widths,
    /// - unsigned vs signed division / remainder (`udiv`/`urem` vs
    ///   `sdiv`/`srem`),
    /// - unsigned vs signed comparisons,
    /// - logical vs arithmetic right shift (`ushr` vs `sshr`).
    ///
    /// v0.36 T1 — fix for the U8 widening bug + downstream unsigned
    /// op-semantics on cranelift.
    fn lower_binop_typed(
        &mut self,
        op: BinOp,
        a: cranelift_codegen::ir::Value,
        b: cranelift_codegen::ir::Value,
        sa: Option<&IrTy>,
        sb: Option<&IrTy>,
    ) -> CompileResult<cranelift_codegen::ir::Value> {
        use cranelift_codegen::ir::condcodes::{FloatCC, IntCC::*};
        let ta = self.b.func.dfg.value_type(a);
        let tb = self.b.func.dfg.value_type(b);
        // If either side is float, promote both to the wider float.
        if ta.is_float() || tb.is_float() {
            let want = if ta.is_float() && tb.is_float() {
                if ta.bits() >= tb.bits() {
                    ta
                } else {
                    tb
                }
            } else if ta.is_float() {
                ta
            } else {
                tb
            };
            let a = self.coerce_to(a, want);
            let b = self.coerce_to(b, want);
            return Ok(match op {
                BinOp::Add => self.b.ins().fadd(a, b),
                BinOp::Sub => self.b.ins().fsub(a, b),
                BinOp::Mul => self.b.ins().fmul(a, b),
                BinOp::Div => self.b.ins().fdiv(a, b),
                BinOp::Eq => self.b.ins().fcmp(FloatCC::Equal, a, b),
                BinOp::Ne => self.b.ins().fcmp(FloatCC::NotEqual, a, b),
                BinOp::Lt => self.b.ins().fcmp(FloatCC::LessThan, a, b),
                BinOp::Le => self.b.ins().fcmp(FloatCC::LessThanOrEqual, a, b),
                BinOp::Gt => self.b.ins().fcmp(FloatCC::GreaterThan, a, b),
                BinOp::Ge => self.b.ins().fcmp(FloatCC::GreaterThanOrEqual, a, b),
                _ => {
                    return Err(CodegenError::Unsupported(format!(
                        "binop {:?} on float operands",
                        op
                    )))
                }
            });
        }
        let ua = sa.is_some_and(Self::is_unsigned_int_ty);
        let ub = sb.is_some_and(Self::is_unsigned_int_ty);
        // If *either* operand is known unsigned, treat the op as
        // unsigned. (Mixing signed + unsigned is itself a typeck error
        // upstream; this fallback is just defensive.)
        let unsigned = ua || ub;
        // Integer path: widen narrower side to the wider, using the
        // *narrower* side's signedness for the extend choice.
        let (a, b) = if ta.bits() == tb.bits() {
            (a, b)
        } else if ta.bits() < tb.bits() {
            let widened = if ua {
                self.b.ins().uextend(tb, a)
            } else {
                self.b.ins().sextend(tb, a)
            };
            (widened, b)
        } else {
            let widened = if ub {
                self.b.ins().uextend(ta, b)
            } else {
                self.b.ins().sextend(ta, b)
            };
            (a, widened)
        };
        Ok(match op {
            BinOp::Add => self.b.ins().iadd(a, b),
            BinOp::Sub => self.b.ins().isub(a, b),
            BinOp::Mul => self.b.ins().imul(a, b),
            BinOp::Div => {
                if unsigned {
                    self.b.ins().udiv(a, b)
                } else {
                    self.b.ins().sdiv(a, b)
                }
            }
            BinOp::Rem => {
                if unsigned {
                    self.b.ins().urem(a, b)
                } else {
                    self.b.ins().srem(a, b)
                }
            }
            BinOp::BitAnd | BinOp::And => self.b.ins().band(a, b),
            BinOp::BitOr | BinOp::Or => self.b.ins().bor(a, b),
            BinOp::BitXor => self.b.ins().bxor(a, b),
            BinOp::Shl => self.b.ins().ishl(a, b),
            BinOp::Shr => {
                if unsigned {
                    self.b.ins().ushr(a, b)
                } else {
                    self.b.ins().sshr(a, b)
                }
            }
            BinOp::Eq => self.b.ins().icmp(Equal, a, b),
            BinOp::Ne => self.b.ins().icmp(NotEqual, a, b),
            BinOp::Lt => self.b.ins().icmp(
                if unsigned {
                    UnsignedLessThan
                } else {
                    SignedLessThan
                },
                a,
                b,
            ),
            BinOp::Le => self.b.ins().icmp(
                if unsigned {
                    UnsignedLessThanOrEqual
                } else {
                    SignedLessThanOrEqual
                },
                a,
                b,
            ),
            BinOp::Gt => self.b.ins().icmp(
                if unsigned {
                    UnsignedGreaterThan
                } else {
                    SignedGreaterThan
                },
                a,
                b,
            ),
            BinOp::Ge => self.b.ins().icmp(
                if unsigned {
                    UnsignedGreaterThanOrEqual
                } else {
                    SignedGreaterThanOrEqual
                },
                a,
                b,
            ),
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
            FnRef::Builtin(BuiltinId::Log | BuiltinId::Print) => {
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
                    "mty_runtime_log"
                } else {
                    "mty_runtime_print"
                };
                self.call_rt(sym, &[ptr, len], None)?;
                Ok(self.b.ins().iconst(ct::I64, 0))
            }
            FnRef::Builtin(BuiltinId::Panic) => {
                if args.len() != 1 {
                    return Err(CodegenError::Unsupported("panic arity".into()));
                }
                let (ptr, len) = self.string_pair(&args[0])?;
                self.call_rt("mty_runtime_panic", &[ptr, len], None)?;
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
                // Match the callee's expected param types: filter out
                // Unit-typed args (they aren't in the signature) and
                // coerce each remaining arg to the declared param
                // cranelift type. Falls back to a positional pass when
                // the callee fn isn't in the program (shouldn't happen).
                let callee = self.prog.fn_by_id(*callee_id);
                let mut callee_param_tys: Vec<IrTy> = callee
                    .params
                    .iter()
                    .map(|p| callee.locals[p.0 as usize].ty.clone())
                    .filter(|t| !matches!(t, IrTy::Unit | IrTy::Never))
                    .collect();
                let expected = callee_param_tys.len();
                let mut arg_vals: Vec<cranelift_codegen::ir::Value> = Vec::with_capacity(expected);
                for a in args {
                    if arg_vals.len() >= expected {
                        break;
                    }
                    // Skip unit constants entirely.
                    if matches!(a, Operand::Const(Const::Unit)) {
                        continue;
                    }
                    // Skip operands whose declared place type is Unit/Never.
                    if let Operand::Copy(p) | Operand::Move(p) = a {
                        if p.proj.is_empty()
                            && matches!(
                                self.f.locals[p.local.0 as usize].ty,
                                IrTy::Unit | IrTy::Never
                            )
                        {
                            continue;
                        }
                    }
                    let v = self.eval_operand(a)?;
                    // v0.36 T1: capture the operand's SIR type so the
                    // coerce path can pick `uextend` for unsigned
                    // widening (U8 → U32/U64 fn args were wrong).
                    let src_ty = self.operand_ir_ty(a);
                    let want_ty = if !callee_param_tys.is_empty() {
                        Some(callee_param_tys.remove(0))
                    } else {
                        None
                    };
                    let coerced = if let Some(t) = &want_ty {
                        let want = if is_aggregate(t) {
                            ct::I64
                        } else {
                            cl_ty_for(t)
                        };
                        self.coerce_to_with_src(v, want, src_ty.as_ref())
                    } else {
                        v
                    };
                    arg_vals.push(coerced);
                }
                // Pad with zeros if we still have leftover expected
                // params (rare — typeck mismatch).
                while !callee_param_tys.is_empty() {
                    let t = callee_param_tys.remove(0);
                    let want = if is_aggregate(&t) {
                        ct::I64
                    } else {
                        cl_ty_for(&t)
                    };
                    let v = if want.is_float() {
                        if want.bits() == 32 {
                            self.b.ins().f32const(0.0_f32)
                        } else {
                            self.b.ins().f64const(0.0_f64)
                        }
                    } else {
                        self.b.ins().iconst(want, 0)
                    };
                    arg_vals.push(v);
                }
                let call = self.b.ins().call(func_ref, &arg_vals);
                let results = self.b.inst_results(call).to_vec();
                Ok(results
                    .first()
                    .copied()
                    .unwrap_or_else(|| self.b.ins().iconst(ct::I64, 0)))
            }
            FnRef::Builtin(BuiltinId::Extern(name)) if name == "Ok" || name == "Err" => {
                // Constructor for Result. We don't know the exact AdtId
                // (typeck didn't propagate); pretend it's "the function's
                // return type" when the call is followed by a return.
                // Strategy: allocate a fresh slot sized like the return
                // type, store tag (0 for Ok, 1 for Err), then store
                // each arg into the payload region. Return slot address.
                let variant = if name == "Ok" { 0usize } else { 1usize };
                let ret_ty = self.f.ret_ty.clone();
                let adt = match &ret_ty {
                    IrTy::Adt(id, _) => self.prog.adt_by_id(*id).cloned(),
                    _ => None,
                };
                let size = type_size(&ret_ty, &self.prog.adts).max(16);
                let align = type_align(&ret_ty, &self.prog.adts).max(8);
                let log2_align = (align.next_power_of_two().trailing_zeros()).min(16) as u8;
                let slot = self.b.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    slot_size(size),
                    log2_align,
                ));
                let addr = self.b.ins().stack_addr(ct::I64, slot, 0);
                // Always write the tag word (Result is an enum).
                let tag = self.b.ins().iconst(ct::I32, variant as i64);
                self.b.ins().store(
                    cranelift_codegen::ir::MemFlags::trusted(),
                    tag,
                    addr,
                    TAG_OFFSET as i32,
                );
                // Field offsets: prefer the declared ADT layout; fall
                // back to "payload starts at 8".
                for (i, arg) in args.iter().enumerate() {
                    let (off, field_ty) = if let Some(a) = &adt {
                        variant_field_offset(a, variant, i, &self.prog.adts)
                            .map(|(o, _)| (o, a.variants[variant].fields[i].ty.clone()))
                            .unwrap_or((8 + (i as u32) * 8, IrTy::Int(IntKind::I64)))
                    } else {
                        (8 + (i as u32) * 8, IrTy::Int(IntKind::I64))
                    };
                    let field_addr = self.b.ins().iadd_imm(addr, off as i64);
                    let v = self.eval_operand(arg)?;
                    if is_aggregate(&field_ty) {
                        let sz = type_size(&field_ty, &self.prog.adts);
                        self.memcpy_bytes(field_addr, v, sz);
                    } else {
                        let _ = self.store_scalar(field_addr, v, &field_ty);
                    }
                }
                Ok(addr)
            }
            FnRef::Builtin(BuiltinId::Extern(name)) => {
                // Generic extern: route through the runtime ABI bridge.
                // Slice-8 lowers this to a no-op-return stub (the
                // runtime's extern_call returns i64). Real argument
                // marshalling is a v0.2.x follow-up.
                let _ = name;
                // Push the name onto the stack as (ptr, len) and call.
                let nstr = name.clone();
                let id = self.mod_ctx.intern_string(&nstr)?;
                let gv = self.mod_ctx.module.declare_data_in_func(id, self.b.func);
                let nptr = self.b.ins().symbol_value(ct::I64, gv);
                let nlen = self.b.ins().iconst(ct::I64, nstr.len() as i64);
                let nargs = self.b.ins().iconst(ct::I64, 0);
                let r = self.call_rt("mty_runtime_extern_call", &[nptr, nlen, nargs], None)?;
                Ok(r.unwrap_or_else(|| self.b.ins().iconst(ct::I64, 0)))
            }
            FnRef::Builtin(
                BuiltinId::Spawn
                | BuiltinId::Fetch
                | BuiltinId::Move
                | BuiltinId::Valid
                | BuiltinId::Null
                | BuiltinId::RawPtr,
            ) => {
                // Slice-8 stubs: return a zero/null pointer. The
                // interpreter handles the real semantics; compiled code
                // that only depends on the stub-shape for control flow
                // still works.
                Ok(self.b.ins().iconst(ct::I64, 0))
            }
            FnRef::Builtin(BuiltinId::DomOp(_)) => {
                // v0.6 — `dom.*` ops have no native target. The DOM
                // capability is wasm32-web only (the imports point at
                // a JS shim). Returning a zero placeholder keeps the
                // SIR shape valid for cross-target programs that
                // never reach a DOM call at runtime on native.
                Ok(self.b.ins().iconst(ct::I64, 0))
            }
            FnRef::Builtin(BuiltinId::CanvasOp(_)) => {
                // v0.24 — `canvas.*` ops are wasm32-web only (the
                // imports point at the JS host's 2D canvas context).
                // The cranelift backend never reaches these on
                // native; return zero placeholder, same as DomOp.
                Ok(self.b.ins().iconst(ct::I64, 0))
            }
            FnRef::Builtin(BuiltinId::Swarm) => {
                // v0.29 Track A — `swarm(...)` on the cranelift native
                // backend isn't directly representable (the real impl
                // lives in `mty_stdlib::swarm::swarm` as an async fn).
                // The cranelift JIT path is reserved for tight numeric
                // kernels — swarm calls are funneled through the SIR
                // interpreter or the host-target build. Return a zero
                // placeholder so cross-target programs that never hit
                // a swarm call at runtime still emit cleanly.
                Ok(self.b.ins().iconst(ct::I64, 0))
            }
        }
    }

    /// Extract the (ptr, len) pair for a string operand.
    ///
    /// v0.36 T1: supports both literal `Const::Str` (writes a fresh
    /// (ptr,len) pair to a stack slot, reads it back) and locals/places
    /// of `Str`/`String`/`Bytes` type (reads (ptr,len) from the
    /// aggregate-backing stack slot). This unblocks `log(s)` where `s`
    /// is the result of `format!()`, a function-call return, or any
    /// other dynamic string.
    fn string_pair(
        &mut self,
        op: &Operand,
    ) -> CompileResult<(cranelift_codegen::ir::Value, cranelift_codegen::ir::Value)> {
        match op {
            Operand::Const(Const::Str(s)) => {
                // Fast path: skip the round-trip through a stack slot
                // for pure literals; we already have the ptr and the
                // exact len. (Saves an alloc + store/load per literal.)
                let id = self.mod_ctx.intern_string(s)?;
                let gv = self.mod_ctx.module.declare_data_in_func(id, self.b.func);
                let ptr = self.b.ins().symbol_value(ct::I64, gv);
                let len = self.b.ins().iconst(ct::I64, s.len() as i64);
                Ok((ptr, len))
            }
            Operand::Copy(_) | Operand::Move(_) => {
                // Dynamic case: locals of Str type now live in 16-byte
                // stack slots holding (ptr@+0, len@+8). Load both
                // halves through the place's address.
                let addr = self.eval_operand(op)?;
                let ptr =
                    self.b
                        .ins()
                        .load(ct::I64, cranelift_codegen::ir::MemFlags::trusted(), addr, 0);
                let len =
                    self.b
                        .ins()
                        .load(ct::I64, cranelift_codegen::ir::MemFlags::trusted(), addr, 8);
                Ok((ptr, len))
            }
            Operand::Const(_) => Err(CodegenError::Unsupported(
                "non-string constant in log/print".into(),
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
///
/// ### v0.10 — egraph workaround knob
///
/// `MTY_CRANELIFT_NO_OPT=1` in the environment forces
/// `opt_level = "none"`, disabling the egraph optimization pass.
/// This is the documented escape hatch for the Cranelift 0.132
/// egraph stack-overflow on generic-over-`T` + `&[T]` + `Option<&T>`
/// shapes — see
/// [`docs/upstream-issues/cranelift-egraph-bug-v0_9.md`](../../docs/upstream-issues/cranelift-egraph-bug-v0_9.md)
/// and upstream issue
/// <https://github.com/bytecodealliance/wasmtime/issues/13476>.
///
/// When the upstream fix lands and we bump to a patched cranelift,
/// remove the env-var honour + this paragraph.
pub fn default_flags(is_pic: bool) -> cranelift_codegen::settings::Flags {
    let mut b = settings::builder();
    let opt_disabled = std::env::var("MTY_CRANELIFT_NO_OPT")
        .ok()
        .is_some_and(|s| !s.is_empty() && s != "0");
    let _ = b.set("opt_level", if opt_disabled { "none" } else { "speed" });
    let _ = b.set("is_pic", if is_pic { "true" } else { "false" });
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
