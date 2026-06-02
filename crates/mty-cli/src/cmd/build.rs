use mty_codegen_cranelift::artifact::BuildMode;
use mty_codegen_wasm::{UserWit, WasmTarget};
use mty_driver::build::WasiPreview;
use mty_driver::manifest::{BuildConfig, ExternLib};
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
    emit: Option<String>,
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

    // v0.36 Track T2 + v0.41 T4: read `[[extern_lib]]` blocks and the
    // `[build]` block from `mighty.toml`. Missing manifest is fine —
    // single-file programs that don't use extern c just see empty lists.
    // Failure to parse a manifest that *does* exist is fatal so the
    // error is loud.
    let inputs = match load_manifest_build_inputs(path) {
        Ok(v) => v,
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
        extern_libs: inputs.extern_libs,
        manifest_dir: inputs.manifest_dir,
        build_config: inputs.build_config,
    };

    let outcome = match build_target {
        BuildTarget::Native => build_native(src, source_id, &opts),
        BuildTarget::Wasm(t) => build_wasm(src, source_id, &opts, t),
    };

    // v0.46 T2 — `--emit obj` lets CI flows opt into object-only output
    // (the historic "no linker" success path). Any other value is
    // rejected up-front so a typo doesn't silently fall through.
    let emit_obj_only = match emit.as_deref() {
        None | Some("exe" | "executable") => false,
        Some("obj" | "object") => true,
        Some(other) => {
            eprintln!("unknown --emit value: {other} (expected `exe` or `obj`)");
            return 2;
        }
    };

    match outcome {
        BuildOutcome::NativeOk(p) => {
            println!("wrote {}", p.display());
            0
        }
        BuildOutcome::NativeOkNoLinker {
            object_path,
            discovery,
        } => {
            // v0.46 T2 — only treat object-only output as success when
            // the caller explicitly asked for `--emit obj`. The default
            // (and historic) behaviour is to build a native executable,
            // so a missing linker must fail loudly: CI scripts and
            // `set -e` shells used to greenlight builds that produced
            // nothing runnable.
            if emit_obj_only {
                println!(
                    "wrote object {} (no linker found; --emit obj keeps this as success)",
                    object_path.display()
                );
                0
            } else {
                eprintln!(
                    "wrote object {} but no linker is available to produce a native executable.",
                    object_path.display()
                );
                eprintln!("{}", discovery.summary());
                eprintln!("(pass `--emit obj` if you only need the object file)");
                2
            }
        }
        BuildOutcome::NativeLinkError { object_path, error } => {
            eprintln!(
                "link error after writing object {}: {}",
                object_path.display(),
                error
            );
            2
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

/// v0.36 Track T2 + v0.41 T4: bundle of manifest-derived inputs the
/// build driver needs to wire the native link command. Returned from
/// [`load_manifest_build_inputs`] so the call site doesn't grow an
/// ever-longer tuple.
struct ManifestBuildInputs {
    extern_libs: Vec<ExternLib>,
    manifest_dir: Option<PathBuf>,
    build_config: Option<BuildConfig>,
}

/// Load the manifest's `[[extern_lib]]` set and `[build]` block for the
/// build driver. Returns empty defaults when the source file isn't
/// anchored to a `mighty.toml`.
fn load_manifest_build_inputs(src_path: &Path) -> Result<ManifestBuildInputs, String> {
    let Some(pkg_root) = find_manifest_root(src_path) else {
        return Ok(ManifestBuildInputs {
            extern_libs: Vec::new(),
            manifest_dir: None,
            build_config: None,
        });
    };
    let manifest_path = pkg_root.join("mighty.toml");
    let m = mty_driver::manifest::load(&manifest_path)
        .map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    Ok(ManifestBuildInputs {
        extern_libs: m.extern_libs,
        manifest_dir: Some(pkg_root),
        build_config: m.build,
    })
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
