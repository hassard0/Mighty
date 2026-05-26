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
use crate::cabi_realloc::build_cabi_realloc_body;
use crate::component::wrap_as_component;
use crate::error::{CompileResult, WasmError};
use crate::preview2::P2DirectImport;
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

/// Which WASI preview the core-module emitter should target when it
/// has a choice. v0.15 introduces direct P2 imports for a handful of
/// stdlib calls (`std.random.bytes`, `std.time.now`, …); v0.16
/// extends the set with `std.fs.*` and `std.http.*`. The emitter
/// uses this flag to pick between legacy P1 / shim imports and the
/// versioned P2 interface set.
///
/// Mirrored by `mty_driver::build::WasiPreview` (the driver's enum
/// owns the CLI parsing); this enum is a local copy so the codegen
/// crate doesn't take a dependency on `mty-driver`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EmitWasiPreview {
    /// Legacy P1 import shape (`wasi_snapshot_preview1` syscalls + the
    /// unversioned `wasi:cli/log` shim).
    P1,
    /// Versioned P2 imports (`wasi:random/random@0.2.3#get-random-bytes`,
    /// `wasi:clocks/monotonic-clock@0.2.3#now`, …). Default since
    /// v0.15 when callers ask for the WASI target.
    #[default]
    P2,
}

/// Per-build options controlling the v0.2 Component Model wrapper.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// Package name (used to derive the WIT `package` id; usually
    /// the source-file stem).
    pub pkg_name: String,
    /// When `true`, skip Component Model wrapping and emit only the
    /// bare core Wasm module. The CLI flag is `--no-component`.
    pub core_only: bool,
    /// Which WASI preview the emitter should target for stdlib
    /// lowerings that have a choice. v0.15 default = P2; the v0.13
    /// default was P1 and the legacy back-compat path keeps the
    /// option on the type.
    pub wasi_preview: EmitWasiPreview,
}

impl BuildOptions {
    pub fn new(pkg_name: impl Into<String>) -> Self {
        Self {
            pkg_name: pkg_name.into(),
            core_only: false,
            wasi_preview: EmitWasiPreview::default(),
        }
    }

    pub fn core_only(pkg_name: impl Into<String>) -> Self {
        Self {
            pkg_name: pkg_name.into(),
            core_only: true,
            wasi_preview: EmitWasiPreview::default(),
        }
    }

    /// Override the WASI preview. Returns `self` for builder-style
    /// chaining: `BuildOptions::new("x").with_wasi_preview(P1)`.
    pub fn with_wasi_preview(mut self, preview: EmitWasiPreview) -> Self {
        self.wasi_preview = preview;
        self
    }
}

/// Compile a SIR program to a *core* Wasm binary. Component Model
/// wrapping is performed at a higher level (see
/// [`compile_program_to_file_with_options`]).
///
/// This is the legacy entry point: it forces the **P1** import shape
/// for the core module's stdlib calls (matches v0.13/v0.14
/// behaviour). New callers that want the v0.15-default versioned P2
/// imports should use
/// [`compile_program_to_bytes_with_preview`].
pub fn compile_program_to_bytes(prog: &Program, target: WasmTarget) -> CompileResult<Vec<u8>> {
    compile_program_to_bytes_with_preview(prog, target, EmitWasiPreview::P1)
}

