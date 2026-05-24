use crate::{Diagnostic, Severity};
use ariadne::{Color, Label as AriadneLabel, Report, ReportKind, Source};

pub fn render(diag: &Diagnostic, source_id: &str, source: &str) -> String {
    let kind = match diag.severity {
        Severity::Error => ReportKind::Error,
        Severity::Warning => ReportKind::Warning,
        Severity::Note => ReportKind::Advice,
        Severity::Help => ReportKind::Advice,
    };
    let mut builder = Report::build(kind, source_id, diag.primary.start)
        .with_code(diag.code.as_str())
        .with_message(&diag.primary.message);
    builder = builder.with_label(
        AriadneLabel::new((source_id, diag.primary.start..diag.primary.end))
            .with_message(&diag.primary.message)
            .with_color(Color::Red),
    );
    for sec in &diag.secondary {
        builder = builder.with_label(
            AriadneLabel::new((source_id, sec.start..sec.end))
                .with_message(&sec.message)
                .with_color(Color::Yellow),
        );
    }
    for note in &diag.notes {
        builder = builder.with_note(note);
    }
    for help in &diag.helps {
        builder = builder.with_help(help);
    }
    let report = builder.finish();
    let mut buf = Vec::new();
    report.write((source_id, Source::from(source)), &mut buf).unwrap();
    String::from_utf8(buf).unwrap()
}

pub fn render_all(diags: &[Diagnostic], source_id: &str, source: &str) -> String {
    diags.iter().map(|d| render(d, source_id, source)).collect::<Vec<_>>().join("\n")
}
