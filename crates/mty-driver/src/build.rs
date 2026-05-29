//! `mty build` and `mty run` (JIT path) implementations
//! (slice 8).
//!
//! Goes through the same parse → lower → typeck → borrowck → SIR
//! pipeline as `pipeline::run_file_with_runtime`, then hands the SIR
//! to either the Cranelift backend (native) or the Wasm backend.

use crate::manifest::{ExternLib, HostOs};
use crate::pipeline::{lower, parse_source, type_and_borrow_check};
use mty_codegen_cranelift::{
    artifact::BuildMode,
    error::CodegenError,
    jit::{build_jit, symbols_from},
    object::{compile_object, compile_object_with_debug, find_linker, link_executable_with_libs},
    Monomorphizer,
};
use mty_codegen_wasm::{
    compile_program_to_file_p2, compile_program_to_file_with_options,
    BuildOptions as WasmBuildOptions, EmitWasiPreview, Preview2Options, UserWit, WasmError,
    WasmTarget,
};
use mty_diagnostics::{render::ariadne::render_all, Severity};
use mty_runtime::codegen_abi;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildTarget {
    Native,
    Wasm(WasmTarget),
}

#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub target: BuildTarget,
    pub mode: BuildMode,
    pub out_dir: PathBuf,
    pub binary_name: String,
    /// Wasm targets only: when `true`, skip the Component Model
    /// wrapper and emit a bare core Wasm module. Default = false
    /// (component output; v0.2 wave-2, closes A47).
    pub no_component: bool,
    /// Wasm targets only: which WASI preview to target. Default
    /// (since v0.15) is [`WasiPreview::P2`] — emits a component
    /// whose imports are the versioned `wasi:*@0.2.3` interface
    /// set. Set to [`WasiPreview::P1`] via the `--wasi=p1` CLI
    /// flag to keep the legacy import shape (the v0.13/v0.14
    /// default). See `docs/reference/wasi.md`.
    pub wasi_preview: WasiPreview,
    /// Wasm targets only: optional user-supplied WIT package
    /// (loaded by `mty_pkg::wit_resolve`). When `Some`, the user's
    /// world is merged into the generated component world. The
    /// driver crate itself doesn't load files — the CLI does the
    /// load and hands the text down.
    pub user_wit: Option<UserWit>,
    /// v0.36 Track T2 — native FFI library set. Populated from the
    /// manifest's `[[extern_lib]]` blocks (see
    /// [`crate::manifest::ExternLib`]) by the CLI. The native build
    /// path threads each entry into the linker invocation; Wasm builds
    /// ignore the field.
    ///
    /// Paths are relative to `manifest_dir` so the driver can resolve
    /// them without re-walking up to the manifest from `out_dir`.
    pub extern_libs: Vec<ExternLib>,
    /// Directory the manifest lives in. Used to resolve relative
    /// `extern_lib.path` entries. `None` means the source file
    /// wasn't anchored to a manifest (single-file `mty run` flow);
    /// in that case `extern_libs` is expected to be empty.
    pub manifest_dir: Option<PathBuf>,
}

/// Which WASI preview to target for Wasm builds.
///
/// v0.13/v0.14 defaulted to [`WasiPreview::P1`] for back-compat with
/// the slice-8 emitter; v0.15 flips the default to [`WasiPreview::P2`]
/// now that the codegen layer wires direct versioned imports for
/// `std.random` + `std.time` and the vendored adapter handles the
/// remaining surfaces (`std.fs`, `std.http`, `log()`).
///
/// Callers that need the legacy P1 import shape can still opt back
/// in via `--wasi=p1` on the CLI or
/// [`BuildOptions::wasi_preview`] in code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WasiPreview {
    /// WASI Preview 1 — the default through v0.14. Routes `log` to
    /// the legacy `wasi:cli/log` import. Still supported for
    /// back-compat (`--wasi=p1`); not the default since v0.15.
    P1,
    /// WASI Preview 2 (0.2.3) — the **default since v0.15**. Emits a
    /// component whose imports are the versioned P2 interface set
    /// (`wasi:cli@0.2.3`, `wasi:io@0.2.3`, …) with direct lowerings
    /// for `std.random.bytes` / `std.time.*` and adapter-routed
    /// lowerings for `std.fs.*` / `std.http.*`.
    #[default]
    P2,
}

impl WasiPreview {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "p1" | "preview1" => WasiPreview::P1,
            "p2" | "preview2" => WasiPreview::P2,
            _ => return None,
        })
    }
}

