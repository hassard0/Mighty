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

    let builder = ObjectBuilder::new(
        isa,
        b"stardust".to_vec(),
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
    // ("ld: unknown platform in '...o'"). cranelift-object 0.132+
    // *does* emit one, but for `OperatingSystem::Darwin(_)` (the
    // value `target_lexicon::Triple::host()` returns on macOS) it
    // sets `platform = PLATFORM_UNKNOWN (0)` and zero versions —
    // which trips a fresh batch of Xcode 15+ ld warnings:
    //
    //   ld: warning: object file (...) was built for an unsupported
    //   file format
    //   ld: warning: object file (...) has malformed LC_BUILD_VERSION
    //   load command (platform=0)
    //
    // Override with a sensible PLATFORM_MACOS + minos + sdk so the
    // linker accepts the object cleanly on every currently-supported
    // macOS host.
    //
    // v0.36 T5: tightened version pack. Apple's ld64 packs version
    // X.Y.Z into nibbles as `(X << 16) | (Y << 8) | Z` per the
    // Mach-O `build_version_command` spec (loader.h). Read
    // MACOSX_DEPLOYMENT_TARGET if set (rustc honors it for the same
    // purpose); otherwise default `minos = 11.0` (Apple's minimum
    // for arm64 binaries) and `sdk = 14.0` to match a recent SDK
    // (Sonoma) so the linker doesn't warn about a too-old SDK.
    if matches!(
        triple.operating_system,
        OperatingSystem::MacOSX(_) | OperatingSystem::Darwin(_) | OperatingSystem::IOS(_)
    ) {
        let platform = match triple.operating_system {
            OperatingSystem::IOS(_) => object::macho::PLATFORM_IOS,
            _ => object::macho::PLATFORM_MACOS,
        };
        let minos = parse_macos_version_env("MACOSX_DEPLOYMENT_TARGET")
            .unwrap_or(pack_macos_version(11, 0, 0));
        // SDK version: use the host SDK if we can guess it; otherwise
        // a recent default that current ld64 accepts without warning.
        let sdk = parse_macos_version_env("MTY_MACOSX_SDK_VERSION")
            .unwrap_or(pack_macos_version(14, 0, 0));
        let mut bv = MachOBuildVersion::default();
        bv.platform = platform;
        bv.minos = minos;
        bv.sdk = sdk;
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

/// Pack X.Y.Z into the 32-bit nibble layout Mach-O's
/// `LC_BUILD_VERSION` expects (`(X << 16) | (Y << 8) | Z`). See the
/// `build_version_command` definition in Apple's `loader.h`.
fn pack_macos_version(major: u32, minor: u32, patch: u32) -> u32 {
    (major << 16) | ((minor & 0xff) << 8) | (patch & 0xff)
}

/// Parse "X.Y" or "X.Y.Z" out of `env_var` (used for
/// `MACOSX_DEPLOYMENT_TARGET` discovery — rustc honors the same env
/// var when picking its own LC_BUILD_VERSION). Returns `None` if the
/// var is unset, empty, or malformed.
fn parse_macos_version_env(env_var: &str) -> Option<u32> {
    let raw = std::env::var(env_var).ok()?;
    parse_macos_version(&raw)
}

fn parse_macos_version(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut parts = s.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next().map(|p| p.parse().ok()).unwrap_or(Some(0))?;
    let patch: u32 = parts.next().map(|p| p.parse().ok()).unwrap_or(Some(0))?;
    Some(pack_macos_version(major, minor, patch))
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
    let linker = find_linker()
        .ok_or_else(|| CodegenError::Linker("no linker found (set STARDUST_LINKER)".into()))?;
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
/// not the MSVC linker. Users who want MSVC pass STARDUST_LINKER.
///
/// We also reject MSYS/Git-Bash's `/usr/bin/link.exe` (the coreutils
/// shim) when found at that exact path, since it speaks GNU
/// arg-syntax, not MSVC's.
pub fn find_linker() -> Option<String> {
    if let Ok(env) = std::env::var("STARDUST_LINKER") {
        if !env.trim().is_empty() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_linker_is_best_effort() {
        // We can't assert success — CI hosts vary. But the function
        // must not panic.
        let _ = find_linker();
    }

    // v0.36 T5: LC_BUILD_VERSION nibble pack must match Apple's
    // `build_version_command` layout in loader.h. Spot-check the
    // common deployment-target values rustc honors.

    #[test]
    fn pack_macos_version_matches_loader_h_layout() {
        // 11.0.0 → 0x000B0000
        assert_eq!(pack_macos_version(11, 0, 0), 0x000B_0000);
        // 14.5.0 → 0x000E0500
        assert_eq!(pack_macos_version(14, 5, 0), 0x000E_0500);
        // 10.15.7 → 0x000A0F07 (Catalina, last MACOSX_DEPLOYMENT_TARGET
        // before Big Sur)
        assert_eq!(pack_macos_version(10, 15, 7), 0x000A_0F07);
        // Patch + minor clamp at 0xff (the field is one byte each).
        assert_eq!(pack_macos_version(0, 0xff, 0xff), 0x0000_FFFF);
    }

    #[test]
    fn parse_macos_version_accepts_common_shapes() {
        assert_eq!(parse_macos_version("11"), Some(0x000B_0000));
        assert_eq!(parse_macos_version("11.0"), Some(0x000B_0000));
        assert_eq!(parse_macos_version("11.0.0"), Some(0x000B_0000));
        assert_eq!(parse_macos_version("14.5"), Some(0x000E_0500));
        assert_eq!(parse_macos_version("14.5.1"), Some(0x000E_0501));
        // Whitespace tolerated.
        assert_eq!(parse_macos_version("  14.5  "), Some(0x000E_0500));
    }

    #[test]
    fn parse_macos_version_rejects_bad_input() {
        assert_eq!(parse_macos_version(""), None);
        assert_eq!(parse_macos_version("  "), None);
        assert_eq!(parse_macos_version("abc"), None);
        assert_eq!(parse_macos_version("14.x"), None);
        assert_eq!(parse_macos_version("14.5.x"), None);
    }

    #[test]
    fn parse_macos_version_env_returns_none_when_unset() {
        // Use a deliberately weird name so we don't collide with the
        // host env. Std env mutation is unsafe for parallel tests, so
        // we test only the negative path (unset → None).
        let key = "MTY_TEST_DOES_NOT_EXIST_LC_BUILD_VERSION_PROBE";
        std::env::remove_var(key);
        assert_eq!(parse_macos_version_env(key), None);
    }

    /// On Linux/Windows hosts `triple.operating_system` doesn't match
    /// the macOS arms, so the LC_BUILD_VERSION block is skipped and
    /// `compile_object` produces an ELF/COFF without surprise. This
    /// test asserts the no-op shape: the helper functions exist and
    /// can be called without compiling a real Mach-O.
    #[test]
    fn pack_helpers_are_pure() {
        // No side effects, no panics — purely a smoke check that
        // the helper module is stable.
        let _ = pack_macos_version(11, 0, 0);
        let _ = parse_macos_version("11.0");
    }

    /// Smoke: when MACOSX_DEPLOYMENT_TARGET is unset the macOS code
    /// path uses the 11.0.0 default. We can't directly observe the
    /// emitted LC_BUILD_VERSION without compiling on macOS, but we
    /// can verify the default packing matches what we document.
    #[test]
    fn default_minos_matches_documented_floor() {
        // The override block uses `pack_macos_version(11, 0, 0)` as
        // the floor. Make sure that's exactly 0x000B0000 (11.0.0).
        assert_eq!(pack_macos_version(11, 0, 0), 0x000B_0000);
        // And the SDK default of 14.0.0 packs to 0x000E0000.
        assert_eq!(pack_macos_version(14, 0, 0), 0x000E_0000);
    }
}
