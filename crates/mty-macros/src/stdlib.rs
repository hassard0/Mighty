//! Standard macro library bundled with mty-macros.
//!
//! Two flavors live here:
//!
//!   1. **Source-shipped declarative macros** in `lib/*.mty`. The text
//!      is `include_str!`d so the crate can hand callers the source for
//!      compilation as part of a project's macro registry. Real
//!      "auto-import on `use mty_macros.…`" wiring is a mty-pkg concern —
//!      we expose the text and let projects splice it into their own
//!      macro discovery.
//!   2. **Code-driven builtin macros**, currently just [`format`]. These
//!      are macros whose arguments have their own grammar
//!      (`format!("score: {}", n)` has a *format-string* DSL inside the
//!      first arg) and so cannot be expressed as pure token-substitution
//!      templates. The HIR preprocessor consults this module *before* it
//!      raises MT6001 for `name!(args)` calls whose name isn't in the
//!      declarative registry.
//!
//! ## Layout
//!
//!   * `assert.mty` — `assert!`, `assert_eq!`, `assert_ne!`
//!   * `debug.mty`  — `debug!` (eprintln of expression text + value)
//!   * `unreachable.mty` — `unreachable!()`
//!   * `format` module — `format!` (v0.24)
//!
//! Use [`load_into`] to merge the bundled declarative macros into a
//! [`PackageMacros`] instance, and [`is_builtin_macro`] /
//! [`expand_builtin_macro`] to check + expand the builtin set.

pub mod computer_use;
pub mod format;
pub mod tool;

use crate::registry::PackageMacros;
use mty_ast::{AstNode, File};
use mty_syntax::SyntaxNode;

/// Source text of `lib/assert.mty`.
pub const ASSERT_SD: &str = include_str!("../lib/assert.mty");
/// Source text of `lib/debug.mty`.
pub const DEBUG_SD: &str = include_str!("../lib/debug.mty");
/// Source text of `lib/unreachable.mty`.
pub const UNREACHABLE_SD: &str = include_str!("../lib/unreachable.mty");

/// Every bundled source file, in load order.
pub fn bundled_sources() -> &'static [&'static str] {
    &[ASSERT_SD, DEBUG_SD, UNREACHABLE_SD]
}

/// Names of every code-driven builtin macro. The HIR preprocessor
/// special-cases these — they bypass the declarative `MacroRegistry`
/// and go straight to a custom expander (see [`expand_builtin_macro`]).
pub const BUILTIN_MACRO_NAMES: &[&str] = &["format"];

/// True if `name` is the name of a code-driven builtin macro shipped
/// with the compiler. These macros are recognised by the preprocessor
/// even with no `use` import and no declarative `macro` decl, mirroring
/// Rust's built-in `format!`/`println!` shape.
pub fn is_builtin_macro(name: &str) -> bool {
    BUILTIN_MACRO_NAMES.contains(&name)
}

/// Expand a builtin macro call. Returns `Some(snippet)` on success,
/// `Some(Err(...))` when the macro is known but the call is malformed,
/// and `None` if the macro isn't a builtin at all (caller should fall
/// back to declarative expansion or raise MT6001).
///
/// The returned source snippet is a Mighty expression text suitable
/// for re-parsing. Callers splice it back into the source and re-run
/// the preprocessor on the next pass.
pub fn expand_builtin_macro(
    name: &str,
    args: &[&str],
) -> Option<Result<String, format::FormatExpandError>> {
    match name {
        "format" => Some(format::expand_format_call(args)),
        _ => None,
    }
}

/// Names of every code-driven builtin **attribute** macro shipped
/// with the compiler. Attribute macros decorate an item (a fn today)
/// and synthesise companion items. This list is consulted by the HIR
/// preprocessor's attribute-resolution pass before raising the
/// "unknown attribute" diagnostic.
///
/// v0.26 Track B adds `tool` — the `@tool(...)` decorator that
/// auto-generates the MCP descriptor + invoke + register companion
/// fns. See [`tool::expand_tool_attribute`] and
/// `docs/reference/macros/tool.md`.
///
/// v0.30 Track C adds `computer_use` — the
/// `@computer_use(width:..., height:..., cap:...)` decorator for
/// agents driven by Anthropic's Computer Use tool family. The
/// preprocessor invokes [`computer_use::expand_computer_use_attribute`]
/// when the decorated item is an `agent` rather than a `fn`.
pub const BUILTIN_ATTRIBUTE_NAMES: &[&str] = &["tool", "computer_use"];

/// True if `name` is the name of a code-driven builtin attribute
/// macro shipped with the compiler.
pub fn is_builtin_attribute(name: &str) -> bool {
    BUILTIN_ATTRIBUTE_NAMES.contains(&name)
}

