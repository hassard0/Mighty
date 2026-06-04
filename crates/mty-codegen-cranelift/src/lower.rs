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

use crate::abi::{build_signature, cl_ty_for, cl_ty_for_variadic, host_call_conv};
use crate::aggregate::{
    field_load_ty, is_aggregate, is_opaque_adt, slot_size, struct_field_offset, tuple_offset,
    type_align, type_size, variant_field_offset, TAG_OFFSET, TAG_SIZE,
};
use crate::error::{CodegenError, CompileResult};
use crate::runtime_imports;
use cranelift_codegen::ir::types as ct;
use cranelift_codegen::ir::{
    AbiParam, Function as ClFunction, InstBuilder, MemFlags, Signature, SourceLoc, StackSlotData,
    StackSlotKind, Type as ClType, UserFuncName,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use mty_ir::ir::{
    BinOp, BlockId, BuiltinId, Const, FnRef, Function, IrFnId, IrTy, Local, Operand, Place,
    Program, Projection, Rvalue, Stmt, Term, UnOp,
};
#[allow(unused_imports)]
use mty_types::{FloatKind, IntKind};
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
    /// v0.36 Track T2 — set of fn ids declared as `Linkage::Import`
    /// because they originated from an `extern c { ... }` block. The
    /// `define_fn` path skips these (the linker provides the body).
    pub extern_fn_ids: std::collections::HashSet<IrFnId>,
    /// v0.38 Track T3 — for each extern fn id, the C-ABI returned-struct
    /// classification (single-reg / two-reg / sret / none). Populated by
    /// `declare_fns` alongside `extern_fn_ids` and consumed by the
    /// call-site lowerer when the callee is an extern fn returning an
    /// aggregate. Non-extern fns omit the entry (treated as `None`).
    pub extern_return_kinds: std::collections::HashMap<IrFnId, crate::abi::AggregateReturnKind>,
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
            extern_fn_ids: std::collections::HashSet::with_capacity(16),
            extern_return_kinds: std::collections::HashMap::with_capacity(16),
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
            // v0.36 Track T2 — an `extern c { fn ... }` declaration
            // produces an SIR shell with no body. We must declare it
            // to cranelift as `Linkage::Import` so the final linker
            // resolves the symbol against one of the manifest's
            // `[[extern_lib]]` archives, *not* as a local fn (which
            // would emit an empty body trampoline that returns 0 and
            // silently mask FFI calls).
            //
            // Detection: any fn id present in `prog.extern_bindings`
            // came from an extern block. The wasm backend already
            // consults this same table for `(import ...)` emission,
            // so the contract is well established.
            let is_extern = prog.extern_bindings.contains_key(&f.id);
            // v0.38 Track T3 — extern fns get a C-ABI signature that
            // models returned-struct conventions (one-reg / two-reg /
            // sret). Non-extern fns keep the slice-8 Mighty-internal
            // shape where aggregate returns ride a single i64 (matches
            // the caller's aggregate-local stack-slot shape).
            //
            // v0.47 T1 — also propagate per-param `mut` flags so
            // `mut Vec[U8]` slots expand into the (ptr, cap, len_ptr)
            // triple. Empty slice for non-extern callees (mut on
            // those is rejected at the type checker).
            let extern_mut_params: &[bool] = prog
                .extern_bindings
                .get(&f.id)
                .map(|b| b.mut_params.as_slice())
                .unwrap_or(&[]);
            let (sig, agg_kind) = if is_extern {
                let (s, k) = crate::abi::build_extern_signature_with_mut(
                    &self.triple,
                    &param_tys,
                    extern_mut_params,
                    &f.ret_ty,
                    &prog.adts,
                );
                (s, k)
            } else {
                (
                    build_signature(&self.triple, &param_tys, &f.ret_ty),
                    crate::abi::AggregateReturnKind::None,
                )
            };
            let linkage = if is_extern {
                Linkage::Import
            } else if f.name == "main" {
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
            // Track extern fns so `define_fn` can skip them; the linker
            // owns the body.
            if is_extern {
                self.extern_fn_ids.insert(f.id);
                if !matches!(agg_kind, crate::abi::AggregateReturnKind::None) {
                    self.extern_return_kinds.insert(f.id, agg_kind);
                }
            }
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
        // v0.36 Track T2 — extern fns get declared with `Linkage::Import`
        // and have no Mighty-side body. `cranelift_module` rejects any
        // attempt to `define_function` against an import; skipping the
        // lowering here is what makes `extern c { ... }` actually link
        // against a vendored archive.
        if self.extern_fn_ids.contains(&f.id) {
            return Ok(());
        }
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
    /// v0.39 T3: destination local's SIR type while lowering an Rvalue.
    /// `lower_assign` sets this before calling `eval_rvalue` so the
    /// `Vec.new()` builtin can read the element type out of the
    /// destination's `IrTy::Adt(Vec, [T])` and seed the typed-slot
    /// header. None outside an assign-rhs context.
    pub current_dest_ty: Option<IrTy>,
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
            current_dest_ty: None,
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
        // v0.41 T3 (L1 fix): push an implicit arena frame at `main`
        // entry — see the rationale below. We thread the push INTO
        // the entry block's lowering instead of doing it before the
        // dispatch loop, because cranelift's debug-mode
        // `switch_to_block` asserts that the previous block was
        // filled (terminated) before re-switching.
        let block_ids: Vec<_> = self.f.blocks.iter().map(|b| b.id).collect();
        let entry_id = self.f.entry;
        let is_main = self.f.name == "main";
        for id in block_ids {
            let cl_blk = self.blocks[&id];
            self.b.switch_to_block(cl_blk);
            if is_main && id == entry_id {
                // The runtime's `mty_runtime_alloc` returns 0 (null)
                // when no arena frame is active, so any source-level
                // use of `Vec.new()` / `String.with_capacity()` /
                // `format!(...)` from a plain `fn main()` (with no
                // surrounding `arena {}` block) would dereference null
                // and segfault under native codegen. Examples
                // 26/30/42/43 all hit this. The interpreter's
                // allocation path doesn't require an explicit arena,
                // so the bug is native-only. We auto-push at
                // main-entry (not all fns — SIR's explicit
                // `ArenaPush`/`ArenaPop` already nests around `arena
                // {}` blocks). The frame is implicitly torn down at
                // process exit, matching the JIT's lifetime.
                self.call_rt_no_args("mty_runtime_arena_push", Some(ct::I64))?;
            }
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
            Stmt::Nop | Stmt::StorageLive(_) | Stmt::StorageDead(_) => Ok(()),
            // v0.47 T4 — `Stmt::Drop(local)` for a local whose type is
            // an ADT registered in `Program::adt_drop_fns` lowers to a
            // call to the registered runtime symbol with the local's
            // scalar value (the resource handle) as the sole arg. The
            // runtime contract per `DefMap::mty_drop_fns` REQUIRES the
            // symbol to no-op on handle=0, which is how explicit
            // `.close()` + auto-Drop idempotence works (the explicit
            // close zeroes the receiver Variable; the auto-Drop loads
            // 0 and the runtime symbol does nothing). Locals that
            // aren't in the table fall through to the v0.46 no-op
            // shape — same MIR-conceptual-drop semantics as the
            // backends had since slice 6.
            Stmt::Drop(local) => {
                let lty = self.f.locals[local.0 as usize].ty.clone();
                if let IrTy::Adt(adt_id, _) = &lty {
                    if let Some(sym) = self.prog.adt_drop_fns.get(adt_id).cloned() {
                        // Resolve the runtime symbol to a static
                        // `&'static str` matching the FuncId table key.
                        // `runtime_ids` is keyed by &'static str, so we
                        // match against the canonical names.
                        let static_name: Option<&'static str> = match sym.as_str() {
                            "mty_runtime_fs_dir_close" => Some("mty_runtime_fs_dir_close"),
                            _ => None,
                        };
                        if let Some(name) = static_name {
                            let var = self.ensure_var(*local);
                            let handle = self.b.use_var(var);
                            let handle = self.coerce_to(handle, ct::I64);
                            self.call_rt(name, &[handle], None)?;
                            // Zero the Variable so a second Drop on
                            // the same local (defensive, e.g. multiple
                            // exit terminators in a single fn) stays a
                            // no-op too. Cheap and pins idempotence.
                            let zero = self.b.ins().iconst(ct::I64, 0);
                            self.b.def_var(var, zero);
                        }
                    }
                }
                Ok(())
            }
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
            Stmt::EffectInvoke { op, args, out, .. } => {
                // Slice-8 stub: route through extern_call with the
                // method name. The runtime stub returns 0 — the
                // compiled program continues with a zero-default value.
                let (path, method) = match op {
                    mty_ir::ir::EffectOp::GenericCall { path, method } => (path, method),
                };
                let full_name = effect_full_name(path, method);
                // v0.45 T1 — native std.fs.* dispatch. Lowers each
                // call to its dedicated runtime ABI symbol; supersedes
                // the v0.44 interpreter-hosted fallback for these
                // method names. See `emit_fs_call` for the per-method
                // arg shape.
                if is_native_fs_method(&full_name) {
                    let args = args.clone();
                    let out = out.clone();
                    return self.emit_fs_call(&full_name, &args, out.as_ref());
                }
                // v0.49 — native std.crypto / std.encoding. Each call
                // takes `(ptr,len)` input(s) and writes a `(ptr,len)`
                // result aggregate (Bytes for digests, String for
                // encoders) into a fresh 16-byte slot. Without this the
                // call hit the `mty_runtime_extern_call` stub that
                // returns 0, and a downstream `hex.encode(0)` dereffed
                // null → SIGSEGV (examples 42/43).
                if is_native_crypto_encoding(&full_name) {
                    let args = args.clone();
                    let out = out.clone();
                    let result = self.emit_crypto_encoding_call(&full_name, &args)?;
                    if let Some(p) = out {
                        if !p.proj.is_empty() {
                            let (addr, ty) = self.place_addr(&p)?;
                            self.store_scalar(addr, result, &ty)?;
                        } else {
                            let var = self.ensure_var(p.local);
                            self.b.def_var(var, result);
                        }
                    }
                    return Ok(());
                }
                if is_interpreter_hosted_stdlib(&full_name) {
                    return Err(CodegenError::Unsupported(format!(
                        "{full_name} is interpreter-hosted"
                    )));
                }
                let method = method.clone();
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
            // v0.39 T3: scoped dest_ty so a Vec.new()-into-projection
            // path still picks up the right element type.
            let prev = self.current_dest_ty.take();
            self.current_dest_ty = Some(ty.clone());
            let v = self.eval_rvalue(rv)?;
            self.current_dest_ty = prev;
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
                    // v0.45 T5 (L28 debug=0 fix): opaque ADTs (Vec /
                    // Page / IoErr / … — registered by the prelude with
                    // no constructable variants) are passed around as
                    // i64 pointers to a runtime-allocated header. The
                    // layout system reports them as `Layout::scalar(8)`
                    // because the body is invisible to the front-end;
                    // memcpy-ing 8 bytes from `src` into a fresh slot
                    // would truncate the actual 32-byte Vec header
                    // (dropping `cap`, `data`, `elem_size`) and then
                    // hand back a dangling stack pointer once the
                    // local escapes the frame. Re-bind the Variable to
                    // the source pointer value instead — that mirrors
                    // what the LLVM backend already does (Adt → ptr →
                    // load/store the pointer word) and what the
                    // direct-`Vec.new()` floor case relies on. Pre-fix
                    // this corruption "worked" under `[profile.dev]
                    // opt-level=0 + debug=2` because Cranelift's
                    // unoptimised register allocator parked the slot
                    // address in a frame slot the OOB stores happened
                    // to leave intact long enough for `g.len()` to
                    // read it back; any other profile (debug=0,
                    // opt-level≥1) lost the bytes and SEGV'd.
                    if is_opaque_adt(&local_ty, &self.prog.adts)
                        || is_opaque_adt(&src_ty, &self.prog.adts)
                    {
                        let var = self.ensure_var(place.local);
                        let src_val = self.eval_operand(&Operand::Copy(src.clone()))?;
                        let val = self.coerce_to(src_val, ct::I64);
                        self.b.def_var(var, val);
                        return Ok(());
                    }
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
        // v0.39 T3: thread the destination local's SIR type into
        // eval_rvalue so `Vec.new()` can pick the element type out of
        // `Vec[T]` and seed the typed-slot header (`elem_size@24`).
        let prev = self.current_dest_ty.take();
        self.current_dest_ty = Some(local_ty.clone());
        let val = self.eval_rvalue(rv)?;
        self.current_dest_ty = prev;
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

    /// v0.42 T2 — value-preserving `as Ty` lowering used by
    /// `Rvalue::Cast`. Picks the right Cranelift conversion instruction
    /// for each combination of source / destination kinds rather than
    /// the bit-preserving fallback in [`Self::coerce_to`]:
    ///
    /// | src         | dst       | instructions                           |
    /// |-------------|-----------|----------------------------------------|
    /// | int (s)     | wider int | `sextend`                              |
    /// | int (u)     | wider int | `uextend`                              |
    /// | int         | smaller int | `ireduce`                            |
    /// | int (s)     | float     | `fcvt_from_sint`                       |
    /// | int (u)     | float     | `fcvt_from_uint`                       |
    /// | float       | int (s)   | `fcvt_to_sint_sat` (NaN→0, ±inf→min/max) |
    /// | float       | int (u)   | `fcvt_to_uint_sat` (NaN→0, +inf→max, -inf→0) |
    /// | float       | wider fp  | `fpromote`                             |
    /// | float       | smaller fp | `fdemote`                             |
    ///
    /// Bool↔Int and `Ref↔Ref` are handled at the caller; for unknown
    /// source types (Use rvalues where `operand_ir_ty` returned None) we
    /// fall back to `coerce_to_with_src` which itself falls back to the
    /// bit-preserving coerce — same conservative behaviour as before
    /// v0.42 T2.
    fn cast_value(
        &mut self,
        v: cranelift_codegen::ir::Value,
        want: cranelift_codegen::ir::Type,
        src_ty: Option<&IrTy>,
        dst_ty: &IrTy,
    ) -> cranelift_codegen::ir::Value {
        let have = self.b.func.dfg.value_type(v);
        if have == want
            && matches!(
                (src_ty, dst_ty),
                (Some(IrTy::Int(_)), IrTy::Int(_)) | (Some(IrTy::Float(_)), IrTy::Float(_))
            )
        {
            // Same Cranelift width but a kind/sign change (e.g. I32→U32).
            // The bits are identical; no instruction needed.
            return v;
        }
        let unsigned_src = src_ty.is_some_and(Self::is_unsigned_int_ty);
        match (src_ty, dst_ty) {
            // ── Int → Float ──────────────────────────────────────────
            (Some(IrTy::Int(_)), IrTy::Float(_)) if want.is_float() && have.is_int() => {
                // Cranelift's fcvt_from_* only accepts I32/I64 inputs;
                // narrow ints (I8/I16) must be widened first via
                // sextend / uextend.
                let widened = if have.bits() < 32 {
                    let i32t = cranelift_codegen::ir::types::I32;
                    if unsigned_src {
                        self.b.ins().uextend(i32t, v)
                    } else {
                        self.b.ins().sextend(i32t, v)
                    }
                } else {
                    v
                };
                if unsigned_src {
                    self.b.ins().fcvt_from_uint(want, widened)
                } else {
                    self.b.ins().fcvt_from_sint(want, widened)
                }
            }
            // ── Float → Int ──────────────────────────────────────────
            (Some(IrTy::Float(_)), IrTy::Int(_)) if want.is_int() && have.is_float() => {
                // fcvt_to_*_sat targets I32 / I64 directly; narrow dst
                // (I8/I16) needs an extra ireduce afterwards.
                let dst_unsigned = Self::is_unsigned_int_ty(dst_ty);
                let wide_int = if want.bits() < 32 {
                    cranelift_codegen::ir::types::I32
                } else {
                    want
                };
                let wide = if dst_unsigned {
                    self.b.ins().fcvt_to_uint_sat(wide_int, v)
                } else {
                    self.b.ins().fcvt_to_sint_sat(wide_int, v)
                };
                if wide_int == want {
                    wide
                } else {
                    self.b.ins().ireduce(want, wide)
                }
            }
            // ── Float → Float ────────────────────────────────────────
            (Some(IrTy::Float(_)), IrTy::Float(_)) if want.is_float() && have.is_float() => {
                if have.bits() < want.bits() {
                    self.b.ins().fpromote(want, v)
                } else if have.bits() > want.bits() {
                    self.b.ins().fdemote(want, v)
                } else {
                    v
                }
            }
            // ── everything else — delegate to coerce_to_with_src,
            // which handles Int↔Int (sextend / uextend / ireduce) and
            // the bit-preserving fallback for unknown SIR types.
            _ => self.coerce_to_with_src(v, want, src_ty),
        }
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
                // NB v0.42 T4 (L23 fix): Str + Str concat now arrives
                // here as a `Rvalue::Call { func:
                // BuiltinId::Extern("__mty_str_concat"), ... }`
                // pre-routed by the SIR lowerer (see
                // `lower_binop` in mty-ir::lower::exprs). So this
                // arm only sees true numeric/bool binops.
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
                // v0.39 T2 — Int → Bool requires a "nonzero" comparison,
                // not a width-narrowing truncate. Without this branch
                // `256_i32 as Bool` would `ireduce` to the low byte (0)
                // and silently produce `false`, contradicting the
                // documented semantics in docs/reference/casts.md
                // §"Bool ↔ Int". Bool is stored as I8, so we compare
                // against zero at the source width, then `bint` to I8.
                if matches!(ty, IrTy::Bool)
                    && src_ty.as_ref().is_some_and(|t| matches!(t, IrTy::Int(_)))
                {
                    let src_cl_ty = self.b.func.dfg.value_type(v);
                    let zero = self.b.ins().iconst(src_cl_ty, 0);
                    let cmp = self.b.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                        v,
                        zero,
                    );
                    // `icmp` returns I8 already in Cranelift's typed-IR
                    // mode, which is exactly the storage Mighty uses for
                    // `Bool` — no further coerce needed.
                    return Ok(self.coerce_to_with_src(cmp, want, Some(&IrTy::Bool)));
                }
                // v0.42 T2 — full numeric `as` matrix. Int↔Int already
                // works through `coerce_to_with_src` (sextend / uextend
                // / ireduce); int↔float and float↔float used to fall
                // into `coerce_to`'s bitcast path, which preserves bits
                // but NOT value. Route those through proper Cranelift
                // conversion instructions:
                //   * Int → Float  : fcvt_from_sint / fcvt_from_uint
                //   * Float → Int  : fcvt_to_sint_sat / fcvt_to_uint_sat
                //                    (saturating — NaN → 0, ±inf → min/max;
                //                    matches docs/reference/casts.md §"Float
                //                    → Int" and Rust's `as` semantics).
                //   * Float → Float: fpromote / fdemote (already in
                //                    coerce_to; routed through cast_value
                //                    for symmetry).
                Ok(self.cast_value(v, want, src_ty.as_ref(), ty))
            }
            Rvalue::StrPtr(src) => {
                // v0.37 Track T3 — read the ptr half (offset 0) of the
                // Mighty Str aggregate. The Str's backing bytes are
                // null-terminated by `intern_string`, so the returned
                // `*U8` is directly usable as a `const char *` in C.
                // Fast paths for literals (skip the stack-slot round
                // trip) and dynamic Str locals (load from the (ptr,len)
                // slot) live in `string_pair`; we only want the first
                // half here.
                let (ptr, _len) = self.string_pair(src)?;
                Ok(ptr)
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
                // `v[i]` element read. Two shapes reach here:
                //   1. A native growable `Vec` (header pointer; element
                //      storage is behind the `data` field). The local is
                //      non-aggregate (`{error}` / opaque Vec), so the
                //      Variable holds the i64 header pointer.
                //   2. A fixed-size `[T; N]` array (aggregate local); the
                //      Variable holds the slot *address* which is the
                //      element storage directly.
                // We pick the base accordingly: for the Vec shape we must
                // first load `data` from `header+16`; for arrays the base
                // is the address itself. v0.39 T3: typed-slot — pick the
                // element width from `Vec[T]` / array element type so a
                // Vec[U8] index read returns 1 byte (zero-extended), not
                // 8 bytes off-the-end of the data buffer.
                let recv_ty = self.f.locals[receiver.local.0 as usize].ty.clone();
                let base_raw = {
                    let var = self.ensure_var(receiver.local);
                    self.b.use_var(var)
                };
                let base_ptr = self.coerce_to(base_raw, ct::I64);
                let mf = cranelift_codegen::ir::MemFlags::trusted();
                let (data, elem_ty) = if is_aggregate(&recv_ty) {
                    // Fixed array: storage is at the slot address.
                    let et = match &recv_ty {
                        IrTy::Array { elem, .. } => Some((**elem).clone()),
                        _ => None,
                    };
                    (base_ptr, et)
                } else {
                    // Native Vec: load the data pointer from the header.
                    let d = self.b.ins().load(ct::I64, mf, base_ptr, Self::VEC_DATA_OFF);
                    (d, self.vec_elem_ty_from(&recv_ty))
                };
                let elem_size = elem_ty
                    .as_ref()
                    .map(|t| self.vec_elem_size_for(t))
                    .unwrap_or(Self::VEC_FALLBACK_ELEM_SIZE);
                let lds = elem_ty.as_ref().and_then(|t| self.vec_elem_ld_st(t));
                let idx = self.eval_operand(index)?;
                let idx_i64 = self.coerce_to(idx, ct::I64);
                let off = self.b.ins().imul_imm(idx_i64, elem_size);
                let addr = self.b.ins().iadd(data, off);
                Ok(self.vec_load_elem(addr, elem_size, lds))
            }
            Rvalue::MethodCall {
                receiver,
                method,
                args,
            } => {
                // v0.38 (L28 fix) — native growable `Vec` ops. The
                // receiver evaluates to the i64 header pointer produced
                // by `emit_vec_new`; `push` mutates it in place and
                // returns the *same* pointer, so the `v = v.push(x)`
                // capture-rebind threads a stable value across the loop
                // back-edge (the bug was that every Vec op stubbed to 0).
                //
                // v0.41 T3 (L1 fix): only dispatch to the Vec lowerings
                // when the receiver is actually a `Vec[T]` — pre-fix we
                // also caught `String.clear()` / `String.push_str(...)`
                // and read the String's (ptr,len) pair as a Vec header,
                // dereferencing junk → infinite loop / segfault on the
                // very next allocation pass (caught by example 26's
                // `_scratchpad` helper).
                let recv_is_vec = matches!(
                    self.operand_ir_ty(receiver),
                    Some(IrTy::Adt(id, _)) if self.prog.adt_by_id(id).map(|a| a.name.as_str()) == Some("Vec")
                );
                if recv_is_vec {
                    match method.as_str() {
                        "push" => return self.emit_vec_push(receiver, args),
                        "len" => return self.emit_vec_len(receiver),
                        "get" => return self.emit_vec_get(receiver, args),
                        "set" => return self.emit_vec_set(receiver, args),
                        "pop" => return self.emit_vec_pop(receiver),
                        "clear" => return self.emit_vec_clear(receiver),
                        _ => {}
                    }
                }
                // v0.42 T4 (L23 fix) — `to_str()` / `to_string()` on
                // scalar receivers (integers, floats, bools, char).
                // Allocates a 16-byte (ptr,len) slot, calls the
                // appropriate `mty_runtime_fmt_*` to fill it, returns
                // the slot address — same aggregate shape any other
                // Str rvalue uses.
                if matches!(method.as_str(), "to_str" | "to_string") {
                    if let Some(addr) = self.try_emit_scalar_to_str(receiver)? {
                        return Ok(addr);
                    }
                }
                // v0.46 T4 — DirIter iterator methods. The receiver is
                // the i64 handle returned by `mty_runtime_fs_dir_open`
                // (see `FsAbiKind::DirOpenHandle`). `.next()` writes a
                // (ptr,len,ok) Str triple into a 24-byte slot and
                // wraps it as `Option<String>` (None on EOF). `.close()`
                // releases the handle — Drop on the Mighty side calls
                // this. Both are dispatched here, before the generic
                // `next` -> defensive-None fallback below, so the real
                // iterator advance lands on the runtime symbol instead
                // of getting stubbed to None forever (which would loop
                // any `while let Some(_) = it.next()` consumer).
                let recv_is_dir_iter = matches!(
                    self.operand_ir_ty(receiver),
                    Some(IrTy::Adt(id, _)) if self.prog.adt_by_id(id).map(|a| a.name.as_str()) == Some("DirIter")
                );
                if recv_is_dir_iter {
                    match method.as_str() {
                        "next" => return self.emit_dir_iter_next(receiver),
                        "close" => return self.emit_dir_iter_close(receiver),
                        _ => {}
                    }
                }
                // v0.41 T3 (L1 fix): when the destination type is an
                // `Option[T]` aggregate (e.g. `Stream.next()`, opaque
                // methods returning `Option`), synthesise a defensive
                // `None` aggregate rather than handing back a raw
                // scalar. The interpreter returns `None` for opaque
                // receivers (`mty-ir::interp::run::eval_method` "next"
                // arm). Pre-fix the caller would dereference the
                // scalar as an Option address and segfault (examples
                // 30/42/43).
                //
                // Detection is two-pronged: (a) current_dest_ty IS the
                // `Option[T]` ADT (rare — the lowerer types the temp
                // as `IrTy::Error`); (b) the method name is one of
                // the known Option-returning shapes (`next`, plus
                // others added as the corpus grows). Without (b),
                // example 30's `stream.next()` falls through with a
                // scalar 0 and the consuming `switch_variant` reads
                // tag from address 0 → segfault.
                let dest_is_option = matches!(
                    self.current_dest_ty.as_ref(),
                    Some(IrTy::Adt(id, _)) if self
                        .prog
                        .adt_by_id(*id)
                        .map(|a| a.name.as_str())
                        == Some("Option")
                );
                let method_returns_option =
                    matches!(method.as_str(), "next" | "peek" | "front" | "back");
                if dest_is_option || method_returns_option {
                    let (slot_addr, _payload_off) = self.alloc_option_slot_for(None)?;
                    let tag_none = self.b.ins().iconst(ct::I32, 1);
                    self.b
                        .ins()
                        .store(MemFlags::trusted(), tag_none, slot_addr, TAG_OFFSET as i32);
                    return Ok(slot_addr);
                }
                // Fallback: route through the extern bridge as a last
                // resort. Real trait-method dispatch needs resolution
                // from the typechecker (still a follow-up).
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
            Const::FnPtr(fref) => {
                // v0.38 Track T3 — fn-pointer surface for FFI (row 11
                // of the extern_c matrix). A `FnPtr(FnRef::User(fid))`
                // operand is materialised by taking the address of the
                // declared function via cranelift's `func_addr`, which
                // emits a `iconst-or-symbol` reference the linker
                // resolves at final-link time. The resulting i64 fits
                // the C-side function-pointer slot directly.
                //
                // Builtins/runtime fns aren't addressable in the same
                // way (they don't always have a stable symbol — `log`
                // routes through `mty_runtime_log` indirectly), so we
                // reject them here with a clean diagnostic. Real FFI
                // callbacks should be plain Mighty fns.
                match fref {
                    mty_ir::ir::FnRef::User(fid) => {
                        let func_id = *self.mod_ctx.fn_ids.get(fid).ok_or_else(|| {
                            CodegenError::Module(format!("fn-ptr to undeclared fn {:?}", fid))
                        })?;
                        let func_ref = self
                            .mod_ctx
                            .module
                            .declare_func_in_func(func_id, self.b.func);
                        self.b.ins().func_addr(ct::I64, func_ref)
                    }
                    mty_ir::ir::FnRef::Builtin(_) => {
                        return Err(CodegenError::Unsupported(
                            "fn-pointer of a builtin (use a plain Mighty fn for FFI callbacks)"
                                .into(),
                        ));
                    }
                }
            }
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
                // v0.42 T4 (L23 fix) — typed log/print lowering.
                //
                // Pre-fix the cranelift backend only accepted a single
                // Str operand for `log(...)`. Now we dispatch on each
                // arg's SIR type: Str → ptr/len (existing); integer /
                // float / bool → call a typed runtime variant so the
                // built binary can trace any computed value without an
                // FFI shim.
                //
                // Multi-arg policy: for `log(a, b, c)` we emit a
                // sequence of `print_*` calls separated by single
                // spaces, terminated with a newline (so `log` still
                // ends a line). The "is this log or print" choice
                // controls only whether the terminating newline is
                // emitted.
                let is_log = matches!(func, FnRef::Builtin(BuiltinId::Log));
                // Drop any Unit-typed sentinel args (the lowering can
                // synthesize one for empty-call shapes).
                let visible: Vec<&Operand> = args
                    .iter()
                    .filter(|a| !matches!(a, Operand::Const(Const::Unit)))
                    .collect();
                if visible.is_empty() {
                    // `log()` with no args — just emit the newline (if
                    // log) so existing programs that only used `log` for
                    // milestones don't regress.
                    if is_log {
                        self.call_rt("mty_runtime_print_newline", &[], None)?;
                    }
                    return Ok(self.b.ins().iconst(ct::I64, 0));
                }
                let single = visible.len() == 1;
                for (i, op) in visible.iter().enumerate() {
                    if i > 0 {
                        // Separator between adjacent args.
                        self.call_rt("mty_runtime_print_sep", &[], None)?;
                    }
                    // Pick the right runtime symbol from the operand's
                    // SIR type.
                    let op_ty = self.operand_ir_ty(op);
                    self.emit_one_log_arg(op, op_ty.as_ref(), single, is_log)?;
                }
                if !single && is_log {
                    // Multi-arg log: we used `print_*` per element, so
                    // close with the terminating newline.
                    self.call_rt("mty_runtime_print_newline", &[], None)?;
                }
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
            FnRef::Builtin(BuiltinId::Extern(name)) if name == "__mty_str_concat" => {
                // v0.42 T4 (L23 fix) — synthetic builtin emitted by
                // the IR lowering for `Str + Str`. Routes to
                // `mty_runtime_str_concat`; returns the (ptr,len)
                // slot address as the result aggregate.
                if args.len() != 2 {
                    return Err(CodegenError::Unsupported(
                        "__mty_str_concat arity != 2".into(),
                    ));
                }
                self.emit_str_concat(&args[0], &args[1])
            }
            // #297 — native String constructors. `String` is a 16-byte
            // (ptr@+0, len@+8) aggregate shared with `Str`, so:
            //   * `String.from_str(s)` is identity — materialise the
            //     argument's (ptr,len) into a fresh String slot.
            //   * `String.new()` / `String.with_capacity(n)` produce an
            //     empty String (ptr=0, len=0); the capacity hint is a
            //     no-op in the JIT (growth reallocates on demand).
            // Without these, the path-call resolved to an unhandled
            // `Extern("String.…")` symbol and yielded garbage (a
            // `Vec[String]` built from `from_str` then SIGSEGV'd, and
            // `String.from_str(x).len()` read 0).
            FnRef::Builtin(BuiltinId::Extern(name)) if name == "String.from_str" => {
                let arg = args
                    .first()
                    .ok_or_else(|| CodegenError::Unsupported("String.from_str arity 0".into()))?;
                let (ptr, len) = self.string_pair(arg)?;
                Ok(self.emit_string_slot(ptr, len))
            }
            FnRef::Builtin(BuiltinId::Extern(name))
                if name == "String.new" || name == "String.with_capacity" =>
            {
                let zero = self.b.ins().iconst(ct::I64, 0);
                Ok(self.emit_string_slot(zero, zero))
            }
            FnRef::User(callee_id) => {
                let func_id = *self.mod_ctx.fn_ids.get(callee_id).ok_or_else(|| {
                    CodegenError::Module(format!("call to undeclared fn {:?}", callee_id))
                })?;
                // v0.37 T6 / v0.38 T2 — variadic extern C fn (e.g.
                // `printf`). The declared signature only has the fixed
                // prefix; v0.38 wires the trailing `...` args by
                // building a *per-call* `ir::Signature`, importing it
                // via `Function::import_signature`, taking the imported
                // symbol's address with `func_addr`, and dispatching
                // through `call_indirect`. Extra args go through C ABI
                // default argument promotion (`abi::cl_ty_for_variadic`).
                let is_variadic_callee = self
                    .prog
                    .extern_bindings
                    .get(callee_id)
                    .map(|b| b.is_variadic)
                    .unwrap_or(false);
                // v0.46 T3 (L52 fix) — extern-c callees expand Mighty
                // Str/String params into (ptr, len) pairs at the ABI
                // boundary. See `abi::build_extern_signature`. We track
                // this here so the fixed-arg lowering loop below can
                // push two i64 values for each Str/String slot.
                let is_extern_c_callee = self
                    .prog
                    .extern_bindings
                    .get(callee_id)
                    .map(|b| b.abi == "c")
                    .unwrap_or(false);
                // v0.47 T1 — per-param `mut` flags for the callee.
                // A `mut Vec[U8]` slot expands into a (ptr, cap,
                // len_ptr) triple at the call site (three i64 ABI
                // slots). Empty list = no mut anywhere, which is the
                // legacy v0.46-and-earlier shape.
                let callee_mut_params: Vec<bool> = self
                    .prog
                    .extern_bindings
                    .get(callee_id)
                    .map(|b| b.mut_params.clone())
                    .unwrap_or_default();
                // v0.38 Track T3 — returned-struct classification for the
                // call site. Drives slot allocation, sret arg insertion,
                // and per-register result store. Non-aggregate callees
                // fall through to the legacy single-result path.
                let agg_kind = self
                    .mod_ctx
                    .extern_return_kinds
                    .get(callee_id)
                    .copied()
                    .unwrap_or(crate::abi::AggregateReturnKind::None);
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
                let callee_param_tys: Vec<IrTy> = callee
                    .params
                    .iter()
                    .map(|p| callee.locals[p.0 as usize].ty.clone())
                    .filter(|t| !matches!(t, IrTy::Unit | IrTy::Never))
                    .collect();
                let callee_ret_ty = callee.ret_ty.clone();
                let expected = callee_param_tys.len();

                // Partition the caller's operand list into "fixed" and
                // "extra" non-unit slots so the variadic path can build
                // the right per-call signature without re-walking.
                #[derive(Clone)]
                struct VisibleArg<'q> {
                    op: &'q Operand,
                }
                let visible: Vec<VisibleArg> = args
                    .iter()
                    .filter(|a| {
                        if matches!(a, Operand::Const(Const::Unit)) {
                            return false;
                        }
                        if let Operand::Copy(p) | Operand::Move(p) = a {
                            if p.proj.is_empty()
                                && matches!(
                                    self.f.locals[p.local.0 as usize].ty,
                                    IrTy::Unit | IrTy::Never
                                )
                            {
                                return false;
                            }
                        }
                        true
                    })
                    .map(|op| VisibleArg { op })
                    .collect();

                let fixed_count = expected.min(visible.len());
                let extras = &visible[fixed_count..];

                // Lower the fixed prefix. This is the same logic as the
                // pre-v0.38 path: coerce each operand to the declared
                // param type, padding with zeros if the caller passed
                // too few visible args.
                let mut callee_param_tys_mut = callee_param_tys.clone();
                let mut arg_vals: Vec<cranelift_codegen::ir::Value> =
                    Vec::with_capacity(visible.len() + 1);
                // v0.38 Track T3 — allocate the return-value slot if the
                // callee uses an aggregate-return convention. For sret
                // we also prepend the slot address as the hidden first
                // arg (matches the SysV / Windows-x64 layout).
                let ret_slot_addr: Option<cranelift_codegen::ir::Value> = if agg_kind.needs_slot() {
                    let size = match agg_kind {
                        crate::abi::AggregateReturnKind::OneReg { size }
                        | crate::abi::AggregateReturnKind::TwoReg { size }
                        | crate::abi::AggregateReturnKind::Sret { size } => size,
                        crate::abi::AggregateReturnKind::None => 0,
                    };
                    let align = type_align(&callee.ret_ty, &self.prog.adts).max(8);
                    let log2_align = (align.next_power_of_two().trailing_zeros()).min(16) as u8;
                    let slot = self.b.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        slot_size(size.max(1)),
                        log2_align,
                    ));
                    let addr = self.b.ins().stack_addr(ct::I64, slot, 0);
                    if matches!(agg_kind, crate::abi::AggregateReturnKind::Sret { .. }) {
                        // sret arg goes first in the actual call.
                        arg_vals.push(addr);
                    }
                    Some(addr)
                } else {
                    None
                };
                // v0.47 T1 — track the callee's *original* param index
                // (matches `callee_mut_params`) as we walk
                // `callee_param_tys_mut`. We pop from the front of the
                // Vec, so the index is just the running count.
                let mut callee_param_idx: usize = 0;
                for va in &visible[..fixed_count] {
                    let want_ty = callee_param_tys_mut.remove(0);
                    let is_mut_slot = callee_mut_params
                        .get(callee_param_idx)
                        .copied()
                        .unwrap_or(false);
                    callee_param_idx += 1;
                    // v0.47 T1 — `mut Vec[U8]` expands to a 3-i64
                    // triple at the ABI boundary: (out_ptr,
                    // out_capacity, out_len_ptr). The Mighty Vec
                    // header lives at offset 0 of the operand's i64
                    // value; the runtime layout (see
                    // `VEC_LEN_OFF`/`_CAP_OFF`/`_DATA_OFF` in
                    // `lower.rs`) makes the per-field loads cheap.
                    // The `out_len_ptr` slot is just `header_ptr`
                    // itself because `VEC_LEN_OFF == 0` — the C
                    // callee writes the byte count straight into the
                    // first 8 bytes of the header.
                    if is_extern_c_callee
                        && is_mut_slot
                        && crate::abi::is_mut_vec_u8_param(&want_ty, &self.prog.adts)
                    {
                        let hdr = self.vec_header(va.op)?;
                        let mf = cranelift_codegen::ir::MemFlags::trusted();
                        let data_ptr = self.b.ins().load(ct::I64, mf, hdr, Self::VEC_DATA_OFF);
                        let cap = self.b.ins().load(ct::I64, mf, hdr, Self::VEC_CAP_OFF);
                        // header_ptr+0 is the len field (VEC_LEN_OFF
                        // == 0), so we hand the header pointer
                        // verbatim as the `out_len` slot. C side
                        // writes `*out_len = N`; Mighty reads
                        // `Vec.len()` and sees N.
                        arg_vals.push(data_ptr);
                        arg_vals.push(cap);
                        arg_vals.push(hdr);
                        continue;
                    }
                    // v0.46 T3 — Str/String at an extern-c param slot
                    // expands to (ptr, len). Pull both halves from the
                    // Str aggregate via `string_pair` and push them in
                    // the same order the extern signature expects (see
                    // `abi::build_extern_signature`).
                    if is_extern_c_callee && crate::abi::is_str_slice_param(&want_ty) {
                        let (ptr, len) = self.string_pair(va.op)?;
                        arg_vals.push(ptr);
                        arg_vals.push(len);
                        continue;
                    }
                    let v = self.eval_operand(va.op)?;
                    let src_ty = self.operand_ir_ty(va.op);
                    let want = if is_aggregate(&want_ty) {
                        ct::I64
                    } else {
                        cl_ty_for(&want_ty)
                    };
                    let coerced = self.coerce_to_with_src(v, want, src_ty.as_ref());
                    arg_vals.push(coerced);
                }
                while !callee_param_tys_mut.is_empty() {
                    let t = callee_param_tys_mut.remove(0);
                    let is_mut_slot = callee_mut_params
                        .get(callee_param_idx)
                        .copied()
                        .unwrap_or(false);
                    callee_param_idx += 1;
                    // v0.47 T1 — pad an unfilled mut Vec[U8] slot
                    // with three zero i64s (ptr=NULL, cap=0,
                    // len_ptr=NULL). Matches the three-slot
                    // expansion so cranelift doesn't see an arity
                    // mismatch.
                    if is_extern_c_callee
                        && is_mut_slot
                        && crate::abi::is_mut_vec_u8_param(&t, &self.prog.adts)
                    {
                        let z = self.b.ins().iconst(ct::I64, 0);
                        arg_vals.push(z);
                        let z2 = self.b.ins().iconst(ct::I64, 0);
                        arg_vals.push(z2);
                        let z3 = self.b.ins().iconst(ct::I64, 0);
                        arg_vals.push(z3);
                        continue;
                    }
                    // v0.46 T3 — pad an unfilled Str/String slot at an
                    // extern-c callee with two zero i64s (ptr=NULL,
                    // len=0). Matches the signature's two-slot
                    // expansion so cranelift doesn't see an arity
                    // mismatch.
                    if is_extern_c_callee && crate::abi::is_str_slice_param(&t) {
                        let z = self.b.ins().iconst(ct::I64, 0);
                        arg_vals.push(z);
                        let z2 = self.b.ins().iconst(ct::I64, 0);
                        arg_vals.push(z2);
                        continue;
                    }
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

                if !extras.is_empty() {
                    // Variadic call site with non-empty extras. If the
                    // callee isn't actually variadic, surface a clean
                    // Unsupported — typeck should have caught this, but
                    // the codegen contract is "Unsupported, not crash".
                    if !is_variadic_callee {
                        return Err(CodegenError::Unsupported(format!(
                            "non-variadic call to `{}` with {} extra args",
                            callee.name,
                            extras.len()
                        )));
                    }

                    // Lower each extra under the C ABI variadic
                    // promotion rules.
                    let mut extra_cl_tys: Vec<cranelift_codegen::ir::Type> =
                        Vec::with_capacity(extras.len());
                    for va in extras {
                        let src_ty = self.operand_ir_ty(va.op);
                        let (want_cl, unsigned_hint) = match &src_ty {
                            Some(t) => cl_ty_for_variadic(t),
                            // No SIR type info — fall back to pointer-
                            // sized integer (treats opaque values as
                            // 64-bit blobs, which matches the existing
                            // is_aggregate→I64 default).
                            None => (ct::I64, true),
                        };
                        let v = self.eval_operand(va.op)?;
                        // Pick uextend / sextend correctly for the
                        // promotion. We synthesize an "IntKind hint"
                        // for the coerce path by passing the original
                        // src_ty when it was signed and a U64-shaped
                        // sentinel otherwise — the existing
                        // coerce_to_with_src already inspects the
                        // hint's unsigned-ness, so a clone of src_ty
                        // suffices. For floats we let coerce_to do
                        // fpromote.
                        let coerced = if want_cl.is_float() {
                            self.coerce_to(v, want_cl)
                        } else if unsigned_hint
                            && src_ty
                                .as_ref()
                                .is_none_or(|t| !FnLower::<M>::is_unsigned_int_ty(t))
                        {
                            // The C-ABI says unsigned promotion; force
                            // uextend even if the SIR type was
                            // ambiguous (e.g. char promoted to int).
                            let have = self.b.func.dfg.value_type(v);
                            if have.is_int() && want_cl.is_int() && have.bits() < want_cl.bits() {
                                self.b.ins().uextend(want_cl, v)
                            } else {
                                self.coerce_to(v, want_cl)
                            }
                        } else {
                            self.coerce_to_with_src(v, want_cl, src_ty.as_ref())
                        };
                        arg_vals.push(coerced);
                        extra_cl_tys.push(want_cl);
                    }

                    // Build a per-call signature: fixed prefix types
                    // from the declared extern + the promoted extras.
                    let mut sig = Signature::new(host_call_conv(&self.mod_ctx.triple));
                    if !matches!(callee_ret_ty, IrTy::Unit | IrTy::Never) {
                        sig.returns.push(AbiParam::new(cl_ty_for(&callee_ret_ty)));
                    }
                    for (i, t) in callee_param_tys.iter().enumerate() {
                        let is_mut_slot = callee_mut_params.get(i).copied().unwrap_or(false);
                        // v0.47 T1 — `mut Vec[U8]` expands to three
                        // i64 slots (ptr, cap, len_ptr). Mirror
                        // `build_extern_signature_with_mut` here.
                        if is_mut_slot && crate::abi::is_mut_vec_u8_param(t, &self.prog.adts) {
                            sig.params.push(AbiParam::new(ct::I64)); // ptr
                            sig.params.push(AbiParam::new(ct::I64)); // cap
                            sig.params.push(AbiParam::new(ct::I64)); // len_ptr
                            continue;
                        }
                        // v0.46 T3 — Str/String at an extern-c fixed
                        // param slot expands to (ptr, len). Mirror
                        // `build_extern_signature` here so the per-call
                        // variadic signature has the same shape.
                        if crate::abi::is_str_slice_param(t) {
                            sig.params.push(AbiParam::new(ct::I64));
                            sig.params.push(AbiParam::new(ct::I64));
                            continue;
                        }
                        let want = if is_aggregate(t) {
                            ct::I64
                        } else {
                            cl_ty_for(t)
                        };
                        sig.params.push(AbiParam::new(want));
                    }
                    for cl in &extra_cl_tys {
                        sig.params.push(AbiParam::new(*cl));
                    }
                    let sig_ref = self.b.func.import_signature(sig);

                    // Take the address of the linked symbol. The
                    // extern was declared with `Linkage::Import`, so
                    // the linker / JIT symbol-resolver fills it in.
                    let ptr_ty = ct::I64; // host is 64-bit (slice 8)
                    let callee_addr = self.b.ins().func_addr(ptr_ty, func_ref);

                    let call = self.b.ins().call_indirect(sig_ref, callee_addr, &arg_vals);
                    let results = self.b.inst_results(call).to_vec();
                    return Ok(results
                        .first()
                        .copied()
                        .unwrap_or_else(|| self.b.ins().iconst(ct::I64, 0)));
                }

                let call = self.b.ins().call(func_ref, &arg_vals);
                let results = self.b.inst_results(call).to_vec();
                // v0.38 Track T3 — fold returned-struct registers into
                // the caller-allocated slot. The caller of `lower_call`
                // expects an i64 holding the slot's address (matches
                // the aggregate-local shape from `agg_addr`).
                if let Some(slot_addr) = ret_slot_addr {
                    match agg_kind {
                        crate::abi::AggregateReturnKind::OneReg { .. } => {
                            if let Some(v) = results.first().copied() {
                                self.b.ins().store(
                                    cranelift_codegen::ir::MemFlags::trusted(),
                                    v,
                                    slot_addr,
                                    0,
                                );
                            }
                        }
                        crate::abi::AggregateReturnKind::TwoReg { .. } => {
                            if let Some(v0) = results.first().copied() {
                                self.b.ins().store(
                                    cranelift_codegen::ir::MemFlags::trusted(),
                                    v0,
                                    slot_addr,
                                    0,
                                );
                            }
                            if let Some(v1) = results.get(1).copied() {
                                self.b.ins().store(
                                    cranelift_codegen::ir::MemFlags::trusted(),
                                    v1,
                                    slot_addr,
                                    8,
                                );
                            }
                        }
                        crate::abi::AggregateReturnKind::Sret { .. } => {
                            // Callee wrote through the slot pointer we
                            // passed; nothing to fold from results.
                        }
                        crate::abi::AggregateReturnKind::None => unreachable!(),
                    }
                    return Ok(slot_addr);
                }
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
            FnRef::Builtin(BuiltinId::Extern(name))
                if name == "Vec.new" || name == "Vec.with_capacity" =>
            {
                // v0.38 (L28 fix) — `Vec.new()` / `Vec[T].new()` /
                // `Vec.with_capacity(n)` construct a real native growable
                // vector header. See `emit_vec_new` + the `MethodCall`
                // push/len/get arms for the layout + ABI. This is what
                // makes a `v = v.push(x)` loop actually grow under native
                // codegen (previously every Vec op stubbed through
                // `mty_runtime_extern_call`, returning 0).
                self.emit_vec_new()
            }
            FnRef::Builtin(BuiltinId::Extern(name)) => {
                if is_interpreter_hosted_stdlib(name) {
                    return Err(CodegenError::Unsupported(format!(
                        "{name} is interpreter-hosted"
                    )));
                }
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

    /// v0.49 — lower a native `std.crypto` / `std.encoding` call. Each
    /// maps to a runtime ABI symbol that takes its `(ptr,len)` input(s)
    /// and writes a `(ptr,len)` result aggregate into a fresh 16-byte
    /// slot. Returns the slot address (a Bytes/String value).
    fn emit_crypto_encoding_call(
        &mut self,
        full_name: &str,
        args: &[Operand],
    ) -> CompileResult<cranelift_codegen::ir::Value> {
        let bare = full_name.strip_prefix("std.").unwrap_or(full_name);
        // (runtime symbol, number of `(ptr,len)` data inputs)
        let (symbol, n_inputs): (&str, usize) = match bare {
            "crypto.sha256" => ("mty_runtime_crypto_sha256", 1),
            "crypto.sha512" => ("mty_runtime_crypto_sha512", 1),
            "crypto.blake3" => ("mty_runtime_crypto_blake3", 1),
            "crypto.hmac_sha256" => ("mty_runtime_crypto_hmac_sha256", 2),
            "encoding.hex.encode" => ("mty_runtime_encoding_hex_encode", 1),
            "encoding.base64.encode" => ("mty_runtime_encoding_base64_encode", 1),
            "encoding.base64.encode_url_no_pad" => {
                ("mty_runtime_encoding_base64_encode_url_no_pad", 1)
            }
            other => {
                return Err(CodegenError::Unsupported(format!(
                    "native crypto/encoding dispatch missing for {other}"
                )))
            }
        };
        // Each input operand (a Str/Bytes aggregate) expands to a
        // `(ptr, len)` pair via `string_pair` (handles literals + slots).
        let mut call_args = Vec::with_capacity(n_inputs * 2 + 1);
        for a in args.iter().take(n_inputs) {
            let (ptr, len) = self.string_pair(a)?;
            call_args.push(ptr);
            call_args.push(len);
        }
        // Fresh 16-byte (ptr@+0, len@+8) result slot.
        let slot = self.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            16,
            3, // log2(8)
        ));
        let slot_addr = self.b.ins().stack_addr(ct::I64, slot, 0);
        call_args.push(slot_addr);
        self.call_rt(symbol, &call_args, None)?;
        Ok(slot_addr)
    }

    /// v0.45 T1 (L18 fix) — lower a `std.fs.*` call to its native
    /// runtime ABI symbol.
    ///
    /// `full_name` is the fully-qualified method name (`"std.fs.read"`,
    /// `"std.fs.write"`, ...). `args` is the lowered SIR operands —
    /// `args[0]` is the path Str, `args[1]` (when present) is the
    /// payload Str/Bytes for write/append. The `out` place receives:
    /// - For read/read_to_string/read_dir_lines: a 24-byte aggregate
    ///   slot address holding (ptr, len, ok). Mighty reads it as a
    ///   Str value (the codegen elsewhere already treats Str as a
    ///   (ptr, len) aggregate — see `string_pair` — so loading at
    ///   offsets +0/+8 just works).
    /// - For read_dir / list_dir (v0.46 T4 iterator surface): an i64
    ///   handle written into the result local as a scalar value. The
    ///   `DirIter` ADT is registered opaque (Layout::scalar(8)) so the
    ///   place type carries the handle correctly.
    /// - For exists/write*/append/create_dir_all/remove*: a single
    ///   i32 coerced to the out local's type.
    /// - For metadata: a 24-byte struct slot {size, mtime_ms,
    ///   is_file, is_dir}. v0.46 T4 wires this through the result
    ///   local's own aggregate stack slot when the typed temp resolves
    ///   to the prelude `Metadata` ADT — so subsequent field
    ///   projections (`md.size`) read straight from the slot the
    ///   runtime just wrote into. Falls back to a freshly-allocated
    ///   24-byte slot when the result temp is untyped (pre-L18 shape
    ///   that just consumed the call as a side effect).
    fn emit_fs_call(
        &mut self,
        full_name: &str,
        args: &[mty_ir::ir::Operand],
        out: Option<&mty_ir::ir::Place>,
    ) -> CompileResult<()> {
        // Every fs call carries the path as its first operand.
        let Some(path_op) = args.first() else {
            return Err(CodegenError::Unsupported(format!(
                "{full_name}: missing path arg"
            )));
        };
        let (path_ptr, path_len) = self.string_pair(path_op)?;

        // Bucket by ABI shape.
        let kind = FsAbiKind::for_method(full_name);

        // For Metadata, prefer writing directly into the out local's
        // aggregate slot — `place_addr` will then walk the struct
        // field offsets when user code does `md.size` etc. Detect the
        // case by inspecting the out local's SIR type.
        let metadata_dst_slot_addr = if matches!(kind, FsAbiKind::MetadataSlot { .. }) {
            out.filter(|p| p.proj.is_empty()).and_then(|p| {
                let lty = self.f.locals[p.local.0 as usize].ty.clone();
                match &lty {
                    IrTy::Adt(id, _)
                        if self.prog.adt_by_id(*id).map(|a| a.name.as_str())
                            == Some("Metadata") =>
                    {
                        // Materialise the local's stack slot up
                        // front so the runtime writes directly
                        // into it. The post-call store path
                        // (def_var of the slot address) then
                        // becomes a no-op rebind to the same
                        // address.
                        self.agg_slot_addr(p.local).ok()
                    }
                    _ => None,
                }
            })
        } else {
            None
        };

        let ret_value: Option<cranelift_codegen::ir::Value> = match kind {
            FsAbiKind::ReadStrSlot { symbol } => {
                // Allocate a 24-byte (ptr, len, ok) slot. The Mighty-
                // side type is a Str aggregate; downstream consumers
                // read offsets +0 / +8 which line up with the
                // string_pair convention used everywhere else. The
                // third word (ok flag at +16) is currently consumed
                // only by the test harness; user code that wants the
                // success bit can read the ok flag through a tuple
                // projection in a future revision.
                let slot = self.b.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    24,
                    3,
                ));
                let slot_addr = self.b.ins().stack_addr(ct::I64, slot, 0);
                self.call_rt(symbol, &[path_ptr, path_len, slot_addr], None)?;
                Some(slot_addr)
            }
            FsAbiKind::WriteI32 { symbol } => {
                // (path_ptr, path_len, data_ptr, data_len) -> i32
                let Some(data_op) = args.get(1) else {
                    return Err(CodegenError::Unsupported(format!(
                        "{full_name}: missing data arg"
                    )));
                };
                let (data_ptr, data_len) = self.string_pair(data_op)?;
                self.call_rt(
                    symbol,
                    &[path_ptr, path_len, data_ptr, data_len],
                    Some(ct::I32),
                )?
            }
            FsAbiKind::PathI32 { symbol } => {
                self.call_rt(symbol, &[path_ptr, path_len], Some(ct::I32))?
            }
            FsAbiKind::MetadataSlot { symbol } => {
                // 24-byte struct slot — {size:u64@+0, mtime_ms:i64@+8,
                // is_file:i8@+16, is_dir:i8@+17, 6B pad}. The runtime
                // writes the fields directly via raw ptr writes; the
                // i32 return is the success flag (1=ok, -errno=err),
                // which the codegen currently swallows. Future versions
                // can surface this through a Result-shaped out.
                //
                // v0.46 T4 — when the result local is typed as the
                // prelude `Metadata` ADT, use its own backing stack
                // slot as the runtime's destination so `md.size` /
                // `md.is_file` reads land on the bytes the runtime
                // just wrote.
                let slot_addr = match metadata_dst_slot_addr {
                    Some(addr) => addr,
                    None => {
                        let slot = self.b.create_sized_stack_slot(StackSlotData::new(
                            StackSlotKind::ExplicitSlot,
                            24,
                            3,
                        ));
                        self.b.ins().stack_addr(ct::I64, slot, 0)
                    }
                };
                self.call_rt(symbol, &[path_ptr, path_len, slot_addr], Some(ct::I32))?;
                Some(slot_addr)
            }
            FsAbiKind::DirOpenHandle { symbol } => {
                // (path_ptr, path_len) -> i64 handle. Returned by the
                // runtime as an opaque pointer to a DirIterState. We
                // hand the i64 straight to the out local — the
                // `DirIter` ADT is registered opaque so its codegen
                // layout is `Layout::scalar(8)`, the same shape the
                // `def_var` path on the result local already expects.
                self.call_rt(symbol, &[path_ptr, path_len], Some(ct::I64))?
            }
        };

        // Write the result into the `out` place, if any.
        if let Some(p) = out {
            if let Some(r) = ret_value {
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
        }
        Ok(())
    }

    /// v0.42 T4 (L23 fix) — `String + String` concat.
    ///
    /// Builds a 16-byte stack slot for the result aggregate
    /// (ptr@+0, len@+8) and calls `mty_runtime_str_concat` to write
    /// the joined (ptr, len) pair into it. Returns the slot address —
    /// same shape as any other Str aggregate value.
    fn emit_str_concat(
        &mut self,
        a: &Operand,
        b: &Operand,
    ) -> CompileResult<cranelift_codegen::ir::Value> {
        let (aptr, alen) = self.string_pair(a)?;
        let (bptr, blen) = self.string_pair(b)?;
        let slot = self.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            16,
            3, // log2(8)
        ));
        let slot_addr = self.b.ins().stack_addr(ct::I64, slot, 0);
        self.call_rt(
            "mty_runtime_str_concat",
            &[aptr, alen, bptr, blen, slot_addr],
            None,
        )?;
        Ok(slot_addr)
    }

    /// #297 — materialise a `String` aggregate: a fresh 16-byte stack
    /// slot holding `(ptr@+0, len@+8)`. Returns the slot address (the
    /// String value's by-address representation). Used by the native
    /// String constructors (`String.new`, `String.with_capacity`,
    /// `String.from_str`).
    fn emit_string_slot(
        &mut self,
        ptr: cranelift_codegen::ir::Value,
        len: cranelift_codegen::ir::Value,
    ) -> cranelift_codegen::ir::Value {
        let slot = self.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            16,
            3, // log2(8)
        ));
        let slot_addr = self.b.ins().stack_addr(ct::I64, slot, 0);
        let mf = MemFlags::trusted();
        self.b.ins().store(mf, ptr, slot_addr, 0);
        self.b.ins().store(mf, len, slot_addr, 8);
        slot_addr
    }

    /// v0.42 T4 (L23 fix) — `n.to_str()` on a scalar receiver.
    ///
    /// Allocates a fresh 16-byte stack slot for the resulting String
    /// aggregate (ptr@+0, len@+8), then calls the type-appropriate
    /// `mty_runtime_fmt_*` runtime helper to format the value and
    /// write the (ptr,len) pair into the slot. Returns `Some(slot)`
    /// when the receiver is a scalar we know how to format. Returns
    /// `None` for non-scalar receivers so the caller can fall through
    /// to the generic extern-bridge path.
    fn try_emit_scalar_to_str(
        &mut self,
        receiver: &Operand,
    ) -> CompileResult<Option<cranelift_codegen::ir::Value>> {
        let Some(ty) = self.operand_ir_ty(receiver) else {
            return Ok(None);
        };
        // Pick the runtime fmt symbol + the cranelift type the value
        // must be coerced to for the call's first param.
        let (sym, cl_ty) = match &ty {
            IrTy::Bool => ("mty_runtime_fmt_bool", ct::I8),
            IrTy::Char => ("mty_runtime_fmt_u32", ct::I32),
            IrTy::Int(k) => match k {
                IntKind::I8 | IntKind::I16 | IntKind::I32 | IntKind::IntInfer => {
                    ("mty_runtime_fmt_i32", ct::I32)
                }
                IntKind::I64 | IntKind::ISize | IntKind::I128 => {
                    ("mty_runtime_fmt_i64_to_slot", ct::I64)
                }
                IntKind::U8 | IntKind::U16 | IntKind::U32 => ("mty_runtime_fmt_u32", ct::I32),
                IntKind::U64 | IntKind::U128 => ("mty_runtime_fmt_u64", ct::I64),
                IntKind::USize => ("mty_runtime_fmt_usize", ct::I64),
            },
            IrTy::Float(k) => match k {
                FloatKind::F32 => ("mty_runtime_fmt_f32", ct::F32),
                FloatKind::F64 | FloatKind::FloatInfer => ("mty_runtime_fmt_f64", ct::F64),
            },
            IrTy::Size | IrTy::Duration => ("mty_runtime_fmt_usize", ct::I64),
            // Str/String already are strings — `to_str()` is the
            // identity; return the receiver's address as a 16-byte
            // aggregate so downstream consumers see a Str shape.
            IrTy::Str | IrTy::String | IrTy::Bytes => {
                let addr = self.eval_operand(receiver)?;
                return Ok(Some(addr));
            }
            _ => return Ok(None),
        };
        // Allocate the (ptr,len) slot — same layout as Const::Str.
        let slot = self.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            16,
            3, // log2(8)
        ));
        let slot_addr = self.b.ins().stack_addr(ct::I64, slot, 0);
        // Coerce the receiver value to the runtime fmt symbol's
        // expected first-arg type.
        let v = self.eval_operand(receiver)?;
        let have_ty = self.b.func.dfg.value_type(v);
        let v = if have_ty == cl_ty {
            v
        } else if have_ty.is_float() && cl_ty.is_float() {
            if have_ty.bits() < cl_ty.bits() {
                self.b.ins().fpromote(cl_ty, v)
            } else {
                self.b.ins().fdemote(cl_ty, v)
            }
        } else {
            self.coerce_to_with_src(v, cl_ty, Some(&ty))
        };
        self.call_rt(sym, &[v, slot_addr], None)?;
        Ok(Some(slot_addr))
    }

    /// v0.42 T4 (L23 fix) — emit a single typed `log`/`print` arg.
    ///
    /// Dispatches on the operand's SIR type to the right runtime
    /// symbol. `single_arg` controls whether the line-terminating
    /// behavior fires for `log` (single-arg uses the existing
    /// `log_*` variants; multi-arg uses `print_*` per element +
    /// caller emits the newline).
    fn emit_one_log_arg(
        &mut self,
        op: &Operand,
        op_ty: Option<&IrTy>,
        single_arg: bool,
        is_log: bool,
    ) -> CompileResult<()> {
        // For single-arg log we use the `log_*` newline variant; for
        // any other shape (multi-arg log, print) we use `print_*` and
        // the caller decides whether to terminate with a newline.
        let use_newline_variant = single_arg && is_log;
        match op_ty {
            Some(IrTy::Str | IrTy::String | IrTy::Bytes) => {
                let (ptr, len) = self.string_pair(op)?;
                let sym = if use_newline_variant {
                    "mty_runtime_log"
                } else {
                    "mty_runtime_print"
                };
                self.call_rt(sym, &[ptr, len], None)?;
            }
            Some(IrTy::Bool) => {
                let v = self.eval_operand(op)?;
                let v = self.coerce_to(v, ct::I8);
                let sym = if use_newline_variant {
                    "mty_runtime_log_bool"
                } else {
                    "mty_runtime_print_bool"
                };
                self.call_rt(sym, &[v], None)?;
            }
            Some(IrTy::Char) => {
                // Treat Char as a U32 print so user gets the code-point
                // number. The interpreter prints the literal char via
                // `to_string`, but the i32 codepoint is more useful for
                // probe output and avoids needing a runtime UTF-8
                // encoder for a single value.
                let v = self.eval_operand(op)?;
                let v = self.coerce_to(v, ct::I32);
                let sym = if use_newline_variant {
                    "mty_runtime_log_u32"
                } else {
                    "mty_runtime_print_u32"
                };
                self.call_rt(sym, &[v], None)?;
            }
            Some(IrTy::Int(k)) => {
                let v = self.eval_operand(op)?;
                let (sym_log, sym_print, cl_ty) = match k {
                    IntKind::I8 | IntKind::I16 | IntKind::I32 | IntKind::IntInfer => {
                        ("mty_runtime_log_i32", "mty_runtime_print_i32", ct::I32)
                    }
                    IntKind::I64 | IntKind::ISize => {
                        ("mty_runtime_log_i64", "mty_runtime_print_i64", ct::I64)
                    }
                    IntKind::U8 | IntKind::U16 | IntKind::U32 => {
                        ("mty_runtime_log_u32", "mty_runtime_print_u32", ct::I32)
                    }
                    IntKind::U64 => ("mty_runtime_log_u64", "mty_runtime_print_u64", ct::I64),
                    IntKind::USize => ("mty_runtime_log_usize", "mty_runtime_print_usize", ct::I64),
                    IntKind::I128 | IntKind::U128 => {
                        // No i128 runtime symbol — narrow lossily to i64.
                        // v0.42 T4 scope is the common width family;
                        // wider ints can land in a follow-up.
                        ("mty_runtime_log_i64", "mty_runtime_print_i64", ct::I64)
                    }
                };
                // coerce_to_with_src already picks zext vs sext from
                // the SIR type — feed it through with the original kind
                // so unsigned widening (U8 → I32 for the i32 print
                // path) zero-extends rather than sign-extends.
                let v = self.coerce_to_with_src(v, cl_ty, Some(&IrTy::Int(*k)));
                let sym = if use_newline_variant {
                    sym_log
                } else {
                    sym_print
                };
                self.call_rt(sym, &[v], None)?;
            }
            Some(IrTy::Float(k)) => {
                let v = self.eval_operand(op)?;
                let (sym_log, sym_print, cl_ty) = match k {
                    FloatKind::F32 => ("mty_runtime_log_f32", "mty_runtime_print_f32", ct::F32),
                    FloatKind::F64 | FloatKind::FloatInfer => {
                        ("mty_runtime_log_f64", "mty_runtime_print_f64", ct::F64)
                    }
                };
                let have_ty = self.b.func.dfg.value_type(v);
                let v = if have_ty == cl_ty {
                    v
                } else if have_ty == ct::F32 && cl_ty == ct::F64 {
                    self.b.ins().fpromote(ct::F64, v)
                } else if have_ty == ct::F64 && cl_ty == ct::F32 {
                    self.b.ins().fdemote(ct::F32, v)
                } else {
                    v
                };
                let sym = if use_newline_variant {
                    sym_log
                } else {
                    sym_print
                };
                self.call_rt(sym, &[v], None)?;
            }
            Some(IrTy::Size | IrTy::Duration) => {
                // Both lower to i64 (see eval_const). Print unsigned.
                let v = self.eval_operand(op)?;
                let v = self.coerce_to(v, ct::I64);
                let sym = if use_newline_variant {
                    "mty_runtime_log_usize"
                } else {
                    "mty_runtime_print_usize"
                };
                self.call_rt(sym, &[v], None)?;
            }
            Some(_) | None => {
                // Pre-v0.42 T4 behaviour: try the string path. We
                // only do this if the operand is *plausibly* a Str —
                // a literal `Const::Str` or an aggregate-shaped local
                // that the lowerer left untyped (`IrTy::Error`). For
                // a scalar local that we can't otherwise classify we
                // would otherwise misread an i32 slot as a 16-byte
                // (ptr,len) pair and segfault.
                let plausibly_str = matches!(op, Operand::Const(Const::Str(_)))
                    || matches!(
                        (op, &op_ty),
                        (Operand::Copy(_) | Operand::Move(_), Some(IrTy::Error))
                    );
                if plausibly_str {
                    if let Ok((ptr, len)) = self.string_pair(op) {
                        let sym = if use_newline_variant {
                            "mty_runtime_log"
                        } else {
                            "mty_runtime_print"
                        };
                        self.call_rt(sym, &[ptr, len], None)?;
                    }
                }
                // Truly unknown — silently drop the trace rather than
                // segfault. Trace-call robustness > strict failure.
            }
        }
        Ok(())
    }

    // ---- v0.38 native growable Vec (L28 fix) -----------------------
    //
    // A native `Vec[T]` value is an i64 pointer to a 24-byte header in
    // the runtime arena:
    //
    //   off 0  : len  (i64)  — element count
    //   off 8  : cap  (i64)  — capacity in elements
    //   off 16 : data (i64)  — pointer to `cap * 8` bytes of storage
    //
    // Every element is stored in an 8-byte slot, which losslessly holds
    // any scalar Mighty element type we currently codegen (U8/I32/USize/
    // I64/bool/char/F64-as-bits). The header pointer is stable across
    // `push`, so the SIR `v = v.push(x)` capture-rebind threads the same
    // i64 through the loop back-edge via the local's cranelift Variable.
    //
    // Growth re-allocates a larger buffer from the arena and copies the
    // live prefix; the old buffer is leaked into the arena (freed when
    // the arena frame pops). The arena allocator already backs every
    // native build, so no new runtime symbol is required.
    // v0.39 T3 — typed-slot Vec header v2 (32 bytes):
    //   off  0 : len       (i64) — element count
    //   off  8 : cap       (i64) — capacity in elements
    //   off 16 : data      (i64) — pointer to `cap * elem_size` bytes
    //   off 24 : elem_size (i64) — size in bytes of one element slot
    //
    // The element type is statically known at codegen time (it's the
    // `T` in the receiver's `Vec[T]` or the destination's). We still
    // store elem_size in the header so:
    //   (a) the runtime grow loop can copy the live prefix without
    //       re-deriving size at codegen time, and
    //   (b) future migration tooling can detect v1 vs v2 layout from
    //       a serialized image (v1 has data@16 + 8-byte slots, no
    //       elem_size word; see `VEC_HEADER_V2` constant + RELEASE
    //       notes for the v0.39 layout pivot).
    pub const VEC_LEN_OFF: i32 = 0;
    pub const VEC_CAP_OFF: i32 = 8;
    pub const VEC_DATA_OFF: i32 = 16;
    pub const VEC_ELEM_SIZE_OFF: i32 = 24;
    pub const VEC_HEADER_SIZE: i64 = 32;
    /// Marker constant for v0.39 header layout (`{len, cap, data,
    /// elem_size}`). Bumped from the v0.38 layout (24-byte header, no
    /// elem_size word). Migration tooling can read this to gate v1→v2
    /// upgrades for serialized Vec values.
    pub const VEC_HEADER_V2: u32 = 2;
    /// Default element-slot width when the element type is unknown to
    /// the lowerer (e.g. an `IrTy::Error` Vec local from a partly
    /// type-checked snippet). Picked as i64 for backward compat with
    /// the v0.38 8-byte-slot behavior.
    const VEC_FALLBACK_ELEM_SIZE: i64 = 8;

    /// Allocate `size` bytes (align 8) from the runtime arena, returning
    /// the i64 pointer (0 on OOM, matching the runtime contract).
    fn rt_alloc(
        &mut self,
        size: cranelift_codegen::ir::Value,
    ) -> CompileResult<cranelift_codegen::ir::Value> {
        let align = self.b.ins().iconst(ct::I64, 8);
        let zero = self.b.ins().iconst(ct::I64, 0);
        let r = self.call_rt("mty_runtime_alloc", &[size, align, zero], Some(ct::I64))?;
        Ok(r.unwrap_or_else(|| self.b.ins().iconst(ct::I64, 0)))
    }

    /// v0.39 T3: pull the element type `T` out of a `Vec[T]` SIR type
    /// (the Vec ADT is registered in `mty-types::prelude` with a single
    /// generic parameter). Returns None for non-Vec / no-generics
    /// inputs — callers fall back to the 8-byte slot size.
    fn vec_elem_ty_from(&self, t: &IrTy) -> Option<IrTy> {
        // We can't carry the prelude AdtId down here, but the `Adt`
        // node carries the type-args list directly. Match the name in
        // the ADT catalog to avoid mis-typing a non-Vec opaque ADT.
        if let IrTy::Adt(id, args) = t {
            let name = self.prog.adt_by_id(*id).map(|a| a.name.as_str());
            if name == Some("Vec") {
                if let Some(elem) = args.first() {
                    return Some(elem.clone());
                }
            }
        }
        None
    }

    /// v0.39 T3: statically pick a slot width for a Vec element type.
    /// Mirrors `field_load_ty` widths but always yields a byte count
    /// (1/2/4/8) for scalars and the layout size for aggregates.
    /// Falls back to 8 for unresolved types so a `Vec[{error}]` keeps
    /// the v0.38 8-byte slot behavior.
    fn vec_elem_size_for(&self, elem_ty: &IrTy) -> i64 {
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
                mty_types::FloatKind::F32 => 4,
                mty_types::FloatKind::F64 | mty_types::FloatKind::FloatInfer => 8,
            },
            Char => 4,
            Duration | Size => 8,
            Ref { .. } | RawPtr(_) | Cap { .. } | Fn { .. } => 8,
            Tuple(_) | Array { .. } | Adt(_, _) | Str | String | Bytes => {
                type_size(elem_ty, &self.prog.adts) as i64
            }
            Error | Param(_) | Module(_) | Unit | Never | Dyn(_) => Self::VEC_FALLBACK_ELEM_SIZE,
        }
    }

    /// v0.39 T3: cranelift load/store width for a scalar element type
    /// plus a sign/zero-extension flag for narrow-int reads. Returns
    /// None when the element is an aggregate (caller must memcpy).
    fn vec_elem_ld_st(&self, elem_ty: &IrTy) -> Option<(ClType, bool)> {
        use IrTy::*;
        Some(match elem_ty {
            Bool => (ct::I8, false),
            Int(k) => match k {
                IntKind::I8 => (ct::I8, true),
                IntKind::U8 => (ct::I8, false),
                IntKind::I16 => (ct::I16, true),
                IntKind::U16 => (ct::I16, false),
                IntKind::I32 | IntKind::IntInfer => (ct::I32, true),
                IntKind::U32 => (ct::I32, false),
                IntKind::I64 | IntKind::ISize => (ct::I64, true),
                IntKind::U64 | IntKind::USize => (ct::I64, false),
                IntKind::I128 | IntKind::U128 => return None,
            },
            Float(k) => match k {
                mty_types::FloatKind::F32 => (ct::F32, false),
                mty_types::FloatKind::F64 | mty_types::FloatKind::FloatInfer => (ct::F64, false),
            },
            Char => (ct::I32, false),
            Duration | Size => (ct::I64, false),
            Ref { .. } | RawPtr(_) | Cap { .. } | Fn { .. } => (ct::I64, false),
            Error | Param(_) | Module(_) | Dyn(_) | Unit | Never => (ct::I64, false),
            Tuple(_) | Array { .. } | Adt(_, _) | Str | String | Bytes => return None,
        })
    }

    /// Convenience: pull `T` out of the receiver operand's local type
    /// and pick its element size + cranelift load width. Falls back to
    /// `(8, i64, false)` when the type information is missing.
    fn vec_elem_info(&self, receiver: &Operand) -> (i64, Option<(ClType, bool)>, Option<IrTy>) {
        let recv_ty = match receiver {
            Operand::Copy(p) | Operand::Move(p) => {
                Some(self.f.locals[p.local.0 as usize].ty.clone())
            }
            Operand::Const(_) => None,
        };
        // v0.39 T3: the SIR lowerer types the MethodCall result temp
        // as `IrTy::Error` (see `lower_expr` MethodCall arm in
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

    /// `Vec.new()` — allocate a zeroed header (len=0, cap=0, data=null,
    /// elem_size=T-sized). The destination local's `Vec[T]` carries `T`;
    /// caller sets `self.current_dest_ty` before invoking eval_rvalue.
    fn emit_vec_new(&mut self) -> CompileResult<cranelift_codegen::ir::Value> {
        // Pull element size from the destination's `Vec[T]` type. When
        // that's unavailable (e.g. dead-store with poisoned typeck),
        // we fall back to the v0.38 8-byte slot.
        let elem_size = self
            .current_dest_ty
            .clone()
            .as_ref()
            .and_then(|t| self.vec_elem_ty_from(t))
            .map(|t| self.vec_elem_size_for(&t))
            .unwrap_or(Self::VEC_FALLBACK_ELEM_SIZE);
        let hsize = self.b.ins().iconst(ct::I64, Self::VEC_HEADER_SIZE);
        let hdr = self.rt_alloc(hsize)?;
        let zero = self.b.ins().iconst(ct::I64, 0);
        let esz = self.b.ins().iconst(ct::I64, elem_size);
        let mf = MemFlags::trusted();
        self.b.ins().store(mf, zero, hdr, Self::VEC_LEN_OFF);
        self.b.ins().store(mf, zero, hdr, Self::VEC_CAP_OFF);
        self.b.ins().store(mf, zero, hdr, Self::VEC_DATA_OFF);
        self.b.ins().store(mf, esz, hdr, Self::VEC_ELEM_SIZE_OFF);
        Ok(hdr)
    }

    /// Evaluate a Vec receiver operand to its i64 header pointer.
    fn vec_header(&mut self, receiver: &Operand) -> CompileResult<cranelift_codegen::ir::Value> {
        let v = self.eval_operand(receiver)?;
        Ok(self.coerce_to(v, ct::I64))
    }

    /// Store one element value into a Vec data slot.
    /// - For scalars we use the typed cranelift load/store width so a
    ///   Vec[U8] really only writes one byte.
    /// - For aggregates (size > 8) we memcpy `elem_size` bytes from the
    ///   value's slot address.
    /// - When the element type is unknown we fall back to the v0.38
    ///   i64-store path (8-byte slot).
    fn vec_store_elem(
        &mut self,
        slot: cranelift_codegen::ir::Value,
        val: cranelift_codegen::ir::Value,
        elem_size: i64,
        lds: Option<(ClType, bool)>,
        elem_ty: Option<&IrTy>,
    ) {
        let mf = MemFlags::trusted();
        if let Some((cty, _signed)) = lds {
            // Scalar slot: coerce the source to the slot's cl type
            // (handles I64 → I8 narrows via ireduce) then store.
            let narrowed = self.coerce_to(val, cty);
            self.b.ins().store(mf, narrowed, slot, 0);
            return;
        }
        // v0.39 T3 — preserve the v0.38 fallback semantics for unknown
        // element types: when lds is None and elem_size matches the
        // fallback (8 bytes), `val` is a scalar i64 (eval_operand
        // returned the value, not an address). Routes like
        // `String.push(' ')` reach this branch — `push` is dispatched
        // to emit_vec_push for any receiver type, and String is not
        // a Vec, so vec_elem_info returns the 8-byte fallback. The
        // v0.38 behaviour was to store the i64 word; do the same.
        if elem_size == Self::VEC_FALLBACK_ELEM_SIZE {
            let narrowed = self.coerce_to(val, ct::I64);
            self.b.ins().store(mf, narrowed, slot, 0);
            return;
        }
        // Aggregate slot: val is the source aggregate's address.
        // Copy elem_size bytes byte-granularly.
        let _ = elem_ty;
        self.memcpy_bytes(slot, val, elem_size as u32);
    }

    /// Load one element value from a Vec data slot.
    /// - Scalar: typed load with sign/zero extend to i64.
    /// - Aggregate: caller is responsible for the memcpy into a fresh
    ///   slot; we return the slot pointer as i64. (None case stays
    ///   compatible with `Rvalue::IndexRead` which historically loaded
    ///   an i64 word.)
    fn vec_load_elem(
        &mut self,
        slot: cranelift_codegen::ir::Value,
        elem_size: i64,
        lds: Option<(ClType, bool)>,
    ) -> cranelift_codegen::ir::Value {
        let mf = MemFlags::trusted();
        if let Some((cty, signed)) = lds {
            let raw = self.b.ins().load(cty, mf, slot, 0);
            // Sign- or zero-extend narrow ints up to i64 for downstream
            // consumers (BinOp / coerce paths expect i64 by default).
            if cty == ct::I8 || cty == ct::I16 || cty == ct::I32 {
                if signed {
                    return self.b.ins().sextend(ct::I64, raw);
                }
                return self.b.ins().uextend(ct::I64, raw);
            }
            return raw;
        }
        // v0.39 T3 — fallback unknown-element-type path mirrors v0.38:
        // load the slot as an i64 word. (Same rationale as
        // vec_store_elem's fallback.)
        if elem_size == Self::VEC_FALLBACK_ELEM_SIZE {
            return self.b.ins().load(ct::I64, mf, slot, 0);
        }
        // Aggregate: hand back the slot pointer; copying is the
        // caller's job. (matches the v0.38 8-byte path's contract.)
        slot
    }

    /// Trap-on-out-of-range bounds check: if `idx >= len`, emit a
    /// `mty_runtime_panic` call with a fixed-message ptr then a hard
    /// trap (TrapCode::user(5)). Used by `get` and `set`.
    ///
    /// v0.39 T3 — the panic stub in the production runtime exits the
    /// process. We follow it with a `trap` so the cranelift verifier
    /// sees the OOB block as terminal even if a stub returns. Tests
    /// that want to observe the panic before the trap fires should
    /// spawn a subprocess and assert non-zero exit.
    fn vec_bounds_check(
        &mut self,
        idx: cranelift_codegen::ir::Value,
        len: cranelift_codegen::ir::Value,
    ) -> CompileResult<()> {
        let oob = self.b.create_block();
        let ok = self.b.create_block();
        let is_oob = self.b.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::UnsignedGreaterThanOrEqual,
            idx,
            len,
        );
        self.b.ins().brif(is_oob, oob, &[], ok, &[]);
        self.b.switch_to_block(oob);
        self.b.seal_block(oob);
        let id = self.mod_ctx.intern_string("Vec index out of bounds")?;
        let gv = self.mod_ctx.module.declare_data_in_func(id, self.b.func);
        let nptr = self.b.ins().symbol_value(ct::I64, gv);
        let nlen = self.b.ins().iconst(ct::I64, 23);
        let _ = self.call_rt("mty_runtime_panic", &[nptr, nlen], None)?;
        self.b
            .ins()
            .trap(cranelift_codegen::ir::TrapCode::user(5).unwrap());
        self.b.switch_to_block(ok);
        self.b.seal_block(ok);
        Ok(())
    }

    /// `v.push(x)` — ensure capacity (growing if `len == cap`), store the
    /// element at `data[len]`, bump `len`, and return the (unchanged)
    /// header pointer so the capture-rebind keeps the same Vec.
    ///
    /// v0.39 T3: typed-slot — picks store width from receiver's `T`,
    /// and grow-loop sizes/copies by `elem_size@24` from the header.
    fn emit_vec_push(
        &mut self,
        receiver: &Operand,
        args: &[Operand],
    ) -> CompileResult<cranelift_codegen::ir::Value> {
        let mf = MemFlags::trusted();
        let (elem_size, lds, elem_ty) = self.vec_elem_info(receiver);
        let hdr = self.vec_header(receiver)?;
        let len = self.b.ins().load(ct::I64, mf, hdr, Self::VEC_LEN_OFF);
        let cap = self.b.ins().load(ct::I64, mf, hdr, Self::VEC_CAP_OFF);

        // grow_block: runs when len == cap. new_cap = max(4, cap*2).
        let grow_block = self.b.create_block();
        let cont_block = self.b.create_block();
        let need_grow = self
            .b
            .ins()
            .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, len, cap);
        self.b
            .ins()
            .brif(need_grow, grow_block, &[], cont_block, &[]);

        // --- grow_block ---
        self.b.switch_to_block(grow_block);
        self.b.seal_block(grow_block);
        let two_cap = self.b.ins().imul_imm(cap, 2);
        let four = self.b.ins().iconst(ct::I64, 4);
        let cap_is_small = self.b.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::UnsignedLessThan,
            two_cap,
            four,
        );
        let new_cap = self.b.ins().select(cap_is_small, four, two_cap);
        let new_bytes = self.b.ins().imul_imm(new_cap, elem_size);
        let new_data = self.rt_alloc(new_bytes)?;
        let old_data = self.b.ins().load(ct::I64, mf, hdr, Self::VEC_DATA_OFF);
        // memcpy the live prefix (len * elem_size bytes) old → new. The
        // byte-granular memcpy keeps Vec[U8] correct (no 8-byte
        // overshoot off the end of the data buffer).
        let copy_bytes = self.b.ins().imul_imm(len, elem_size);
        self.emit_memcpy_dynamic_bytes(new_data, old_data, copy_bytes);
        self.b.ins().store(mf, new_cap, hdr, Self::VEC_CAP_OFF);
        self.b.ins().store(mf, new_data, hdr, Self::VEC_DATA_OFF);
        self.b.ins().jump(cont_block, &[]);

        // --- cont_block ---
        self.b.switch_to_block(cont_block);
        self.b.seal_block(cont_block);
        // Reload data (it may have changed in grow_block); len unchanged.
        let data = self.b.ins().load(ct::I64, mf, hdr, Self::VEC_DATA_OFF);
        let byte_off = self.b.ins().imul_imm(len, elem_size);
        let slot = self.b.ins().iadd(data, byte_off);
        // #297 layer 4 — String/Str/Bytes elements are a 16-byte
        // (ptr@+0, len@+8) pair, but their operand is NOT a uniform
        // slot address: a string literal materialises as an inline
        // (ptr,len) pair (no slot) and `eval_operand` on a bare
        // call-result temp returns the produced VALUE, not a 16-byte
        // slot address. memcpy-from-operand-address therefore reads past
        // a literal's bytes and corrupts the heap (segfault). Route
        // these through `string_pair`, which yields the correct (ptr,len)
        // for both the literal fast-path and the slot-backed dynamic
        // case, and store both halves explicitly.
        if matches!(
            elem_ty.as_ref(),
            Some(IrTy::String | IrTy::Str | IrTy::Bytes)
        ) {
            if let Some(a) = args.first() {
                let (ptr, slen) = self.string_pair(a)?;
                self.b.ins().store(mf, ptr, slot, 0);
                self.b.ins().store(mf, slen, slot, 8);
            }
        } else {
            // Scalars: the raw i64 value. Other aggregates: the slot
            // address (eval_operand returns the address when the type
            // is_aggregate), memcpy'd by `vec_store_elem`.
            let raw = if let Some(a) = args.first() {
                self.eval_operand(a)?
            } else {
                self.b.ins().iconst(ct::I64, 0)
            };
            self.vec_store_elem(slot, raw, elem_size, lds, elem_ty.as_ref());
        }
        let new_len = self.b.ins().iadd_imm(len, 1);
        self.b.ins().store(mf, new_len, hdr, Self::VEC_LEN_OFF);
        Ok(hdr)
    }

    /// `v.len()` — load the element count.
    fn emit_vec_len(&mut self, receiver: &Operand) -> CompileResult<cranelift_codegen::ir::Value> {
        let hdr = self.vec_header(receiver)?;
        Ok(self
            .b
            .ins()
            .load(ct::I64, MemFlags::trusted(), hdr, Self::VEC_LEN_OFF))
    }

    /// `v.get(i)` — returns `Option[T]`. In-range indices return
    /// `Some(elem)`; out-of-range returns `None` (matching the
    /// interpreter in `mty-ir::interp::run::eval_method`).
    ///
    /// v0.41 T3 (L1 fix): pre-v0.41 this raised a runtime panic + trap
    /// on OOB and returned a bare scalar element value, but the
    /// destination is an `Option[T]` aggregate. The caller's match arm
    /// would then dereference the scalar as an aggregate address and
    /// segfault (example 26 `_vec_get_oob`). Now we synthesise a real
    /// Option aggregate: bounds-check folds into the Some/None branch.
    ///
    /// v0.39 T3: typed slot — width and sign-extension are picked from
    /// the receiver's element type. Vec[U8] reads 1 byte and
    /// zero-extends; Vec[I32] reads 4 bytes and sign-extends; etc.
    fn emit_vec_get(
        &mut self,
        receiver: &Operand,
        args: &[Operand],
    ) -> CompileResult<cranelift_codegen::ir::Value> {
        let mf = MemFlags::trusted();
        let (elem_size, lds, elem_ty) = self.vec_elem_info(receiver);
        let hdr = self.vec_header(receiver)?;
        let len = self.b.ins().load(ct::I64, mf, hdr, Self::VEC_LEN_OFF);
        let data = self.b.ins().load(ct::I64, mf, hdr, Self::VEC_DATA_OFF);
        let idx = if let Some(a) = args.first() {
            let raw = self.eval_operand(a)?;
            self.coerce_to(raw, ct::I64)
        } else {
            self.b.ins().iconst(ct::I64, 0)
        };
        // Synthesise an Option[T] aggregate. Tag: 0 = Some, 1 = None.
        let (slot_addr, payload_off) = self.alloc_option_slot_for(elem_ty.as_ref())?;
        let some_block = self.b.create_block();
        let none_block = self.b.create_block();
        let join_block = self.b.create_block();
        let is_oob = self.b.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::UnsignedGreaterThanOrEqual,
            idx,
            len,
        );
        self.b.ins().brif(is_oob, none_block, &[], some_block, &[]);
        // --- Some(elem) ---
        self.b.switch_to_block(some_block);
        self.b.seal_block(some_block);
        let tag0 = self.b.ins().iconst(ct::I32, 0);
        self.b.ins().store(mf, tag0, slot_addr, TAG_OFFSET as i32);
        let byte_off = self.b.ins().imul_imm(idx, elem_size);
        let slot = self.b.ins().iadd(data, byte_off);
        let elem = self.vec_load_elem(slot, elem_size, lds);
        let payload_addr = self.b.ins().iadd_imm(slot_addr, payload_off as i64);
        if let Some(ty) = elem_ty.as_ref() {
            // store_scalar handles width-correct narrowing for U8/I32 etc.
            self.store_scalar(payload_addr, elem, ty)?;
        } else {
            self.b.ins().store(mf, elem, payload_addr, 0);
        }
        self.b.ins().jump(join_block, &[]);
        // --- None ---
        self.b.switch_to_block(none_block);
        self.b.seal_block(none_block);
        let tag1 = self.b.ins().iconst(ct::I32, 1);
        self.b.ins().store(mf, tag1, slot_addr, TAG_OFFSET as i32);
        self.b.ins().jump(join_block, &[]);
        // --- join ---
        self.b.switch_to_block(join_block);
        self.b.seal_block(join_block);
        Ok(slot_addr)
    }

    /// Allocate a stack slot for an Option-shaped aggregate. Returns
    /// (slot_addr, payload_offset) where payload_offset matches the
    /// layout the consuming match arm reads via `variant_field_offset`.
    ///
    /// Selection order:
    ///   1. `current_dest_ty` is `IrTy::Adt(option_id, _)` — exact
    ///      match: build a stack slot sized for the Option ADT and
    ///      use `variant_field_offset` for the payload offset.
    ///   2. `payload_ty` was passed (Vec element type known) — locate
    ///      the prelude `Option` ADT by name and substitute the
    ///      payload type into its `Some(T)` field for the layout
    ///      computation. This is the common case: the SIR temp for
    ///      `v.get(i)` carries `IrTy::Error` (lowerer convention),
    ///      but the receiver's `Vec[T]` pins `T`.
    ///   3. Neither — fall back to a 16-byte slot with payload@8,
    ///      which is correct for any scalar ≤8 bytes.
    ///
    /// v0.41 T3.
    fn alloc_option_slot_for(
        &mut self,
        payload_ty: Option<&IrTy>,
    ) -> CompileResult<(cranelift_codegen::ir::Value, u32)> {
        // (1) dest_ty is exactly Option[T].
        if let Some(IrTy::Adt(id, _)) = self.current_dest_ty.clone() {
            if let Some(adt) = self.prog.adt_by_id(id).cloned() {
                if adt.name == "Option" && adt.variants.len() == 2 {
                    let (off, _) = variant_field_offset(&adt, 0, 0, &self.prog.adts)
                        .unwrap_or((8, crate::layout::Layout::scalar(8)));
                    let size = type_size(&IrTy::Adt(id, vec![]), &self.prog.adts).max(8);
                    let align = type_align(&IrTy::Adt(id, vec![]), &self.prog.adts).max(8);
                    let log2_align = (align.next_power_of_two().trailing_zeros()).min(16) as u8;
                    let slot = self.b.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        slot_size(size),
                        log2_align,
                    ));
                    let addr = self.b.ins().stack_addr(ct::I64, slot, 0);
                    return Ok((addr, off));
                }
            }
        }
        // (2) Look up the prelude Option ADT by name and substitute
        // the known payload type into its Some(T) field for the layout
        // computation. The Some variant is always variant 0 (see
        // `mty-types/src/prelude.rs`).
        if let Some(pt) = payload_ty {
            if let Some(opt_adt) = self.prog.adts.iter().find(|a| a.name == "Option") {
                let mut adt = opt_adt.clone();
                if !adt.variants.is_empty() && !adt.variants[0].fields.is_empty() {
                    adt.variants[0].fields[0].ty = pt.clone();
                    let (off, _) = variant_field_offset(&adt, 0, 0, &self.prog.adts)
                        .unwrap_or((8, crate::layout::Layout::scalar(8)));
                    // Layout the synthetic Option[T] to size the slot.
                    let tmp_ty = IrTy::Adt(adt.adt, vec![pt.clone()]);
                    // Use type_size with a transient adts list that
                    // overrides our Option with the payload-specialised
                    // copy. The prelude Option in `self.prog.adts` has
                    // a generic param payload (Param), so its size
                    // there is wrong — but the override gives us the
                    // right answer for `T`.
                    let mut adts = self.prog.adts.clone();
                    if let Some(slot_in_list) = adts
                        .iter_mut()
                        .find(|a| a.adt == adt.adt && a.name == "Option")
                    {
                        *slot_in_list = adt.clone();
                    }
                    let size = type_size(&tmp_ty, &adts).max(8);
                    let align = type_align(&tmp_ty, &adts).max(8);
                    let log2_align = (align.next_power_of_two().trailing_zeros()).min(16) as u8;
                    let slot = self.b.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        slot_size(size),
                        log2_align,
                    ));
                    let addr = self.b.ins().stack_addr(ct::I64, slot, 0);
                    return Ok((addr, off));
                }
            }
        }
        // (3) Fallback: 16-byte slot, payload@8 — correct for any
        // scalar payload ≤8 bytes (i64/USize/F64/pointer/Str-handle/etc.).
        let slot = self.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            16,
            3, // log2(8)
        ));
        Ok((self.b.ins().stack_addr(ct::I64, slot, 0), 8))
    }

    /// `v.set(i, x)` — bounds-checked element store. v0.39 T3 surface;
    /// previously the only mutation path was `v[i] = x` lowering to a
    /// store-through-projection, which the v0.38 code didn't wire.
    fn emit_vec_set(
        &mut self,
        receiver: &Operand,
        args: &[Operand],
    ) -> CompileResult<cranelift_codegen::ir::Value> {
        let mf = MemFlags::trusted();
        let (elem_size, lds, elem_ty) = self.vec_elem_info(receiver);
        let hdr = self.vec_header(receiver)?;
        let len = self.b.ins().load(ct::I64, mf, hdr, Self::VEC_LEN_OFF);
        let data = self.b.ins().load(ct::I64, mf, hdr, Self::VEC_DATA_OFF);
        let idx = if let Some(a) = args.first() {
            let raw = self.eval_operand(a)?;
            self.coerce_to(raw, ct::I64)
        } else {
            self.b.ins().iconst(ct::I64, 0)
        };
        self.vec_bounds_check(idx, len)?;
        let val = if let Some(a) = args.get(1) {
            self.eval_operand(a)?
        } else {
            self.b.ins().iconst(ct::I64, 0)
        };
        let byte_off = self.b.ins().imul_imm(idx, elem_size);
        let slot = self.b.ins().iadd(data, byte_off);
        self.vec_store_elem(slot, val, elem_size, lds, elem_ty.as_ref());
        Ok(hdr)
    }

    /// `v.pop()` — returns `Option[T]`. `Some(last)` when non-empty
    /// (also decrements len); `None` when empty (no mutation).
    ///
    /// v0.41 T3 (L1 fix): pre-v0.41 returned a bare i64 scalar (0 on
    /// empty) — but the destination is `Option[T]` (aggregate), so the
    /// consuming `match` would dereference a scalar value as an
    /// aggregate address and segfault. Now we synthesise a real
    /// Option aggregate, matching the interpreter.
    ///
    /// v0.39 T3: guard the load behind a real branch (not a select).
    /// `Vec.new()` initialises `data` to null; a select-based load
    /// would still dereference null when `len == 0`, segfaulting on
    /// `let v: Vec[U8] = Vec.new(); v.pop()`. The new shape only loads
    /// from `data + new_len * elem_size` on the non-empty arm.
    fn emit_vec_pop(&mut self, receiver: &Operand) -> CompileResult<cranelift_codegen::ir::Value> {
        let mf = MemFlags::trusted();
        let (elem_size, lds, elem_ty) = self.vec_elem_info(receiver);
        let hdr = self.vec_header(receiver)?;
        let len = self.b.ins().load(ct::I64, mf, hdr, Self::VEC_LEN_OFF);
        let zero = self.b.ins().iconst(ct::I64, 0);

        let (slot_addr, payload_off) = self.alloc_option_slot_for(elem_ty.as_ref())?;
        let empty_block = self.b.create_block();
        let pop_block = self.b.create_block();
        let join_block = self.b.create_block();

        let is_empty = self
            .b
            .ins()
            .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, len, zero);
        self.b
            .ins()
            .brif(is_empty, empty_block, &[], pop_block, &[]);

        // --- empty_block: tag=1 (None), don't touch len/data ---
        self.b.switch_to_block(empty_block);
        self.b.seal_block(empty_block);
        let tag_none = self.b.ins().iconst(ct::I32, 1);
        self.b
            .ins()
            .store(mf, tag_none, slot_addr, TAG_OFFSET as i32);
        self.b.ins().jump(join_block, &[]);

        // --- pop_block: decrement len, load tail element, tag=0 (Some) ---
        self.b.switch_to_block(pop_block);
        self.b.seal_block(pop_block);
        let new_len = self.b.ins().iadd_imm(len, -1);
        self.b.ins().store(mf, new_len, hdr, Self::VEC_LEN_OFF);
        let data = self.b.ins().load(ct::I64, mf, hdr, Self::VEC_DATA_OFF);
        let byte_off = self.b.ins().imul_imm(new_len, elem_size);
        let slot = self.b.ins().iadd(data, byte_off);
        let elem = self.vec_load_elem(slot, elem_size, lds);
        let tag_some = self.b.ins().iconst(ct::I32, 0);
        self.b
            .ins()
            .store(mf, tag_some, slot_addr, TAG_OFFSET as i32);
        let payload_addr = self.b.ins().iadd_imm(slot_addr, payload_off as i64);
        if let Some(ty) = elem_ty.as_ref() {
            self.store_scalar(payload_addr, elem, ty)?;
        } else {
            self.b.ins().store(mf, elem, payload_addr, 0);
        }
        self.b.ins().jump(join_block, &[]);

        // --- join ---
        self.b.switch_to_block(join_block);
        self.b.seal_block(join_block);
        Ok(slot_addr)
    }

    /// `v.clear()` — reset len to 0 (keeps the allocation). Returns the
    /// header pointer for the capture-rebind shape.
    fn emit_vec_clear(
        &mut self,
        receiver: &Operand,
    ) -> CompileResult<cranelift_codegen::ir::Value> {
        let hdr = self.vec_header(receiver)?;
        let zero = self.b.ins().iconst(ct::I64, 0);
        self.b
            .ins()
            .store(MemFlags::trusted(), zero, hdr, Self::VEC_LEN_OFF);
        Ok(hdr)
    }

    /// v0.46 T4 — `dir_iter.next() -> Option<String>`. Receiver
    /// evaluates to the i64 handle from `mty_runtime_fs_dir_open`.
    /// Allocates a 24-byte (ptr,len,ok) Str slot, calls
    /// `mty_runtime_fs_dir_next(handle, slot)`, and wraps the result
    /// into an Option aggregate whose payload is a Str (the entry
    /// name). Returns the Option slot address — same shape any other
    /// `Option<String>`-returning method uses.
    ///
    /// Layout: the runtime ABI's (ptr,len,ok) triple already fits the
    /// Mighty `String` aggregate convention (ptr@+0, len@+8). The
    /// `ok` flag at +16 doubles as the runtime's "have an entry" bit
    /// and is what the codegen branches on to decide Some vs None —
    /// no separate return-value check needed.
    fn emit_dir_iter_next(
        &mut self,
        receiver: &Operand,
    ) -> CompileResult<cranelift_codegen::ir::Value> {
        let mf = MemFlags::trusted();
        // Receiver is the opaque-ADT handle: a scalar i64.
        let handle = self.eval_operand(receiver)?;
        let handle = self.coerce_to(handle, ct::I64);

        // Slot for the runtime's (ptr,len,ok) write — same 24-byte
        // shape as `read_dir_lines`/`read`/etc.
        let str_slot =
            self.b
                .create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 24, 3));
        let str_slot_addr = self.b.ins().stack_addr(ct::I64, str_slot, 0);

        // The dir_next runtime call's i32 return mirrors the slot's
        // ok flag; we keep it for diagnostic plumbing but branch on
        // the slot bit (which the codegen reads with a wider type so
        // sign-extension isn't a concern).
        let _ret = self
            .call_rt(
                "mty_runtime_fs_dir_next",
                &[handle, str_slot_addr],
                Some(ct::I32),
            )?
            .unwrap_or_else(|| self.b.ins().iconst(ct::I32, 0));

        // Option<String> aggregate slot — same allocator the rest of
        // the Option-returning method paths use.
        let str_ty = IrTy::String;
        let (opt_slot_addr, payload_off) = self.alloc_option_slot_for(Some(&str_ty))?;

        let ok = self.b.ins().load(ct::I32, mf, str_slot_addr, 16);
        let some_block = self.b.create_block();
        let none_block = self.b.create_block();
        let join_block = self.b.create_block();
        self.b.ins().brif(ok, some_block, &[], none_block, &[]);

        // --- some_block: copy (ptr,len) into the Option payload, tag=0 ---
        self.b.switch_to_block(some_block);
        self.b.seal_block(some_block);
        let ptr_word = self.b.ins().load(ct::I64, mf, str_slot_addr, 0);
        let len_word = self.b.ins().load(ct::I64, mf, str_slot_addr, 8);
        let payload_addr = self.b.ins().iadd_imm(opt_slot_addr, payload_off as i64);
        self.b.ins().store(mf, ptr_word, payload_addr, 0);
        self.b.ins().store(mf, len_word, payload_addr, 8);
        let tag_some = self.b.ins().iconst(ct::I32, 0);
        self.b
            .ins()
            .store(mf, tag_some, opt_slot_addr, TAG_OFFSET as i32);
        self.b.ins().jump(join_block, &[]);

        // --- none_block: tag=1, payload untouched ---
        self.b.switch_to_block(none_block);
        self.b.seal_block(none_block);
        let tag_none = self.b.ins().iconst(ct::I32, 1);
        self.b
            .ins()
            .store(mf, tag_none, opt_slot_addr, TAG_OFFSET as i32);
        self.b.ins().jump(join_block, &[]);

        // --- join ---
        self.b.switch_to_block(join_block);
        self.b.seal_block(join_block);
        Ok(opt_slot_addr)
    }

    /// v0.46 T4 — `dir_iter.close()`. Maps to
    /// `mty_runtime_fs_dir_close(handle)` which frees the boxed
    /// DirIterState. The runtime accepts handle==0 as a no-op so
    /// double-close / never-opened iterators don't trap. Returns 0 as
    /// the caller's value — the source-side method is `-> Unit`.
    ///
    /// v0.47 T4 — when the receiver is a bare local (`it.close()` with
    /// no projection), zero the local's i64 Variable after the runtime
    /// call. The auto-Drop pass injects a `Stmt::Drop(it)` at every
    /// fn-exit terminator; the trailing drop will reload the Variable
    /// and dispatch the runtime symbol with handle=0 — a no-op per the
    /// ABI contract. That's how explicit `.close()` + auto-Drop stays
    /// idempotent (no double-free of the `Box<DirIterState>`).
    fn emit_dir_iter_close(
        &mut self,
        receiver: &Operand,
    ) -> CompileResult<cranelift_codegen::ir::Value> {
        let handle = self.eval_operand(receiver)?;
        let handle = self.coerce_to(handle, ct::I64);
        self.call_rt("mty_runtime_fs_dir_close", &[handle], None)?;
        // v0.47 T4 — zero the receiver local so the auto-Drop at fn
        // exit dispatches with handle=0 (runtime no-op). Only safe for
        // bare-local receivers; deeper projections / temps don't
        // participate in the auto-Drop pass.
        if let Operand::Copy(p) | Operand::Move(p) = receiver {
            if p.proj.is_empty() {
                let var = self.ensure_var(p.local);
                let zero = self.b.ins().iconst(ct::I64, 0);
                self.b.def_var(var, zero);
            }
        }
        Ok(self.b.ins().iconst(ct::I64, 0))
    }

    /// v0.39 T3: byte-granular dynamic memcpy. Used by Vec growth so
    /// non-multiple-of-8 sizes (e.g. Vec[U8] with 5 elements) don't
    /// overshoot the source/dest buffers. Loops one byte at a time —
    /// fine for the small-vec sizes Vec growth re-copies (typeck
    /// already vetoes Vec[T] where T is large in tight loops, and the
    /// existing 8-byte loop assumed 8-byte slots which is no longer
    /// guaranteed in v0.39).
    fn emit_memcpy_dynamic_bytes(
        &mut self,
        dst: cranelift_codegen::ir::Value,
        src: cranelift_codegen::ir::Value,
        nbytes: cranelift_codegen::ir::Value,
    ) {
        let mf = MemFlags::trusted();
        let off_var = self.b.declare_var(ct::I64);
        let zero = self.b.ins().iconst(ct::I64, 0);
        self.b.def_var(off_var, zero);

        let header = self.b.create_block();
        let body = self.b.create_block();
        let done = self.b.create_block();
        self.b.ins().jump(header, &[]);

        self.b.switch_to_block(header);
        let off = self.b.use_var(off_var);
        let more = self.b.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::UnsignedLessThan,
            off,
            nbytes,
        );
        self.b.ins().brif(more, body, &[], done, &[]);

        self.b.switch_to_block(body);
        let off_b = self.b.use_var(off_var);
        let sptr = self.b.ins().iadd(src, off_b);
        let dptr = self.b.ins().iadd(dst, off_b);
        let byte = self.b.ins().load(ct::I8, mf, sptr, 0);
        self.b.ins().store(mf, byte, dptr, 0);
        let next = self.b.ins().iadd_imm(off_b, 1);
        self.b.def_var(off_var, next);
        self.b.ins().jump(header, &[]);

        self.b.seal_block(body);
        self.b.seal_block(header);
        self.b.switch_to_block(done);
        self.b.seal_block(done);
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

