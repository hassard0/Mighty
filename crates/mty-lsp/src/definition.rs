//! Go-to-definition support.
//!
//! Strategy: find the identifier under the cursor, look it up in the
//! top-level HIR Package. If a matching fn / struct / enum / type-alias
//! exists, return its `SourceSpan` translated to an LSP `Range`.
//!
//! v0.2 MVP limitations: we resolve **top-level item names only**.
//! Locals, fields, methods, and trait impls require deeper resolution
//! tables (HIR resolve maps) that the v0.2 LSP doesn't surface — a
//! follow-up amendment will wire those up.
//!
//! v0.46 T5: the response shape now uses `GotoDefinitionResponse::Link`
//! (`LocationLink`) so the IDE receives BOTH the `originSelectionRange`
//! (the clicked-on identifier under the cursor) AND the
//! `targetSelectionRange` (the name slice inside the definition,
//! distinct from the full `targetRange` that spans the whole item).
//! Editors that don't grok `LocationLink` still see the legacy
//! `Location` data via the structured envelope, so this is fully
//! back-compat with v0.2-vintage clients.

use crate::docs::DocAnalysis;
use mty_hir::{Item, SourceSpan};
use mty_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use rowan::TextSize;
use tower_lsp::lsp_types::{GotoDefinitionResponse, LocationLink, Position, Url};

pub fn definition(
    uri: Url,
    doc: &DocAnalysis,
    position: Position,
) -> Option<GotoDefinitionResponse> {
    let offset = doc
        .line_index
        .position_to_offset(&doc.source, position.line, position.character);
    let root = SyntaxNode::new_root(doc.parsed.green.clone());
    let token = token_at(&root, offset)?;
    if token.kind() != SyntaxKind::IDENT {
        return None;
    }
    let name = token.text().to_string();
    let span = find_item_span(&doc.package, &name)?;
    let target_range =
        crate::conv::span_to_range(&doc.line_index, &doc.source, span.start, span.end);

    // The clicked-on identifier's range — given to the editor as the
    // `originSelectionRange` so the source highlight shrinks to exactly
    // the identifier rather than the entire word boundary the editor
    // guesses on its own.
    let tok_r = token.text_range();
    let origin_selection_range = Some(crate::conv::span_to_range(
        &doc.line_index,
        &doc.source,
        tok_r.start().into(),
        tok_r.end().into(),
    ));

    // The name-only slice inside the definition span — given to the
    // editor as the `targetSelectionRange` so the destination cursor
    // lands on the symbol's identifier rather than the `fn`/`struct`
    // keyword that prefixes the item.
    let target_selection_range = name_range_in_span(&doc.source, &span, &name)
        .map(|(start, end)| crate::conv::span_to_range(&doc.line_index, &doc.source, start, end))
        .unwrap_or(target_range);

    Some(GotoDefinitionResponse::Link(vec![LocationLink {
        origin_selection_range,
        target_uri: uri,
        target_range,
        target_selection_range,
    }]))
}

fn token_at(root: &SyntaxNode, offset: u32) -> Option<SyntaxToken> {
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

/// Search the package's top-level items for an item whose declared name
/// matches `name`. Returns the item's span on success.
pub(crate) fn find_item_span(pkg: &mty_hir::Package, name: &str) -> Option<SourceSpan> {
    for &iid in &pkg.top_level {
        let item = &pkg.items[iid];
        match item {
            Item::Fn(id) => {
                let f = &pkg.fns[*id];
                if f.name == name {
                    return Some(f.span.clone());
                }
            }
            Item::Struct(id) => {
                let s = &pkg.structs[*id];
                if s.name == name {
                    return Some(s.span.clone());
                }
            }
            Item::Enum(id) => {
                let e = &pkg.enums[*id];
                if e.name == name {
                    return Some(e.span.clone());
                }
            }
            Item::TypeAlias(id) => {
                let t = &pkg.type_aliases[*id];
                if t.name == name {
                    return Some(t.span.clone());
                }
            }
            Item::Agent(id) => {
                let a = &pkg.agents[*id];
                if a.name == name {
                    return Some(a.span.clone());
                }
            }
            Item::Protocol(id) => {
                let p = &pkg.protocols[*id];
                if p.name == name {
                    return Some(p.span.clone());
                }
            }
            Item::Supervisor(id) => {
                let s = &pkg.supervisors[*id];
                if s.name == name {
                    return Some(s.span.clone());
                }
            }
            Item::Const(c) if c.name == name => {
                return Some(c.span.clone());
            }
            Item::Const(_) => {}
            Item::Trait(t) if t.name == name => {
                return Some(t.span.clone());
            }
            Item::Trait(_) => {}
            Item::Macro(m) if m.name == name => {
                return Some(m.span.clone());
            }
            Item::Macro(_) => {}
            // Items without a single user-visible name (use/mod/impl/extern/export/sandbox).
            _ => {}
        }
    }
    None
}

/// v0.46 T5: best-effort locate the identifier `name` inside the
/// definition `span` so the editor can navigate to the name itself
/// rather than the leading `fn`/`struct`/`enum` keyword.
///
/// Returns the byte offsets of the first identifier match found.
/// We scan from the start of the span and stop at the first occurrence
/// whose character boundaries make sense (alpha-numeric on neither
/// side). Falls back to the full span when no isolated match is found
/// (so the caller still has a valid range).
fn name_range_in_span(source: &str, span: &SourceSpan, name: &str) -> Option<(u32, u32)> {
    let start = span.start as usize;
    let end = (span.end as usize).min(source.len());
    if start >= end || name.is_empty() {
        return None;
    }
    let region = &source[start..end];
    let mut search_from = 0usize;
    while let Some(rel) = region[search_from..].find(name) {
        let abs = search_from + rel;
        let before = abs.checked_sub(1).and_then(|i| region.as_bytes().get(i));
        let after = region.as_bytes().get(abs + name.len());
        let isolated =
            before.is_none_or(|b| !is_ident_byte(*b)) && after.is_none_or(|a| !is_ident_byte(*a));
        if isolated {
            let s = start + abs;
            let e = s + name.len();
            return Some((s as u32, e as u32));
        }
        search_from = abs + 1;
        if search_from >= region.len() {
            break;
        }
    }
    None
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
