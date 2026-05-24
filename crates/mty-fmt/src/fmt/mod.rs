use crate::doc::Doc;
use mty_syntax::SyntaxNode;

pub mod agents;
pub mod concurrency;
pub mod exprs;
pub mod items;
pub mod patterns;
pub mod types;

/// Verbatim fallback: emit the node's source text unchanged. Used as
/// the conservative branch in per-node formatters until each kind has
/// a canonical printer.
pub fn verbatim(n: &SyntaxNode) -> Doc {
    Doc::text(n.text().to_string())
}

/// Format a parsed source file as a [`Doc`].
///
/// Slice 2 implements the file-level canonical rules:
/// * Exactly one trailing newline at EOF.
/// * Between adjacent top-level items, exactly one blank line iff the
///   original source had at least one blank line; otherwise just one
///   newline.
/// * Leading comments on a top-level item are preserved attached to
///   that item.
/// * No leading whitespace before the first item; no trailing
///   whitespace after the last newline.
///
/// Per-item content stays verbatim (each item emits its own source
/// text), so round-trip and idempotence both hold by construction:
/// re-parsing produces the same CST, re-formatting normalizes the
/// already-normal inter-item spacing to itself.
pub fn file(node: &SyntaxNode) -> Doc {
    items::file(node)
}
