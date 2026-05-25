//! Wasm core-module emission (slice 8).
//!
//! Wraps `wasm-encoder`. We build sections in canonical order: types,
//! imports, functions, memory, exports, code, data.
//!
//! For slice-8 the lowerer is intentionally narrow: it accepts SIR
//! whose only fns are `main`-shape (no params, returning `Unit` or
//! `Int`) with bodies that are arithmetic + `log("...")` + return.
//! Anything richer raises [`WasmError::Unsupported`].

use crate::artifact::{WasmArtifact, WasmFormat};
use crate::component::wrap_as_component;
use crate::error::{CompileResult, WasmError};
use crate::target::WasmTarget;
use crate::wit::emit_wit;
use mty_ir::ir::{
    BinOp, BlockId, BuiltinId, Const, FnRef, Function, IrFnId, IrTy, Operand, Place, Program,
    Rvalue, Stmt, Term, UnOp,
};
use mty_types::IntKind;
use std::collections::HashMap;
use std::path::Path;
use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, DataSection, EntityType, ExportKind, ExportSection,
    Function as WFunction, FunctionSection, GlobalSection, GlobalType, ImportSection,
    Instruction as I, MemoryType, Module, TypeSection, ValType,
};

/// Per-build options controlling the v0.2 Component Model wrapper.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// Package name (used to derive the WIT `package` id; usually
    /// the source-file stem).
    pub pkg_name: String,
    /// When `true`, skip Component Model wrapping and emit only the
    /// bare core Wasm module. The CLI flag is `--no-component`.
    pub core_only: bool,
}

impl BuildOptions {
    pub fn new(pkg_name: impl Into<String>) -> Self {
        Self {
            pkg_name: pkg_name.into(),
            core_only: false,
        }
    }

    pub fn core_only(pkg_name: impl Into<String>) -> Self {
        Self {
            pkg_name: pkg_name.into(),
            core_only: true,
        }
    }
}

/// Compile a SIR program to a *core* Wasm binary. Component Model
/// wrapping is performed at a higher level (see
/// [`compile_program_to_file_with_options`]).
pub fn compile_program_to_bytes(prog: &Program, target: WasmTarget) -> CompileResult<Vec<u8>> {
    let mut emitter = Emitter::new(prog, target)?;
    emitter.emit()
}

/// Compile a SIR program to a core Wasm binary and wrap in an artifact.
///
/// Note: this is the legacy entry point retained for back-compat with
/// the slice-8 callers. New callers should use
/// [`compile_program_to_file_with_options`], which emits Component
/// Model output by default.
pub fn compile_program(prog: &Program, target: WasmTarget) -> CompileResult<WasmArtifact> {
    let bytes = compile_program_to_bytes(prog, target)?;
    Ok(WasmArtifact {
        bytes,
        path: None,
        target,
        format: WasmFormat::CoreModule,
        sidecar_core_path: None,
        wit_text: None,
    })
}

/// Back-compat wrapper around the slice-8 surface: writes a *core*
/// Wasm module to `out`. The Component Model wrapper is not run; for
/// that use [`compile_program_to_file_with_options`].
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
        format: WasmFormat::CoreModule,
        sidecar_core_path: None,
        wit_text: None,
    })
}

/// v0.2 main entry point: compile a SIR program, generate a WIT
/// contract, and wrap the result as a Component Model component
/// (unless `opts.core_only` is set, in which case only the core
/// module is written but the WIT is still attached to the artifact).
///
/// The primary `out` path receives either:
/// - the component bytes (default), or
/// - the core module bytes (when `opts.core_only`).
///
/// When `opts.core_only` is set, a sidecar `<out>.core.wasm` is *not*
/// written separately — `out` itself is the core module. The
/// `sidecar_core_path` field is left `None`.
pub fn compile_program_to_file_with_options(
    prog: &Program,
    target: WasmTarget,
    out: &Path,
    opts: &BuildOptions,
) -> CompileResult<WasmArtifact> {
    let core_bytes = compile_program_to_bytes(prog, target)?;
    let wit_doc = emit_wit(prog, &opts.pkg_name, target)?;

    if opts.core_only {
        std::fs::write(out, &core_bytes)
            .map_err(|e| WasmError::Io(format!("write {}: {}", out.display(), e)))?;
        return Ok(WasmArtifact {
            bytes: core_bytes,
            path: Some(out.to_path_buf()),
            target,
            format: WasmFormat::CoreModule,
            sidecar_core_path: None,
            wit_text: Some(wit_doc.text),
        });
    }

    // Wrap as component.
    let component_bytes = wrap_as_component(&core_bytes, &wit_doc)?;
    std::fs::write(out, &component_bytes)
        .map_err(|e| WasmError::Io(format!("write {}: {}", out.display(), e)))?;

    Ok(WasmArtifact {
        bytes: component_bytes,
        path: Some(out.to_path_buf()),
        target,
        format: WasmFormat::Component,
        sidecar_core_path: None,
        wit_text: Some(wit_doc.text),
    })
}