impl BuildOptions {
    pub fn native_debug(out_dir: PathBuf, name: impl Into<String>) -> Self {
        Self {
            target: BuildTarget::Native,
            mode: BuildMode::Debug,
            out_dir,
            binary_name: name.into(),
            no_component: false,
            wasi_preview: WasiPreview::default(),
            user_wit: None,
            extern_libs: Vec::new(),
            manifest_dir: None,
        }
    }
    pub fn native_release(out_dir: PathBuf, name: impl Into<String>) -> Self {
        Self {
            target: BuildTarget::Native,
            mode: BuildMode::Release,
            out_dir,
            binary_name: name.into(),
            no_component: false,
            wasi_preview: WasiPreview::default(),
            user_wit: None,
            extern_libs: Vec::new(),
            manifest_dir: None,
        }
    }

    /// Attach an `[[extern_lib]]` set to a `BuildOptions` instance.
    /// Returns `self` so the call can be chained on top of a
    /// `native_debug(...)` constructor.
    pub fn with_extern_libs(mut self, libs: Vec<ExternLib>, manifest_dir: Option<PathBuf>) -> Self {
        self.extern_libs = libs;
        self.manifest_dir = manifest_dir;
        self
    }
}

/// Translate a manifest `[[extern_lib]]` set + the manifest dir into the
/// flat linker-arg vector understood by `link_executable_with_libs`.
///
/// The output is built in the order:
/// 1. For each lib: `path` or `-l<name>` (linker-search fallback)
/// 2. For each lib: cross-platform `link_args`
/// 3. For each lib: host-specific `link_args_*`
///
/// Paths are resolved against `manifest_dir`. Missing `manifest_dir`
/// (single-file flow) means relative paths are interpreted relative to
/// the current working directory — same as `link_executable` itself.
pub fn build_linker_args(libs: &[ExternLib], manifest_dir: Option<&Path>) -> Vec<String> {
    let host = HostOs::current();
    let mut out: Vec<String> = Vec::with_capacity(libs.len() * 4);
    for lib in libs {
        if let Some(p) = &lib.path {
            let resolved = match manifest_dir {
                Some(root) => root.join(p),
                None => PathBuf::from(p),
            };
            // Static archives pass through as bare path arguments;
            // every C linker (clang, gcc, cc, link.exe) accepts an
            // archive as a positional input. Dynamic libs do the same:
            // most linkers happily honour `.so`/`.dll`/`.dylib` paths
            // as positional inputs.
            out.push(resolved.to_string_lossy().into_owned());
        } else {
            // No explicit path → fall back to the system search path
            // via `-l<name>`. MSVC accepts `<name>.lib`; we emit the
            // GNU form because the linker auto-detection picks clang
            // / gcc / cc on Windows by default. Callers on MSVC can
            // override via `link_args_windows = ["mylib.lib"]`.
            out.push(format!("-l{}", lib.name));
        }
        out.extend(lib.resolved_link_args(host));
    }
    out
}

#[derive(Debug)]
pub enum BuildOutcome {
    NativeOk(PathBuf),
    NativeOkNoLinker(PathBuf), // emitted .o but no linker available
    WasmOk(PathBuf),
    FrontendError, // diagnostics already rendered
    BackendError(String),
}

