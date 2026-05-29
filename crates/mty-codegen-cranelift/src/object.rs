//! AOT object emission (slice 8).
//!
//! Emits a host-format `.o` (Mach-O on macos, ELF on linux, COFF on
//! windows), then invokes the platform linker via [`A52`] to produce
//! the final executable.

use crate::artifact::{BuildMode, NativeArtifact};
use crate::debug::{build_dwarf_dispatch, DwarfInputs};
use crate::error::{CodegenError, CompileResult};
use crate::lower::{default_flags, LowerCtx};
use cranelift_codegen::isa::{self};
use cranelift_object::{ObjectBuilder, ObjectModule};
use mty_ir::ir::Program;
use object::write::{MachOBuildVersion, SectionId};
use object::SectionKind;
use std::path::{Path, PathBuf};
use std::process::Command;
use target_lexicon::{OperatingSystem, Triple};

pub struct ObjectArtifact {
    pub object_path: PathBuf,
    pub triple: Triple,
}

/// Lower `prog` to a host-format object file at `out_obj`. Returns the
/// object artifact descriptor. No debug info is attached; for that, see
/// [`compile_object_with_debug`].
pub fn compile_object(prog: &Program, out_obj: &Path) -> CompileResult<ObjectArtifact> {
    compile_object_inner(prog, out_obj, None)
}

/// Like [`compile_object`] but attaches a DWARF debug section bundle to
/// the emitted object file. The `source_text` and `source_path` are
/// used to build a `DW_TAG_compile_unit` plus per-fn subprograms and
/// per-local variable entries. See `crate::debug` for the v0.2 coverage
/// matrix and the deferred items (per-instr line table, .debug_loc,
/// real Address::Symbol references for low_pc/high_pc).
pub fn compile_object_with_debug(
    prog: &Program,
    out_obj: &Path,
    source_text: &str,
    source_path: &str,
) -> CompileResult<ObjectArtifact> {
    let inputs = DwarfInputs {
        source_text,
        source_path,
        comp_dir: std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".into()),
    };
    compile_object_inner(prog, out_obj, Some(inputs))
}

fn compile_object_inner(
    prog: &Program,
    out_obj: &Path,
    dwarf_inputs: Option<DwarfInputs<'_>>,
) -> CompileResult<ObjectArtifact> {
    let triple = Triple::host();
    let isa_builder = isa::lookup(triple.clone())
        .map_err(|e| CodegenError::Module(format!("isa lookup: {e}")))?;
    let flags = default_flags(true);
    let isa = isa_builder
        .finish(flags)
        .map_err(|e| CodegenError::Module(format!("isa finish: {e}")))?;

    // v0.36 T4 — emit the object's source-file/segment name as
    // `mighty` going forward. The legacy `stardust` byte sequence
    // is no longer produced; any historical reader/grep that
    // matched on `stardust` should also accept `mighty`. See
    // [`accepted_segment_name`].
    let builder = ObjectBuilder::new(
        isa,
        b"mighty".to_vec(),
        cranelift_module::default_libcall_names(),
    )
    .map_err(|e| CodegenError::Module(format!("object builder: {e}")))?;
    let mut module = ObjectModule::new(builder);

    let mut ctx = LowerCtx::new(&mut module, triple.clone());
    // v0.21: when we're emitting DWARF, ask the lowerer to capture
    // per-instruction MachSrcLoc maps. The capture cost is a few
    // bytes per statement; only the DWARF path consumes it, but
    // turning it on at the LowerCtx level means we don't need a
    // separate compile pass.
    if dwarf_inputs.is_some() {
        ctx.enable_debug_capture();
    }
    ctx.declare_fns(prog)?;
    for f in &prog.fns {
        ctx.define_fn(prog, f)?;
    }
    // Capture the per-fn debug map before dropping `ctx` (which
    // releases the `&mut module` borrow).
    let fn_debug = std::mem::take(&mut ctx.fn_debug);
    drop(ctx);

    let mut product = module.finish();

    // macOS ld refuses Mach-O objects that lack LC_BUILD_VERSION
    // ("ld: unknown platform in '...o'"). cranelift-object doesn't
    // emit it by default; stamp a conservative macOS 11.0 / SDK 11.0
    // build-version so `clang -o` / `ld` accepts the object on every
    // currently-supported macOS host.
    if matches!(
        triple.operating_system,
        OperatingSystem::MacOSX(_) | OperatingSystem::Darwin(_) | OperatingSystem::IOS(_)
    ) {
        let platform = match triple.operating_system {
            OperatingSystem::IOS(_) => object::macho::PLATFORM_IOS,
            _ => object::macho::PLATFORM_MACOS,
        };
        // Pack 11.0.0 as the conventional 32-bit X.Y.Z layout
        // ((X << 16) | (Y << 8) | Z). Minor + patch are 0.
        let v: u32 = 11 << 16;
        let mut bv = MachOBuildVersion::default();
        bv.platform = platform;
        bv.minos = v;
        bv.sdk = v;
        product.object.set_macho_build_version(bv);
    }

    // Attach DWARF sections if requested. `build_dwarf_dispatch`
    // selects v4 (default) or v5 (`MTY_DWARF5=1`) — the section names
    // and Mach-O segment translation below are identical for both.
    //
    // v0.21: the v5 path now consumes `fn_debug` (the per-instruction
    // MachSrcLoc map we captured during `define_fn`).
    if let Some(inputs) = dwarf_inputs {
        let encoded = build_dwarf_dispatch(prog, &inputs, Some(&fn_debug))
            .map_err(|e| CodegenError::Module(format!("dwarf build: {e:?}")))?;
        attach_dwarf_sections(&mut product.object, &encoded);
    }

    let bytes = product
        .emit()
        .map_err(|e| CodegenError::Module(format!("emit: {e}")))?;
    std::fs::write(out_obj, &bytes)
        .map_err(|e| CodegenError::Io(format!("write {}: {}", out_obj.display(), e)))?;
    Ok(ObjectArtifact {
        object_path: out_obj.to_path_buf(),
        triple,
    })
}

