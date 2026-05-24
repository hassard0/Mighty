//! Canonical printers for pattern CST nodes.
//!
//! Exposed as library surface for slice 3 (the type checker may want
//! to round-trip patterns through canonical form). Slice 2's `file`
//! printer doesn't drive these — they're tested directly in
//! `tests/canonical.rs`.

use crate::doc::Doc;
use mty_syntax::{SyntaxKind, SyntaxNode};

pub fn pattern(n: &SyntaxNode) -> Doc {
    match n.kind() {
        SyntaxKind::WILDCARD_PAT => Doc::text("_"),
        SyntaxKind::LITERAL_PAT => super::verbatim(n),
        SyntaxKind::IDENT_PAT => super::verbatim(n),
        SyntaxKind::BINDING_PAT => super::verbatim(n),
        SyntaxKind::REF_PAT => super::verbatim(n),
        SyntaxKind::TUPLE_PAT => tuple_pat(n),
        SyntaxKind::STRUCT_PAT => super::verbatim(n),
        SyntaxKind::ENUM_PAT => enum_pat(n),
        SyntaxKind::RANGE_PAT => super::verbatim(n),
        _ => super::verbatim(n),
    }
}

fn tuple_pat(n: &SyntaxNode) -> Doc {
    let parts: Vec<Doc> = n.children().map(|c| pattern(&c)).collect();
    Doc::concat(
        Doc::text("("),
        Doc::concat(Doc::join(Doc::text(", "), parts), Doc::text(")")),
    )
}

fn enum_pat(n: &SyntaxNode) -> Doc {
    let path = n
        .children()
        .find(|c| c.kind() == SyntaxKind::PATH)
        .map(|p| super::types::path_node(&p))
        .unwrap_or(Doc::nil());
    let inner: Vec<Doc> = n
        .children()
        .filter(|c| c.kind() != SyntaxKind::PATH)
        .map(|c| pattern(&c))
        .collect();
    if inner.is_empty() {
        path
    } else {
        Doc::concat(
            path,
            Doc::concat(
                Doc::text("("),
                Doc::concat(Doc::join(Doc::text(", "), inner), Doc::text(")")),
            ),
        )
    }
}
