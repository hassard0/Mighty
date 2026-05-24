//! Conversion helpers between Mighty diagnostic / span types and the
//! corresponding `lsp-types` types.

use crate::line_index::LineIndex;
use mty_diagnostics::Severity;
use tower_lsp::lsp_types as lt;

/// Convert a byte-offset span (half-open `[start, end)`) into an LSP
/// `Range`. The `source` and `line_index` must correspond to the same
/// document version.
pub fn span_to_range(line_index: &LineIndex, source: &str, start: u32, end: u32) -> lt::Range {
    let (sl, sc) = line_index.offset_to_position(source, start);
    let (el, ec) = line_index.offset_to_position(source, end);
    lt::Range {
        start: lt::Position {
            line: sl,
            character: sc,
        },
        end: lt::Position {
            line: el,
            character: ec,
        },
    }
}

/// Map a Mighty [`Severity`] onto the LSP severity enum. We collapse
/// `Note` / `Help` to `Information` / `Hint` respectively so editors
/// render them with reduced visual weight.
pub fn severity_to_lsp(s: Severity) -> lt::DiagnosticSeverity {
    match s {
        Severity::Error => lt::DiagnosticSeverity::ERROR,
        Severity::Warning => lt::DiagnosticSeverity::WARNING,
        Severity::Note => lt::DiagnosticSeverity::INFORMATION,
        Severity::Help => lt::DiagnosticSeverity::HINT,
    }
}

/// Convert a single [`mty_diagnostics::Diagnostic`] to its LSP form.
///
/// Related-information `Location`s carry a placeholder URI here; the
/// caller (see [`crate::diagnostics::build_publish`]) rewrites them to
/// the document URI before publishing.
pub fn diagnostic_to_lsp(
    d: &mty_diagnostics::Diagnostic,
    line_index: &LineIndex,
    source: &str,
) -> lt::Diagnostic {
    let range = span_to_range(
        line_index,
        source,
        d.primary.start as u32,
        d.primary.end as u32,
    );
    let related: Vec<_> = d
        .secondary
        .iter()
        .map(|l| lt::DiagnosticRelatedInformation {
            location: lt::Location {
                uri: lt::Url::parse("file:///__placeholder__").unwrap(),
                range: span_to_range(line_index, source, l.start as u32, l.end as u32),
            },
            message: l.message.clone(),
        })
        .collect();
    let mut message = d.primary.message.clone();
    for note in &d.notes {
        message.push_str("\nnote: ");
        message.push_str(note);
    }
    for help in &d.helps {
        message.push_str("\nhelp: ");
        message.push_str(help);
    }
    lt::Diagnostic {
        range,
        severity: Some(severity_to_lsp(d.severity)),
        code: Some(lt::NumberOrString::String(d.code.as_str())),
        code_description: None,
        source: Some("stardust".to_string()),
        message,
        related_information: if related.is_empty() {
            None
        } else {
            Some(related)
        },
        tags: None,
        data: None,
    }
}