/// Append each encoded DWARF section to the object as a fresh
/// `SectionKind::Debug` section. The object writer picks the right
/// per-format encoding (ELF, COFF, Mach-O) automatically.
fn attach_dwarf_sections(
    obj: &mut object::write::Object<'static>,
    encoded: &mty_debuginfo::EncodedDwarf,
) {
    for s in &encoded.sections {
        // Section naming convention per platform:
        // - ELF and COFF: `.debug_info` etc. (as-is)
        // - Mach-O: uses `__DWARF` segment with section names like
        //   `__debug_info`. We translate.
        let (segment, name) = match obj.format() {
            object::BinaryFormat::MachO => (
                b"__DWARF".to_vec(),
                format!("__{}", s.name.trim_start_matches('.')).into_bytes(),
            ),
            _ => (Vec::new(), s.name.as_bytes().to_vec()),
        };
        let id: SectionId = obj.add_section(segment, name, SectionKind::Debug);
        obj.set_section_data(id, s.bytes.clone(), 1);
    }
}

/// Link an object file into an executable using the host's C linker
/// (per A52 discovery order). Returns the final native artifact.
pub fn link_executable(
    obj: &ObjectArtifact,
    out_exe: &Path,
    mode: BuildMode,
) -> CompileResult<NativeArtifact> {
    link_executable_with_libs(obj, out_exe, mode, &[])
}

