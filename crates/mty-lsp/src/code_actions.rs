//! Code actions — `textDocument/codeAction`.
//!
//! v0.5 ships quick fixes triggered by selected diagnostics:
//!
//! | code | fix                                                                 |
//! |------|---------------------------------------------------------------------|
//! | MT2021 unresolved value  | suggest top-3 in-scope names by edit distance |
//! | MT2002 unresolved type   | suggest top-3 in-scope type names by edit distance |
//! | MT3001 use-after-move    | suggest `.clone()` (best-effort; user confirms)    |
//! | MT4001 effect undeclared | suggest adding the missing effect to the fn signature |
//!
//! Each action returns a single-text-edit [`WorkspaceEdit`] so the
//! editor can preview before applying.

use crate::docs::DocAnalysis;
use mty_diagnostics::Diagnostic;
use mty_types::DefRef;
use std::collections::HashMap;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionResponse, NumberOrString, Position,
    Range, TextEdit, Url, WorkspaceEdit,
};

/// Maximum edit distance for "did you mean" suggestions.
const SUGGEST_MAX_DISTANCE: usize = 2;

/// Top-level handler entry. `cursor_range` is the editor's current
/// selection (so we can scope the suggestions); `diagnostics` is the
/// list the client thinks applies at that range.
pub fn code_actions(
    uri: &Url,
    doc: &DocAnalysis,
    cursor_range: Range,
    diagnostics: &[tower_lsp::lsp_types::Diagnostic],
) -> CodeActionResponse {
    let mut out: Vec<CodeActionOrCommand> = Vec::new();
    for diag in diagnostics {
        // Pull our SD-code string out of the diagnostic.
        let Some(NumberOrString::String(code)) = &diag.code else {
            continue;
        };
        match code.as_str() {
            "MT2021" => actions_for_unresolved_value(uri, doc, diag, &mut out),
            "MT2002" => actions_for_unresolved_type(uri, doc, diag, &mut out),
            "MT3001" => actions_for_use_after_move(uri, doc, diag, &mut out),
            "MT4001" => actions_for_effect_undeclared(uri, doc, diag, &mut out),
            "MT6001" => actions_for_unknown_macro(uri, doc, diag, &mut out),
            _ => {}
        }
    }
    // Even without diagnostics, sometimes the editor calls codeAction
    // with an empty diag list to populate the lightbulb. We also try to
    // match any cursor-position diagnostic the server itself produced
    // (so the action works on first paint, before the client round-trips
    // the diagnostic back).
    if diagnostics.is_empty() {
        for d in &doc.diagnostics {
            if diag_contains_cursor(d, doc, cursor_range) {
                let lsp_diag = crate::conv::diagnostic_to_lsp(d, &doc.line_index, &doc.source);
                match d.code.as_str().as_str() {
                    "MT2021" => actions_for_unresolved_value(uri, doc, &lsp_diag, &mut out),
                    "MT2002" => actions_for_unresolved_type(uri, doc, &lsp_diag, &mut out),
                    "MT3001" => actions_for_use_after_move(uri, doc, &lsp_diag, &mut out),
                    "MT4001" => actions_for_effect_undeclared(uri, doc, &lsp_diag, &mut out),
                    "MT6001" => actions_for_unknown_macro(uri, doc, &lsp_diag, &mut out),
                    _ => {}
                }
            }
        }
    }
    out
}

fn actions_for_unknown_macro(
    uri: &Url,
    doc: &DocAnalysis,
    diag: &tower_lsp::lsp_types::Diagnostic,
    out: &mut Vec<CodeActionOrCommand>,
) {
    let bad = extract_name_from_range(doc, diag.range);
    if bad.is_empty() {
        return;
    }
    // Macros are top-level items lowered into the HIR; their names are
    // surfaced via Item::Macro nodes. Collect them by walking the
    // package's items so the suggestion list reflects the real set
    // of macros visible in this file.
    let mut macro_names: Vec<String> = Vec::new();
    for iid in &doc.package.top_level {
        if let mty_hir::Item::Macro(m) = &doc.package.items[*iid] {
            macro_names.push(m.name.clone());
        }
    }
    let sugg = nearest(&bad, &macro_names);
    for s in sugg {
        out.push(quickfix_replace(
            uri,
            diag.clone(),
            diag.range,
            s.clone(),
            format!("Replace macro `{}` with `{}`", bad, s),
        ));
    }
}

