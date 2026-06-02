//! mty-diagnostics: diagnostic types and rendering.
pub mod apply;
pub mod codes;
pub mod codes_fix;
pub mod diagnostic;
pub mod fix;
pub mod render;
pub use apply::{apply_unified_diff, try_apply_alternatives};
pub use codes::DiagCode;
pub use diagnostic::{Diagnostic, Label, Severity};
pub use fix::{
    build_check_result, snippet_around, span_info_from, to_check_result_json, to_ndjson,
    CheckDiagnostic, CheckResult, CheckSpan, DiagnosticEnvelope, Fix, FixAlternative, FixBuilder,
    FixKind, SourceSnippet, SpanInfo, ToEnvelope,
};