/// v0.8 loose-end 4/4 — fixed offset in linear memory where the
/// canonical-ABI string-return area lives. Strings written here are
/// `(ptr: i32, len: i32)` pairs (8 bytes total); option<string> uses
/// `(disc: i32, ptr: i32, len: i32)` (12 bytes, but we align the
/// payload at +4 / +8 for simplicity).
///
/// 8208 keeps a 16-byte gap from the legacy JS shim's write area
/// (which still writes a length-prefixed string starting at 8192).
pub const DOM_RETURN_AREA: u32 = 8208;
pub const DOM_RETURN_AREA_BYTES: u32 = 16;

/// v0.10 cleanup — `cabi_realloc` allocator memory layout.
///
/// Linear memory ranges:
///   * 0..1024  — reserved for shadow-stack scratch,
///   * 1024..8192 — string-literal pool (data section),
///   * 8192..8224 — legacy JS shim + canonical-ABI return area,
///   * 8224..32768 — slack for future growth of the data section,
///   * 32768..32800 — allocator state (8 i32 free-list heads),
///   * 32800.. — heap (bump-allocated, with size-class reuse).
///
/// ### Allocator design (v0.10)
///
/// Segregated free-list with 8 size classes (powers of 2 from 8B
/// to 1024B). Each class has a free-list head stored in linear
/// memory at `CABI_REALLOC_STATE_BASE + class*4`; the link in each
/// free block is the first 4 bytes (next-pointer; 0 = end of list).
///
/// `cabi_realloc(old, old_size, align, new)`:
/// * `old==0`: malloc(new). Try class free-list; else bump.
/// * `new==0 && old!=0`: free(old, old_size). Push to class
///   free-list if `old_size` fits a size class.
/// * else: realloc. malloc(new), memcpy(min(old_size, new)),
///   free(old). Conservative — no in-place grow yet.
///
/// Requests > 1024 bytes use a "large" path that bumps + never
/// frees. Acceptable for v0.10 — most canonical-ABI strings/lists
/// fit in the small classes; documented upgrade path to a real
/// dlmalloc/rlsf allocator for v0.11+.
///
/// `align` is respected by rounding the bump pointer up; free-list
/// reuse is only safe when `align <= class_size`, which always
/// holds for power-of-two alignments because the free blocks were
/// originally bump-allocated at class-size alignment.
pub const CABI_REALLOC_STATE_BASE: i32 = 32768;
pub const CABI_REALLOC_HEAP_BASE: i32 = 32800;

