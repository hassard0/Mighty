//! sdust-fmt: canonical formatter.

pub mod doc;
pub mod fmt;
pub mod printer;
pub mod trivia;

use sdust_syntax::{GreenNode, SyntaxNode};

/// Format a parsed source tree, given its `GreenNode` root.
///
/// In this slice the formatter is an identity-ish stub that re-emits the
/// source text verbatim. Tasks 25-26 replace [`fmt::file`] with real
/// per-node formatters and trivia handling.
pub fn format(green: GreenNode) -> String {
    let root = SyntaxNode::new_root(green);
    let d = fmt::file(&root);
    printer::pretty(&d, &printer::Layout::default())
}
