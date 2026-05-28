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

/// MT6007 — Runtime detection of impurity in a proc-macro body that
/// slipped past the static MT6005 check (e.g. an aliased effect call).
pub const PROC_MACRO_IMPURE_AT_RUNTIME: u16 = codes::PROC_MACRO_IMPURE_AT_RUNTIME.0;

/// MT6008 — Sandboxed proc-macro expansion exceeded its wall, step, or
/// memory bound. Expansion is aborted; the call site becomes inert.
pub const PROC_MACRO_RESOURCE_EXCEEDED: u16 = codes::PROC_MACRO_RESOURCE_EXCEEDED.0;

/// MT6009 — `format!` template was malformed (not a string literal,
/// unbalanced braces, bad named-arg ident). v0.24.
pub const MACRO_FORMAT_BAD_TEMPLATE: u16 = codes::MACRO_FORMAT_BAD_TEMPLATE.0;

/// MT6010 — `format!` template used a spec the expander doesn't
/// understand. v0.25 narrows this to indexed-positional + dynamic
/// width/precision (v0.26 follow-ups).
pub const MACRO_FORMAT_UNSUPPORTED_SPEC: u16 = codes::MACRO_FORMAT_UNSUPPORTED_SPEC.0;

/// MT6011 — `format!` template width is malformed (overflows U32 or
/// non-digit characters where digits expected). v0.25 Track D.
pub const MACRO_FORMAT_BAD_WIDTH: u16 = codes::MACRO_FORMAT_BAD_WIDTH.0;

/// MT6012 — `format!` template precision is malformed (overflows U32
/// or missing digits after `.`). v0.25 Track D.
pub const MACRO_FORMAT_BAD_PRECISION: u16 = codes::MACRO_FORMAT_BAD_PRECISION.0;

/// MT6017 — `@computer_use(...)` missing required `cap:`. v0.30 Track C.
pub const COMPUTER_USE_MISSING_CAP: u16 = codes::COMPUTER_USE_MISSING_CAP.0;

/// MT6018 — `@computer_use(cap: ...)` is malformed. v0.30 Track C.
pub const COMPUTER_USE_MALFORMED_CAP: u16 = codes::COMPUTER_USE_MALFORMED_CAP.0;

/// MT6019 — `@computer_use(width|height: ...)` is malformed. v0.30 Track C.
pub const COMPUTER_USE_MALFORMED_DIMENSION: u16 = codes::COMPUTER_USE_MALFORMED_DIMENSION.0;

/// MT6020 — `@computer_use` decorated non-agent item. v0.30 Track C.
pub const COMPUTER_USE_NOT_AN_AGENT: u16 = codes::COMPUTER_USE_NOT_AN_AGENT.0;

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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_codes_are_explainable() {
        assert!(explain(PROC_MACRO_IMPURE_AT_RUNTIME).is_some());
        assert!(explain(PROC_MACRO_RESOURCE_EXCEEDED).is_some());
    }

    #[test]
    fn format_codes_are_explainable() {
        assert!(explain(MACRO_FORMAT_BAD_TEMPLATE).is_some());
        assert!(explain(MACRO_FORMAT_UNSUPPORTED_SPEC).is_some());
    }

    #[test]
    fn format_v0_25_codes_are_explainable() {
        assert!(explain(MACRO_FORMAT_BAD_WIDTH).is_some());
        assert!(explain(MACRO_FORMAT_BAD_PRECISION).is_some());
    }

    #[test]
    fn computer_use_codes_are_explainable() {
        assert!(explain(COMPUTER_USE_MISSING_CAP).is_some());
        assert!(explain(COMPUTER_USE_MALFORMED_CAP).is_some());
        assert!(explain(COMPUTER_USE_MALFORMED_DIMENSION).is_some());
        assert!(explain(COMPUTER_USE_NOT_AN_AGENT).is_some());
    }
}