fn diag_contains_cursor(d: &Diagnostic, doc: &DocAnalysis, cursor: Range) -> bool {
    let r = crate::conv::span_to_range(
        &doc.line_index,
        &doc.source,
        d.primary.start as u32,
        d.primary.end as u32,
    );
    !(r.end.line < cursor.start.line || r.start.line > cursor.end.line)
}

fn actions_for_unresolved_value(
    uri: &Url,
    doc: &DocAnalysis,
    diag: &tower_lsp::lsp_types::Diagnostic,
    out: &mut Vec<CodeActionOrCommand>,
) {
    let bad = extract_name_from_range(doc, diag.range);
    if bad.is_empty() {
        return;
    }
    let candidates: Vec<String> = doc
        .typed
        .def_map
        .by_name
        .iter()
        .filter(|(_, r)| matches!(r, DefRef::Fn(_) | DefRef::Variant(_, _) | DefRef::Module(_)))
        .map(|(k, _)| k.clone())
        .collect();
    let suggestions = nearest(&bad, &candidates);
    for sugg in suggestions {
        out.push(quickfix_replace(
            uri,
            diag.clone(),
            diag.range,
            sugg.clone(),
            format!("Replace `{}` with `{}`", bad, sugg),
        ));
    }
}

fn actions_for_unresolved_type(
    uri: &Url,
    doc: &DocAnalysis,
    diag: &tower_lsp::lsp_types::Diagnostic,
    out: &mut Vec<CodeActionOrCommand>,
) {
    let bad = extract_name_from_range(doc, diag.range);
    if bad.is_empty() {
        return;
    }
    let candidates: Vec<String> = doc
        .typed
        .def_map
        .by_name
        .iter()
        .filter(|(_, r)| matches!(r, DefRef::Adt(_) | DefRef::Param(_)))
        .map(|(k, _)| k.clone())
        .collect();
    let suggestions = nearest(&bad, &candidates);
    for sugg in suggestions {
        out.push(quickfix_replace(
            uri,
            diag.clone(),
            diag.range,
            sugg.clone(),
            format!("Replace type `{}` with `{}`", bad, sugg),
        ));
    }
}

fn actions_for_use_after_move(
    uri: &Url,
    doc: &DocAnalysis,
    diag: &tower_lsp::lsp_types::Diagnostic,
    out: &mut Vec<CodeActionOrCommand>,
) {
    // Pragmatic: suggest inserting `.clone()` after the moved expression.
    // The user gets to verify in the preview before applying — for non-
    // Copy types without a Clone impl this won't compile and they'll
    // see the new diagnostic.
    let target = extract_name_from_range(doc, diag.range);
    if target.is_empty() {
        return;
    }
    let end = diag.range.end;
    let edit = TextEdit {
        range: Range { start: end, end },
        new_text: ".clone()".to_string(),
    };
    out.push(quickfix_with_edit(
        uri,
        diag.clone(),
        vec![edit],
        format!("Add `.clone()` after `{}`", target),
    ));
}

fn actions_for_effect_undeclared(
    uri: &Url,
    doc: &DocAnalysis,
    diag: &tower_lsp::lsp_types::Diagnostic,
    out: &mut Vec<CodeActionOrCommand>,
) {
    // We don't have a precise "missing effect name" parser, but the
    // diagnostic's message typically embeds it. Extract a quoted name.
    let effect = extract_quoted(&diag.message).unwrap_or_else(|| "io".to_string());
    // Locate the enclosing fn and decide where to insert.
    let Some(insert_pos) = find_effect_insert_pos(doc, diag.range) else {
        return;
    };
    let edit = TextEdit {
        range: Range {
            start: insert_pos,
            end: insert_pos,
        },
        new_text: format!(" effect {{ {} }}", effect),
    };
    out.push(quickfix_with_edit(
        uri,
        diag.clone(),
        vec![edit],
        format!("Add `effect {{ {} }}` to fn signature", effect),
    ));
}

