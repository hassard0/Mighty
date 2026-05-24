//! Hover support — given a `(uri, position)`, return a human-readable
//! markdown blurb describing the token under the cursor.
//!
//! The v0.2 MVP scope is "show what we can cheaply": the enclosing CST
//! node kind, and — for identifiers that resolve to a top-level
//! definition — the def kind plus a one-line signature.

use crate::docs::DocAnalysis;
use crate::line_index::LineIndex;
use rowan::TextSize;
use mty_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use mty_types::{pretty_ty, DefRef};
use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position, Range};

/// Top-level hover entry.
///
/// Returns `None` if no useful hover information is available at this
/// position (so the client shows nothing rather than an empty box).
pub fn hover(doc: &DocAnalysis, position: Position) -> Option<Hover> {
    let offset = doc
        .line_index
        .position_to_offset(&doc.source, position.line, position.character);
    let root = SyntaxNode::new_root(doc.parsed.green.clone());
    let token = token_at_offset(&root, offset)?;
    let token_text = token.text().to_string();
    let token_kind = token.kind();

    let mut sections: Vec<String> = Vec::new();

    // Identifier-style hover: try to look the name up in the DefMap.
    if matches!(token_kind, SyntaxKind::IDENT) {
        if let Some(rendered) = render_named_def(doc, &token_text) {
            sections.push(rendered);
        } else {
            sections.push(format!("```\n{}\n```", token_text));
        }
    } else {
        // Show the literal token text in a code fence so syntax tokens
        // (keywords, literals) still get a friendly box.
        sections.push(format!("```\n{}\n```", token_text));
    }

    // Always tag with the surrounding node kind for debuggability.
    if let Some(parent) = token.parent() {
        sections.push(format!("_node_: `{:?}`", parent.kind()));
    }
    sections.push(format!("_token_: `{:?}`", token_kind));

    let range = token_range(&token, &doc.line_index, &doc.source);
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: sections.join("\n\n"),
        }),
        range: Some(range),
    })
}

fn render_named_def(doc: &DocAnalysis, name: &str) -> Option<String> {
    let def = doc.typed.def_map.by_name.get(name)?;
    match def {
        DefRef::Fn(id) => {
            let f = doc.typed.def_map.fn_def(*id)?;
            let params: Vec<String> = f
                .params
                .iter()
                .map(|(pn, t)| {
                    format!(
                        "{}: {}",
                        pn,
                        pretty_ty(*t, &doc.typed.ty_arena, None, Some(&doc.typed.def_map))
                    )
                })
                .collect();
            let ret = pretty_ty(f.ret, &doc.typed.ty_arena, None, Some(&doc.typed.def_map));
            let vis = if f.is_pub { "pub " } else { "" };
            let effects = if f.effects.is_empty() {
                String::new()
            } else {
                " effect <...>".to_string()
            };
            Some(format!(
                "```mty\n{vis}fn {}({}) -> {}{}\n```",
                f.name,
                params.join(", "),
                ret,
                effects
            ))
        }
        DefRef::Adt(id) => {
            let a = doc.typed.def_map.adt(*id)?;
            let kw = match a.kind {
                mty_types::AdtKind::Struct => "struct",
                mty_types::AdtKind::Enum => "enum",
                mty_types::AdtKind::Opaque => "type",
            };
            Some(format!("```mty\n{kw} {}\n```", a.name))
        }
        DefRef::Variant(id, idx) => {
            let a = doc.typed.def_map.adt(*id)?;
            let v = a.variants.get(*idx)?;
            Some(format!("```mty\n{}.{}\n```", a.name, v.name))
        }
        DefRef::Module(_) => Some(format!("```mty\nmod {}\n```", name)),
        DefRef::Param(_) => Some(format!("```mty\ntype param {}\n```", name)),
    }
}

fn token_at_offset(root: &SyntaxNode, offset: u32) -> Option<SyntaxToken> {
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
            if is_interesting(a.kind()) {
                Some(a)
            } else {
                Some(b)
            }
        }
    }
}

fn is_interesting(k: SyntaxKind) -> bool {
    matches!(
        k,
        SyntaxKind::IDENT
            | SyntaxKind::INT_LITERAL
            | SyntaxKind::FLOAT_LITERAL
            | SyntaxKind::STRING_LITERAL
            | SyntaxKind::CHAR_LITERAL
            | SyntaxKind::DURATION_LITERAL
            | SyntaxKind::SIZE_LITERAL
            | SyntaxKind::HTML_LITERAL
    )
}

fn token_range(token: &SyntaxToken, line_index: &LineIndex, source: &str) -> Range {
    let r = token.text_range();
    crate::conv::span_to_range(line_index, source, r.start().into(), r.end().into())
}
