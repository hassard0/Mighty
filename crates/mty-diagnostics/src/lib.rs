//! mty-diagnostics: diagnostic types and rendering.
pub mod codes;
pub mod codes_fix;
pub mod diagnostic;
pub mod fix;
pub mod render;
pub use codes::DiagCode;
pub use diagnostic::{Diagnostic, Label, Severity};
pub use fix::{
    snippet_around, span_info_from, to_ndjson, DiagnosticEnvelope, Fix, FixAlternative, FixBuilder,
    FixKind, SourceSnippet, SpanInfo, ToEnvelope,
};
