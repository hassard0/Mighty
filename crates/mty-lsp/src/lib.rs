//! mty-lsp: Language Server Protocol implementation for Mighty.
//!
//! Implements LSP 3.17 over stdio. Wires the Mighty compiler pipeline
//! (`mty-driver` → parse + lower + type-check) to LSP features so an
//! editor (VS Code, Neovim, etc.) can get diagnostics, hover, go-to-def,
//! formatting, completion, and (v0.5) semantic tokens / rename / inlay
//! hints / code actions / signature help for `.mty` files.
//!
//! The single public entry point is [`run_stdio`], which is called by
//! `mty lsp` (see `crates/mty-cli/src/cmd/lsp.rs`).
//!
//! Scope (v0.5):
//! - `textDocument/didOpen`, `didChange` (incremental), `didClose`
//! - `textDocument/publishDiagnostics` on every change
//! - `textDocument/hover` (CST node kind + resolved type if available)
//! - `textDocument/definition` (top-level item names; locals deferred)
//! - `textDocument/formatting` (whole-document via `mty-fmt`)
//! - `textDocument/completion` (keywords + def names + locals +
//!   receiver-type methods + built-in methods after `.`)
//! - `textDocument/semanticTokens/full` + `/range` (whole-CST classify)
//! - `textDocument/rename` + `prepareRename` (single-file)
//! - `textDocument/inlayHint` (inferred-type hints for `let` + params)
//! - `textDocument/codeAction` (MT2021 / MT2002 / MT3001 / MT4001 fixes)
//! - `textDocument/signatureHelp` (call + method-call sites)
//! - `textDocument/documentSymbol` (CST-backed outline symbols)
//! - `workspace/didChangeWorkspaceFolders` (re-analyzes per-folder open
//!   files; cross-file resolution remains single-file inside the LSP)
//! - lifecycle: `initialize` / `initialized` / `shutdown` / `exit`
//!
//! Out-of-scope (still): borrow-check diagnostics live in `mty check`,
//! full cross-file go-to-def, and call-hierarchy / type-hierarchy. See
//! `docs/internals/lsp.md` for the architecture deep dive.

/// Re-export of the `lsp-types` version tower-lsp 0.20 is built
/// against (currently 0.94). All sub-modules use this re-export
/// rather than depending on `lsp-types` directly so the types align
/// with tower-lsp's trait bounds.
pub use tower_lsp::lsp_types;

pub mod code_actions;
pub mod completion;
pub mod conv;
pub mod definition;
pub mod diagnostics;
pub mod diff_apply;
pub mod docs;
pub mod document_symbols;
pub mod hover;
pub mod inlay_hints;
pub mod line_index;
pub mod references;
pub mod rename;
pub mod semantic_tokens;
pub mod server;
pub mod signature_help;
pub mod workspace;

pub use server::{run_stdio, Backend};
