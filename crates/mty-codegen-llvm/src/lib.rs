//! mty-codegen-llvm: SIR → LLVM IR backend (v0.2).
//!
//! This crate provides an opt-in LLVM 17 backend behind the `llvm`
//! feature flag. When the feature is *off* (the default), `compile()`
//! returns [`LlvmError::FeatureDisabled`] so the driver can fall back
//! to the Cranelift backend.
//!
//! When the feature is *on*, the crate uses `inkwell` to build an
//! `LLVMModule` from a SIR `Program`, optimizes it with the LLVM
//! `PassBuilder`, and emits either:
//! - a host-format object file (`.o`) for native linking, or
//! - LLVM IR text (`.ll`) for inspection.
//!
//! ## Why the feature gate?
//!
//! The build host this crate ships on (the v0.1 swarm host) does not
//! have LLVM 17 development headers/libs installed. `llvm-sys` (the
//! transitive dependency of `inkwell`) fails to build without them.
//! Rather than make the entire workspace dependent on a system LLVM,
//! we gate the LLVM backend off by default and document install
//! requirements (see `docs/internals/codegen-llvm.md`).
//!
//! To enable on a host with LLVM 17 installed:
//!
//! ```bash
//! cargo build -p mty-codegen-llvm --features llvm
//! ```
//!
//! ## Backend coverage
//!
//! The LLVM lowerer mirrors what the Cranelift backend supports:
//!
//! - integer / float / bool arithmetic
//! - locals → stack allocas, aggregates → stack buffers
//! - direct fn-to-fn calls (monomorphized)
//! - ADT construction (struct + enum tag+payload)
//! - Field reads / variant-field reads
//! - `if` / `goto` / `return` / `unreachable`
//! - `SwitchInt` / `SwitchVariant` as `switch` instructions
//! - `?` propagation via `TryReturnErr`
//! - `log` / `print` / `panic` via C-ABI runtime calls
//! - Agent send/ask/spawn as runtime ABI stubs
//!
//! Out of scope (same as Cranelift):
//!
//! - effect-system call dispatch (compiled inline)
//! - `dyn Trait` vtables
//! - closure capture
//!
//! See `CODEGEN_V0_2_NOTES.md` for the design rationale.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlvmError {
    #[error("LLVM backend disabled at compile time (build with --features llvm)")]
    FeatureDisabled,
    #[error("LLVM backend: lowering bailed on shape: {0}")]
    Unsupported(String),
    #[error("LLVM backend: module error: {0}")]
    Module(String),
    #[error("LLVM backend: verifier rejected fn `{name}`: {msg}")]
    VerifierFailed { name: String, msg: String },
    #[error("LLVM backend: io error: {0}")]
    Io(String),
}

pub type CompileResult<T> = Result<T, LlvmError>;

/// Build mode passed to the LLVM optimizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlvmOptLevel {
    /// `-O0`: no optimization, fast compile.
    O0,
    /// `-O2`: standard release optimization.
    O2,
    /// `-O3`: aggressive optimization.
    O3,
}

/// Output kind for [`compile_to_path`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    /// Host-format relocatable object (`.o`).
    Object,
    /// Textual LLVM IR (`.ll`).
    IrText,
}

/// True when the LLVM backend is compiled into the binary.
pub const fn enabled() -> bool {
    cfg!(feature = "llvm")
}

/// Attempt to compile a SIR program via the LLVM backend.
///
/// - With `--features llvm`: lowers `prog`, optimizes, and returns Ok.
///   The compiled artifact stays in-memory; use [`compile_to_path`] to
///   write it out.
/// - Without `--features llvm`: always returns
///   [`LlvmError::FeatureDisabled`].
pub fn compile(_prog: &mty_ir::ir::Program) -> CompileResult<()> {
    #[cfg(not(feature = "llvm"))]
    {
        Err(LlvmError::FeatureDisabled)
    }
    #[cfg(feature = "llvm")]
    {
        // Validate-only path: build the module then drop it.
        let ctx = inkwell::context::Context::create();
        let _module = crate::lower::lower_program(&ctx, _prog, LlvmOptLevel::O2)?;
        Ok(())
    }
}

/// Compile and write to disk. Output format chosen by `kind`.
pub fn compile_to_path(
    _prog: &mty_ir::ir::Program,
    _out: &std::path::Path,
    _kind: OutputKind,
    _opt: LlvmOptLevel,
) -> CompileResult<()> {
    #[cfg(not(feature = "llvm"))]
    {
        Err(LlvmError::FeatureDisabled)
    }
    #[cfg(feature = "llvm")]
    {
        let ctx = inkwell::context::Context::create();
        let module = crate::lower::lower_program(&ctx, _prog, _opt)?;
        match _kind {
            OutputKind::IrText => module
                .print_to_file(_out)
                .map_err(|e| LlvmError::Io(e.to_string())),
            OutputKind::Object => crate::lower::write_object(&module, _out),
        }
    }
}

#[cfg(feature = "llvm")]
pub mod lower;

#[cfg(test)]
mod tests {
    use super::*;
    use mty_ir::ir::Program;

    #[test]
    fn compile_returns_expected_disabled_error_when_off() {
        // This test is true regardless of whether the `llvm` feature is
        // enabled — it only asserts that the *disabled* variant fires
        // when the feature is off.
        let p = Program::default();
        let r = compile(&p);
        if cfg!(feature = "llvm") {
            // Feature on: compile should succeed for an empty program.
            assert!(
                r.is_ok(),
                "empty program should compile under --features llvm: {r:?}"
            );
        } else {
            assert!(matches!(r, Err(LlvmError::FeatureDisabled)));
        }
    }

    #[test]
    fn enabled_flag_matches_feature() {
        assert_eq!(enabled(), cfg!(feature = "llvm"));
    }

    #[cfg(feature = "llvm")]
    #[test]
    fn empty_program_lowers() {
        let p = Program::default();
        let r = compile(&p);
        assert!(
            r.is_ok(),
            "empty program failed under --features llvm: {r:?}"
        );
    }
}
