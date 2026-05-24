use sdust_codegen_cranelift::artifact::BuildMode;
use sdust_codegen_wasm::WasmTarget;
use sdust_driver::{build_native, build_wasm, BuildOptions, BuildOutcome, BuildTarget};
use std::fs;
use std::path::{Path, PathBuf};

pub fn run(
    path: &Path,
    debug: bool,
    release: bool,
    target: Option<String>,
    out_dir: Option<PathBuf>,
) -> i32 {
    let src = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to read {}: {}", path.display(), e);
            return 1;
        }
    };
    let source_id = path.display().to_string();

    // Mode: explicit --release wins over --debug; default is debug.
    let _ = debug; // explicit flag for clarity; default already matches
    let mode = if release {
        BuildMode::Release
    } else {
        BuildMode::Debug
    };

    // Target: --target native|wasm32-wasi|wasm32-web. Default native.
    let target = target.unwrap_or_else(|| "native".to_string());
    let build_target = match target.as_str() {
        "native" => BuildTarget::Native,
        "wasm32-wasi" | "wasi" => BuildTarget::Wasm(WasmTarget::Wasi),
        "wasm32-web" | "web" | "browser" => BuildTarget::Wasm(WasmTarget::Web),
        other => {
            eprintln!("unknown target: {other}");
            return 2;
        }
    };

    let out_dir = out_dir.unwrap_or_else(|| PathBuf::from("target"));
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("a")
        .to_string();
    let opts = BuildOptions {
        target: build_target,
        mode,
        out_dir,
        binary_name: name,
    };

    let outcome = match build_target {
        BuildTarget::Native => build_native(src, source_id, &opts),
        BuildTarget::Wasm(t) => build_wasm(src, source_id, &opts, t),
    };

    match outcome {
        BuildOutcome::NativeOk(p) => {
            println!("wrote {}", p.display());
            0
        }
        BuildOutcome::NativeOkNoLinker(p) => {
            println!(
                "wrote object {} (no linker found; set $STARDUST_LINKER)",
                p.display()
            );
            0
        }
        BuildOutcome::WasmOk(p) => {
            println!("wrote {}", p.display());
            0
        }
        BuildOutcome::FrontendError => 1,
        BuildOutcome::BackendError(e) => {
            eprintln!("build error: {e}");
            2
        }
    }
}
