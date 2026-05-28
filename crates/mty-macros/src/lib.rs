//! mty-macros: declarative macro registry + expander.
//!
//! v0.5 ships:
//!
//!   * `macro Name(p1, p2) => { body tokens }` — declarative macros
//!     parsed by mty-syntax as a `MACRO_DECL`. v0.4 baseline.
//!   * `proc macro Name(input: TokenStream) -> TokenStream { body }` —
//!     procedural macros parsed by mty-syntax as a `PROC_MACRO_DECL`.
//!     v0.5 parses + stores; execution gated by MT6006 until v0.6.
//!   * `Path!(args)` invocation syntax — `MACRO_CALL` node with an
//!     opaque `TOKEN_TREE` argument list.
//!   * Extended hygiene mangling for tuple/struct/ref/binding `let`
//!     patterns (v0.4 only covered `let IDENT`).
//!   * `pub macro …` cross-file visibility via [`PackageMacros`].
//!
//! No I/O, no procedural-macro execution yet — fully compile-time,
//! sandboxed-by-shape.

pub mod diag;
pub mod expand;
pub mod hygiene;
pub mod proc;
pub mod registry;
pub mod scopes;
pub mod stdlib;
pub mod token;

pub use diag::{
    MACRO_ARITY_MISMATCH, MACRO_BODY_PARSE_FAILED, MACRO_FORMAT_BAD_TEMPLATE,
    MACRO_FORMAT_UNSUPPORTED_SPEC, PROC_MACRO_IMPURE, PROC_MACRO_IMPURE_AT_RUNTIME,
    PROC_MACRO_RESOURCE_EXCEEDED, PROC_MACRO_UNSUPPORTED_V0_5, RECURSIVE_MACRO_TOO_DEEP,
    UNKNOWN_MACRO,
};
pub use expand::{expand_scoped, expand_scoped_to_source, ExpandError, ScopedExpansion};
pub use hygiene::{strip_scopes, HygieneEnv, ScopedTok};
pub use proc::{
    check_proc_macro_purity, expand_proc, ImpurityReason, ProcMacroResult, ResourceBreach, Sandbox,
    SandboxObservation, PROC_MACRO_MEM_BYTES, PROC_MACRO_STEPS, PROC_MACRO_WALL_MS,
};
pub use registry::{MacroDef, MacroKind, MacroRegistry, PackageMacros};
pub use scopes::{resolve, ResolveAmbiguity, ScopeGen, ScopeId, Scopes};
pub use stdlib::computer_use::{
    expand_computer_use_attribute, parse_computer_use_attribute_args, render_spec_json,
    ComputerUseAttributeArgs, ComputerUseExpansion, ComputerUseMacroError, ParsedAgent,
};
pub use stdlib::tool::{
    build_input_schema_json, expand_tool_attribute, is_optional_param,
    mty_type_to_json_schema_type, parse_tool_attribute_args, render_descriptor_json, ParsedFn,
    ParsedParam, ToolAttributeArgs, ToolDescriptorSnippet, ToolExpansion, ToolMacroError,
};
pub use stdlib::{
    expand_builtin_agent_attribute, expand_builtin_attribute, expand_builtin_macro,
    is_builtin_attribute, is_builtin_macro, BUILTIN_ATTRIBUTE_NAMES, BUILTIN_MACRO_NAMES,
};
pub use token::{lex_fragment, tokens_from_body_node, tokens_to_source, Tok};

/// v0.5 macro-expansion depth limit. Recursive macro definitions are
/// rejected after this many nested expansions to prevent runaway
/// compilation. The integer is intentionally generous for hand-written
/// macros; lift it only with a corresponding spec amendment.
pub const MAX_EXPANSION_DEPTH: u32 = 32;
