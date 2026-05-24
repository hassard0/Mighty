//! Codegen error taxonomy. The driver translates these into either
//! diagnostics (real bugs) or interpreter-fallback events (the
//! `Unsupported` variant).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CodegenError {
    /// The SIR shape isn't (yet) supported by the slice-8 lowerer. The
    /// driver responds by falling back to the slice-6 interpreter so
    /// the program still runs.
    #[error("codegen: unsupported SIR shape: {0}")]
    Unsupported(String),

    /// Cranelift's verifier rejected the function we built. This is a
    /// codegen bug — the user shouldn't see it in production.
    #[error("codegen: cranelift verifier rejected fn `{name}`: {msg}")]
    VerifierFailed { name: String, msg: String },

    /// Layout impossible (e.g. infinite-size recursive ADT).
    #[error("codegen: layout impossible: {0}")]
    Layout(String),

    /// Module-level error (declare/define collision, etc.).
    #[error("codegen: module error: {0}")]
    Module(String),

    /// IO error during artifact writing.
    #[error("codegen: io error: {0}")]
    Io(String),

    /// Linker invocation failed.
    #[error("codegen: linker failure: {0}")]
    Linker(String),

    /// The runtime ABI bridge couldn't supply an imported symbol.
    #[error("codegen: runtime import `{0}` not registered")]
    MissingImport(String),
}

pub type CompileResult<T> = std::result::Result<T, CodegenError>;