/// Expand a builtin attribute macro on a parsed fn item. Returns
/// `Some(expansion)` on success, `Some(Err(...))` when the attribute
/// is known but the call is malformed, and `None` if the attribute
/// isn't a builtin at all.
///
/// The `attr_args` slice is the comma-split source of the
/// parenthesised argument list (matches the shape
/// [`format::expand_format_call`] consumes). `func` is the parsed
/// view of the user's fn — see [`tool::ParsedFn`].
pub fn expand_builtin_attribute(
    name: &str,
    attr_args: &[&str],
    func: &tool::ParsedFn,
) -> Option<Result<tool::ToolExpansion, tool::ToolMacroError>> {
    match name {
        "tool" => Some(tool::expand_tool_attribute(attr_args, func)),
        _ => None,
    }
}

/// v0.30 Track C — expand a builtin attribute macro on a parsed
/// `agent` decl. Mirrors [`expand_builtin_attribute`] but takes
/// [`computer_use::ParsedAgent`] instead of [`tool::ParsedFn`] because
/// the `@computer_use` decorator targets agent items, not fns.
///
/// Returns `Some(expansion)` on success, `Some(Err(...))` when the
/// attribute is known but the call is malformed, and `None` if the
/// attribute is not an agent-shaped builtin (caller falls back to the
/// fn-shaped path or raises MT6001).
pub fn expand_builtin_agent_attribute(
    name: &str,
    attr_args: &[&str],
    agent: &computer_use::ParsedAgent,
) -> Option<Result<computer_use::ComputerUseExpansion, computer_use::ComputerUseMacroError>> {
    match name {
        "computer_use" => Some(computer_use::expand_computer_use_attribute(
            attr_args, agent,
        )),
        _ => None,
    }
}

/// Load every bundled stdlib macro into `pm.local`. Public macros land
/// in `pm.exported` as well so projects can re-export them. Returns
/// the number of macros added.
pub fn load_into(pm: &mut PackageMacros) -> usize {
    let before = pm.local.len();
    for src in bundled_sources() {
        let parsed = mty_syntax::parse(src);
        let root = SyntaxNode::new_root(parsed.green);
        let Some(file) = File::cast(root) else {
            continue;
        };
        let bundled = PackageMacros::from_file(&file.0);
        // Merge: every bundled macro (public, since the .mty files use
        // `pub macro`) goes into the importer's local + exported set.
        for (name, def) in bundled.local.macros {
            if def.is_pub {
                pm.exported.macros.insert(name.clone(), def.clone());
            }
            pm.local.macros.insert(name, def);
        }
    }
    pm.local.len() - before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assert_sd_parses_and_registers_three_macros() {
        let parsed = mty_syntax::parse(ASSERT_SD);
        let root = SyntaxNode::new_root(parsed.green);
        let file = File::cast(root).expect("FILE root");
        let pm = PackageMacros::from_file(&file.0);
        assert!(pm.local.contains("assert"), "assert missing");
        assert!(pm.local.contains("assert_eq"), "assert_eq missing");
        assert!(pm.local.contains("assert_ne"), "assert_ne missing");
        assert!(pm.exported.contains("assert"));
        assert!(pm.exported.contains("assert_eq"));
        assert!(pm.exported.contains("assert_ne"));
    }

    #[test]
    fn debug_sd_parses() {
        let parsed = mty_syntax::parse(DEBUG_SD);
        let root = SyntaxNode::new_root(parsed.green);
        let file = File::cast(root).expect("FILE root");
        let pm = PackageMacros::from_file(&file.0);
        assert!(pm.local.contains("debug"));
    }

    #[test]
    fn unreachable_sd_parses() {
        let parsed = mty_syntax::parse(UNREACHABLE_SD);
        let root = SyntaxNode::new_root(parsed.green);
        let file = File::cast(root).expect("FILE root");
        let pm = PackageMacros::from_file(&file.0);
        assert!(pm.local.contains("unreachable"));
    }

    #[test]
    fn load_into_merges_all_bundled_macros() {
        let mut pm = PackageMacros::new();
        let added = load_into(&mut pm);
        assert!(added >= 5, "expected >=5 macros, added {added}");
        for name in ["assert", "assert_eq", "assert_ne", "debug", "unreachable"] {
            assert!(pm.local.contains(name), "{name} missing from local");
            assert!(pm.exported.contains(name), "{name} missing from exported");
        }
    }

    #[test]
    fn format_is_a_builtin() {
        assert!(is_builtin_macro("format"));
        assert!(!is_builtin_macro("frobnicate"));
        assert!(!is_builtin_macro("assert_eq")); // declarative, not builtin
    }

    #[test]
    fn expand_builtin_format_round_trips() {
        let out = expand_builtin_macro("format", &["\"hi {}\"", "n"])
            .expect("format is a builtin")
            .expect("expansion succeeds");
        assert!(out.contains("(n).to_str()"));
    }

    #[test]
    fn expand_builtin_unknown_returns_none() {
        assert!(expand_builtin_macro("nope", &[]).is_none());
    }
}
