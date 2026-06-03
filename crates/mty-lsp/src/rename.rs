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
use crate::workspace::WorkspaceRegistry;
use mty_syntax::{SyntaxKind, SyntaxNode};
use mty_types::DefRef;
use std::collections::HashMap;
use tower_lsp::jsonrpc::Error as JsonRpcError;
use tower_lsp::lsp_types::{
    DocumentChanges, OneOf, OptionalVersionedTextDocumentIdentifier, Position,
    PrepareRenameResponse, Range, TextDocumentEdit, TextEdit, Url, WorkspaceEdit,
};

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
///
/// Single-file path: the v0.5 behaviour. Use
/// [`rename_with_workspace`] for the v0.8 cross-file variant.
pub fn rename(
    uri: Url,
    doc: &DocAnalysis,
    pos: Position,
    new_name: &str,
) -> Result<WorkspaceEdit, JsonRpcError> {
    rename_with_workspace(uri, doc, pos, new_name, None)
}

/// v0.8 cross-file rename. If `workspace` is supplied AND the target
/// symbol is top-level + public, the edit includes references from
/// every file in the same workspace folder. Falls back to single-file
/// behaviour otherwise.
///
/// This is the legacy `changes`-shaped entrypoint preserved for
/// back-compat — v0.46 T5 and earlier IDE/L31 clients call this.
/// New callers should use [`rename_with_caps`] which honours the
/// client's `workspace.workspaceEdit.documentChanges` capability.
pub fn rename_with_workspace(
    uri: Url,
    doc: &DocAnalysis,
    pos: Position,
    new_name: &str,
    workspace: Option<&WorkspaceRegistry>,
) -> Result<WorkspaceEdit, JsonRpcError> {
    // Preserve legacy `changes` shape — the IDE L31 path predates the
    // v0.47 T5 documentChanges migration and parses `changes`.
    rename_with_caps(uri, doc, pos, new_name, workspace, false)
}

/// v0.47 T5 cross-file rename with capability negotiation. When
/// `document_changes_support` is `true`, the returned [`WorkspaceEdit`]
/// uses the LSP-3.16+ `documentChanges` shape — a
/// `Vec<TextDocumentEdit>` with each entry carrying an
/// [`OptionalVersionedTextDocumentIdentifier`] so the editor can
/// version-check the buffer before applying. When `false`, falls back
/// to the legacy `changes: HashMap<Url, Vec<TextEdit>>` shape that
/// v0.2-vintage clients understand.
///
/// The buffer version threaded into the per-file
/// `OptionalVersionedTextDocumentIdentifier` is read off the open
/// document's [`DocAnalysis::version`] field (set on every
/// `didOpen` / `didChange`).
pub fn rename_with_caps(
    uri: Url,
    doc: &DocAnalysis,
    pos: Position,
    new_name: &str,
    workspace: Option<&WorkspaceRegistry>,
    document_changes_support: bool,
) -> Result<WorkspaceEdit, JsonRpcError> {
    if !is_valid_ident(new_name) {
        return Err(invalid_rename(format!(
            "`{}` is not a valid Mighty identifier",
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

    // Collect per-file (uri, version, edits) so we can emit either
    // shape — `changes` (legacy) or `documentChanges` (versioned) —
    // depending on the client capability.
    //
    // The current file's version is the open buffer's version; for
    // cross-file edits the workspace index tracks the per-file analysis
    // version (default `0` for on-disk files that haven't been opened
    // for editing yet).
    let mut per_file: Vec<(Url, Option<i32>, Vec<TextEdit>)> = Vec::new();
    per_file.push((uri.clone(), Some(doc.version), edits));

    // v0.8: if the symbol is top-level + the caller supplied a
    // workspace registry, harvest references from every other file in
    // the same folder. The current file is already covered above; we
    // skip its URI to avoid duplicate edits.
    if is_top_level {
        if let Some(reg) = workspace {
            if let Some(path) = reg.folders.iter().find_map(|kv| {
                if uri
                    .to_file_path()
                    .map(|p| p.starts_with(kv.key()))
                    .unwrap_or(false)
                {
                    Some(kv.value().clone())
                } else {
                    None
                }
            }) {
                for (file, occs) in path.find_refs_across_files(&name) {
                    if file.uri == uri {
                        continue;
                    }
                    let other_doc = file.analysis.as_ref();
                    let edits: Vec<TextEdit> = occs
                        .into_iter()
                        .map(|o| TextEdit {
                            range: crate::conv::span_to_range(
                                &other_doc.line_index,
                                &other_doc.source,
                                o.start,
                                o.end,
                            ),
                            new_text: new_name.to_string(),
                        })
                        .collect();
                    if !edits.is_empty() {
                        // For workspace files that have an open buffer
                        // we have a real version; for the on-disk
                        // `analyze_path` shape we have `0` — pass it
                        // through verbatim. `OptionalVersionedTextDocumentIdentifier`
                        // permits `null` so we hand the editor a
                        // version of `None` when the file was never
                        // opened, signalling "no version check".
                        let v = other_doc.version;
                        let version_field = if v <= 0 { None } else { Some(v) };
                        per_file.push((file.uri.clone(), version_field, edits));
                    }
                }
            }
        }
    }

    if document_changes_support {
        let document_changes: Vec<TextDocumentEdit> = per_file
            .into_iter()
            .map(|(uri, version, edits)| TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier { uri, version },
                edits: edits.into_iter().map(OneOf::Left).collect(),
            })
            .collect();
        Ok(WorkspaceEdit {
            changes: None,
            document_changes: Some(DocumentChanges::Edits(document_changes)),
            change_annotations: None,
        })
    } else {
        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
        for (uri, _, edits) in per_file {
            // Merge in case the same URI appears twice (defensive — the
            // current logic always uses one entry per URI).
            changes.entry(uri).or_default().extend(edits);
        }
        Ok(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
    }
}

fn invalid_rename(message: String) -> JsonRpcError {
    JsonRpcError {
        code: tower_lsp::jsonrpc::ErrorCode::InvalidParams,
        message: message.into(),
        data: None,
    }
}