fn is_interpreter_hosted_stdlib(name: &str) -> bool {
    // v0.49 — stdlib modules that have NO native Cranelift codegen yet.
    // Returning `true` makes the EffectInvoke lowering report
    // `CodegenError::Unsupported`, which `mty run` turns into a
    // transparent fall-back to the interpreter (see `cmd::run::run`).
    //
    // Before this, an unimplemented stdlib call fell through to the
    // silent `mty_runtime_extern_call` stub, which returns 0 — and the
    // moment that 0 is dereferenced as an aggregate (a `Str`/`Bytes`/
    // struct result) the program SIGSEGV'd with no diagnostic
    // (`42_crypto_url` / `43_secure_session`). Routing to the
    // interpreter runs the program correctly instead of crashing.
    //
    // The natively-lowered surfaces are dispatched BEFORE this check
    // (`is_native_fs_method` → `emit_fs_call`; `is_native_crypto_encoding`
    // → `emit_crypto_encoding_call`), so naming a whole module here only
    // catches its NOT-yet-native methods: e.g. `crypto.sha256` is handled
    // natively and never reaches us, while `crypto.aes_gcm.encrypt`
    // falls back. As more methods go native, they simply stop reaching
    // this function — no edit needed here.
    let bare = name.strip_prefix("std.").unwrap_or(name);
    let module = bare.split('.').next().unwrap_or("");
    matches!(module, "url" | "uuid" | "regex" | "crypto" | "encoding")
}

