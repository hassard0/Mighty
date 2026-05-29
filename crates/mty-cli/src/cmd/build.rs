use mty_codegen_cranelift::artifact::BuildMode;
use mty_codegen_wasm::{UserWit, WasmTarget};
use mty_driver::build::WasiPreview;
use mty_driver::manifest::ExternLib;
use mty_driver::{build_native, build_wasm, BuildOptions, BuildOutcome, BuildTarget};
use std::fs;
use std::path::{Path, PathBuf};

#[allow(clippy::too_many_arguments)]
pub fn run(
    path: &Path,
    debug: bool,
    release: bool,
    target: Option<String>,
    out_dir: Option<PathBuf>,
    no_component: bool,
    wasi: Option<String>,
    world: Option<String>,
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
    // Debug builds now actually emit debug info (DWARF for native, name
    // section + .wasm.map sidecar for wasm) — see `mty-debuginfo`.
    let _ = debug; // flag retained for explicitness; default is already Debug
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

    // v0.15: WASI Preview selection. Default = P2 — the codegen
    // layer now wires direct versioned imports for std.random +
    // std.time and the vendored adapter handles the remaining
    // surfaces. Pass `--wasi=p1` to opt back into the legacy
    // import shape (see docs/reference/wasi.md). Invalid values
    // are surfaced before invoking the codegen so the diagnostic
    // shows the user's typo rather than a downstream wasm-encoder
    // error.
    let wasi_preview = match wasi.as_deref() {
        None => WasiPreview::default(),
        Some(s) => match WasiPreview::parse(s) {
            Some(v) => v,
            None => {
                eprintln!("unknown --wasi value: {s} (expected `p1` or `p2`)");
                return 2;
            }
        },
    };

    // v0.13: load `[wit]` from `mighty.toml` if present (next to the
    // source file). We deliberately treat WIT-load failures as fatal
    // because they always indicate user misconfiguration (a manifest
    // with `[wit]` but missing files, malformed TOML, etc).
    let user_wit = match load_user_wit(path, world.clone()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("user-wit load error: {e}");
            return 2;
        }
    };

    // v0.36 Track T2: read `[[extern_lib]]` blocks from `mighty.toml`.
    // Missing manifest is fine — single-file programs that don't use
    // extern c just see an empty list. Failure to parse a manifest
    // that *does* exist is fatal so the error is loud.
    let (extern_libs, manifest_dir) = match load_extern_libs(path) {
        Ok((libs, dir)) => (libs, dir),
        Err(e) => {
            eprintln!("manifest error: {e}");
            return 2;
        }
    };
    let opts = BuildOptions {
        target: build_target,
        mode,
        out_dir,
        binary_name: name,
        no_component,
        wasi_preview,
        user_wit,
        extern_libs,
        manifest_dir,
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
                "wrote object {} (no linker found; set $MTY_LINKER)",
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

/// Walk upward from the source file looking for a `mighty.toml`; if
/// one is found, ask `mty_pkg::wit_resolve` to load any `[wit]`
/// section. Returns `Ok(None)` when there's no manifest or no `[wit]`
/// section (the common case for v0.13).
///
/// `world_override` mirrors the CLI's `--world <name>` flag.
fn load_user_wit(
    src_path: &Path,
    world_override: Option<String>,
) -> Result<Option<UserWit>, String> {
    let Some(pkg_root) = find_manifest_root(src_path) else {
        return Ok(None);
    };
    let loaded = mty_pkg::wit_resolve::load_from_manifest(&pkg_root, world_override)
        .map_err(|e| e.to_string())?;
    Ok(loaded.map(|l| UserWit {
        text: l.text,
        world: l.world,
        source_label: l.source_label,
    }))
}

/// v0.36 Track T2: load the manifest's `[[extern_lib]]` set for the
/// build driver. Returns the list plus the manifest's directory so the
/// driver can resolve relative `path` entries. `Ok((vec![], None))`
/// when the source file isn't anchored to a `mighty.toml`.
fn load_extern_libs(src_path: &Path) -> Result<(Vec<ExternLib>, Option<PathBuf>), String> {
    let Some(pkg_root) = find_manifest_root(src_path) else {
        return Ok((Vec::new(), None));
    };
    let manifest_path = pkg_root.join("mighty.toml");
    let m = mty_driver::manifest::load(&manifest_path)
        .map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    Ok((m.extern_libs, Some(pkg_root)))
}

/// Walk upward from `src` looking for a directory containing
/// `mighty.toml`. Returns the directory on success.
fn find_manifest_root(src: &Path) -> Option<PathBuf> {
    let abs = src.canonicalize().ok()?;
    let mut cur = abs.parent()?.to_path_buf();
    loop {
        if cur.join("mighty.toml").is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}
