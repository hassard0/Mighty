//! Diagnostic publishing — convert the cached analysis's diagnostic
//! list into LSP `PublishDiagnosticsParams` and patch any placeholder
//! related-info URIs to point at the same document.

use crate::conv::diagnostic_to_lsp;
use crate::docs::DocAnalysis;
use tower_lsp::lsp_types::{PublishDiagnosticsParams, Url};

/// Build a [`PublishDiagnosticsParams`] payload for `doc` at `uri`.
/// Related-info `Location`s are rewritten to point at `uri` (we currently
/// only generate same-file secondary labels).
pub fn build_publish(uri: Url, doc: &DocAnalysis) -> PublishDiagnosticsParams {
    let lsp_diags = doc
        .diagnostics
        .iter()
        .map(|d| {
            let mut ld = diagnostic_to_lsp(d, &doc.line_index, &doc.source);
            if let Some(related) = ld.related_information.as_mut() {
                for r in related.iter_mut() {
                    r.location.uri = uri.clone();
                }
            }
            ld
        })
        .collect();
    PublishDiagnosticsParams {
        uri,
        diagnostics: lsp_diags,
        version: Some(doc.version),
    }
}