/// v0.45 T1 — recognise the `std.fs.*` methods that the codegen
/// lowers natively. Keep this table in sync with `FsAbiKind::for_method`
/// and `runtime_imports::RUNTIME_IMPORTS`. Accept both the fully-
/// qualified `std.fs.*` form and the bare `fs.*` form a `use std.fs`
/// import produces (the wasm emitter accepts the same pair — see
/// `crates/mty-codegen-wasm/src/emit.rs`).
///
/// v0.46 T4: `read_dir` now ships as the iterator-handle shape
/// (`std.fs.DirIter`).
///
/// v0.47 T4: `read_dir_lines` is gone — the deprecated alias for the
/// v0.45 newline-joined Str behaviour has been removed from the
/// frontend dispatch table. The runtime symbol `mty_runtime_fs_read_dir`
/// stays live so v0.45-built binaries still link; the codegen just no
/// longer routes anything to it.
/// v0.49 — true for the `std.crypto.*` / `std.encoding.*` calls that
/// have a native Cranelift lowering (`emit_crypto_encoding_call`). Kept
/// in sync with that match.
fn is_native_crypto_encoding(full_name: &str) -> bool {
    let bare = full_name.strip_prefix("std.").unwrap_or(full_name);
    matches!(
        bare,
        "crypto.sha256"
            | "crypto.sha512"
            | "crypto.blake3"
            | "crypto.hmac_sha256"
            | "encoding.hex.encode"
            | "encoding.base64.encode"
            | "encoding.base64.encode_url_no_pad"
    )
}