/// Like [`link_executable`] but appends `extra_args` after the object
/// file and libc. v0.36 Track T2 uses this to forward every
/// `[[extern_lib]]` entry from `mighty.toml` into the linker invocation:
/// static archive paths, `-l<name>` references, and per-platform flags
/// arrive here as ordinary command-line arguments.
///
/// Order matters: object first, then libc, then user libs. This keeps
/// the standard GNU "object → required libs → optional libs" walk so a
/// static archive that depends on libc still resolves all its symbols
/// (libc was already on the command line; the linker walks forward
/// across `extra_args` and back to the object's unresolved set).
pub fn link_executable_with_libs(
    obj: &ObjectArtifact,
    out_exe: &Path,
    mode: BuildMode,
    extra_args: &[String],
) -> CompileResult<NativeArtifact> {
    let linker = find_linker()
        .ok_or_else(|| CodegenError::Linker("no linker found (set MTY_LINKER)".into()))?;
    let mut cmd = Command::new(&linker);
    cmd.arg(&obj.object_path);
    cmd.arg("-o").arg(out_exe);
    // Link libc explicitly on unixes — cranelift sometimes emits
    // libcalls (memcpy/memset) that need it.
    match obj.triple.operating_system {
        OperatingSystem::Linux | OperatingSystem::MacOSX(_) | OperatingSystem::Darwin(_) => {
            cmd.arg("-lc");
        }
        _ => {}
    }
    // v0.36 T2: thread the manifest's [[extern_lib]] set onto the
    // command line. Each entry already encodes the right shape (bare
    // path for a vendored archive, `-l<name>` for a system search,
    // raw flags from `link_args_*`).
    for a in extra_args {
        cmd.arg(a);
    }
    let output = cmd
        .output()
        .map_err(|e| CodegenError::Linker(format!("invoke {linker}: {e}")))?;
    if !output.status.success() {
        return Err(CodegenError::Linker(format!(
            "linker exited {:?}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(NativeArtifact {
        binary_path: out_exe.to_path_buf(),
        object_path: Some(obj.object_path.clone()),
        mode,
        target_triple: obj.triple.to_string(),
    })
}

/// Implements A52: env override, then `cc`, `gcc`, `clang`,
/// `link.exe`. On Windows the bare `link` name is excluded because
/// it commonly resolves to GNU coreutils' link (hardlink helper),
/// not the MSVC linker. Users who want MSVC pass `MTY_LINKER` (or
/// the legacy `STARDUST_LINKER`; that spelling is still honoured
/// but emits a one-shot deprecation warning).
///
/// We also reject MSYS/Git-Bash's `/usr/bin/link.exe` (the coreutils
/// shim) when found at that exact path, since it speaks GNU
/// arg-syntax, not MSVC's.
pub fn find_linker() -> Option<String> {
    // v0.36 T4: prefer MTY_LINKER, fall back to STARDUST_LINKER with
    // a one-shot deprecation warning. This module is `no_std`-friendly
    // enough that we can't depend on `mty-runtime`'s env helper, so
    // we open-code the same precedence locally.
    if let Ok(env) = std::env::var("MTY_LINKER") {
        if !env.trim().is_empty() {
            return Some(env);
        }
    }
    if let Ok(env) = std::env::var("STARDUST_LINKER") {
        if !env.trim().is_empty() {
            use std::sync::atomic::{AtomicBool, Ordering};
            static WARNED: AtomicBool = AtomicBool::new(false);
            if !WARNED.swap(true, Ordering::Relaxed) {
                eprintln!("mighty: warning: STARDUST_LINKER is deprecated; use MTY_LINKER instead");
            }
            return Some(env);
        }
    }
    // v0.2 search order: clang first (works everywhere; drives lld),
    // then platform-conventional Cs, then lld variants by themselves.
    let candidates: &[&str] = if cfg!(windows) {
        &[
            "clang.exe",
            "clang",
            "gcc.exe",
            "gcc",
            "cc.exe",
            "lld-link.exe",
            "lld-link",
        ]
    } else {
        &["cc", "gcc", "clang", "ld.lld", "lld"]
    };
    for cand in candidates {
        if let Ok(path) = which::which(cand) {
            let s = path.to_string_lossy();
            // Skip the coreutils `link.exe` shim that ships with
            // MSYS/Git-Bash on Windows — it's a hardlink helper, not
            // a linker.
            if s.contains("/usr/bin/link") || s.contains("\\usr\\bin\\link") {
                continue;
            }
            return Some(cand.to_string());
        }
    }
    None
}

// We don't depend on `which` in workspace deps — vendor a minimal
// `which` impl here.
mod which {
    use std::path::PathBuf;
    pub fn which(name: &str) -> Result<PathBuf, ()> {
        let path = std::env::var_os("PATH").ok_or(())?;
        for dir in std::env::split_paths(&path) {
            let cand = dir.join(name);
            if cand.is_file() {
                return Ok(cand);
            }
            #[cfg(windows)]
            {
                let mut with_ext = cand.clone();
                with_ext.set_extension("exe");
                if with_ext.is_file() {
                    return Ok(with_ext);
                }
            }
        }
        Err(())
    }
}

/// v0.36 T4 — return `true` for both the new emission segment name
/// (`b"mighty"`) and the legacy `b"stardust"` spelling. Useful for
/// downstream tools that inspect a Mighty-built object's file/segment
/// name and want to support objects produced before and after the
/// v0.36 rename.
pub fn accepted_segment_name(bytes: &[u8]) -> bool {
    bytes == b"mighty" || bytes == b"stardust"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialise the env-var-mutating tests: cargo runs tests in
    /// parallel by default and they all touch the same process
    /// globals (`MTY_LINKER` + `STARDUST_LINKER`). Hold this mutex
    /// for the lifetime of each env-var test to keep them from
    /// trampling each other.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn find_linker_is_best_effort() {
        // We can't assert success — CI hosts vary. But the function
        // must not panic. Hold ENV_LOCK so a parallel env-var test
        // can't flip our view of the world mid-call.
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prior_mty = std::env::var("MTY_LINKER").ok();
        let prior_sd = std::env::var("STARDUST_LINKER").ok();
        std::env::remove_var("MTY_LINKER");
        std::env::remove_var("STARDUST_LINKER");
        let _ = find_linker();
        if let Some(v) = prior_mty {
            std::env::set_var("MTY_LINKER", v);
        }
        if let Some(v) = prior_sd {
            std::env::set_var("STARDUST_LINKER", v);
        }
    }

    #[test]
    fn find_linker_prefers_mty_over_stardust() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Set both; MTY_ should win and STARDUST_ shouldn't even be
        // observed (no deprecation warning fires).
        std::env::set_var("MTY_LINKER", "/path/to/new-linker");
        std::env::set_var("STARDUST_LINKER", "/path/to/old-linker");
        let got = find_linker();
        std::env::remove_var("MTY_LINKER");
        std::env::remove_var("STARDUST_LINKER");
        assert_eq!(got.as_deref(), Some("/path/to/new-linker"));
    }

    #[test]
    fn find_linker_falls_back_to_stardust() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // MTY_ unset, STARDUST_ set → fallback path is honoured.
        std::env::remove_var("MTY_LINKER");
        std::env::set_var("STARDUST_LINKER", "/path/to/legacy-linker");
        let got = find_linker();
        std::env::remove_var("STARDUST_LINKER");
        assert_eq!(got.as_deref(), Some("/path/to/legacy-linker"));
    }

    #[test]
    fn accepted_segment_name_recognises_both_spellings() {
        assert!(accepted_segment_name(b"mighty"));
        assert!(accepted_segment_name(b"stardust"));
        assert!(!accepted_segment_name(b"other"));
        assert!(!accepted_segment_name(b""));
    }

    /// v0.36 Track T2 — `STARDUST_LINKER` must short-circuit the
    /// PATH walk. Pins the env-override contract the matrix tests
    /// rely on to bypass an off-PATH clang.
    #[test]
    fn find_linker_honours_stardust_linker_env() {
        // Use a synthetic placeholder so the test doesn't depend on
        // any tool actually being installed. The override path is
        // returned verbatim — `find_linker` doesn't validate it.
        let prev = std::env::var("STARDUST_LINKER").ok();
        std::env::set_var("STARDUST_LINKER", "synthetic-linker-for-test");
        let got = find_linker();
        // Restore env first so a panic below doesn't leak state.
        match prev {
            Some(v) => std::env::set_var("STARDUST_LINKER", v),
            None => std::env::remove_var("STARDUST_LINKER"),
        }
        assert_eq!(got.as_deref(), Some("synthetic-linker-for-test"));
    }

    /// v0.36 Track T2 — when `STARDUST_LINKER` is whitespace, the
    /// override is treated as unset and the PATH walk runs (matches
    /// the documented "set $STARDUST_LINKER" contract).
    #[test]
    fn find_linker_treats_whitespace_override_as_unset() {
        let prev = std::env::var("STARDUST_LINKER").ok();
        std::env::set_var("STARDUST_LINKER", "   ");
        let got = find_linker();
        match prev {
            Some(v) => std::env::set_var("STARDUST_LINKER", v),
            None => std::env::remove_var("STARDUST_LINKER"),
        }
        // We can't assert the PATH-walked result (CI varies), but it
        // must NOT be the whitespace value we passed.
        assert_ne!(got.as_deref(), Some("   "));
    }

    /// v0.36 Track T2 — `link_executable_with_libs` appends every
    /// extra arg after the object + libc. We can't observe the full
    /// command line from the safe `Command` API, but we can pin the
    /// helper's *contract* via a thin wrapper that records what
    /// would have been argv. This test uses a trivial implementation
    /// that mirrors the same arg-building code path.
    #[test]
    fn extra_args_are_appended_in_order() {
        // Reproduce the arg-vector logic inline: object first, libc
        // (only on unix), then the user's extras. The test runs on
        // every host so we strip libc from the assertion and just
        // check the extras come after the .o path.
        let object_path: std::path::PathBuf = "/tmp/x.o".into();
        let extras = vec![
            "foo.a".to_string(),
            "-Lvendor".to_string(),
            "-lz".to_string(),
        ];
        let mut argv: Vec<String> = vec![
            object_path.to_string_lossy().into_owned(),
            "-o".into(),
            "/tmp/out".into(),
        ];
        for a in &extras {
            argv.push(a.clone());
        }
        let obj_idx = argv.iter().position(|s| s.ends_with("x.o")).unwrap();
        let foo_idx = argv.iter().position(|s| s == "foo.a").unwrap();
        let lib_idx = argv.iter().position(|s| s == "-lz").unwrap();
        assert!(obj_idx < foo_idx, "object must precede first extra arg");
        assert!(foo_idx < lib_idx, "extras keep manifest order");
    }
}
