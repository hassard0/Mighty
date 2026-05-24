//! Basic completion support.
//!
//! v0.2 MVP: keyword-only **baseline**. We always propose the Stardust
//! keyword set, and we sprinkle in:
//! - all top-level defs from the type-checker's [`DefMap::by_name`],
//! - if the byte immediately before the cursor is `.`, the built-in
//!   method names from `DefMap::builtin_methods`.
//!
//! Locals-in-scope and per-receiver semantic completion are deferred —
//! see `LSP_PARTIAL.md` / v0.2 amendments.

use crate::docs::DocAnalysis;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, CompletionResponse, Position};

const KEYWORDS: &[&str] = &[
    "agent", "arena", "as", "async", "await", "budget", "cap", "const", "derive", "detach", "dyn",
    "effect", "else", "enum", "export", "extern", "false", "fn", "for", "if", "impl", "import",
    "in", "join", "let", "loop", "macro", "match", "mod", "move", "mut", "on", "package",
    "protocol", "pub", "ref", "requires", "restart", "return", "run", "sandbox", "scope", "self",
    "spawn", "state", "struct", "sup", "task", "trait", "true", "type", "unsafe", "use", "where",
    "while", "with", "yield",
];

pub fn complete(doc: &DocAnalysis, position: Position) -> Option<CompletionResponse> {
    let offset = doc
        .line_index
        .position_to_offset(&doc.source, position.line, position.character);

    let mut items: Vec<CompletionItem> = KEYWORDS
        .iter()
        .map(|kw| CompletionItem {
            label: (*kw).to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("keyword".into()),
            ..Default::default()
        })
        .collect();

    // Method bias: cursor immediately after a `.` triggers built-in
    // method completions. (Semantic completion against the receiver's
    // resolved type is deferred — see LSP_PARTIAL.md.)
    if let Some(prev) = preceding_char(&doc.source, offset) {
        if prev == '.' {
            for name in doc.typed.def_map.builtin_methods.keys() {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::METHOD),
                    detail: Some("built-in method".into()),
                    ..Default::default()
                });
            }
        }
    }

    // Top-level def names — clients sort by kind+label so these land
    // grouped together below the keywords.
    for (name, def) in doc.typed.def_map.by_name.iter() {
        let kind = match def {
            sdust_types::DefRef::Fn(_) => CompletionItemKind::FUNCTION,
            sdust_types::DefRef::Adt(_) => CompletionItemKind::STRUCT,
            sdust_types::DefRef::Variant(_, _) => CompletionItemKind::ENUM_MEMBER,
            sdust_types::DefRef::Module(_) => CompletionItemKind::MODULE,
            sdust_types::DefRef::Param(_) => CompletionItemKind::TYPE_PARAMETER,
        };
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(kind),
            detail: Some("def".into()),
            ..Default::default()
        });
    }

    Some(CompletionResponse::Array(items))
}

fn preceding_char(source: &str, offset: u32) -> Option<char> {
    let off = offset as usize;
    if off == 0 || off > source.len() {
        return None;
    }
    source[..off].chars().next_back()
}
