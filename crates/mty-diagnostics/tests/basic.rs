use mty_diagnostics::codes::UNEXPECTED_TOKEN;
use mty_diagnostics::*;

#[test]
fn code_format() {
    assert_eq!(UNEXPECTED_TOKEN.as_str(), "MT0001");
}

#[test]
fn build_diagnostic() {
    let d = Diagnostic::error(
        UNEXPECTED_TOKEN,
        Label {
            start: 5,
            end: 8,
            message: "here".into(),
        },
    )
    .with_note("try removing the token")
    .with_help("see MT0001 reference");
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(d.notes.len(), 1);
    assert_eq!(d.helps.len(), 1);
}
