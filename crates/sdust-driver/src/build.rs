//! `sdust build` and `sdust run` (JIT path) implementations
//! (slice 8).
//!
//! Goes through the same parse → lower → typeck → borrowck → SIR
//! pipeline as `pipeline::run_file_with_runtime`, then hands the SIR
//! to either the Cranelift backend (native) or the Wasm backend.

use crate::pipeline::{lower, parse_source, type_and_borrow_check};
use sdust_codegen_cranelift::{
    artifact::BuildMode,
    error::CodegenError,
    jit::{build_jit, symbols_from},
    object::{compile_object, find_linker, link_executable},
    Monomorphizer,
};
use sdust_codegen_wasm::{compile_program_to_file, WasmError, WasmTarget};
use sdust_diagnostics::{render::ariadne::render_all, Severity};
use sdust_runtime::codegen_abi;
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
}

impl BuildOptions {
    pub fn native_debug(out_dir: PathBuf, name: impl Into<String>) -> Self {
        Self {
            target: BuildTarget::Native,
            mode: BuildMode::Debug,
            out_dir,
            binary_name: name.into(),
        }
    }
    pub fn native_release(out_dir: PathBuf, name: impl Into<String>) -> Self {
        Self {
            target: BuildTarget::Native,
            mode: BuildMode::Release,
            out_dir,
            binary_name: name.into(),
        }
    }
}

#[derive(Debug)]
pub enum BuildOutcome {
    NativeOk(PathBuf),
    NativeOkNoLinker(PathBuf), // emitted .o but no linker available
    WasmOk(PathBuf),
    FrontendError, // diagnostics already rendered
    BackendError(String),
}

/// Build a Stardust source file to a native executable.
pub fn build_native(
    src: String,
    source_id: String,
    opts: &BuildOptions,
) -> BuildOutcome {
    let prog = match lower_to_sir_strict(src, source_id) {
        Ok(p) => p,
        Err(()) => return BuildOutcome::FrontendError,
    };
    let prog = Monomorphizer::new(&prog).run();
    // Ensure output directory exists.
    if let Err(e) = std::fs::create_dir_all(&opts.out_dir) {
        return BuildOutcome::BackendError(format!("mkdir {}: {}", opts.out_dir.display(), e));
    }
    let obj_path = opts.out_dir.join(format!("{}.o", opts.binary_name));
    let obj = match compile_object(&prog, &obj_path) {
        Ok(a) => a,
        Err(e) => return BuildOutcome::BackendError(format!("codegen: {e}")),
    };
    if find_linker().is_none() {
        return BuildOutcome::NativeOkNoLinker(obj.object_path);
    }
    let exe_path = exe_path_for(&opts.out_dir, &opts.binary_name);
    match link_executable(&obj, &exe_path, opts.mode) {
        Ok(a) => BuildOutcome::NativeOk(a.binary_path),
        Err(e) => BuildOutcome::BackendError(format!("link: {e}")),
    }
}

/// Build a Stardust source file to a Wasm module.
pub fn build_wasm(
    src: String,
    source_id: String,
    opts: &BuildOptions,
    target: WasmTarget,
) -> BuildOutcome {
    let prog = match lower_to_sir_strict(src, source_id) {
        Ok(p) => p,
        Err(()) => return BuildOutcome::FrontendError,
    };
    if let Err(e) = std::fs::create_dir_all(&opts.out_dir) {
        return BuildOutcome::BackendError(format!("mkdir {}: {}", opts.out_dir.display(), e));
    }
    let out = opts.out_dir.join(format!("{}.wasm", opts.binary_name));
    match compile_program_to_file(&prog, target, &out) {
        Ok(art) => match art.path {
            Some(p) => BuildOutcome::WasmOk(p),
            None => BuildOutcome::BackendError("wasm artifact missing path".into()),
        },
        Err(WasmError::Unsupported(reason)) => {
            BuildOutcome::BackendError(format!("wasm: unsupported SIR — {reason}"))
        }
        Err(e) => BuildOutcome::BackendError(format!("wasm: {e}")),
    }
}

/// JIT-compile and run `main`. Falls back silently to the interpreter
/// runtime on `CodegenError::Unsupported`.
///
/// Returns:
/// - `Some(exit_code)` on JIT execution
/// - `None` if codegen reported Unsupported (caller should fall back
///   to interpreter)
pub fn jit_run(src: String, source_id: String) -> Result<Option<i32>, i32> {
    let prog = match lower_to_sir_strict(src, source_id) {
        Ok(p) => p,
        Err(()) => return Err(1),
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

fn lower_to_sir_strict(src: String, source_id: String) -> Result<sdust_sir::Program, ()> {
    let parsed = parse_source(src.clone(), source_id.clone());
    let (pkg, mut diags) = lower(&parsed);
    if !diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
        diags.extend(type_and_borrow_check(&pkg));
    }
    if diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
        eprint!("{}", render_all(&diags, &source_id, &src));
        return Err(());
    }
    let typed = sdust_types::check_package_typed(&pkg);
    Ok(sdust_sir::lower_package(&pkg, &typed))
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
        match jit_run(src, "test.sd".into()) {
            Ok(Some(0)) | Ok(None) => {}
            other => panic!("expected Ok(Some(0)) or Ok(None), got {other:?}"),
        }
    }

    #[test]
    fn build_native_creates_object() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = BuildOptions::native_debug(dir.path().to_path_buf(), "hello");
        let outcome = build_native("fn main() {}\n".into(), "x.sd".into(), &opts);
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
        };
        let outcome = build_wasm(
            "fn main() {}\n".into(),
            "x.sd".into(),
            &opts,
            WasmTarget::Wasi,
        );
        match outcome {
            BuildOutcome::WasmOk(p) => {
                assert!(p.exists());
                let bytes = std::fs::read(&p).expect("read wasm");
                let mut v = wasmparser::Validator::new();
                v.validate_all(&bytes).expect("valid wasm");
            }
            other => panic!("wrong outcome: {other:?}"),
        }
    }
}
