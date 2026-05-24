//! Diagnostic codes raised by macro expansion.
//!
//! These constants intentionally live in `sdust-macros` rather than in
//! `sdust-diagnostics`: the macros crate is loaded by lowering, which
//! does not (and should not) depend on the central diagnostics catalog
//! growing every time a feature crate ships its own code. The constants
//! are exposed as plain `u16`s so that callers (lowering, the CLI's
//! `sdust explain` table) can wrap them in their own `DiagCode` newtype.
//!
//! v0.4 reserves the **SD6000** band for macro-expansion errors.
//! Subsequent slices may extend the band; do not renumber.

/// SD6001 — A call site referenced a macro name that wasn't declared.
pub const UNKNOWN_MACRO: u16 = 6001;

/// SD6002 — A macro call had the wrong number of arguments.
pub const MACRO_ARITY_MISMATCH: u16 = 6002;

/// SD6003 — After substitution + hygiene, the expanded body failed to
/// re-parse as a valid expression / statement sequence.
pub const MACRO_BODY_PARSE_FAILED: u16 = 6003;

/// SD6004 — A macro expanded itself (directly or transitively) past
/// the v0.4 depth cap (`MAX_EXPANSION_DEPTH`).
pub const RECURSIVE_MACRO_TOO_DEEP: u16 = 6004;

/// Human-readable explanation for an SD6xxx code. Returns `None` for
/// unknown codes; the CLI may merge this table into its own.
pub fn explain(code: u16) -> Option<&'static str> {
    Some(match code {
        6001 => {
            "SD6001: Unknown macro. The call site refers to a name that is not \
             a registered declarative macro. Declare it with `macro Name(...) => { ... }` \
             above the call site, or check for a typo."
        }
        6002 => {
            "SD6002: Macro arity mismatch. The macro was declared with a fixed \
             number of parameters; the call site supplied a different count. \
             v0.4 macros do not support variadic parameters."
        }
        6003 => {
            "SD6003: Macro body did not parse after expansion. Substituting the \
             call-site arguments into the body produced tokens that no longer \
             form a valid expression or statement. Check for missing punctuation \
             in the macro body, or for arguments that need parentheses to remain \
             a single sub-expression after substitution."
        }
        6004 => {
            "SD6004: Recursive macro expansion exceeded the depth cap (32). The \
             macro called itself, directly or via another macro, more times \
             than v0.4 permits. Rewrite the macro non-recursively, or wait for \
             v0.5's bounded-recursion proposal."
        }
        _ => return None,
    })
}
