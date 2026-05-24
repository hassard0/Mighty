use crate::doc::Doc;
use sdust_syntax::SyntaxNode;

pub mod agents;
pub mod concurrency;
pub mod exprs;
pub mod items;
pub mod patterns;
pub mod types;

/// Format a parsed source file as a [`Doc`].
///
/// This is a placeholder that emits the source text verbatim. Task 25
/// replaces it with real per-node formatters.
pub fn file(node: &SyntaxNode) -> Doc {
    Doc::text(node.text().to_string())
}