fn find_effect_insert_pos(doc: &DocAnalysis, diag_range: Range) -> Option<Position> {
    use mty_syntax::{SyntaxKind, SyntaxNode};
    let offset = doc.line_index.position_to_offset(
        &doc.source,
        diag_range.start.line,
        diag_range.start.character,
    );
    let root = SyntaxNode::new_root(doc.parsed.green.clone());
    // Find the smallest enclosing FN_DECL that contains the offset.
    let pos = rowan::TextSize::from(offset);
    let mut best: Option<SyntaxNode> = None;
    for n in root.descendants() {
        if n.kind() == SyntaxKind::FN_DECL && n.text_range().contains(pos) {
            match &best {
                None => best = Some(n.clone()),
                Some(prev) if n.text_range().len() < prev.text_range().len() => {
                    best = Some(n.clone())
                }
                _ => {}
            }
        }
    }
    let fn_decl = best?;
    // Insert before the body block.
    let block = fn_decl.children().find(|c| c.kind() == SyntaxKind::BLOCK)?;
    let insert_byte: u32 = block.text_range().start().into();
    let (line, character) = doc.line_index.offset_to_position(&doc.source, insert_byte);
    Some(Position { line, character })
}

fn extract_quoted(msg: &str) -> Option<String> {
    // Pull the first backtick-delimited or single-quote-delimited token.
    let mut chars = msg.chars();
    while let Some(c) = chars.next() {
        if c == '`' || c == '\'' {
            let close = c;
            let s: String = chars.by_ref().take_while(|c| *c != close).collect();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

fn extract_name_from_range(doc: &DocAnalysis, range: Range) -> String {
    let start =
        doc.line_index
            .position_to_offset(&doc.source, range.start.line, range.start.character)
            as usize;
    let end =
        doc.line_index
            .position_to_offset(&doc.source, range.end.line, range.end.character) as usize;
    if start > end || end > doc.source.len() {
        return String::new();
    }
    doc.source[start..end].to_string()
}

fn quickfix_replace(
    uri: &Url,
    diag: tower_lsp::lsp_types::Diagnostic,
    range: Range,
    new_text: String,
    title: String,
) -> CodeActionOrCommand {
    let edit = TextEdit { range, new_text };
    quickfix_with_edit(uri, diag, vec![edit], title)
}

fn quickfix_with_edit(
    uri: &Url,
    diag: tower_lsp::lsp_types::Diagnostic,
    edits: Vec<TextEdit>,
    title: String,
) -> CodeActionOrCommand {
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    changes.insert(uri.clone(), edits);
    CodeActionOrCommand::CodeAction(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag]),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: Some(true),
        disabled: None,
        data: None,
    })
}

/// Return the up to 3 closest candidates to `target` by edit distance,
/// filtering to those within [`SUGGEST_MAX_DISTANCE`].
fn nearest(target: &str, candidates: &[String]) -> Vec<String> {
    let mut scored: Vec<(usize, &String)> = candidates
        .iter()
        .filter(|c| c.as_str() != target)
        .map(|c| (edit_distance(target, c), c))
        .filter(|(d, _)| *d <= SUGGEST_MAX_DISTANCE)
        .collect();
    scored.sort_by_key(|(d, _)| *d);
    scored.into_iter().take(3).map(|(_, c)| c.clone()).collect()
}

/// Levenshtein distance with the standard O(m*n) DP.
fn edit_distance(a: &str, b: &str) -> usize {
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let m = ac.len();
    let n = bc.len();
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut cur: Vec<usize> = vec![0; n + 1];
    for i in 1..=m {
        cur[0] = i;
        for j in 1..=n {
            let cost = if ac[i - 1] == bc[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[n]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_basics() {
        assert_eq!(edit_distance("foo", "foo"), 0);
        assert_eq!(edit_distance("foo", "fob"), 1);
        assert_eq!(edit_distance("foo", "fo"), 1);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn nearest_picks_within_threshold() {
        let cands: Vec<String> = ["greet", "great", "creep", "wildly_different"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let n = nearest("grret", &cands);
        assert!(n.contains(&"greet".to_string()));
        assert!(!n.contains(&"wildly_different".to_string()));
    }

    #[test]
    fn extract_quoted_picks_first() {
        assert_eq!(extract_quoted("missing `io` effect"), Some("io".into()));
        assert_eq!(extract_quoted("no quotes here"), None);
    }
}
