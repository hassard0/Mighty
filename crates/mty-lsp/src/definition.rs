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

use crate::docs::DocAnalysis;
use mty_hir::{Item, SourceSpan};
use mty_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use rowan::TextSize;
use tower_lsp::lsp_types::{GotoDefinitionResponse, Location, Position, Url};

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
    let range = crate::conv::span_to_range(&doc.line_index, &doc.source, span.start, span.end);
    Some(GotoDefinitionResponse::Scalar(Location { uri, range }))
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