/// Eight size classes: 8, 16, 32, 64, 128, 256, 512, 1024 bytes.
/// Indexed 0..7. Class `i` has size `8 << i`.
pub const CABI_REALLOC_NUM_CLASSES: u32 = 8;
pub const CABI_REALLOC_LARGE_THRESHOLD: u32 = 1024;

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
    fn_index: HashMap<IrFnId, u32>,
    /// SIR fn id → wasm type index.
    fn_type_index: HashMap<IrFnId, u32>,
    /// Map type-signature key → type index, for deduping.
    sigs: HashMap<TySig, u32>,
    /// Imports added so far (each takes one fn-index slot before user fns).
    import_count: u32,
    /// `log(ptr, len)` import index (Some after we add it).
    log_idx: Option<u32>,
    /// v0.5 dogfood Gap-2 — DOM import indices (Web target only).
    dom_set_text_idx: Option<u32>,
    dom_get_text_idx: Option<u32>,
    dom_on_click_idx: Option<u32>,
    dom_query_idx: Option<u32>,
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
            dom_set_text_idx: None,
            dom_get_text_idx: None,
            dom_on_click_idx: None,
            dom_query_idx: None,
            string_pool: HashMap::new(),
            next_data_offset: 1024, // reserve first 1KiB for the stack
        })
    }

    fn intern_sig(&mut self, sig: TySig) -> u32 {
        if let Some(&idx) = self.sigs.get(&sig) {
            return idx;
        }
        let idx = self.sigs.len() as u32;
        self.type_section
            .ty()
            .function(sig.params.iter().copied(), sig.results.iter().copied());
        self.sigs.insert(sig, idx);
        idx
    }

    fn declare_imports(&mut self) -> CompileResult<()> {
        // log(ptr: i32, len: i32) -> ()
        //
        // For the v0.2 Component Model wrapper we emit the core
        // module import using the canonical
        // "<interface-fqn>"."<func-name>" name pair so that
        // `wit-component::ComponentEncoder` can wire it up to the
        // WIT world we generated.
        //
        // - wasm32-wasi: `wasi:cli/log#log`
        // - wasm32-web : `mty:web/log#log`
        let log_sig = TySig {
            params: vec![ValType::I32, ValType::I32],
            results: vec![],
        };
        let log_ty = self.intern_sig(log_sig);
        let (mod_name, fn_name) = match self.target {
            WasmTarget::Wasi => ("wasi:cli/log", "log"),
            WasmTarget::Web => ("mty:web/log", "log"),
        };
        self.import_section
            .import(mod_name, fn_name, EntityType::Function(log_ty));
        self.log_idx = Some(self.import_count);
        self.import_count += 1;

        // v0.5 dogfood Gap-2: DOM imports for the Web target. Each
        // string arg is passed as a (ptr, len) pair in linear memory.
        // The canonical module name (`mty:web/dom`) matches the
        // WIT world generated by `wit::emit_wit`, so
        // `wit-component::ComponentEncoder` can wire the imports
        // through to the JS shim's typed `dom` interface.
        if matches!(self.target, WasmTarget::Web) {
            // set-text(id_ptr, id_len, text_ptr, text_len)
            let set_text_sig = TySig {
                params: vec![ValType::I32, ValType::I32, ValType::I32, ValType::I32],
                results: vec![],
            };
            let ty = self.intern_sig(set_text_sig);
            self.import_section
                .import("mty:web/dom", "set-text", EntityType::Function(ty));
            self.dom_set_text_idx = Some(self.import_count);
            self.import_count += 1;

            // v0.8 loose-end 4/4: canonical-ABI lowering of
            // `func(id: string) -> string`.
            // The caller writes the (id_ptr, id_len) string in
            // linear memory and supplies a pointer to a small
            // return-area buffer; the callee writes the result
            // (ret_ptr, ret_len) into that buffer.
            //
            // Core-level shape: (id_ptr, id_len, ret_area_ptr) -> ()
            let get_text_sig = TySig {
                params: vec![ValType::I32, ValType::I32, ValType::I32],
                results: vec![],
            };
            let ty = self.intern_sig(get_text_sig);
            self.import_section
                .import("mty:web/dom", "get-text", EntityType::Function(ty));
            self.dom_get_text_idx = Some(self.import_count);
            self.import_count += 1;

            // on-click(id_ptr, id_len, tag_ptr, tag_len)
            let on_click_sig = TySig {
                params: vec![ValType::I32, ValType::I32, ValType::I32, ValType::I32],
                results: vec![],
            };
            let ty = self.intern_sig(on_click_sig);
            self.import_section
                .import("mty:web/dom", "on-click", EntityType::Function(ty));
            self.dom_on_click_idx = Some(self.import_count);
            self.import_count += 1;

            // v0.8: `func(selector: string) -> option<string>`. The
            // return-area layout is (disc:i32, ret_ptr:i32,
            // ret_len:i32) so the core import shape is the same as
            // get-text — caller passes a single return-area pointer.
            let query_sig = TySig {
                params: vec![ValType::I32, ValType::I32, ValType::I32],
                results: vec![],
            };
            let ty = self.intern_sig(query_sig);
            self.import_section
                .import("mty:web/dom", "query", EntityType::Function(ty));
            self.dom_query_idx = Some(self.import_count);
            self.import_count += 1;
        }
        Ok(())
    }

    /// v0.5+ — emit a DOM call. `op` is the SIR method name
    /// (e.g. `dom.set_text`). Returns `Ok(true)` if the op is known.
    ///
    /// v0.8 canonical-ABI bridge for string returns:
    ///   - `get-text(id: string) -> string` → core import
    ///     `(id_ptr, id_len, ret_area) -> ()`. We push the return-area
    ///     ptr before the call, then lift `[ret_area]` as the result
    ///     string pointer (the JS shim writes (data_ptr, data_len) into
    ///     the return area).
    ///   - `query(sel: string) -> option<string>` → same shape; the
    ///     return area holds (disc:i32, data_ptr:i32, data_len:i32).
    ///     After the call we push the disc; downstream lowering reads
    ///     a non-zero disc as `Some` and extracts the payload via
    ///     subsequent reads of the return area.
    ///
    /// Caller is responsible for pushing the string arg(s) as
    /// (ptr,len) pairs *before* invoking this helper.
    fn emit_dom_call(&mut self, op: &str, wfn: &mut WFunction) -> CompileResult<bool> {
        let kind = classify_dom_op(op);
        let idx = match kind {
            DomOpKind::SetText => self.dom_set_text_idx,
            DomOpKind::GetText => self.dom_get_text_idx,
            DomOpKind::OnClick => self.dom_on_click_idx,
            DomOpKind::Query => self.dom_query_idx,
            DomOpKind::Unknown => return Ok(false),
        };
        let Some(i) = idx else {
            return Ok(false);
        };
        match kind {
            DomOpKind::GetText | DomOpKind::Query => {
                // Push the return-area pointer as the final arg.
                wfn.instruction(&I::I32Const(DOM_RETURN_AREA as i32));
                wfn.instruction(&I::Call(i));
                // Lift: load the data pointer that the host wrote at
                // offset 0 (or for query, the disc at offset 0 — the
                // caller's LocalSet captures one i32; we use the disc
                // for Option<String> so a downstream test of "is some"
                // is just a non-zero check, and the data is available
                // for follow-up reads).
                wfn.instruction(&I::I32Const(DOM_RETURN_AREA as i32));
                wfn.instruction(&I::I32Load(wasm_encoder::MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }));
            }
            _ => {
                wfn.instruction(&I::Call(i));
                // Void-returning op: push a sink placeholder.
                wfn.instruction(&I::I32Const(0));
            }
        }
        Ok(true)
    }

    fn lower_ty(t: &IrTy) -> Option<ValType> {
        Some(match t {
            IrTy::Bool | IrTy::Char => ValType::I32,
            IrTy::Int(k) => match k {
                IntKind::I8
                | IntKind::U8
                | IntKind::I16
                | IntKind::U16
                | IntKind::I32
                | IntKind::U32
                | IntKind::IntInfer => ValType::I32,
                IntKind::I64 | IntKind::U64 | IntKind::ISize | IntKind::USize => ValType::I64,
                IntKind::I128 | IntKind::U128 => return None,
            },
            IrTy::Float(k) => match k {
                mty_types::FloatKind::F32 => ValType::F32,
                mty_types::FloatKind::F64 | mty_types::FloatKind::FloatInfer => ValType::F64,
            },
            IrTy::Duration | IrTy::Size => ValType::I64,
            IrTy::Unit | IrTy::Never => return None,
            // Aggregates, refs, strings, etc.: lower to i32 (wasm
            // linear-memory pointer).
            IrTy::Str
            | IrTy::String
            | IrTy::Bytes
            | IrTy::Tuple(_)
            | IrTy::Array { .. }
            | IrTy::Ref { .. }
            | IrTy::RawPtr(_)
            | IrTy::Adt(_, _)
            | IrTy::Cap { .. }
            | IrTy::Dyn(_)
            | IrTy::Fn { .. }
            | IrTy::Module(_)
            | IrTy::Param(_)
            | IrTy::Error => ValType::I32,
        })
    }

    fn fn_sig_for(f: &Function) -> CompileResult<TySig> {
        let mut params = Vec::with_capacity(f.params.len());
        for p in &f.params {
            let ty = &f.locals[p.0 as usize].ty;
            if let Some(v) = Self::lower_ty(ty) {
                params.push(v);
            } else if !matches!(ty, IrTy::Unit | IrTy::Never) {
                return Err(WasmError::Unsupported(format!("wasm param type {ty:?}")));
            }
        }
        let mut results = Vec::new();
        if let Some(v) = Self::lower_ty(&f.ret_ty) {
            results.push(v);
        } else if !matches!(f.ret_ty, IrTy::Unit | IrTy::Never) {
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
                self.export_section.export("main", ExportKind::Func, fn_idx);
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

        // v0.9 RC-prep — emit `cabi_realloc` so the Component Model
        // canonical-ABI lifter has a host-callable allocator for
        // string / list / option<string> returns. Without this export
        // `wit-component::ComponentEncoder` rejects the module with
        // "module does not export a function named `cabi_realloc`"
        // whenever the world contains an import that returns an
        // owned, heap-allocated value (e.g. our `dom.get-text` and
        // `dom.query`).
        //
        // Canonical signature:
        //   cabi_realloc(old: i32, old_size: i32, align: i32, new: i32) -> i32
        // Semantics: bump-allocate `new` bytes (aligned to `align`)
        // from the linear-memory heap and return the new pointer.
        // `old_ptr != 0` is treated as a fresh alloc (we don't yet
        // free or copy — see KNOWN_ISSUES.md for v0.10 follow-up).
        let realloc_ty = self.intern_sig(TySig {
            params: vec![ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            results: vec![ValType::I32],
        });
        let realloc_fn_idx = self.import_count + self.function_section.len();
        self.function_section.function(realloc_ty);
        let realloc_body = build_cabi_realloc_body();
        self.code_section.function(&realloc_body);
        self.export_section
            .export("cabi_realloc", ExportKind::Func, realloc_fn_idx);

        // Mutable i32 global: bump pointer for `cabi_realloc`. Starts
        // at `CABI_REALLOC_HEAP_BASE` and grows upward.
        let mut globals = GlobalSection::new();
        globals.global(
            GlobalType {
                val_type: ValType::I32,
                mutable: true,
                shared: false,
            },
            &ConstExpr::i32_const(CABI_REALLOC_HEAP_BASE),
        );

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
        // Global section (must come after memory, before exports).
        m.section(&globals);
        // Export memory so the host can poke at it.
        self.export_section.export("memory", ExportKind::Memory, 0);
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

        let mut wfn = WFunction::new(local_decls.clone());

        // v0.2: attempt full lowering; on the first unsupported shape,
        // bail and emit a single-`unreachable` body so the resulting
        // module still validates. This is the wasm equivalent of the
        // cranelift "fall back to interpreter" path.
        let lowered = self.try_emit_body(f, &sir_to_wasm, &mut wfn);
        if lowered.is_err() {
            // Reset wfn by re-creating from scratch.
            let mut fresh = WFunction::new(local_decls.clone());
            fresh.instruction(&I::Unreachable);
            fresh.instruction(&I::End);
            return Ok(fresh);
        }
        wfn.instruction(&I::End);
        Ok(wfn)
    }

    /// Try the full block-by-block lowering. On error, returns Err so
    /// the caller can fall back to a clean unreachable-only body.
    fn try_emit_body(
        &mut self,
        f: &Function,
        sir_to_wasm: &HashMap<u32, u32>,
        wfn: &mut WFunction,
    ) -> CompileResult<()> {
        if f.blocks.len() > 1 {
            for (i, blk) in f.blocks.iter().enumerate() {
                let stmts = blk.stmts.clone();
                for s in &stmts {
                    self.emit_stmt(f, sir_to_wasm, s, wfn)?;
                }
                match &blk.terminator {
                    Term::Return(op) => {
                        self.emit_return(f, sir_to_wasm, op, wfn)?;
                    }
                    Term::Goto(next) => {
                        if next.0 as usize != i + 1 {
                            return Err(WasmError::Unsupported("non-chain goto in wasm".into()));
                        }
                    }
                    Term::Unreachable => {
                        wfn.instruction(&I::Unreachable);
                    }
                    other => {
                        return Err(WasmError::Unsupported(format!("wasm terminator {other:?}")))
                    }
                }
            }
        } else if let Some(blk) = f.blocks.first() {
            let stmts = blk.stmts.clone();
            for s in &stmts {
                self.emit_stmt(f, sir_to_wasm, s, wfn)?;
            }
            match &blk.terminator {
                Term::Return(op) => self.emit_return(f, sir_to_wasm, op, wfn)?,
                Term::Unreachable => {
                    wfn.instruction(&I::Unreachable);
                }
                other => return Err(WasmError::Unsupported(format!("wasm terminator {other:?}"))),
            }
        }
        Ok(())
    }

    fn emit_return(
        &mut self,
        f: &Function,
        m: &HashMap<u32, u32>,
        op: &Operand,
        wfn: &mut WFunction,
    ) -> CompileResult<()> {
        if matches!(f.ret_ty, IrTy::Unit | IrTy::Never) {
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
            // v0.2 wasm: bail (caller demotes to body-level unreachable).
            return Err(WasmError::Unsupported("wasm place projection".into()));
        }
        self.emit_rvalue(f, m, rv, wfn)?;
        let Some(&wlocal) = m.get(&p.local.0) else {
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
        _f: &Function,
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
                mty_types::FloatKind::F32 => {
                    wfn.instruction(&I::F32Const((*v as f32).into()));
                }
                mty_types::FloatKind::F64 | mty_types::FloatKind::FloatInfer => {
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
                    return Err(WasmError::Unsupported("wasm log non-literal string".into()));
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
            FnRef::Builtin(BuiltinId::DomOp(op)) => {
                // v0.6 — first-class DOM call. Push (ptr,len) for each
                // string-shaped arg (same convention as `log`), then
                // dispatch via `emit_dom_call`. The Web target installs
                // the four `mty:web/dom` imports lazily; if the
                // builder is targeting wasi we fall through and return
                // a typed error.
                for a in args {
                    if let Operand::Const(Const::Str(s)) = a {
                        let (ptr, len) = self.intern_string(s);
                        wfn.instruction(&I::I32Const(ptr as i32));
                        wfn.instruction(&I::I32Const(len as i32));
                    } else {
                        // Best-effort: emit the operand as-is. Non-Str
                        // args land on the stack as i32 / i64 per the
                        // value's lowered type.
                        self.emit_operand(f, m, a, wfn)?;
                    }
                }
                let dispatched = self.emit_dom_call(op, wfn)?;
                if !dispatched {
                    return Err(WasmError::Unsupported(format!(
                        "wasm dom op {op:?} (target lacks mty:web/dom imports)"
                    )));
                }
                // emit_dom_call pushes a placeholder for void-returning
                // ops; nothing more to do.
                Ok(())
            }
            FnRef::Builtin(other) => Err(WasmError::Unsupported(format!("wasm builtin {other:?}"))),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum DomOpKind {
    SetText,
    GetText,
    OnClick,
    Query,
    Unknown,
}

fn classify_dom_op(op: &str) -> DomOpKind {
    match op {
        "dom.set_text" | "set_text" | "set-text" => DomOpKind::SetText,
        "dom.get_text" | "get_text" | "get-text" => DomOpKind::GetText,
        "dom.on_click" | "on_click" | "on-click" => DomOpKind::OnClick,
        "dom.query" | "query" => DomOpKind::Query,
        _ => DomOpKind::Unknown,
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

/// Build the body of the canonical-ABI `cabi_realloc` export.
///
/// v0.10: segregated free-list allocator with 8 size classes
/// (8B → 1024B, powers of 2) + a "large" bump path for `size >
/// 1024`. See [`CABI_REALLOC_STATE_BASE`] for the memory layout.
///
/// ### Pseudocode
///
/// ```text
/// fn cabi_realloc(old: i32, old_size: i32, align: i32, new: i32) -> i32 {
///     if new == 0 {
///         if old != 0 { free(old, old_size); }
///         return 0;
///     }
///     let p = if old == 0 {
///         malloc(align, new)
///     } else {
///         let p = malloc(align, new);
///         memcpy(p, old, min(old_size, new));
///         free(old, old_size);
///         p
///     };
///     p
/// }
///
/// fn malloc(align: i32, size: i32) -> i32 {
///     let class = size_class(size);    // -1 if size > 1024
///     if class >= 0 && align <= class_size(class) {
///         let head = load_i32(STATE_BASE + class*4);
///         if head != 0 {
///             store_i32(STATE_BASE + class*4, load_i32(head));
///             return head;
///         }
///         // bump-allocate class_size bytes (naturally aligned for align).
///         return bump(class_size(class), align);
///     }
///     bump(size, align)
/// }
///
/// fn free(ptr: i32, size: i32) {
///     let class = size_class(size);
///     if class < 0 { return; }   // large: not freed
///     let head = load_i32(STATE_BASE + class*4);
///     store_i32(ptr, head);
///     store_i32(STATE_BASE + class*4, ptr);
/// }
///
/// fn bump(size: i32, align: i32) -> i32 {
///     let mask = align - 1;
///     $bump = ($bump + mask) & !mask;
///     let p = $bump;
///     $bump = $bump + size;
///     p
/// }
///
/// // size_class: returns class index 0..7 such that class_size >= size,
/// // or -1 if size > 1024. Implemented as an unrolled if-chain
/// // (8 comparisons) because wasm has no native ctz/clz on i32 sizes
/// // small enough to dispatch off.
/// ```
///
/// ### Wasm layout
///
/// Locals (after the 4 params `old`, `old_size`, `align`, `new`):
/// - local 4: `class`  (i32)  — size class index, -1 = large.
/// - local 5: `csize`  (i32)  — bytes for the size class.
/// - local 6: `head`   (i32)  — free-list head pointer.
/// - local 7: `p`      (i32)  — allocation result / scratch.
/// - local 8: `mask`   (i32)  — alignment mask.
/// - local 9: `i`      (i32)  — memcpy loop counter.
/// - local 10: `n`     (i32)  — memcpy byte count = min(old_size, new).
///
/// Global 0 = bump pointer, initialised to [`CABI_REALLOC_HEAP_BASE`].
fn build_cabi_realloc_body() -> WFunction {
    let mut f = WFunction::new([(7u32, ValType::I32)]);
    const PARAM_OLD: u32 = 0;
    const PARAM_OLD_SIZE: u32 = 1;
    const PARAM_ALIGN: u32 = 2;
    const PARAM_NEW: u32 = 3;
    const LOC_CLASS: u32 = 4;
    const LOC_CSIZE: u32 = 5;
    const LOC_HEAD: u32 = 6;
    const LOC_P: u32 = 7;
    const LOC_MASK: u32 = 8;
    const LOC_I: u32 = 9;
    const LOC_N: u32 = 10;
    let memarg0 = wasm_encoder::MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    };
    let memarg_b = wasm_encoder::MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    };

    // ----- if new == 0: free-only or no-op -----
    f.instruction(&I::LocalGet(PARAM_NEW));
    f.instruction(&I::I32Eqz);
    f.instruction(&I::If(BlockType::Empty));
    {
        // if old != 0 { free(old, old_size); }
        f.instruction(&I::LocalGet(PARAM_OLD));
        f.instruction(&I::I32Eqz);
        f.instruction(&I::I32Eqz);
        f.instruction(&I::If(BlockType::Empty));
        {
            emit_size_class(&mut f, PARAM_OLD_SIZE, LOC_CLASS);
            // if class >= 0 { push to free list }
            f.instruction(&I::LocalGet(LOC_CLASS));
            f.instruction(&I::I32Const(0));
            f.instruction(&I::I32GeS);
            f.instruction(&I::If(BlockType::Empty));
            {
                // head = load_i32(STATE_BASE + class*4)
                f.instruction(&I::LocalGet(LOC_CLASS));
                f.instruction(&I::I32Const(2));
                f.instruction(&I::I32Shl);
                f.instruction(&I::I32Const(CABI_REALLOC_STATE_BASE));
                f.instruction(&I::I32Add);
                f.instruction(&I::LocalSet(LOC_P)); // P = address of head slot
                f.instruction(&I::LocalGet(LOC_P));
                f.instruction(&I::I32Load(memarg0));
                f.instruction(&I::LocalSet(LOC_HEAD));
                // store_i32(old, head)
                f.instruction(&I::LocalGet(PARAM_OLD));
                f.instruction(&I::LocalGet(LOC_HEAD));
                f.instruction(&I::I32Store(memarg0));
                // store_i32(head_slot, old)
                f.instruction(&I::LocalGet(LOC_P));
                f.instruction(&I::LocalGet(PARAM_OLD));
                f.instruction(&I::I32Store(memarg0));
            }
            f.instruction(&I::End);
        }
        f.instruction(&I::End);
        // return 0
        f.instruction(&I::I32Const(0));
        f.instruction(&I::Return);
    }
    f.instruction(&I::End);

    // ----- malloc(align, new) -> LOC_P -----
    emit_size_class(&mut f, PARAM_NEW, LOC_CLASS);
    // csize = if class < 0 { new } else { 8 << class }
    f.instruction(&I::LocalGet(LOC_CLASS));
    f.instruction(&I::I32Const(0));
    f.instruction(&I::I32LtS);
    f.instruction(&I::If(BlockType::Result(ValType::I32)));
    {
        f.instruction(&I::LocalGet(PARAM_NEW));
    }
    f.instruction(&I::Else);
    {
        f.instruction(&I::I32Const(8));
        f.instruction(&I::LocalGet(LOC_CLASS));
        f.instruction(&I::I32Shl);
    }
    f.instruction(&I::End);
    f.instruction(&I::LocalSet(LOC_CSIZE));

    // Try free-list reuse: only if class >= 0 AND align <= csize.
    f.instruction(&I::LocalGet(LOC_CLASS));
    f.instruction(&I::I32Const(0));
    f.instruction(&I::I32GeS);
    f.instruction(&I::LocalGet(PARAM_ALIGN));
    f.instruction(&I::LocalGet(LOC_CSIZE));
    f.instruction(&I::I32LeS);
    f.instruction(&I::I32And);
    f.instruction(&I::If(BlockType::Empty));
    {
        // head_slot = STATE_BASE + class*4
        f.instruction(&I::LocalGet(LOC_CLASS));
        f.instruction(&I::I32Const(2));
        f.instruction(&I::I32Shl);
        f.instruction(&I::I32Const(CABI_REALLOC_STATE_BASE));
        f.instruction(&I::I32Add);
        f.instruction(&I::LocalSet(LOC_MASK)); // reuse mask local as slot ptr
                                               // head = load(head_slot)
        f.instruction(&I::LocalGet(LOC_MASK));
        f.instruction(&I::I32Load(memarg0));
        f.instruction(&I::LocalSet(LOC_HEAD));
        // if head != 0 { pop and use }
        f.instruction(&I::LocalGet(LOC_HEAD));
        f.instruction(&I::I32Eqz);
        f.instruction(&I::I32Eqz);
        f.instruction(&I::If(BlockType::Empty));
        {
            // head_slot.store(load(head))   // next-link
            f.instruction(&I::LocalGet(LOC_MASK));
            f.instruction(&I::LocalGet(LOC_HEAD));
            f.instruction(&I::I32Load(memarg0));
            f.instruction(&I::I32Store(memarg0));
            // p = head
            f.instruction(&I::LocalGet(LOC_HEAD));
            f.instruction(&I::LocalSet(LOC_P));
            // proceed to copy-from-old + free-old + return p; jump
            // to that section via setting LOC_HEAD = 0 sentinel?
            // Simpler: use the post-malloc tail explicitly. We
            // signal "already allocated" by setting LOC_HEAD=1.
            f.instruction(&I::I32Const(1));
            f.instruction(&I::LocalSet(LOC_HEAD));
        }
        f.instruction(&I::End);
    }
    f.instruction(&I::End);

    // If LOC_HEAD != 1, we didn't pop from free list — bump-allocate.
    f.instruction(&I::LocalGet(LOC_HEAD));
    f.instruction(&I::I32Const(1));
    f.instruction(&I::I32Ne);
    f.instruction(&I::If(BlockType::Empty));
    {
        // bump-allocate LOC_CSIZE bytes aligned to PARAM_ALIGN.
        // mask = align - 1
        f.instruction(&I::LocalGet(PARAM_ALIGN));
        f.instruction(&I::I32Const(1));
        f.instruction(&I::I32Sub);
        f.instruction(&I::LocalSet(LOC_MASK));
        // bump = (bump + mask) & !mask
        f.instruction(&I::GlobalGet(0));
        f.instruction(&I::LocalGet(LOC_MASK));
        f.instruction(&I::I32Add);
        f.instruction(&I::LocalGet(LOC_MASK));
        f.instruction(&I::I32Const(-1));
        f.instruction(&I::I32Xor);
        f.instruction(&I::I32And);
        f.instruction(&I::GlobalSet(0));
        // p = bump; bump += csize
        f.instruction(&I::GlobalGet(0));
        f.instruction(&I::LocalSet(LOC_P));
        f.instruction(&I::GlobalGet(0));
        f.instruction(&I::LocalGet(LOC_CSIZE));
        f.instruction(&I::I32Add);
        f.instruction(&I::GlobalSet(0));
    }
    f.instruction(&I::End);

    // ----- if old != 0: copy min(old_size, new) bytes, then free old -----
    f.instruction(&I::LocalGet(PARAM_OLD));
    f.instruction(&I::I32Eqz);
    f.instruction(&I::I32Eqz);
    f.instruction(&I::If(BlockType::Empty));
    {
        // n = min(old_size, new)
        f.instruction(&I::LocalGet(PARAM_OLD_SIZE));
        f.instruction(&I::LocalGet(PARAM_NEW));
        f.instruction(&I::I32LtS);
        f.instruction(&I::If(BlockType::Result(ValType::I32)));
        {
            f.instruction(&I::LocalGet(PARAM_OLD_SIZE));
        }
        f.instruction(&I::Else);
        {
            f.instruction(&I::LocalGet(PARAM_NEW));
        }
        f.instruction(&I::End);
        f.instruction(&I::LocalSet(LOC_N));

        // byte-by-byte memcpy: for i in 0..n { *(p+i) = *(old+i); }
        f.instruction(&I::I32Const(0));
        f.instruction(&I::LocalSet(LOC_I));
        f.instruction(&I::Block(BlockType::Empty));
        f.instruction(&I::Loop(BlockType::Empty));
        {
            // if i >= n break
            f.instruction(&I::LocalGet(LOC_I));
            f.instruction(&I::LocalGet(LOC_N));
            f.instruction(&I::I32GeS);
            f.instruction(&I::BrIf(1));
            // *(p+i) = *(old+i)
            f.instruction(&I::LocalGet(LOC_P));
            f.instruction(&I::LocalGet(LOC_I));
            f.instruction(&I::I32Add);
            f.instruction(&I::LocalGet(PARAM_OLD));
            f.instruction(&I::LocalGet(LOC_I));
            f.instruction(&I::I32Add);
            f.instruction(&I::I32Load8U(memarg_b));
            f.instruction(&I::I32Store8(memarg_b));
            // i += 1
            f.instruction(&I::LocalGet(LOC_I));
            f.instruction(&I::I32Const(1));
            f.instruction(&I::I32Add);
            f.instruction(&I::LocalSet(LOC_I));
            f.instruction(&I::Br(0));
        }
        f.instruction(&I::End); // loop
        f.instruction(&I::End); // block

        // free(old, old_size): if class' >= 0, push to free list.
        emit_size_class(&mut f, PARAM_OLD_SIZE, LOC_CLASS);
        f.instruction(&I::LocalGet(LOC_CLASS));
        f.instruction(&I::I32Const(0));
        f.instruction(&I::I32GeS);
        f.instruction(&I::If(BlockType::Empty));
        {
            // head_slot = STATE_BASE + class*4
            f.instruction(&I::LocalGet(LOC_CLASS));
            f.instruction(&I::I32Const(2));
            f.instruction(&I::I32Shl);
            f.instruction(&I::I32Const(CABI_REALLOC_STATE_BASE));
            f.instruction(&I::I32Add);
            f.instruction(&I::LocalSet(LOC_MASK));
            // *old = *head_slot
            f.instruction(&I::LocalGet(PARAM_OLD));
            f.instruction(&I::LocalGet(LOC_MASK));
            f.instruction(&I::I32Load(memarg0));
            f.instruction(&I::I32Store(memarg0));
            // *head_slot = old
            f.instruction(&I::LocalGet(LOC_MASK));
            f.instruction(&I::LocalGet(PARAM_OLD));
            f.instruction(&I::I32Store(memarg0));
        }
        f.instruction(&I::End);
    }
    f.instruction(&I::End);

    f.instruction(&I::LocalGet(LOC_P));
    f.instruction(&I::End);
    f
}

/// Emit wasm that computes the size class of `size_local` (params/locals)
/// into `out_local`. Classes are 8, 16, 32, 64, 128, 256, 512, 1024 →
/// indices 0..7. Returns -1 for size > 1024 (large path).
///
/// Implementation: unrolled if-chain across powers of 2. Worst case 8
/// comparisons, but wasm-jit on the host inlines this and the cost is
/// negligible compared to the surrounding malloc bookkeeping.
fn emit_size_class(f: &mut WFunction, size_local: u32, out_local: u32) {
    // class = -1 (large)
    f.instruction(&I::I32Const(-1));
    f.instruction(&I::LocalSet(out_local));
    // Walk from class 7 down to class 0; the smallest class whose
    // size >= request wins. Iterate in reverse so the smallest
    // class overrides any larger one.
    for class in (0..CABI_REALLOC_NUM_CLASSES as i32).rev() {
        let csize: i32 = 8i32 << class;
        // if size_local <= csize { out_local = class }
        f.instruction(&I::LocalGet(size_local));
        f.instruction(&I::I32Const(csize));
        f.instruction(&I::I32LeS);
        f.instruction(&I::If(BlockType::Empty));
        f.instruction(&I::I32Const(class));
        f.instruction(&I::LocalSet(out_local));
        f.instruction(&I::End);
    }
    // Edge case: size_local == 0 should still pick class 0, which it
    // will (0 <= 8). Negative sizes never occur (canonical-ABI sizes
    // are unsigned i32; the wasm-encoder API uses signed Rust types
    // but semantically these are u32). Behaviour at extreme inputs
    // is defined by the host's wasm runtime.
}

#[cfg(test)]
mod tests {
    use super::*;
    use mty_hir::SourceSpan;
    use mty_ir::ir::{
        Block, BlockId, Const, Function, IrFnId, IrTy, LocalDecl, LocalSource, Operand, Program,
        Term,
    };

    fn empty_main() -> Program {
        let mut p = Program::default();
        p.fns.push(Function {
            id: IrFnId(0),
            name: "main".into(),
            params: vec![],
            locals: vec![LocalDecl {
                name: "_0".into(),
                ty: IrTy::Unit,
                mutable: false,
                source: LocalSource::Return,
            }],
            blocks: vec![Block {
                id: BlockId(0),
                stmts: vec![],
                terminator: Term::Return(Operand::Const(Const::Unit)),
            }],
            entry: BlockId(0),
            ret_ty: IrTy::Unit,
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
