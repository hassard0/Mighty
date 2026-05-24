//! Diagnostic codes raised by macro expansion.
//!
//! These constants intentionally live in `sdust-macros` rather than in
//! `sdust-diagnostics`: the macros crate is loaded by lowering, which
//! does not (and should not) depend on the central diagnostics catalog
//! growing every time a feature crate ships its own code. The constants
//! are exposed as plain `u16`s so that callers (lowering, the CLI's
//! `sdust explain` table) can wrap them in their own `DiagCode` newtype.
//!
//! v0.4 reserved the **SD6000** band for macro-expansion errors.
//! v0.5 extends with SD6005 (procedural-macro impurity) and SD6006
//! (procedural-macro execution not yet supported); do not renumber.

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

/// SD6005 — v0.5: a procedural macro's body references an `effect`
/// call (I/O, time, env, model, rand). Proc macros must be pure
/// token-tree manipulations.
pub const PROC_MACRO_IMPURE: u16 = 6005;

/// SD6006 — v0.5: a procedural macro was invoked but the v0.5
/// compiler can only parse and store proc macros, not execute them.
/// Lift this constraint in v0.6 once the sandboxed SIR sub-context
/// lands. The macro declaration is preserved so call-site source can
/// stay stable across the v0.5 → v0.6 upgrade.
pub const PROC_MACRO_UNSUPPORTED_V0_5: u16 = 6006;

/// Human-readable explanation for an SD6xxx code. Returns `None` for
/// unknown codes; the CLI may merge this table into its own.
pub fn explain(code: u16) -> Option<&'static str> {
    Some(match code {
        6001 => {
            "SD6001: Unknown macro. The call site `name!(...)` refers to a name \
             that is not a registered declarative or procedural macro. Declare it \
             with `macro Name(...) => { ... }` above the call site, or check for \
             a typo. Cross-file macros must be `pub macro` in the exporting file \
             and imported with `use otherpkg.name`."
        }
        6002 => {
            "SD6002: Macro arity mismatch. The macro was declared with a fixed \
             number of parameters; the call site supplied a different count. \
             v0.5 macros do not support variadic parameters."
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
             than v0.5 permits. Rewrite the macro non-recursively, or wait for \
             a future bounded-recursion proposal."
        }
        6005 => {
            "SD6005: Procedural macro impurity. The proc-macro body contains a \
             call that looks like an effect (I/O, time, env, model, rand). \
             Procedural macros must be pure functions over TokenStream; effects \
             are forbidden because expansion happens at compile time, inside a \
             sandbox, with no access to the runtime environment."
        }
        6006 => {
            "SD6006: Procedural macro execution is not supported in v0.5. The \
             declaration parses and is stored in the registry, but the body \
             cannot run until v0.6 ships the sandboxed compile-time interpreter. \
             Replace the call with a hand-expanded equivalent, or wait for v0.6."
        }
        _ => return None,
    })
}
