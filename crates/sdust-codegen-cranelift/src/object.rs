//! AOT object emission (slice 8).
//!
//! Emits a host-format `.o` (Mach-O on macos, ELF on linux, COFF on
//! windows), then invokes the platform linker via [`A52`] to produce
//! the final executable.

use crate::artifact::{BuildMode, NativeArtifact};
use crate::error::{CodegenError, CompileResult};
use crate::lower::{default_flags, LowerCtx};
use cranelift_codegen::isa::{self};
use cranelift_object::{ObjectBuilder, ObjectModule};
use sdust_sir::sir::Program;
use std::path::{Path, PathBuf};
use std::process::Command;
use target_lexicon::{OperatingSystem, Triple};

pub struct ObjectArtifact {
    pub object_path: PathBuf,
    pub triple: Triple,
}

/// Lower `prog` to a host-format object file at `out_obj`. Returns the
/// object artifact descriptor.
pub fn compile_object(prog: &Program, out_obj: &Path) -> CompileResult<ObjectArtifact> {
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
    ctx.declare_fns(prog)?;
    for f in &prog.fns {
        ctx.define_fn(prog, f)?;
    }
    drop(ctx);

    let product = module.finish();
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
    let candidates: &[&str] = if cfg!(windows) {
        &["clang.exe", "clang", "gcc.exe", "gcc", "cc.exe"]
    } else {
        &["cc", "gcc", "clang"]
    };
    for cand in candidates {
        if let Ok(path) = which::which(cand) {
            // Skip the coreutils `link.exe` shim that ships with
            // MSYS/Git-Bash on Windows — it's a hardlink helper, not
            // a linker.
            let s = path.to_string_lossy();
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
}
