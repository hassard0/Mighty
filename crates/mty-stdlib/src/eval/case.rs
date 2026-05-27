//! One eval input — either a raw prompt or a recorded trace file.
//!
//! Two case sources today, both producing the same `(prompt, expected)`
//! shape the runner consumes:
//!
//! 1. **`Case::from_input(prompt)`** — a fresh prompt, no recorded
//!    expectation. The runner dispatches the prompt to every member
//!    and compares the replies against each other.
//!
//! 2. **`Case::from_trace(path)`** — a recorded `.mty-trace` produced
//!    by `mty trace record`. The runner extracts the first user-turn
//!    prompt from the trace + the recorded assistant reply (treated
//!    as the "baseline"), then re-runs the prompt against each member
//!    and compares each reply against the baseline + the other
//!    members.
//!
//! The trace path is lazily decoded — construction is cheap; the
//! actual file read happens in [`Case::resolve`] when the runner
//! materialises the case. This keeps `Suite::new(...).case(...)`
//! synchronous + lets the suite builder be constructed in non-async
//! contexts.

use std::path::{Path, PathBuf};

use crate::eval::replay_glue::{decode_trace_baseline, ReplayGlueError};

/// One eval case. Use [`Case::from_input`] for raw prompts and
/// [`Case::from_trace`] for recorded-trace replay.
#[derive(Debug, Clone)]
pub struct Case {
    /// Stable name surfaced in [`super::Report`] per-case rows. Pulled
    /// from the trace filename for [`CaseKind::Trace`], or derived from
    /// the first 24 chars of the prompt for [`CaseKind::Input`].
    pub name: String,
    pub kind: CaseKind,
}

/// Case source — input string vs trace-on-disk.
#[derive(Debug, Clone)]
pub enum CaseKind {
    /// Raw prompt. No recorded baseline — comparators run member-vs-
    /// member.
    Input { prompt: String },
    /// Recorded trace file. The runner resolves to the first user
    /// prompt + the recorded assistant reply.
    Trace { path: PathBuf },
}

/// Materialised case — what the runner actually dispatches. Built by
/// [`Case::resolve`] from a [`Case`] + any disk reads.
#[derive(Debug, Clone)]
pub struct CaseRun {
    /// Display label for the report.
    pub name: String,
    /// Prompt to dispatch to every panel member.
    pub prompt: String,
    /// Recorded assistant reply when this case came from a trace.
    /// `None` for raw-input cases.
    pub baseline_reply: Option<String>,
    /// Source path of the trace (when applicable). Useful for
    /// divergence reports that point back at the recorded fixture.
    pub source_trace: Option<PathBuf>,
}

impl Case {
    /// Build a case from a raw prompt. The case name defaults to the
    /// first 24 chars (trimmed); use [`Case::with_name`] to override.
    pub fn from_input(prompt: impl Into<String>) -> Self {
        let prompt = prompt.into();
        let name = derive_input_name(&prompt);
        Self {
            name,
            kind: CaseKind::Input { prompt },
        }
    }

    /// Build a case from a recorded trace. The path is decoded lazily
    /// — [`Case::resolve`] reads + parses the file at run time.
    pub fn from_trace(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "trace".to_string());
        Self {
            name,
            kind: CaseKind::Trace { path },
        }
    }

    /// Override the auto-derived case name. The runner uses this name
    /// in every report row + every divergence record.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Materialise the case — for [`CaseKind::Input`] this is a
    /// straight conversion; for [`CaseKind::Trace`] this reads the
    /// trace file off disk + extracts the first user prompt + recorded
    /// assistant reply.
    pub fn resolve(&self) -> Result<CaseRun, ReplayGlueError> {
        match &self.kind {
            CaseKind::Input { prompt } => Ok(CaseRun {
                name: self.name.clone(),
                prompt: prompt.clone(),
                baseline_reply: None,
                source_trace: None,
            }),
            CaseKind::Trace { path } => {
                let baseline = decode_trace_baseline(path)?;
                Ok(CaseRun {
                    name: self.name.clone(),
                    prompt: baseline.prompt,
                    baseline_reply: Some(baseline.assistant_reply),
                    source_trace: Some(path.clone()),
                })
            }
        }
    }
}

/// Derive a short case label from a prompt string. Trims whitespace,
/// collapses internal whitespace runs to single spaces, then truncates
/// at 24 chars. Pure / deterministic.
fn derive_input_name(prompt: &str) -> String {
    let collapsed: String = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.len() <= 24 {
        collapsed
    } else {
        // Truncate on a char boundary to avoid splitting a multi-byte
        // codepoint mid-grapheme.
        let mut end = 24;
        while !collapsed.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}…", &collapsed[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn from_input_resolves_to_prompt() {
        let c = Case::from_input("hello world");
        let r = c.resolve().unwrap();
        assert_eq!(r.prompt, "hello world");
        assert_eq!(r.name, "hello world");
        assert!(r.baseline_reply.is_none());
    }

    #[test]
    fn from_input_name_truncates_long_prompt() {
        let c = Case::from_input(
            "this is a very long prompt that should be truncated at 24 chars or so",
        );
        assert!(c.name.ends_with('…'));
        // 24 chars + ellipsis (1 char, 3 bytes); allow some slack for
        // boundary correction.
        assert!(c.name.chars().count() <= 26);
    }

    #[test]
    fn from_input_collapses_internal_whitespace() {
        let c = Case::from_input("hello\n\n   world");
        assert_eq!(c.name, "hello world");
    }

    #[test]
    fn with_name_overrides_derived_label() {
        let c = Case::from_input("hello").with_name("custom-label");
        assert_eq!(c.name, "custom-label");
    }

    #[test]
    fn from_trace_uses_file_stem_as_name() {
        let c = Case::from_trace("traces/research-001.mty-trace");
        // file_stem of "research-001.mty-trace" is
        // "research-001.mty" (Path::file_stem strips only the last
        // extension).
        assert!(c.name.starts_with("research-001"));
    }

    #[test]
    fn from_trace_missing_path_errors_on_resolve() {
        let c = Case::from_trace("/nonexistent/path/never.mty-trace");
        let r = c.resolve();
        assert!(r.is_err());
    }

    #[test]
    fn from_trace_resolves_baseline() {
        // Write a minimal v0.28-eval-format trace file: two JSON lines
        // representing a user prompt + recorded assistant reply.
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        let user = serde_json::json!({
            "type": "user",
            "content": "What's the capital of France?",
        });
        let assistant = serde_json::json!({
            "type": "assistant",
            "content": "Paris",
        });
        writeln!(tmp, "{}", user).unwrap();
        writeln!(tmp, "{}", assistant).unwrap();
        tmp.flush().unwrap();

        let c = Case::from_trace(tmp.path());
        let r = c.resolve().unwrap();
        assert_eq!(r.prompt, "What's the capital of France?");
        assert_eq!(r.baseline_reply.as_deref(), Some("Paris"));
        assert!(r.source_trace.is_some());
    }

    #[test]
    fn unicode_prompt_truncation_lands_on_boundary() {
        // 25 codepoints, each 3 bytes — total 75 bytes. Truncation
        // must not split a codepoint.
        let prompt: String = "日".repeat(25);
        let c = Case::from_input(&prompt);
        // Round-trip name must be valid UTF-8 (i.e. not panic on
        // .chars().count()).
        let _ = c.name.chars().count();
    }
}
