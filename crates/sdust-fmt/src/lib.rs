//! sdust-fmt: canonical formatter.

pub mod doc;
pub mod fmt;
pub mod printer;
pub mod trivia;

use sdust_syntax::{GreenNode, SyntaxNode};

/// Format a parsed source tree, given its `GreenNode` root.
///
/// Slice 2 applies file-level canonical rules (exactly one trailing
/// newline, normalized inter-item spacing). Per-item content is still
/// emitted verbatim — the per-node printers in `fmt::types`,
/// `fmt::patterns`, and `fmt::exprs` are exposed for direct use but
/// don't yet drive top-level formatting.
pub fn format(green: GreenNode) -> String {
    let root = SyntaxNode::new_root(green);
    let d = fmt::file(&root);
    let raw = printer::pretty(&d, &printer::Layout::default());
    normalize_eof(&raw)
}

/// Normalize trailing whitespace so the output ends with exactly one
/// `\n` (and no other trailing whitespace). Idempotent: applying it
/// twice produces the same result. Critical for the format-sweep
/// idempotence guarantee — without this, verbatim items that already
/// carry trailing newlines accumulate extras on each pass.
fn normalize_eof(s: &str) -> String {
    let trimmed = s.trim_end();
    let mut out = String::with_capacity(trimmed.len() + 1);
    out.push_str(trimmed);
    out.push('\n');
    out
}
