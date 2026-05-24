//! sdust-codegen-llvm: LLVM backend scaffold.
//!
//! Slice 8 ships this crate as a **scaffold only** (A46). The build
//! host did not have LLVM/llvm-config installed, so the slice-leader
//! chose to degrade to Cranelift-only for v0.1. This crate exists so
//! a future host with LLVM installed can flip the `llvm` feature on
//! and start emitting real LLVM IR.
//!
//! When the `llvm` feature is **off** (the default), `compile()`
//! returns [`LlvmError::FeatureDisabled`]. When **on**, it returns
//! [`LlvmError::NotYetImplemented`] (real lowering is v0.2 work).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlvmError {
    #[error("LLVM backend disabled at compile time (build with --features llvm)")]
    FeatureDisabled,
    #[error("LLVM backend not yet implemented (v0.2 work; see A46)")]
    NotYetImplemented,
}

/// Attempt to compile a SIR program via the LLVM backend. Slice-8
/// always returns an error; the caller falls back to
/// [`sdust_codegen_cranelift`].
pub fn compile(_prog: &sdust_sir::sir::Program) -> Result<(), LlvmError> {
    #[cfg(not(feature = "llvm"))]
    {
        Err(LlvmError::FeatureDisabled)
    }
    #[cfg(feature = "llvm")]
    {
        Err(LlvmError::NotYetImplemented)
    }
}

/// True when the LLVM backend is compiled into the binary.
pub const fn enabled() -> bool {
    cfg!(feature = "llvm")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdust_sir::sir::Program;

    #[test]
    fn compile_returns_expected_disabled_error() {
        let p = Program::default();
        let r = compile(&p);
        match r {
            Err(LlvmError::FeatureDisabled) | Err(LlvmError::NotYetImplemented) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn enabled_flag_matches_feature() {
        assert_eq!(enabled(), cfg!(feature = "llvm"));
    }
}