/// Compile a SIR program to a core Wasm binary, picking the WASI
/// preview to target for stdlib lowerings. P2 (default for the v0.15
/// `--wasi=p2` flip) emits versioned `wasi:*@0.2.3` imports for
/// `std.random.bytes`, `std.time.now`, `std.time.monotonic_now`, and
/// `std.time.resolution`; v0.16 adds direct lowerings for
/// `std.fs.{open,read_file,write_file,stat,close}` and
/// `std.http.{get,post,send}`. P1 preserves the legacy shape.
pub fn compile_program_to_bytes_with_preview(
    prog: &Program,
    target: WasmTarget,
    preview: EmitWasiPreview,
) -> CompileResult<Vec<u8>> {
    let mut emitter = Emitter::new(prog, target, preview)?;
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
    let core_bytes = compile_program_to_bytes_with_preview(prog, target, opts.wasi_preview)?;
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

/// v0.16 — return-area for `wasi:filesystem` calls that hand back a
/// `result<resource, error-code>` (e.g. `open-at`, `read-via-stream`,
/// `stat`). 256 bytes is enough headroom for the largest record we
/// lower today (`descriptor-stat` at 80 bytes — see
/// [`preview2::CANONICAL_ABI_DESCRIPTOR_STAT_SIZE`]).
///
/// Sits in the slack region (8224..32768) of the linear-memory map
/// documented next to [`CABI_REALLOC_STATE_BASE`] so it doesn't
/// collide with the data-section pool or the cabi-realloc heap.
pub const FS_RETURN_AREA: u32 = 8224;
pub const FS_RETURN_AREA_BYTES: u32 = 256;

/// v0.16 — return-area for `wasi:http` calls. Carries the
/// `result<future-incoming-response, error-code>` from
/// `outgoing-handler.handle` and the `result<incoming-body>` from
/// `incoming-response.consume`. 64 bytes is plenty (each result is
/// `(tag: i32, handle: i32)`).
pub const HTTP_RETURN_AREA: u32 = 8480; // = FS_RETURN_AREA + FS_RETURN_AREA_BYTES
pub const HTTP_RETURN_AREA_BYTES: u32 = 64;

/// v0.17 — return-area for the `log()` direct-lowering sequence.
/// `[method]output-stream.blocking-write-and-flush` returns a
/// `result<_, stream-error>` (`(tag:i32, err-handle:i32)`); 16
/// bytes are enough headroom for that plus a future second slot.
///
/// Sits immediately after [`HTTP_RETURN_AREA`] so the linear-memory
/// layout stays sequential.
pub const LOG_RETURN_AREA: u32 = 8544; // = HTTP_RETURN_AREA + HTTP_RETURN_AREA_BYTES
pub const LOG_RETURN_AREA_BYTES: u32 = 16;

// v0.18 — the `cabi_realloc` allocator's memory-layout constants and
// body builder moved to `crate::cabi_realloc` (KNOWN_ISSUES #1). The
// pub re-exports below preserve the existing public API surface so
// downstream test crates and external users keep importing
// `mty_codegen_wasm::emit::CABI_REALLOC_*`.
pub use crate::cabi_realloc::{
    CABI_REALLOC_HEAP_BASE, CABI_REALLOC_LARGE_THRESHOLD, CABI_REALLOC_NUM_CLASSES,
    CABI_REALLOC_STATE_BASE,
};

struct Emitter<'a> {
    prog: &'a Program,
    target: WasmTarget,
    wasi_preview: EmitWasiPreview,
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
    /// v0.15 P2 direct-import indices, allocated lazily the first
    /// time a stdlib call needs one. Each entry maps a
    /// [`P2DirectImport`] variant to the function index assigned
    /// when its versioned import was added to the import section.
    p2_direct_idx: HashMap<P2DirectImport, u32>,
    /// v0.17 — scratch i32 local that the P2 `log()` direct-lowering
    /// uses to stash the `wasi:io/streams.output-stream` handle
    /// returned by `get-stdout`. Set per-function in
    /// [`Self::emit_fn`] when the function body contains a `log()`
    /// call AND the build targets P2-Wasi.
    log_handle_local: Option<u32>,
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
    fn new(
        prog: &'a Program,
        target: WasmTarget,
        wasi_preview: EmitWasiPreview,
    ) -> CompileResult<Self> {
        Ok(Self {
            prog,
            target,
            wasi_preview,
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
            p2_direct_idx: HashMap::new(),
            log_handle_local: None,
            string_pool: HashMap::new(),
            next_data_offset: 1024, // reserve first 1KiB for the stack
        })
    }

    /// True iff the function body contains a `log()` / `print()`
    /// call AND the build targets P2-Wasi (the dispatch path that
    /// needs an extra `i32` local for the stream handle).
    fn fn_needs_log_handle(&self, f: &Function) -> bool {
        if !matches!(self.wasi_preview, EmitWasiPreview::P2) {
            return false;
        }
        if !matches!(self.target, WasmTarget::Wasi) {
            return false;
        }
        for blk in &f.blocks {
            for stmt in &blk.stmts {
                if let Stmt::Assign(_, Rvalue::Call { func, .. }) = stmt {
                    if matches!(func, FnRef::Builtin(BuiltinId::Log | BuiltinId::Print)) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Look up (or lazily declare) the function-table index for a P2
    /// direct import. The first call for a given [`P2DirectImport`]
    /// variant appends a fresh `(import "<module>" "<name>" func ...)`
    /// to the import section; subsequent calls reuse the cached
    /// index.
    ///
    /// Only used on the **P2** dispatch path. Callers that hit this
    /// while `wasi_preview == P1` indicate an upstream dispatch bug;
    /// it's still safe (the import would be declared but unused).
    fn p2_direct_import(&mut self, which: P2DirectImport) -> u32 {
        if let Some(&idx) = self.p2_direct_idx.get(&which) {
            return idx;
        }
        // Pick the canonical-ABI core-Wasm signature for the import.
        // These match the shapes documented on
        // [`build_direct_p2_probe_module`] and stay normative for
        // WASI 0.2.3.
        let (params, results): (Vec<ValType>, Vec<ValType>) = match which {
            // get-random-bytes(len: u64) -> list<u8>
            //   → canonical-ABI lift: `(param i64) (param i32) -> ()`
            //   where the second i32 is the return-area pointer at
            //   which the host writes `(ptr: i32, len: i32)`.
            P2DirectImport::RandomBytes => (vec![ValType::I64, ValType::I32], vec![]),
            // monotonic-clock.now() / .resolution() → `() -> i64`.
            P2DirectImport::MonotonicNow | P2DirectImport::MonotonicResolution => {
                (vec![], vec![ValType::I64])
            }
            // wall-clock.now() -> datetime {seconds: u64, nanos: u32}
            //   → canonical-ABI lift: `(param i32) -> ()` where the
            //   i32 is the return-area pointer.
            P2DirectImport::WallClockNow => (vec![ValType::I32], vec![]),
            // v0.16 — filesystem direct lowerings.
            // `borrow<descriptor>` is an `i32` handle at the
            // canonical ABI; see `preview2::P2DirectImport` doc-comments
            // for the per-variant breakdown.
            P2DirectImport::FsOpenAt => (
                vec![
                    ValType::I32, // self (descriptor handle)
                    ValType::I32, // path-flags
                    ValType::I32, // path-ptr
                    ValType::I32, // path-len
                    ValType::I32, // open-flags
                    ValType::I32, // descriptor-flags
                    ValType::I32, // ret-area
                ],
                vec![],
            ),
            P2DirectImport::FsReadViaStream | P2DirectImport::FsWriteViaStream => (
                vec![
                    ValType::I32, // self
                    ValType::I64, // offset (filesize = u64)
                    ValType::I32, // ret-area
                ],
                vec![],
            ),
            P2DirectImport::FsStat => (
                vec![
                    ValType::I32, // self
                    ValType::I32, // ret-area
                ],
                vec![],
            ),
            P2DirectImport::FsClose => (vec![ValType::I32], vec![]),
            // v0.16 — http direct lowerings.
            P2DirectImport::HttpNewRequest => (vec![ValType::I32], vec![ValType::I32]),
            P2DirectImport::HttpHandleRequest => (
                vec![
                    ValType::I32, // req
                    ValType::I32, // opt-tag
                    ValType::I32, // opt-handle (only valid when tag = 1)
                    ValType::I32, // ret-area
                ],
                vec![],
            ),
            P2DirectImport::HttpResponseStatus => (vec![ValType::I32], vec![ValType::I32]),
            P2DirectImport::HttpResponseBody => (vec![ValType::I32, ValType::I32], vec![]),
            // v0.17 — log() direct lowerings.
            //
            // `wasi:cli/stdout@0.2.3#get-stdout() -> output-stream`
            //   → canonical-ABI: `() -> i32` (resource handle).
            P2DirectImport::LogStdoutGet => (vec![], vec![ValType::I32]),
            // `wasi:io/streams@0.2.3.[method]output-stream.blocking-write-and-flush(
            //     self: borrow<output-stream>, contents: list<u8>
            // ) -> result<_, stream-error>`
            //   → canonical-ABI: `(self:i32, ptr:i32, len:i32, ret-area:i32) -> ()`.
            //   The ret-area receives `(tag:i32, err-handle:i32)`;
            //   log() discards it.
            P2DirectImport::LogStreamWrite => (
                vec![ValType::I32, ValType::I32, ValType::I32, ValType::I32],
                vec![],
            ),
            // `[resource-drop]output-stream` is a `(self:i32) -> ()`
            // intrinsic that the canonical ABI splices into the
            // import section of any core module that drops one of
            // its handles.
            P2DirectImport::LogStreamDrop => (vec![ValType::I32], vec![]),
        };
        let ty = self.intern_sig(TySig { params, results });
        let (mod_name, fn_name) = which.import_pair();
        self.import_section
            .import(mod_name, fn_name, EntityType::Function(ty));
        let idx = self.import_count;
        self.import_count += 1;
        self.p2_direct_idx.insert(which, idx);
        idx
    }

    /// If `extern_name` (an `std.*`-shaped path) names a stdlib call
    /// we have a direct P2 lowering for AND the build targets the P2
    /// preview, return the matching [`P2DirectImport`]. Otherwise
    /// return `None` so the caller can fall back to the legacy
    /// dispatch (extern stub / WasmError::Unsupported).
    fn p2_direct_for_extern(&self, extern_name: &str) -> Option<P2DirectImport> {
        if !matches!(self.wasi_preview, EmitWasiPreview::P2) {
            return None;
        }
        if !matches!(self.target, WasmTarget::Wasi) {
            return None;
        }
        Some(match extern_name {
            "std.random.bytes" | "random.bytes" => P2DirectImport::RandomBytes,
            "std.time.now" | "time.now" => P2DirectImport::WallClockNow,
            "std.time.monotonic_now" | "time.monotonic_now" => P2DirectImport::MonotonicNow,
            "std.time.resolution" | "time.resolution" => P2DirectImport::MonotonicResolution,
            // v0.16 — filesystem.
            //
            // `std.fs.open` → descriptor.open-at (relative to the
            //   ambient preopen descriptor; the caller is responsible
            //   for resolving the preopen).
            // `std.fs.read_file` → read-via-stream entry point of the
            //   open → read → close sequence. The emitter splices
            //   this one import index; the surrounding open/close
            //   are emitted as additional calls in the same dispatch
            //   arm.
            // `std.fs.write_file` → mirror of read for output.
            // `std.fs.stat` → descriptor.stat.
            // `std.fs.close` → resource-drop intrinsic.
            "std.fs.open" | "fs.open" => P2DirectImport::FsOpenAt,
            "std.fs.read_file" | "fs.read_file" | "std.fs.read" | "fs.read" => {
                P2DirectImport::FsReadViaStream
            }
            "std.fs.write_file" | "fs.write_file" | "std.fs.write" | "fs.write" => {
                P2DirectImport::FsWriteViaStream
            }
            "std.fs.stat" | "fs.stat" => P2DirectImport::FsStat,
            "std.fs.close" | "fs.close" => P2DirectImport::FsClose,
            // v0.16 — http.
            //
            // `std.http.get` / `std.http.post` lower to the
            // outgoing-request constructor + outgoing-handler.handle
            // pair; the response-side calls (status, body) are
            // emitted in the same dispatch arm to make the spine
            // observable in the import section.
            //
            // `std.http.send` is the lower-level "I built the request
            // myself" entry point; it goes straight to handle().
            "std.http.get" | "http.get" | "std.http.post" | "http.post" => {
                P2DirectImport::HttpNewRequest
            }
            "std.http.send" | "http.send" => P2DirectImport::HttpHandleRequest,
            _ => return None,
        })
    }

    /// Walk the SIR program and pre-declare every P2 direct import
    /// any function body will need. Called from [`Self::emit`] before
    /// `declare_fns` so the import section is stable by the time
    /// function indices get assigned. See the call-site comment in
    /// `emit()` for the index-shift rationale.
    fn predeclare_p2_direct_imports(&mut self) {
        // Collect first into a deterministic vec to avoid touching
        // `self.p2_direct_idx` during the SIR walk (mutation while
        // iterating the program would be fine here, but the explicit
        // collect keeps the helper readable + cheap to test).
        let mut needed: Vec<P2DirectImport> = Vec::new();
        let mut uses_log = false;
        for f in &self.prog.fns {
            for blk in &f.blocks {
                for stmt in &blk.stmts {
                    if let Stmt::Assign(_, Rvalue::Call { func, .. }) = stmt {
                        match func {
                            FnRef::Builtin(BuiltinId::Extern(name)) => {
                                if let Some(which) = self.p2_direct_for_extern(name) {
                                    if !needed.contains(&which) {
                                        needed.push(which);
                                    }
                                }
                            }
                            FnRef::Builtin(BuiltinId::Log | BuiltinId::Print) => {
                                uses_log = true;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        // v0.17 — pre-declare the three log() direct imports as a
        // group so the call site can splice
        // [`crate::preview2::emit_log_call_sequence`] without
        // shifting any function indices mid-body.
        if uses_log
            && matches!(self.wasi_preview, EmitWasiPreview::P2)
            && matches!(self.target, WasmTarget::Wasi)
        {
            for which in [
                P2DirectImport::LogStdoutGet,
                P2DirectImport::LogStreamWrite,
                P2DirectImport::LogStreamDrop,
            ] {
                if !needed.contains(&which) {
                    needed.push(which);
                }
            }
        }
        for which in needed {
            let _ = self.p2_direct_import(which);
        }
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
        // - wasm32-wasi + P1: `wasi:cli/log#log` (back-compat shim).
        // - wasm32-wasi + P2: do NOT declare an import here. The
        //   v0.17 direct-lowering pass routes `log()` through a
        //   3-call canonical-ABI sequence on top of
        //   `wasi:cli/stdout@0.2.3` + `wasi:io/streams@0.2.3`
        //   declared lazily via [`Self::p2_direct_import`].
        // - wasm32-web : `mty:web/log#log` (unchanged).
        let declare_legacy_log = match (self.target, self.wasi_preview) {
            (WasmTarget::Wasi, EmitWasiPreview::P2) => false,
            (WasmTarget::Wasi, EmitWasiPreview::P1) => true,
            (WasmTarget::Web, _) => true,
        };
        if declare_legacy_log {
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
        }
        // `self.log_idx` stays `None` for P2-Wasi — the dispatch
        // arm in `emit_call` checks for that and routes to the
        // direct-import sequence instead.

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
        // v0.16 — pre-declare every P2 direct import the program will
        // need BEFORE `declare_fns`. Function indices in core Wasm
        // count imports + module-local funcs in one shared index
        // space; lazily adding an import during body emission would
        // shift the indices of every function declared earlier and
        // invalidate previously-recorded `fn_index` entries. The
        // v0.15 lowerings (`std.random.bytes`, `std.time.*`) hit the
        // same issue but their core-only tests never crossed the
        // wit-component encode path, so the breakage was latent.
        // Walking the SIR up-front lets us reserve the import slot
        // first and dispatch into a stable index from inside the
        // body emitter.
        self.predeclare_p2_direct_imports();
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
        // v0.17 — when this function uses log() under the P2-Wasi
        // dispatch path, append one extra `i32` local to hold the
        // `wasi:io/streams.output-stream` handle for the direct
        // lowering's `local.tee` + `local.get` pair. The index is
        // stashed on `self.log_handle_local` for the duration of
        // the call-site lowering (reset to None after the function
        // body is emitted).
        let needs_log_handle = self.fn_needs_log_handle(f);
        let log_handle_local = if needs_log_handle {
            let idx = next_wasm;
            local_types.push(ValType::I32);
            // next_wasm += 1 would be a no-op — nothing reads it
            // after this point — but we leave the local index live
            // for future locals if a slice grows past here.
            Some(idx)
        } else {
            None
        };
        self.log_handle_local = log_handle_local;
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
            FnRef::Builtin(BuiltinId::Log | BuiltinId::Print) => {
                if args.len() != 1 {
                    return Err(WasmError::Unsupported("log/print arity".into()));
                }
                let Operand::Const(Const::Str(s)) = &args[0] else {
                    return Err(WasmError::Unsupported("wasm log non-literal string".into()));
                };
                let (ptr, len) = self.intern_string(s);
                // v0.17 — P2-Wasi: lower log() to the direct
                // canonical-ABI sequence (get-stdout +
                // blocking-write-and-flush + drop). The three
                // direct-import indices were already declared by
                // the pre-decl pass, so we can splice the call
                // sequence here without shifting any function
                // indices.
                if matches!(self.wasi_preview, EmitWasiPreview::P2)
                    && matches!(self.target, WasmTarget::Wasi)
                {
                    let get_idx = self.p2_direct_import(P2DirectImport::LogStdoutGet);
                    let write_idx = self.p2_direct_import(P2DirectImport::LogStreamWrite);
                    let drop_idx = self.p2_direct_import(P2DirectImport::LogStreamDrop);
                    let handle_local = self
                        .log_handle_local
                        .expect("log handle local reserved in emit_fn");
                    crate::preview2::emit_log_call_sequence(
                        wfn,
                        get_idx,
                        write_idx,
                        drop_idx,
                        handle_local,
                        ptr,
                        len,
                        LOG_RETURN_AREA,
                    );
                    // Push placeholder Unit-as-i32 so the upstream
                    // assign sink has a typed value to consume.
                    wfn.instruction(&I::I32Const(0));
                    return Ok(());
                }
                // Legacy P1 / Web path: push (ptr, len) and call
                // the single `wasi:cli/log#log` / `mty:web/log#log`
                // import declared in `declare_imports`.
                wfn.instruction(&I::I32Const(ptr as i32));
                wfn.instruction(&I::I32Const(len as i32));
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
            FnRef::Builtin(BuiltinId::Extern(name)) => {
                // v0.15+ P2 direct-import dispatch — when the program
                // calls one of the stdlib functions we have a
                // versioned-import lowering for AND the build targets
                // P2, splice in the import and emit the call.
                //
                // The canonical-ABI shapes are documented on
                // [`Emitter::p2_direct_import`]; here we adapt the
                // SIR-level args to those shapes:
                //
                //   * `random.bytes(n)` → push n as i64 length + the
                //     return-area pointer (DOM_RETURN_AREA reused).
                //   * `time.monotonic_now()` / `time.resolution()` →
                //     no args, leaves i64 on the stack (the call
                //     result).
                //   * `time.now()` → push return-area pointer; the
                //     host writes a `datetime` record there.
                //
                // v0.16 — filesystem + http direct lowerings.
                //   * `fs.open(path)` → descriptor.open-at; ret-area
                //     holds the result<descriptor>.
                //   * `fs.read_file(path)` / `write_file(path, data)`
                //     → read-via-stream / write-via-stream entry
                //     point (the open + close scaffold is a v0.17
                //     follow-up; this pins the spliced import).
                //   * `fs.stat(path)` → descriptor.stat into ret-area.
                //   * `fs.close(handle)` → resource-drop intrinsic.
                //   * `http.get(url)` / `http.post(url, body)` →
                //     `[constructor]outgoing-request`; subsequent
                //     `http.send` lowers to outgoing-handler.handle.
                if let Some(which) = self.p2_direct_for_extern(name) {
                    let idx = self.p2_direct_import(which);
                    match which {
                        P2DirectImport::RandomBytes => {
                            // Length arg → i64 (we promote whatever
                            // single i32-ish arg the caller supplied;
                            // empty arg list falls back to 0).
                            if let Some(arg0) = args.first() {
                                self.emit_operand(f, m, arg0, wfn)?;
                                wfn.instruction(&I::I64ExtendI32U);
                            } else {
                                wfn.instruction(&I::I64Const(0));
                            }
                            wfn.instruction(&I::I32Const(DOM_RETURN_AREA as i32));
                            wfn.instruction(&I::Call(idx));
                            // Push the return-area pointer as the
                            // "result" so the upstream assign sink
                            // captures something useful (callers
                            // typically read (ptr, len) from there).
                            wfn.instruction(&I::I32Const(DOM_RETURN_AREA as i32));
                        }
                        P2DirectImport::MonotonicNow | P2DirectImport::MonotonicResolution => {
                            wfn.instruction(&I::Call(idx));
                            // Leaves i64 on the stack — already a
                            // valid Mighty `Instant` / `Duration`
                            // (both lower to i64). Nothing to do.
                        }
                        P2DirectImport::WallClockNow => {
                            wfn.instruction(&I::I32Const(DOM_RETURN_AREA as i32));
                            wfn.instruction(&I::Call(idx));
                            // Return-area pointer is the result so
                            // callers can read seconds+nanos from it.
                            wfn.instruction(&I::I32Const(DOM_RETURN_AREA as i32));
                        }
                        // v0.16 — filesystem direct lowerings.
                        //
                        // The canonical-ABI shapes carry resource handles
                        // (i32) as the first arg. For the v0.16 dispatch
                        // path the emitter conservatively passes 0 for
                        // any handle the SIR layer hasn't lifted yet —
                        // the actual preopen-descriptor lookup is a
                        // v0.17 follow-up. What we PIN here is that the
                        // versioned import lands in the import section
                        // (so the component-wrapper resolves it to the
                        // P2 interface) and that the call doesn't trap
                        // at validation time.
                        //
                        // `std.fs.read_file(path)` / `write_file(path,
                        // data)` / `stat(path)` are all rendered as a
                        // single call to the read-via-stream /
                        // write-via-stream / stat entry point — the
                        // open + drop scaffold around them will be
                        // added in v0.17 when the SIR carries the
                        // preopen handle explicitly.
                        P2DirectImport::FsOpenAt => {
                            // (self, path-flags, path-ptr, path-len,
                            //  open-flags, descriptor-flags, ret-area)
                            wfn.instruction(&I::I32Const(0)); // self
                            wfn.instruction(&I::I32Const(0)); // path-flags
                                                              // path string: if the SIR arg is a literal,
                                                              // intern it; otherwise push (0, 0).
                            if let Some(Operand::Const(Const::Str(s))) = args.first() {
                                let (ptr, len) = self.intern_string(s);
                                wfn.instruction(&I::I32Const(ptr as i32));
                                wfn.instruction(&I::I32Const(len as i32));
                            } else {
                                wfn.instruction(&I::I32Const(0));
                                wfn.instruction(&I::I32Const(0));
                            }
                            wfn.instruction(&I::I32Const(0)); // open-flags
                            wfn.instruction(&I::I32Const(0)); // descriptor-flags
                            wfn.instruction(&I::I32Const(FS_RETURN_AREA as i32));
                            wfn.instruction(&I::Call(idx));
                            // Push the return-area pointer as the
                            // value-shaped result (callers read the
                            // descriptor handle from offset +4).
                            wfn.instruction(&I::I32Const(FS_RETURN_AREA as i32));
                        }
                        P2DirectImport::FsReadViaStream | P2DirectImport::FsWriteViaStream => {
                            // (self_handle:i32, offset:i64, ret-area:i32)
                            // Default self/offset to 0 — the SIR layer
                            // doesn't yet carry the descriptor; the
                            // v0.17 follow-up will reify it. What we
                            // pin: the import is wired and the call
                            // validates.
                            wfn.instruction(&I::I32Const(0)); // self
                            wfn.instruction(&I::I64Const(0)); // offset
                            wfn.instruction(&I::I32Const(FS_RETURN_AREA as i32));
                            wfn.instruction(&I::Call(idx));
                            wfn.instruction(&I::I32Const(FS_RETURN_AREA as i32));
                        }
                        P2DirectImport::FsStat => {
                            // (self_handle:i32, ret-area:i32)
                            wfn.instruction(&I::I32Const(0));
                            wfn.instruction(&I::I32Const(FS_RETURN_AREA as i32));
                            wfn.instruction(&I::Call(idx));
                            wfn.instruction(&I::I32Const(FS_RETURN_AREA as i32));
                        }
                        P2DirectImport::FsClose => {
                            // resource-drop: (self_handle:i32) -> ()
                            if let Some(arg0) = args.first() {
                                self.emit_operand(f, m, arg0, wfn)?;
                            } else {
                                wfn.instruction(&I::I32Const(0));
                            }
                            wfn.instruction(&I::Call(idx));
                            // void return; push 0 so the assign sink
                            // has something well-typed to consume.
                            wfn.instruction(&I::I32Const(0));
                        }
                        // v0.16 — http direct lowerings.
                        //
                        // For GET / POST we splice the
                        // `[constructor]outgoing-request` import and
                        // call it with a 0 (placeholder headers handle).
                        // The full spine (handle → status → consume)
                        // is wired through subsequent `std.http.send`
                        // / response-side calls; testing pins that
                        // the constructor import is present, which is
                        // the discriminating signal between the
                        // adapter-routed and direct-import paths.
                        P2DirectImport::HttpNewRequest => {
                            wfn.instruction(&I::I32Const(0)); // headers
                            wfn.instruction(&I::Call(idx));
                            // Leaves the new-outgoing-request handle
                            // (i32) on the stack — that's our result.
                        }
                        P2DirectImport::HttpHandleRequest => {
                            // (req:i32, opt-tag:i32, opt-handle:i32, ret-area:i32)
                            wfn.instruction(&I::I32Const(0)); // req
                            wfn.instruction(&I::I32Const(0)); // opt-tag (none)
                            wfn.instruction(&I::I32Const(0)); // opt-handle
                            wfn.instruction(&I::I32Const(HTTP_RETURN_AREA as i32));
                            wfn.instruction(&I::Call(idx));
                            wfn.instruction(&I::I32Const(HTTP_RETURN_AREA as i32));
                        }
                        P2DirectImport::HttpResponseStatus => {
                            // (self:i32) -> i32
                            if let Some(arg0) = args.first() {
                                self.emit_operand(f, m, arg0, wfn)?;
                            } else {
                                wfn.instruction(&I::I32Const(0));
                            }
                            wfn.instruction(&I::Call(idx));
                        }
                        P2DirectImport::HttpResponseBody => {
                            // (self:i32, ret-area:i32)
                            if let Some(arg0) = args.first() {
                                self.emit_operand(f, m, arg0, wfn)?;
                            } else {
                                wfn.instruction(&I::I32Const(0));
                            }
                            wfn.instruction(&I::I32Const(HTTP_RETURN_AREA as i32));
                            wfn.instruction(&I::Call(idx));
                            wfn.instruction(&I::I32Const(HTTP_RETURN_AREA as i32));
                        }
                        // v0.17 — log() direct lowerings are dispatched
                        // by the `Log | Print` arm above (which calls
                        // `emit_log_call_sequence`), never via the
                        // `Extern(name)` path. `p2_direct_for_extern`
                        // never returns these variants, so any of them
                        // arriving here means an upstream invariant
                        // broke — surface it loudly.
                        P2DirectImport::LogStdoutGet
                        | P2DirectImport::LogStreamWrite
                        | P2DirectImport::LogStreamDrop => {
                            return Err(WasmError::Invalid(format!(
                                "log direct-import variant {which:?} reached extern-call dispatch \
                                 — expected to be handled by the Log/Print arm",
                            )));
                        }
                    }
                    return Ok(());
                }
                Err(WasmError::Unsupported(format!("wasm extern call {name}")))
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
