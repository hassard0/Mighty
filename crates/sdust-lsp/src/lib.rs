//! sdust-lsp: Language Server Protocol implementation for Stardust.
//!
//! Implements LSP 3.17 over stdio. Wires the Stardust compiler pipeline
//! (`sdust-driver` → parse + lower + type-check) to LSP features so an
//! editor (VS Code, Neovim, etc.) can get diagnostics, hover, go-to-def,
//! formatting and basic completion for `.sd` files.
//!
//! The single public entry point is [`run_stdio`], which is called by
//! `sdust lsp` (see `crates/sdust-cli/src/cmd/lsp.rs`).
//!
//! Scope (v0.2 MVP):
//! - `textDocument/didOpen`, `didChange` (incremental), `didClose`
//! - `textDocument/publishDiagnostics` on every change
//! - `textDocument/hover` (CST node kind + resolved type if available)
//! - `textDocument/definition` (top-level item names)
//! - `textDocument/formatting` (whole-document via `sdust-fmt`)
//! - `textDocument/completion` (keyword-only; semantic completion deferred)
//! - lifecycle: `initialize` / `initialized` / `shutdown` / `exit`
//!
//! Out-of-scope: workspace folders, code actions, inlay hints, rename,
//! signature help, semantic tokens.

/// Re-export of the `lsp-types` version tower-lsp 0.20 is built
/// against (currently 0.94). All sub-modules use this re-export
/// rather than depending on `lsp-types` directly so the types align
/// with tower-lsp's trait bounds.
pub use tower_lsp::lsp_types;

pub mod completion;
pub mod conv;
pub mod definition;
pub mod diagnostics;
pub mod docs;
pub mod hover;
pub mod line_index;
pub mod server;

pub use server::{run_stdio, Backend};
