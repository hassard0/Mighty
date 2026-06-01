//! `textDocument/documentSymbol` support.
//!
//! This is intentionally CST-backed: outline panels should keep working
//! while the user is editing through temporarily invalid code, and the
//! parser still preserves declaration nodes in many partially-written
//! files.

use crate::docs::DocAnalysis;
use mty_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use tower_lsp::lsp_types::{DocumentSymbol, DocumentSymbolResponse, Position, Range, SymbolKind};

pub fn document_symbols(doc: &DocAnalysis) -> Option<DocumentSymbolResponse> {
    let root = SyntaxNode::new_root(doc.parsed.green.clone());
    let mut out = Vec::new();
    for node in root.children() {
        if let Some(symbol) = symbol_for_node(doc, &node) {
            out.push(symbol);
        }
    }
    Some(DocumentSymbolResponse::Nested(out))
}

#[allow(deprecated)]
fn symbol_for_node(doc: &DocAnalysis, node: &SyntaxNode) -> Option<DocumentSymbol> {
    let (kind, detail) = match node.kind() {
        SyntaxKind::FN_DECL => (SymbolKind::FUNCTION, Some(signature_detail(node))),
        SyntaxKind::STRUCT_DECL => (SymbolKind::STRUCT, None),
        SyntaxKind::ENUM_DECL => (SymbolKind::ENUM, None),
        SyntaxKind::TYPE_ALIAS => (SymbolKind::TYPE_PARAMETER, None),
        SyntaxKind::CONST_DECL => (SymbolKind::CONSTANT, None),
        SyntaxKind::AGENT_DECL => (SymbolKind::CLASS, None),
        SyntaxKind::PROTOCOL_DECL => (SymbolKind::INTERFACE, None),
        SyntaxKind::TRAIT_DECL => (SymbolKind::INTERFACE, None),
        SyntaxKind::IMPL_BLOCK => (SymbolKind::OBJECT, Some("impl".to_string())),
        SyntaxKind::ON_HANDLER => (SymbolKind::METHOD, None),
        SyntaxKind::PROTOCOL_MSG => (SymbolKind::METHOD, Some(signature_detail(node))),
        SyntaxKind::STRUCT_FIELD | SyntaxKind::AGENT_STATE_DECL => (SymbolKind::FIELD, None),
        SyntaxKind::ENUM_VARIANT => (SymbolKind::ENUM_MEMBER, None),
        SyntaxKind::TRAIT_METHOD => (SymbolKind::METHOD, Some(signature_detail(node))),
        _ => return None,
    };
    let name = symbol_name(node)?;
    let range = range_for_node(doc, node);
    let selection_range = selection_range_for_node(doc, node).unwrap_or(range);
    let children = child_symbols(doc, node);
    Some(DocumentSymbol {
        name,
        detail,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    })
}

fn child_symbols(doc: &DocAnalysis, node: &SyntaxNode) -> Vec<DocumentSymbol> {
    let wanted: &[SyntaxKind] = match node.kind() {
        SyntaxKind::STRUCT_DECL => &[SyntaxKind::STRUCT_FIELD],
        SyntaxKind::ENUM_DECL => &[SyntaxKind::ENUM_VARIANT],
        SyntaxKind::AGENT_DECL => &[SyntaxKind::AGENT_STATE_DECL, SyntaxKind::ON_HANDLER],
        SyntaxKind::PROTOCOL_DECL => &[SyntaxKind::PROTOCOL_MSG],
        SyntaxKind::TRAIT_DECL => &[SyntaxKind::TRAIT_METHOD],
        SyntaxKind::IMPL_BLOCK => &[SyntaxKind::FN_DECL],
        _ => &[],
    };
    if wanted.is_empty() {
        return Vec::new();
    }
    node.descendants()
        .filter(|child| wanted.contains(&child.kind()))
        .filter_map(|child| symbol_for_node(doc, &child))
        .collect()
}

fn symbol_name(node: &SyntaxNode) -> Option<String> {
    node.children()
        .find(|child| child.kind() == SyntaxKind::NAME)
        .and_then(first_ident_text)
        .or_else(|| {
            if node.kind() == SyntaxKind::IMPL_BLOCK {
                node.descendants()
                    .find(|child| {
                        matches!(child.kind(), SyntaxKind::TYPE_PATH | SyntaxKind::TYPE_REF)
                    })
                    .and_then(first_ident_text)
                    .map(|name| format!("impl {name}"))
            } else {
                None
            }
        })
}

fn first_ident_text(node: SyntaxNode) -> Option<String> {
    node.descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .find(|token| token.kind() == SyntaxKind::IDENT)
        .map(|token| token.text().to_string())
}

fn selection_range_for_node(doc: &DocAnalysis, node: &SyntaxNode) -> Option<Range> {
    let token = node
        .children()
        .find(|child| child.kind() == SyntaxKind::NAME)
        .and_then(first_ident_token)
        .or_else(|| {
            node.descendants_with_tokens()
                .filter_map(|element| element.into_token())
                .find(|token| token.kind() == SyntaxKind::IDENT)
        })?;
    Some(range_for_token(doc, &token))
}

fn first_ident_token(node: SyntaxNode) -> Option<SyntaxToken> {
    node.descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .find(|token| token.kind() == SyntaxKind::IDENT)
}

fn range_for_node(doc: &DocAnalysis, node: &SyntaxNode) -> Range {
    let text_range = node.text_range();
    let (start_line, start_char) = doc
        .line_index
        .offset_to_position(&doc.source, text_range.start().into());
    let (end_line, end_char) = doc
        .line_index
        .offset_to_position(&doc.source, text_range.end().into());
    Range {
        start: Position {
            line: start_line,
            character: start_char,
        },
        end: Position {
            line: end_line,
            character: end_char,
        },
    }
}

fn range_for_token(doc: &DocAnalysis, token: &SyntaxToken) -> Range {
    let text_range = token.text_range();
    let (start_line, start_char) = doc
        .line_index
        .offset_to_position(&doc.source, text_range.start().into());
    let (end_line, end_char) = doc
        .line_index
        .offset_to_position(&doc.source, text_range.end().into());
    Range {
        start: Position {
            line: start_line,
            character: start_char,
        },
        end: Position {
            line: end_line,
            character: end_char,
        },
    }
}

fn signature_detail(node: &SyntaxNode) -> String {
    node.text()
        .to_string()
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}