fn is_native_fs_method(full_name: &str) -> bool {
    fn strip(s: &str) -> &str {
        s.strip_prefix("std.").unwrap_or(s)
    }
    matches!(
        strip(full_name),
        "fs.read"
            | "fs.read_file"
            | "fs.read_to_string"
            | "fs.read_dir"
            | "fs.list_dir"
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

/// v0.45 T1 — ABI shape selector for `std.fs.*` calls. Buckets each
/// method into one of four runtime-symbol families so `emit_fs_call`
/// can choose the right param/return convention.
///
/// v0.46 T4 added the `DirOpenHandle` shape for the new iterator
/// surface — `read_dir(p)` lowers to `mty_runtime_fs_dir_open(path)
/// -> i64 handle`, no dst slot.
///
/// v0.47 T4 drops the deprecated `read_dir_lines` arm — the v0.45
/// newline-joined Str shape is no longer exposed. The runtime symbol
/// `mty_runtime_fs_read_dir` stays live for v0.45-built-binary link
/// compatibility but the codegen no longer routes anything to it.
#[derive(Debug, Clone, Copy)]
enum FsAbiKind {
    /// (path_ptr, path_len, dst_slot) — runtime writes (ptr, len, ok)
    /// triple into the 24-byte slot; codegen returns the slot address.
    ReadStrSlot { symbol: &'static str },
    /// (path_ptr, path_len, data_ptr, data_len) -> i32 (1=ok, -errno).
    WriteI32 { symbol: &'static str },
    /// (path_ptr, path_len) -> i32 (1/0 for exists; 1=ok or -errno
    /// for the side-effecting verbs).
    PathI32 { symbol: &'static str },
    /// (path_ptr, path_len, dst_slot) -> i32; runtime writes the
    /// {size:u64, mtime_ms:i64, is_file:i8, is_dir:i8} record into a
    /// 24-byte slot. Codegen returns the slot address.
    MetadataSlot { symbol: &'static str },
    /// (path_ptr, path_len) -> i64; runtime returns an opaque
    /// `DirIter` handle (0 on open failure). Codegen routes the
    /// handle directly into the result place — no dst slot.
    DirOpenHandle { symbol: &'static str },
}

impl FsAbiKind {
    fn for_method(full_name: &str) -> Self {
        // Accept both `std.fs.*` and `fs.*` shapes — see
        // `is_native_fs_method` for the rationale.
        let bare = full_name.strip_prefix("std.").unwrap_or(full_name);
        match bare {
            "fs.read" | "fs.read_file" => FsAbiKind::ReadStrSlot {
                symbol: "mty_runtime_fs_read",
            },
            "fs.read_to_string" => FsAbiKind::ReadStrSlot {
                symbol: "mty_runtime_fs_read_to_string",
            },
            // v0.46 T4 — `read_dir` / `list_dir` open an iterator
            // handle. v0.47 T4: the old newline-joined behaviour
            // (`read_dir_lines`) is gone from this dispatch table.
            "fs.read_dir" | "fs.list_dir" => FsAbiKind::DirOpenHandle {
                symbol: "mty_runtime_fs_dir_open",
            },
            "fs.write" | "fs.write_file" => FsAbiKind::WriteI32 {
                symbol: "mty_runtime_fs_write",
            },
            "fs.write_string" => FsAbiKind::WriteI32 {
                symbol: "mty_runtime_fs_write_string",
            },
            "fs.append" => FsAbiKind::WriteI32 {
                symbol: "mty_runtime_fs_append",
            },
            "fs.exists" => FsAbiKind::PathI32 {
                symbol: "mty_runtime_fs_exists",
            },
            "fs.create_dir_all" => FsAbiKind::PathI32 {
                symbol: "mty_runtime_fs_create_dir_all",
            },
            "fs.remove_file" => FsAbiKind::PathI32 {
                symbol: "mty_runtime_fs_remove_file",
            },
            "fs.remove_dir_all" => FsAbiKind::PathI32 {
                symbol: "mty_runtime_fs_remove_dir_all",
            },
            "fs.metadata" | "fs.stat" => FsAbiKind::MetadataSlot {
                symbol: "mty_runtime_fs_metadata",
            },
            _ => unreachable!("FsAbiKind::for_method called on non-fs method {full_name}"),
        }
    }
}

fn effect_full_name(path: &[String], method: &str) -> String {
    if path.is_empty() {
        method.to_string()
    } else {
        format!("{}.{}", path.join("."), method)
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
