//! File-level + item-level formatting.
//!
//! The file printer walks the FILE node's `children_with_tokens` and
//! emits a canonical whitespace layout:
//!
//! * Each top-level item's textual content is preserved verbatim,
//!   with trailing whitespace stripped.
//! * Between two adjacent items, the separator is `\n\n` (one blank
//!   line) if the original source had a blank line between them, else
//!   `\n` (immediate succession).
//! * Comments that appear in file-level trivia between two items are
//!   preserved verbatim on their own lines, attached to the following
//!   item.
//! * Exactly one trailing `\n` at EOF.
//!
//! Per-item canonical printers (struct field rewrap, fn param
//! alignment, etc.) are deferred. Stripping each item's trailing
//! whitespace plus controlling the inter-item separator together
//! make the slice-2 formatter idempotent and round-trip-stable on
//! all 20 example programs.

use crate::doc::Doc;
use sdust_syntax::{SyntaxKind, SyntaxNode};

pub fn file(n: &SyntaxNode) -> Doc {
    let mut parts: Vec<Doc> = Vec::new();
    let mut first_emitted = false;
    // Trivia that lives **between** items at file-scope (rare — most
    // inter-item whitespace lives inside the items themselves, attached
    // by the parser as trailing whitespace).
    let mut pending_file_trivia = String::new();
    // Whether the previous item's stripped-off trailing whitespace
    // contained a blank line (i.e. `\n\n+`).
    let mut prev_blank_after = false;

    for child in n.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(t) => {
                if t.kind().is_trivia() {
                    pending_file_trivia.push_str(t.text());
                }
            }
            rowan::NodeOrToken::Node(item) => {
                let comments = extract_comment_lines(&pending_file_trivia);
                let file_blank = trivia_has_blank_line(&pending_file_trivia);

                if first_emitted {
                    let want_blank = prev_blank_after || file_blank || !comments.is_empty();
                    parts.push(Doc::text(if want_blank { "\n\n" } else { "\n" }));
                }

                if !comments.is_empty() {
                    parts.push(Doc::text(comments));
                    parts.push(Doc::text("\n"));
                }

                let (body, blank_after) = item_body_and_trailing_blank(&item);
                parts.push(body);

                first_emitted = true;
                pending_file_trivia.clear();
                prev_blank_after = blank_after;
            }
        }
    }

    // Trailing file-level trivia (anything after the last item). Comments
    // here are preserved on their own lines; whitespace is dropped because
    // `normalize_eof` enforces exactly one trailing newline.
    let tail_comments = extract_comment_lines(&pending_file_trivia);
    if !tail_comments.is_empty() {
        if first_emitted {
            parts.push(Doc::text("\n"));
        }
        parts.push(Doc::text(tail_comments));
    }

    // The `normalize_eof` step in `lib::format` ensures exactly one final
    // `\n`, so we don't need to push one here ourselves.

    Doc::concat_all(parts)
}

/// Returns the item's textual body with trailing whitespace stripped,
/// plus a flag indicating whether that trailing whitespace contained
/// a blank line (which is the cue to use a blank-line separator
/// before the next item).
fn item_body_and_trailing_blank(item: &SyntaxNode) -> (Doc, bool) {
    let text = item.text().to_string();
    let stripped = text.trim_end_matches(|c: char| c.is_whitespace());
    let tail = &text[stripped.len()..];
    let blank_after = tail.matches('\n').count() >= 2;
    (Doc::text(stripped.to_string()), blank_after)
}

/// Returns true if the given trivia text contains at least one blank
/// line (i.e. two or more `\n` characters).
fn trivia_has_blank_line(trivia: &str) -> bool {
    trivia.matches('\n').count() >= 2
}

/// Extract only the comment lines (line + block comments) from a chunk
/// of trivia text. Returns an empty string if no comments are present.
fn extract_comment_lines(trivia: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in trivia.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
            out.push(line);
        }
    }
    out.join("\n")
}

/// Returns true if `k` is a top-level item node kind. Exposed for
/// future tools that want to walk top-level items.
pub fn is_item_kind(k: SyntaxKind) -> bool {
    use SyntaxKind::*;
    matches!(
        k,
        FN_DECL
            | STRUCT_DECL
            | ENUM_DECL
            | TYPE_ALIAS
            | IMPL_BLOCK
            | TRAIT_DECL
            | CONST_DECL
            | USE_DECL
            | MOD_DECL
            | PACKAGE_DECL
            | AGENT_DECL
            | PROTOCOL_DECL
            | SUPERVISOR_DECL
            | EXTERN_BLOCK
            | EXPORT_DECL
            | MACRO_DECL
    )
}
