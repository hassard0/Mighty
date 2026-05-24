//! sdust-macros: declarative macro registry + expander.
//!
//! v0.4 ships token-substitution macros with mangling-based hygiene.
//! See `docs/spec/macros-v0.4.md` and `docs/internals/macros.md` for the
//! formal contract; the short version is:
//!
//!   * `macro Name(p1, p2) => { body tokens }` is parsed by sdust-syntax
//!     as a `MACRO_DECL` with an opaque body of brace-balanced tokens.
//!   * A `MacroRegistry` collected at HIR-lowering time turns those CST
//!     nodes into `MacroDef { name, params, body }`.
//!   * When the lowerer sees a `CALL_EXPR` whose callee is a single-segment
//!     path matching a registered macro, it calls `expand` to produce a
//!     re-parsable source string, re-parses, and lowers the result inline.
//!   * Hygiene is implemented by renaming macro-introduced `let` bindings
//!     to `__mac_<ctx>_<orig>` so they cannot capture caller locals. This
//!     is a pragmatic subset of Set-of-Scopes hygiene that is sound for
//!     v0.4's small surface area; v0.5 plans to upgrade to a real
//!     set-of-scopes implementation.
//!
//! No I/O, no procedural macros — fully compile-time, sandboxed-by-shape.

pub mod diag;
pub mod expand;
pub mod registry;
pub mod token;

pub use diag::{
    MACRO_ARITY_MISMATCH, MACRO_BODY_PARSE_FAILED, RECURSIVE_MACRO_TOO_DEEP, UNKNOWN_MACRO,
};
pub use expand::{expand, expand_to_source, ExpandError, MacroContext};
pub use registry::{MacroDef, MacroRegistry};
pub use token::{tokens_from_body_node, tokens_to_source, Tok};

/// v0.4 macro-expansion depth limit. Recursive macro definitions are
/// rejected after this many nested expansions to prevent runaway
/// compilation. The integer is intentionally generous for hand-written
/// macros; lift it only with a corresponding spec amendment.
pub const MAX_EXPANSION_DEPTH: u32 = 32;
