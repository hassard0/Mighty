//! Per-document analysis cache.
//!
//! On every change we re-parse + lower + type-check the affected
//! document. The cached `DocAnalysis` is stored keyed by `Url` and
//! reused by hover, definition, completion, and formatting.

use crate::line_index::LineIndex;
use dashmap::DashMap;
use mty_diagnostics::Diagnostic;
use mty_driver::{lower, parse_source, ParsedFile};
use mty_hir::Package;
use mty_types::TypedPackage;
use std::sync::Arc;
use tower_lsp::lsp_types::Url;

/// All compiler artifacts for one open file.
pub struct DocAnalysis {
    pub source: String,
    pub version: i32,
    pub line_index: LineIndex,
    pub parsed: ParsedFile,
    pub package: Package,
    pub typed: TypedPackage,
    pub diagnostics: Vec<Diagnostic>,
}

impl DocAnalysis {
    /// Run the full parse → lower → type-check pipeline for `source`.
    pub fn analyze(source: String, source_id: String, version: i32) -> Self {
        let line_index = LineIndex::new(&source);
        let parsed = parse_source(source.clone(), source_id);
        let (package, lower_diags) = lower(&parsed);
        let mut diagnostics: Vec<Diagnostic> = lower_diags;
        let has_lower_error = diagnostics
            .iter()
            .any(|d| matches!(d.severity, mty_diagnostics::Severity::Error));
        let typed = if has_lower_error {
            TypedPackage::default()
        } else {
            let typed = mty_types::check_package_typed(&package);
            diagnostics.extend(typed.diagnostics.clone());
            // v0.2 MVP: surface parse + lower + type-check diagnostics.
            // Borrow check is deferred (it shares state with the typed
            // package and adds a heavyweight dep — `mty check` from
            // the CLI still runs it).
            typed
        };
        Self {
            source,
            version,
            line_index,
            parsed,
            package,
            typed,
            diagnostics,
        }
    }
}

/// Thread-safe map of open URIs → most-recent analysis.
#[derive(Default)]
pub struct DocStore {
    inner: DashMap<Url, Arc<DocAnalysis>>,
}

impl DocStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(&self, uri: Url, source: String, version: i32) -> Arc<DocAnalysis> {
        let analysis = Arc::new(DocAnalysis::analyze(source, uri.to_string(), version));
        self.inner.insert(uri, analysis.clone());
        analysis
    }

    pub fn update(&self, uri: Url, source: String, version: i32) -> Arc<DocAnalysis> {
        let analysis = Arc::new(DocAnalysis::analyze(source, uri.to_string(), version));
        self.inner.insert(uri, analysis.clone());
        analysis
    }

    pub fn close(&self, uri: &Url) {
        self.inner.remove(uri);
    }

    pub fn get(&self, uri: &Url) -> Option<Arc<DocAnalysis>> {
        self.inner.get(uri).map(|r| r.clone())
    }
}

/// Apply an incremental edit (LSP `Range` based) or a full-document
/// replacement to `source`, returning the new text.
///
/// Returns `None` if any edit's range refers to an invalid position
/// (caller may then fall back to requesting a full sync).
pub fn apply_change(
    source: &str,
    line_index: &LineIndex,
    change: &tower_lsp::lsp_types::TextDocumentContentChangeEvent,
) -> Option<String> {
    match &change.range {
        None => Some(change.text.clone()),
        Some(range) => {
            let start =
                line_index.position_to_offset(source, range.start.line, range.start.character)
                    as usize;
            let end =
                line_index.position_to_offset(source, range.end.line, range.end.character) as usize;
            if start > end || end > source.len() {
                return None;
            }
            let mut out = String::with_capacity(source.len() + change.text.len());
            out.push_str(&source[..start]);
            out.push_str(&change.text);
            out.push_str(&source[end..]);
            Some(out)
        }
    }
}