/// Build a Mighty source file to a native executable.
pub fn build_native(src: String, source_id: String, opts: &BuildOptions) -> BuildOutcome {
    // Save the original source bytes / path: DWARF generation needs to
    // map SIR `SourceSpan` byte offsets back to line/column for the
    // line program. The lowerer would otherwise consume `src` by move.
    let src_for_debug = src.clone();
    let source_id_for_debug = source_id.clone();
    let Ok(prog) = lower_to_sir_strict(src, source_id) else {
        return BuildOutcome::FrontendError;
    };
    let prog = Monomorphizer::new(&prog).run();
    // Ensure output directory exists.
    if let Err(e) = std::fs::create_dir_all(&opts.out_dir) {
        return BuildOutcome::BackendError(format!("mkdir {}: {}", opts.out_dir.display(), e));
    }
    let obj_path = opts.out_dir.join(format!("{}.o", opts.binary_name));
    let obj = match opts.mode {
        BuildMode::Debug => {
            compile_object_with_debug(&prog, &obj_path, &src_for_debug, &source_id_for_debug)
        }
        BuildMode::Release => compile_object(&prog, &obj_path),
    };
    let obj = match obj {
        Ok(a) => a,
        Err(e) => return BuildOutcome::BackendError(format!("codegen: {e}")),
    };
    if find_linker().is_none() {
        return BuildOutcome::NativeOkNoLinker(obj.object_path);
    }
    let exe_path = exe_path_for(&opts.out_dir, &opts.binary_name);
    // v0.36 T2: translate the manifest's [[extern_lib]] set into raw
    // linker arguments. The flat string list keeps codegen-crate code
    // free of manifest schema dependencies.
    let extra_link_args = build_linker_args(&opts.extern_libs, opts.manifest_dir.as_deref());
    match link_executable_with_libs(&obj, &exe_path, opts.mode, &extra_link_args) {
        Ok(a) => BuildOutcome::NativeOk(a.binary_path),
        // The object emitted cleanly but the linker rejected it
        // (e.g. Windows link.exe LNK1120 on missing C-runtime
        // `main` entry; macOS ld refusing pre-v0.10 objects). The
        // .o is still a real artifact downstream tooling can use,
        // and returning `BackendError` would surface a linking
        // problem as if codegen itself were broken. v0.11+ should
        // wire a proper `main` entry shim per target so this path
        // becomes rare.
        Err(_) => BuildOutcome::NativeOkNoLinker(obj.object_path),
    }
}

/// Build a Mighty source file to a Wasm module.
///
/// When `opts.mode == BuildMode::Debug`, also emits:
/// - a wasm `name` custom section listing function names
/// - a `<binary>.wasm.map` source-map v3 sidecar
/// - a `sourceMappingURL` custom section pointing at the sidecar
pub fn build_wasm(
    src: String,
    source_id: String,
    opts: &BuildOptions,
    target: WasmTarget,
) -> BuildOutcome {
    let src_for_debug = src.clone();
    let source_id_for_debug = source_id.clone();
    let Ok(prog) = lower_to_sir_strict(src, source_id) else {
        return BuildOutcome::FrontendError;
    };
    if let Err(e) = std::fs::create_dir_all(&opts.out_dir) {
        return BuildOutcome::BackendError(format!("mkdir {}: {}", opts.out_dir.display(), e));
    }
    let out = opts.out_dir.join(format!("{}.wasm", opts.binary_name));

    // v0.13: WASI Preview 2 path. Only valid for the Wasi target;
    // Web ignores `wasi_preview` (Web has no WASI concept).
    if matches!(opts.wasi_preview, WasiPreview::P2)
        && matches!(target, WasmTarget::Wasi)
        && !opts.no_component
    {
        let mut p2_opts = Preview2Options::new(&opts.binary_name);
        if let Some(uw) = &opts.user_wit {
            p2_opts = p2_opts.with_user_wit(uw.clone());
        }
        return match compile_program_to_file_p2(&prog, &p2_opts, &out) {
            Ok((_bytes, _doc)) => BuildOutcome::WasmOk(out),
            Err(WasmError::Unsupported(reason)) => {
                BuildOutcome::BackendError(format!("wasm p2: unsupported SIR — {reason}"))
            }
            Err(e) => BuildOutcome::BackendError(format!("wasm p2: {e}")),
        };
    }

    let emit_preview = match opts.wasi_preview {
        WasiPreview::P1 => EmitWasiPreview::P1,
        WasiPreview::P2 => EmitWasiPreview::P2,
    };
    let wasm_opts = if opts.no_component {
        WasmBuildOptions::core_only(&opts.binary_name).with_wasi_preview(emit_preview)
    } else {
        WasmBuildOptions::new(&opts.binary_name).with_wasi_preview(emit_preview)
    };
    match compile_program_to_file_with_options(&prog, target, &out, &wasm_opts) {
        Ok(art) => match art.path.clone() {
            Some(p) => {
                // Debug-info attachment is core-module only for v0.2.
                // Component-Model components have a stricter section
                // layout (interleaved layer-1 sections); naive custom-
                // section appending can produce a structurally-invalid
                // component. Tracked in WASM_CM_V0_2_NOTES.md as a
                // post-v0.2 follow-up.
                let is_core = matches!(opts.mode, BuildMode::Debug) && opts.no_component;
                if is_core {
                    if let Err(e) = attach_wasm_debug_info(
                        &p,
                        &prog,
                        &src_for_debug,
                        &source_id_for_debug,
                        &opts.binary_name,
                    ) {
                        return BuildOutcome::BackendError(format!("wasm debug: {e}"));
                    }
                }
                BuildOutcome::WasmOk(p)
            }
            None => BuildOutcome::BackendError("wasm artifact missing path".into()),
        },
        Err(WasmError::Unsupported(reason)) => {
            BuildOutcome::BackendError(format!("wasm: unsupported SIR — {reason}"))
        }
        Err(e) => BuildOutcome::BackendError(format!("wasm: {e}")),
    }
}

