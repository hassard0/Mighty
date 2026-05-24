//! Rename — `textDocument/rename` + `textDocument/prepareRename`.
//!
//! Strategy:
//! - `prepareRename(uri, pos)`: find the IDENT under the cursor and
//!   return its range as a [`PrepareRenameResponse::Range`]. If the
//!   token isn't an IDENT, return `None` so the editor rejects the
//!   operation cleanly.
//! - `rename(uri, pos, new_name)`: validate `new_name` is a legal
//!   identifier (and not a keyword), find every reference of the
//!   target name (top-level or local; see [`crate::references`]), and
//!   emit a [`WorkspaceEdit`] with one `TextEdit` per occurrence.
//!
//! v0.5 limitations:
//! - Single-file rename only — we don't yet build a cross-file resolve
//!   map in the LSP layer (workspace folders track open files but the
//!   compiler driver still treats each file as its own translation
//!   unit; see `LSP_V0_5_NOTES.md`).
//! - Top-level rename is conservative: every IDENT with the same text
//!   in the file is rewritten. For locals we restrict to the smallest
//!   enclosing block. Shadowing within a fn body is left to the user
//!   to confirm in the editor's preview.

use crate::docs::DocAnalysis;
use crate::references::{
    find_local_refs, find_top_level_refs, ident_at, is_valid_ident, Occurrence,
};
use sdust_syntax::{SyntaxKind, SyntaxNode};
use sdust_types::DefRef;
use std::collections::HashMap;
use tower_lsp::jsonrpc::Error as JsonRpcError;
use tower_lsp::lsp_types::{Position, PrepareRenameResponse, Range, TextEdit, Url, WorkspaceEdit};

/// `textDocument/prepareRename` handler body.
pub fn prepare(doc: &DocAnalysis, pos: Position) -> Option<PrepareRenameResponse> {
    let offset = doc
        .line_index
        .position_to_offset(&doc.source, pos.line, pos.character);
    let root = SyntaxNode::new_root(doc.parsed.green.clone());
    let token = ident_at(&root, offset)?;
    if token.kind() != SyntaxKind::IDENT {
        return None;
    }
    let r = token.text_range();
    let range = crate::conv::span_to_range(
        &doc.line_index,
        &doc.source,
        r.start().into(),
        r.end().into(),
    );
    Some(PrepareRenameResponse::Range(range))
}

/// `textDocument/rename` handler body. Returns the workspace edit or an
/// LSP-friendly error if the new name is invalid / there's nothing to
/// rename.
pub fn rename(
    uri: Url,
    doc: &DocAnalysis,
    pos: Position,
    new_name: &str,
) -> Result<WorkspaceEdit, JsonRpcError> {
    if !is_valid_ident(new_name) {
        return Err(invalid_rename(format!(
            "`{}` is not a valid Stardust identifier",
            new_name
        )));
    }

    let offset = doc
        .line_index
        .position_to_offset(&doc.source, pos.line, pos.character);
    let root = SyntaxNode::new_root(doc.parsed.green.clone());
    let token = ident_at(&root, offset)
        .ok_or_else(|| invalid_rename("no identifier under cursor".to_string()))?;
    if token.kind() != SyntaxKind::IDENT {
        return Err(invalid_rename("not an identifier".to_string()));
    }
    let name = token.text().to_string();

    // Classify: top-level vs local. We treat a name as top-level if it
    // resolves in the DefMap.
    let is_top_level = doc
        .typed
        .def_map
        .by_name
        .get(&name)
        .map(|r| !matches!(r, DefRef::Param(_)))
        .unwrap_or(false);

    let occs: Vec<Occurrence> = if is_top_level {
        find_top_level_refs(doc, &name)
    } else {
        find_local_refs(doc, &name, token.text_range().start().into())
    };

    if occs.is_empty() {
        return Err(invalid_rename("no occurrences found".to_string()));
    }

    let edits: Vec<TextEdit> = occs
        .into_iter()
        .map(|o| {
            let range: Range =
                crate::conv::span_to_range(&doc.line_index, &doc.source, o.start, o.end);
            TextEdit {
                range,
                new_text: new_name.to_string(),
            }
        })
        .collect();

    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    changes.insert(uri, edits);
    Ok(WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    })
}

fn invalid_rename(message: String) -> JsonRpcError {
    JsonRpcError {
        code: tower_lsp::jsonrpc::ErrorCode::InvalidParams,
        message: message.into(),
        data: None,
    }
}
