//! Diagnostic codes raised by macro expansion.
//!
//! As of v0.6 the canonical home for these codes is
//! `mty_diagnostics::codes` (the central catalog). This module
//! re-exports the same numeric values as bare `u16`s so that the
//! historical call-sites (`DiagCode::new(mty_macros::diag::FOO)`,
//! `mty_macros::UNKNOWN_MACRO`, …) keep working without churn.
//!
//! Adding a NEW macro code:
//!   1. Add it to `mty_diagnostics::codes` (a `DiagCode::new(N)` const
//!      plus an arm in `explain`).
//!   2. Re-export the bare `u16` here so legacy lowering code keeps
//!      compiling.
//!
//! Do not duplicate the explanation text — `mty explain SDxxxx`
//! reads exclusively from the central catalog.

use mty_diagnostics::codes;

/// MT6001 — A call site referenced a macro name that wasn't declared.
pub const UNKNOWN_MACRO: u16 = codes::UNKNOWN_MACRO.0;

/// MT6002 — A macro call had the wrong number of arguments.
pub const MACRO_ARITY_MISMATCH: u16 = codes::MACRO_ARITY_MISMATCH.0;

/// MT6003 — After substitution + hygiene, the expanded body failed to
/// re-parse as a valid expression / statement sequence.
pub const MACRO_BODY_PARSE_FAILED: u16 = codes::MACRO_BODY_PARSE_FAILED.0;

/// MT6004 — A macro expanded itself (directly or transitively) past
/// the depth cap (`MAX_EXPANSION_DEPTH`).
pub const RECURSIVE_MACRO_TOO_DEEP: u16 = codes::RECURSIVE_MACRO_TOO_DEEP.0;

/// MT6005 — A procedural macro's body references an `effect` call
/// (I/O, time, env, model, rand). Proc macros must be pure token-tree
/// manipulations.
pub const PROC_MACRO_IMPURE: u16 = codes::PROC_MACRO_IMPURE.0;

/// MT6006 — A procedural macro was invoked but the current compiler
/// can only parse and store proc macros, not execute them. Lift this
/// constraint once the sandboxed SIR sub-context lands.
pub const PROC_MACRO_UNSUPPORTED_V0_5: u16 = codes::PROC_MACRO_UNSUPPORTED_V0_5.0;

/// Human-readable explanation for an SD6xxx code. v0.6: delegates to
/// `mty_diagnostics::codes::explain` so the catalog stays
/// single-sourced. Returns `None` for codes outside the macro band.
pub fn explain(code: u16) -> Option<&'static str> {
    if (6001..=6099).contains(&code) {
        codes::explain(codes::DiagCode::new(code))
    } else {
        None
    }
}