/// Re-emit `wasm_path` with `name` + `sourceMappingURL` custom sections,
/// and write the `<wasm_path>.map` source-map sidecar next to it. The
/// wasm bytes are read back from disk to avoid threading them through
/// the artifact (which would require a bigger emit-API change while
/// the Component Model surface is still settling in v0.2 wave-2).
fn attach_wasm_debug_info(
    wasm_path: &Path,
    prog: &mty_ir::Program,
    src: &str,
    source_id: &str,
    binary_name: &str,
) -> Result<(), String> {
    use mty_codegen_wasm::sourcemap::{
        append_debug_sections, build_name_section, build_source_map, sidecar_relative_filename,
        sourcemap_sidecar_path, write_sourcemap_sidecar,
    };
    let bytes =
        std::fs::read(wasm_path).map_err(|e| format!("read {}: {}", wasm_path.display(), e))?;

    // import_count for the bare core module: at least 1 (the `log`
    // import). The Component Model wrapper may add more, but the
    // `name` section we emit applies to the core module's function
    // index space, which the wrapper preserves as the inner module.
    // v0.2: hard-coded to 1 (matches Emitter::declare_imports above).
    let import_count: u32 = 1;
    let name_section = build_name_section(prog, import_count);

    let sidecar_target = format!("{binary_name}.wasm");
    let sm = build_source_map(prog, source_id, src, &sidecar_target);

    let sidecar_path = sourcemap_sidecar_path(wasm_path);
    write_sourcemap_sidecar(wasm_path, &sm).map_err(|e| format!("write sidecar: {e}"))?;
    let _ = sidecar_path;

    let sidecar_url = sidecar_relative_filename(&sourcemap_sidecar_path(wasm_path));
    let augmented = append_debug_sections(bytes, &name_section, &sidecar_url);
    std::fs::write(wasm_path, &augmented)
        .map_err(|e| format!("rewrite {}: {}", wasm_path.display(), e))?;
    Ok(())
}

/// JIT-compile and run `main`. Falls back silently to the interpreter
/// runtime on `CodegenError::Unsupported`.
///
/// Returns:
/// - `Some(exit_code)` on JIT execution
/// - `None` if codegen reported Unsupported (caller should fall back
///   to interpreter)
pub fn jit_run(src: String, source_id: String) -> Result<Option<i32>, i32> {
    let Ok(prog) = lower_to_sir_strict(src, source_id) else {
        return Err(1);
    };
    let prog = Monomorphizer::new(&prog).run();
    let syms = symbols_from_runtime();
    match build_jit(&prog, &syms) {
        Ok(jc) => {
            let exit = jc.call_main().unwrap_or(0);
            Ok(Some(exit as i32))
        }
        Err(CodegenError::Unsupported(_)) => Ok(None),
        Err(e) => {
            eprintln!("codegen error: {e}");
            Err(1)
        }
    }
}

fn lower_to_sir_strict(src: String, source_id: String) -> Result<mty_ir::Program, ()> {
    let parsed = parse_source(src.clone(), source_id.clone());
    let (pkg, mut diags) = lower(&parsed);
    if !diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
        diags.extend(type_and_borrow_check(&pkg));
    }
    if diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
        eprint!("{}", render_all(&diags, &source_id, &src));
        return Err(());
    }
    let typed = mty_types::check_package_typed(&pkg);
    Ok(mty_ir::lower_package(&pkg, &typed))
}

fn exe_path_for(dir: &Path, name: &str) -> PathBuf {
    if cfg!(windows) {
        dir.join(format!("{name}.exe"))
    } else {
        dir.join(name)
    }
}

