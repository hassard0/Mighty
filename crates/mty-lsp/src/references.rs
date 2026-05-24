//! Reference-finding helper used by rename / future codelens features.
//!
//! v0.5 model: single-file. For top-level items (fns, types, variants,
//! consts, traits, agents, ...) we walk the CST and collect every IDENT
//! whose text equals the target name. Locals are resolved by limiting
//! the scan to the smallest enclosing fn body.
//!
//! This is intentionally textual-with-scope: we don't have a true
//! per-occurrence ResolveMap surfaced by `mty-hir` yet (see
//! `LSP_V0_5_NOTES.md`), so we approximate. Shadowing inside the same
//! fn body is handled by stopping the scan at re-binding `let` stmts
//! for the same name (caller pre-filters; see `find_local_refs`).

use crate::docs::DocAnalysis;
use rowan::{TextRange, TextSize};
use mty_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

/// One identifier-occurrence range in source bytes.
#[derive(Debug, Clone, Copy)]
pub struct Occurrence {
    pub start: u32,
    pub end: u32,
}

/// Find every occurrence (declaration + uses) of `name` at the top
/// level of `doc`. Returns occurrences in source order.
pub fn find_top_level_refs(doc: &DocAnalysis, name: &str) -> Vec<Occurrence> {
    let root = SyntaxNode::new_root(doc.parsed.green.clone());
    let mut out = Vec::new();
    collect_idents(&root, name, &mut out);
    dedup_sorted(out)
}

/// Find every occurrence of a local named `name` whose declaration
/// lives at byte offset `decl_offset`. We restrict the scan to the
/// smallest enclosing fn or agent-handler body that *contains*
/// `decl_offset`. Re-binding `let` stmts later in the same scope do
/// not stop the scan (we deliberately rename shadows together —
/// callers that don't want this should reject the rename in
/// prepareRename, see [`crate::rename`]).
pub fn find_local_refs(doc: &DocAnalysis, name: &str, decl_offset: u32) -> Vec<Occurrence> {
    let root = SyntaxNode::new_root(doc.parsed.green.clone());
    let scope = enclosing_scope(&root, decl_offset).unwrap_or(root);
    let mut out = Vec::new();
    collect_idents(&scope, name, &mut out);
    dedup_sorted(out)
}

/// True iff `s` is a valid Mighty IDENT and not a reserved keyword.
pub fn is_valid_ident(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    if is_keyword(s) {
        return false;
    }
    true
}

fn is_keyword(s: &str) -> bool {
    // Mirror of crate::completion::KEYWORDS plus a few extras the parser
    // treats as reserved (`true`, `false`, `child`, `on_fail`, ...).
    matches!(
        s,
        "agent"
            | "arena"
            | "as"
            | "async"
            | "await"
            | "backoff"
            | "break"
            | "budget"
            | "cap"
            | "child"
            | "const"
            | "continue"
            | "derive"
            | "detach"
            | "dyn"
            | "effect"
            | "else"
            | "enum"
            | "export"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "import"
            | "in"
            | "join"
            | "let"
            | "loop"
            | "macro"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "on"
            | "on_fail"
            | "package"
            | "protocol"
            | "pub"
            | "ref"
            | "requires"
            | "restart"
            | "return"
            | "run"
            | "sandbox"
            | "scope"
            | "self"
            | "spawn"
            | "state"
            | "struct"
            | "sup"
            | "task"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "up_to"
            | "use"
            | "where"
            | "while"
            | "with"
            | "yield"
    )
}

/// Walk `node` collecting `IDENT` tokens whose text matches `name`.
fn collect_idents(node: &SyntaxNode, name: &str, out: &mut Vec<Occurrence>) {
    for d in node.descendants_with_tokens() {
        if let Some(t) = d.as_token() {
            if t.kind() == SyntaxKind::IDENT && t.text() == name {
                let r: TextRange = t.text_range();
                out.push(Occurrence {
                    start: r.start().into(),
                    end: r.end().into(),
                });
            }
        }
    }
}

/// Find the smallest enclosing scope (fn body, agent handler body, or
/// closure body) that contains `offset`. Returns the BLOCK node if one
/// is found, or `None` if `offset` is outside every block (in which
/// case the caller should fall back to a file-wide scan).
fn enclosing_scope(root: &SyntaxNode, offset: u32) -> Option<SyntaxNode> {
    let pos = TextSize::from(offset);
    let mut found: Option<SyntaxNode> = None;
    for n in root.descendants() {
        if !matches!(n.kind(), SyntaxKind::BLOCK | SyntaxKind::ON_HANDLER) {
            continue;
        }
        if n.text_range().contains(pos) {
            // Prefer the smallest enclosing scope.
            match &found {
                None => found = Some(n.clone()),
                Some(prev) => {
                    if n.text_range().len() < prev.text_range().len() {
                        found = Some(n.clone());
                    }
                }
            }
        }
    }
    found
}

/// Find the IDENT token at `offset` if any. Used by rename /
/// prepareRename / signature help.
pub fn ident_at(root: &SyntaxNode, offset: u32) -> Option<SyntaxToken> {
    let pos = TextSize::from(offset);
    let len = root.text_range().len();
    let pos = if pos >= len {
        len.checked_sub(TextSize::from(1))?
    } else {
        pos
    };
    match root.token_at_offset(pos) {
        rowan::TokenAtOffset::None => None,
        rowan::TokenAtOffset::Single(t) => Some(t),
        rowan::TokenAtOffset::Between(a, b) => {
            if a.kind() == SyntaxKind::IDENT {
                Some(a)
            } else {
                Some(b)
            }
        }
    }
}

fn dedup_sorted(mut occs: Vec<Occurrence>) -> Vec<Occurrence> {
    occs.sort_by_key(|o| o.start);
    occs.dedup_by_key(|o| o.start);
    occs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_idents() {
        assert!(is_valid_ident("foo"));
        assert!(is_valid_ident("foo_bar"));
        assert!(is_valid_ident("_underscore"));
        assert!(is_valid_ident("camelCase"));
        assert!(is_valid_ident("PascalCase"));
        assert!(!is_valid_ident(""));
        assert!(!is_valid_ident("1foo"));
        assert!(!is_valid_ident("foo-bar"));
        assert!(!is_valid_ident("fn")); // keyword
        assert!(!is_valid_ident("let")); // keyword
        assert!(!is_valid_ident("Hello World")); // space
    }
}
