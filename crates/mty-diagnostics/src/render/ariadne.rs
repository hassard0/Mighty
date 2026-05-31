use crate::{Diagnostic, Severity};
use ariadne::{Color, Config, Label as AriadneLabel, Report, ReportKind, Source};

/// v0.42 T6 (L22 fix 2) — true iff colored output (ANSI SGR escapes) is
/// appropriate for this process. We honor the two conventional opt-outs:
///
///   * `NO_COLOR=<anything>` — the cross-tool convention (https://no-color.org).
///     Presence alone (any value) disables color.
///   * `TERM=dumb`           — used by Emacs M-x compile, CI workers, and the
///     Mighty IDE's embedded process driver, all of which want plain text.
///
/// The default (both vars unset, or `TERM` set to something other than
/// `dumb`) preserves the historic colored behavior. The diagnostics
/// crate is `no-std`-friendly only at the API surface; it already pulls
/// in `std` via `ariadne`, so a direct `std::env::var` call is fine.
fn ansi_color_enabled() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    !matches!(std::env::var("TERM"), Ok(t) if t == "dumb")
}

pub fn render(diag: &Diagnostic, source_id: &str, source: &str) -> String {
    render_with_color(diag, source_id, source, ansi_color_enabled())
}

/// Lower-level renderer that takes the color decision as a parameter.
/// Exposed so tests can pin the behavior without mutating process env
/// vars (which would race with other parallel tests).
pub fn render_with_color(
    diag: &Diagnostic,
    source_id: &str,
    source: &str,
    colored: bool,
) -> String {
    let kind = match diag.severity {
        Severity::Error => ReportKind::Error,
        Severity::Warning => ReportKind::Warning,
        Severity::Note => ReportKind::Advice,
        Severity::Help => ReportKind::Advice,
    };
    // `Config::with_color(false)` runs every label color through
    // `Config::filter_color`, which drops `Some(Color::Red|Yellow|...)`
    // to `None` so ariadne's renderer emits no SGR escapes — both for
    // the per-label highlights below and for the report chrome
    // (line numbers, gutters, etc.). The filter is applied at
    // `add_label` time, so the order matters: `with_config` must
    // come BEFORE the `with_label` chain (we set it first below).
    let mut builder = Report::build(kind, source_id, diag.primary.start)
        .with_config(Config::default().with_color(colored))
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
    report
        .write((source_id, Source::from(source)), &mut buf)
        .unwrap();
    String::from_utf8(buf).unwrap()
}

pub fn render_all(diags: &[Diagnostic], source_id: &str, source: &str) -> String {
    diags
        .iter()
        .map(|d| render(d, source_id, source))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{codes::DiagCode, Diagnostic, Label};

    fn sample_diag() -> Diagnostic {
        Diagnostic::error(
            DiagCode::new(2001),
            Label {
                start: 0,
                end: 5,
                message: "type mismatch: expected I32, got Str".to_string(),
            },
        )
    }

    /// L22 fix 2: when `colored=false`, the ariadne renderer must emit
    /// zero ANSI SGR escape sequences. The IDE has been carrying a
    /// `strip_ansi` helper (`mui-sys/src/diagnostics.rs`) precisely
    /// because this used to be unconditionally true; once this test
    /// passes, that helper becomes a no-op safety net.
    #[test]
    fn no_color_renders_without_ansi_escapes() {
        let d = sample_diag();
        let out = render_with_color(&d, "probe.mty", "1 + \"x\"\n", false);
        assert!(
            !out.contains('\x1b'),
            "expected no ANSI escape (0x1B), got: {out:?}"
        );
    }

    /// Sanity: with color enabled (the historic default) the renderer
    /// still emits at least one SGR escape. Guards against regressing
    /// the colored path while plumbing the new switch.
    #[test]
    fn colored_renders_with_ansi_escapes() {
        let d = sample_diag();
        let out = render_with_color(&d, "probe.mty", "1 + \"x\"\n", true);
        assert!(
            out.contains('\x1b'),
            "expected at least one ANSI escape (0x1B) in colored render, got: {out:?}"
        );
    }
}