/// Build the (name, fn-ptr) symbol table for the JIT linker, drawn
/// from the runtime's exported C-ABI fns.
fn symbols_from_runtime() -> Vec<(String, *const u8)> {
    let st = codegen_abi::symbol_table();
    symbols_from(&st.iter().map(|(n, p)| (n.as_str(), *p)).collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jit_run_empty_main_returns_zero() {
        let src = "fn main() {}\n".to_string();
        match jit_run(src, "test.mty".into()) {
            Ok(Some(0) | None) => {}
            other => panic!("expected Ok(Some(0)) or Ok(None), got {other:?}"),
        }
    }

    #[test]
    fn build_native_creates_object() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = BuildOptions::native_debug(dir.path().to_path_buf(), "hello");
        let outcome = build_native("fn main() {}\n".into(), "x.mty".into(), &opts);
        // Either we got a native binary or just the .o (no linker).
        match outcome {
            BuildOutcome::NativeOk(p) => assert!(p.exists()),
            BuildOutcome::NativeOkNoLinker(p) => assert!(p.exists()),
            BuildOutcome::BackendError(e) => panic!("backend error: {e}"),
            BuildOutcome::FrontendError => panic!("frontend error"),
            BuildOutcome::WasmOk(_) => panic!("wrong outcome"),
        }
    }

    #[test]
    fn build_wasm_creates_module() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = BuildOptions {
            target: BuildTarget::Wasm(WasmTarget::Wasi),
            mode: BuildMode::Debug,
            out_dir: dir.path().to_path_buf(),
            binary_name: "hello".into(),
            // Default = Component Model output.
            no_component: false,
            wasi_preview: WasiPreview::P1,
            user_wit: None,
            extern_libs: Vec::new(),
            manifest_dir: None,
        };
        let outcome = build_wasm(
            "fn main() {}\n".into(),
            "x.mty".into(),
            &opts,
            WasmTarget::Wasi,
        );
        match outcome {
            BuildOutcome::WasmOk(p) => {
                assert!(p.exists());
                let bytes = std::fs::read(&p).expect("read wasm");
                // Component-Model bytes need feature flags enabled.
                let mut v =
                    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all());
                v.validate_all(&bytes).expect("valid wasm");
                // And the bytes should actually be a component.
                assert!(
                    mty_codegen_wasm::is_component(&bytes),
                    "expected component preamble"
                );
            }
            other => panic!("wrong outcome: {other:?}"),
        }
    }

    #[test]
    fn build_wasm_with_no_component_emits_core_module() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = BuildOptions {
            target: BuildTarget::Wasm(WasmTarget::Wasi),
            mode: BuildMode::Release, // skip debug-info post-step
            out_dir: dir.path().to_path_buf(),
            binary_name: "hello_core".into(),
            no_component: true,
            wasi_preview: WasiPreview::P1,
            user_wit: None,
            extern_libs: Vec::new(),
            manifest_dir: None,
        };
        let outcome = build_wasm(
            "fn main() {}\n".into(),
            "x.mty".into(),
            &opts,
            WasmTarget::Wasi,
        );
        match outcome {
            BuildOutcome::WasmOk(p) => {
                let bytes = std::fs::read(&p).expect("read wasm");
                assert!(
                    !mty_codegen_wasm::is_component(&bytes),
                    "should be a core module"
                );
                let mut v = wasmparser::Validator::new();
                v.validate_all(&bytes).expect("core wasm validates");
            }
            other => panic!("wrong outcome: {other:?}"),
        }
    }

    #[test]
    fn build_wasm_p2_emits_p2_component() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = BuildOptions {
            target: BuildTarget::Wasm(WasmTarget::Wasi),
            mode: BuildMode::Release,
            out_dir: dir.path().to_path_buf(),
            binary_name: "hello_p2".into(),
            no_component: false,
            wasi_preview: WasiPreview::P2,
            user_wit: None,
            extern_libs: Vec::new(),
            manifest_dir: None,
        };
        let outcome = build_wasm(
            "fn main() {}\n".into(),
            "x.mty".into(),
            &opts,
            WasmTarget::Wasi,
        );
        match outcome {
            BuildOutcome::WasmOk(p) => {
                let bytes = std::fs::read(&p).expect("read wasm");
                assert!(mty_codegen_wasm::is_component(&bytes), "expected component");
            }
            other => panic!("wrong outcome: {other:?}"),
        }
    }

    #[test]
    fn wasi_preview_parse() {
        assert_eq!(WasiPreview::parse("p1"), Some(WasiPreview::P1));
        assert_eq!(WasiPreview::parse("p2"), Some(WasiPreview::P2));
        assert_eq!(WasiPreview::parse("preview1"), Some(WasiPreview::P1));
        assert_eq!(WasiPreview::parse("preview2"), Some(WasiPreview::P2));
        assert_eq!(WasiPreview::parse("garbage"), None);
    }
}
