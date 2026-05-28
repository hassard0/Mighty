//! v0.33 T4 — Structured agent-actionable diagnostics.
//!
//! Every `MTxxxx` Mighty emits today is a human-readable string built
//! by `render::ariadne`. T4 layers a *structured* JSON envelope on top
//! so an agent (Mighty's first-class consumer) can read the diagnostic,
//! understand the precise span + cause, and apply a proposed fix
//! without re-parsing prose.
//!
//! The envelope shape is the agent-mode protocol contract for v0.33 T5
//! (`mty agent-mode`). It is documented in
//! `docs/internals/diagnostic-envelopes.md`; tests in this crate plus
//! integration tests in `crates/mty-driver/tests/json_envelope.rs` lock
//! the shape down.
//!
//! # Envelope shape
//!
//! ```json
//! {
//!   "code": "MT4099",
//!   "severity": "error",
//!   "span": {"file": "src/main.mty", "line": 18, "col": 27, "len": 12},
//!   "title": "tainted value flows to fs.write",
//!   "prose": "...",
//!   "fix": {
//!     "kind": "untaint",
//!     "confidence": 0.92,
//!     "alternatives": [
//!       {"label": "...", "diff": "...", "rationale": "...", "confidence": 0.92}
//!     ]
//!   },
//!   "see_also": ["MT4098", "docs/internals/taint-types.md"]
//! }
//! ```
//!
//! # Confidence scale (informative, not enforced)
//!
//! - `1.0` — mechanical (e.g. trivially-typed local annotation).
//! - `0.9` — high (taint untaint via regex, well-known idiom).
//! - `0.7` — medium (rename suggestion from scope similarity).
//! - `0.5` — low (might apply, might not).
//! - `<0.5` — not emitted; envelope ships without a fix.

use serde::{Deserialize, Serialize};

use crate::diagnostic::{Diagnostic, Severity};

/// The structured envelope an agent (or LSP / IDE / CI dashboard)
/// consumes. Every `Diagnostic` that flows through the v0.33 T4
/// envelope builder lands as exactly one of these.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiagnosticEnvelope {
    /// `MTxxxx` stable code, as a string.
    pub code: String,
    /// Severity. Lowercase: `"error"`, `"warning"`, `"note"`, `"help"`.
    pub severity: String,
    /// Where the diagnostic points. Always 1-indexed for line/col
    /// (matching MTxxxx human messages); `len` is byte length.
    pub span: SpanInfo,
    /// One-sentence title — the primary label message.
    pub title: String,
    /// Multi-sentence human explanation. Mirrors what
    /// `mty explain <code>` would print, optionally enriched with
    /// per-site notes from the original Diagnostic.
    pub prose: String,
    /// Optional fix. Absent when no fix proposal has high enough
    /// confidence (under 0.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
    /// Related codes + doc references the agent should consider.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub see_also: Vec<String>,
    /// Optional source snippet — populated when
    /// `--include-source` is passed to `mty check --format json`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSnippet>,
}

/// 1-indexed span. `file` is the source-id (path string) the
/// `mty-diagnostics` pipeline was given.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpanInfo {
    pub file: String,
    pub line: u32,
    pub col: u32,
    pub len: u32,
    /// Byte offsets into the source, half-open `[start, end)`.
    /// Included so consumers can splice without re-walking lines.
    pub byte_start: u32,
    pub byte_end: u32,
}

/// Source-snippet companion. Always uses LF line endings; the line
/// number is the 1-indexed line of the *first* element of `lines`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceSnippet {
    /// 1-indexed line number for `lines[0]`.
    pub start_line: u32,
    /// Source lines without trailing `\n`. Usually `[line-1, line, line+1]`.
    pub lines: Vec<String>,
}

/// A fix proposal. Carries its kind (for agents that want to filter)
/// plus one or more alternatives.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Fix {
    /// Discriminator. Lowercase snake-case so JSON-side filtering is easy.
    pub kind: String,
    /// Confidence of the highest-confidence alternative. The wider envelope
    /// is meant to expose this so agents can short-circuit without
    /// scanning every alternative.
    pub confidence: f32,
    /// At least one alternative; never empty.
    pub alternatives: Vec<FixAlternative>,
}

/// One fix-strategy choice. The diff is a unified-diff string keyed
/// off the same `span.file`; agents apply it with their tooling of
/// choice (`git apply --3way`, Mighty's LSP code action infrastructure,
/// etc.).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FixAlternative {
    /// Short human title (UI-side).
    pub label: String,
    /// Unified diff. Always `\n`-terminated, LF line endings.
    pub diff: String,
    /// 1-sentence "why this works" rationale.
    pub rationale: String,
    /// Per-alternative confidence. May be lower than the parent
    /// `Fix.confidence` (the parent surfaces the best of the bunch).
    pub confidence: f32,
}

/// Classifies the fix's mechanism. Agents can filter on kind to skip
/// classes of suggestion they don't want to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixKind {
    /// Untaint via regex/allowlist/sanitizer (MT4099 + friends).
    Untaint,
    /// Add a `use pkg.mod.item` declaration (MT1001, MT1002, MT2002).
    MissingImport,
    /// Add an effect to the enclosing fn (MT4001, MT4050, MT4055..).
    AddEffect,
    /// Add a capability to scope (MT4060..).
    AddCapability,
    /// Rename to match a similarly-spelled in-scope name (MT1001/2007).
    RenameToMatchDecl,
    /// Wrap a value in `Some(...)` (MT2013-ish when a field is Option).
    WrapInSome,
    /// Convert via `.to_string()`, `.to_str()`, `.parse()`, or cast.
    TypeConversion,
    /// Declare a protocol message before referencing it (MT2026 / MT4033).
    DeclareProtocolMessage,
    /// Add `.clone()` before a move.
    AddClone,
    /// Take a reference (`&` or `&mut`) instead of moving.
    TakeReference,
    /// Add `mut` to the binding (MT3013/MT3014).
    AddMutability,
    /// Fix a brace / delimiter mismatch (MT0012).
    BalanceDelimiters,
    /// Match-arm completion (MT2015).
    AddMatchArm,
    /// Drop unreachable / impossible code (MT2016).
    RemoveUnreachable,
    /// Correct an attribute-macro spelling (MT6001, MT6017..MT6020).
    CorrectMacroAttr,
    /// Add a return type annotation to a `pub` fn (MT0021, MT2020).
    AddReturnType,
    /// Add an explicit type annotation (MT2003).
    AddTypeAnnotation,
    /// Insert a missing struct field (MT2013).
    AddStructField,
    /// Replace `?` with a `match` for non-Result fns (MT2010).
    UnpackQuestion,
    /// Initialize a binding before use (MT3015).
    InitializeBinding,
    /// Catch-all for ad-hoc fixes that don't fit a canonical bucket.
    Other,
}

impl FixKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            FixKind::Untaint => "untaint",
            FixKind::MissingImport => "missing_import",
            FixKind::AddEffect => "add_effect",
            FixKind::AddCapability => "add_capability",
            FixKind::RenameToMatchDecl => "rename_to_match_decl",
            FixKind::WrapInSome => "wrap_in_some",
            FixKind::TypeConversion => "type_conversion",
            FixKind::DeclareProtocolMessage => "declare_protocol_message",
            FixKind::AddClone => "add_clone",
            FixKind::TakeReference => "take_reference",
            FixKind::AddMutability => "add_mutability",
            FixKind::BalanceDelimiters => "balance_delimiters",
            FixKind::AddMatchArm => "add_match_arm",
            FixKind::RemoveUnreachable => "remove_unreachable",
            FixKind::CorrectMacroAttr => "correct_macro_attr",
            FixKind::AddReturnType => "add_return_type",
            FixKind::AddTypeAnnotation => "add_type_annotation",
            FixKind::AddStructField => "add_struct_field",
            FixKind::UnpackQuestion => "unpack_question",
            FixKind::InitializeBinding => "initialize_binding",
            FixKind::Other => "other",
        }
    }
}

/// Builder helper for `FixAlternative` instances. The fix-proposal
/// engines (`codes_fix.rs`) lean on this to keep their handlers
/// readable. The builder is intentionally tiny — no string-interning
/// or other cleverness — so the call sites remain obvious.
pub struct FixBuilder {
    label: String,
    rationale: String,
    confidence: f32,
    diff: String,
}

impl FixBuilder {
    pub fn new(label: impl Into<String>, rationale: impl Into<String>, confidence: f32) -> Self {
        Self {
            label: label.into(),
            rationale: rationale.into(),
            confidence,
            diff: String::new(),
        }
    }

    /// Set the diff text directly. Use [`unified_diff`] when you want
    /// the builder to generate the hunk header for you.
    pub fn diff(mut self, diff: impl Into<String>) -> Self {
        self.diff = diff.into();
        self
    }

    /// Generate a unified diff hunk header + replacement of a single
    /// line. `line_no` is 1-indexed (matching MTxxxx output).
    pub fn replace_line(
        mut self,
        file: &str,
        line_no: u32,
        old_line: &str,
        new_line: &str,
    ) -> Self {
        self.diff = unified_diff(file, line_no, &[old_line], &[new_line]);
        self
    }

    /// Generate a unified diff hunk header + multi-line replacement.
    pub fn replace_lines(
        mut self,
        file: &str,
        line_no: u32,
        old_lines: &[&str],
        new_lines: &[&str],
    ) -> Self {
        self.diff = unified_diff(file, line_no, old_lines, new_lines);
        self
    }

    /// Insert one or more lines BEFORE `line_no` (1-indexed).
    pub fn insert_before(mut self, file: &str, line_no: u32, lines: &[&str]) -> Self {
        let header = format!(
            "--- a/{file}\n+++ b/{file}\n@@ -{line_no},0 +{line_no},{count} @@\n",
            file = file,
            line_no = line_no,
            count = lines.len(),
        );
        let mut body = String::new();
        for l in lines.iter() {
            body.push('+');
            body.push_str(l);
            body.push('\n');
        }
        self.diff = header + &body;
        self
    }

    pub fn build(self) -> FixAlternative {
        FixAlternative {
            label: self.label,
            diff: self.diff,
            rationale: self.rationale,
            confidence: self.confidence,
        }
    }
}

/// Build a unified-diff hunk replacing `old_lines` (consecutive,
/// starting at 1-indexed `line_no`) with `new_lines`. The output uses
/// LF line endings throughout.
pub fn unified_diff(file: &str, line_no: u32, old_lines: &[&str], new_lines: &[&str]) -> String {
    let mut buf = String::new();
    buf.push_str(&format!("--- a/{file}\n"));
    buf.push_str(&format!("+++ b/{file}\n"));
    buf.push_str(&format!(
        "@@ -{line_no},{old_n} +{line_no},{new_n} @@\n",
        line_no = line_no,
        old_n = old_lines.len(),
        new_n = new_lines.len(),
    ));
    for l in old_lines {
        buf.push('-');
        buf.push_str(l);
        buf.push('\n');
    }
    for l in new_lines {
        buf.push('+');
        buf.push_str(l);
        buf.push('\n');
    }
    buf
}

/// Convert a byte-offset (into LF-normalized source) to a 1-indexed
/// (line, col) pair. Identical math to the slice-6 interpreter's
/// `offset_to_line_col` so spans round-trip identically across the
/// compiler.
pub fn offset_to_line_col(src: &str, offset: usize) -> (u32, u32) {
    let bytes = src.as_bytes();
    let cap = bytes.len().min(offset);
    let mut line: u32 = 1;
    let mut col: u32 = 1;
    let mut i = 0;
    while i < cap {
        if bytes[i] == b'\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
        i += 1;
    }
    (line, col)
}

/// Severity → lowercase JSON tag (`"error"`, `"warning"`, ...).
pub fn severity_str(sev: Severity) -> &'static str {
    match sev {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Note => "note",
        Severity::Help => "help",
    }
}

/// Extract a 1-indexed [`SpanInfo`] from a primary label + source.
pub fn span_info_from(file: &str, src: &str, start: usize, end: usize) -> SpanInfo {
    let (line, col) = offset_to_line_col(src, start);
    let safe_end = end.max(start);
    SpanInfo {
        file: file.to_string(),
        line,
        col,
        len: (safe_end - start) as u32,
        byte_start: start as u32,
        byte_end: safe_end as u32,
    }
}

/// Build a 3-line `SourceSnippet` centered on `line_no` (1-indexed).
pub fn snippet_around(src: &str, line_no: u32, context: u32) -> SourceSnippet {
    let lines: Vec<&str> = src.split('\n').collect();
    if lines.is_empty() {
        return SourceSnippet {
            start_line: 1,
            lines: vec![],
        };
    }
    let idx = (line_no as usize).saturating_sub(1).min(lines.len() - 1);
    let start = idx.saturating_sub(context as usize);
    let end = (idx + context as usize + 1).min(lines.len());
    let start_line = (start + 1) as u32;
    let snippet_lines: Vec<String> = lines[start..end].iter().map(|s| (*s).to_string()).collect();
    SourceSnippet {
        start_line,
        lines: snippet_lines,
    }
}

/// Trait for converting a Diagnostic into a structured envelope.
/// Implementations live alongside the per-code fix engines in
/// `codes_fix`. The default impl produces an envelope without a fix
/// (just code + span + prose).
pub trait ToEnvelope {
    fn to_envelope(&self, source_id: &str, source: &str) -> DiagnosticEnvelope;
}

impl ToEnvelope for Diagnostic {
    fn to_envelope(&self, source_id: &str, source: &str) -> DiagnosticEnvelope {
        crate::codes_fix::build_envelope(self, source_id, source)
    }
}

/// Convert a slice of diagnostics to NDJSON (one envelope per line).
/// Used by `mty check --format json`.
pub fn to_ndjson(
    diags: &[Diagnostic],
    source_id: &str,
    source: &str,
    include_source: bool,
) -> String {
    let mut out = String::new();
    for d in diags {
        let mut env = d.to_envelope(source_id, source);
        if include_source {
            env.source = Some(snippet_around(source, env.span.line, 1));
        }
        // serde_json::to_string always succeeds for owned-data structs.
        let line = serde_json::to_string(&env).expect("envelope serializes");
        out.push_str(&line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codes::DiagCode;

    #[test]
    fn fixkind_round_trip_strings() {
        assert_eq!(FixKind::Untaint.as_str(), "untaint");
        assert_eq!(FixKind::MissingImport.as_str(), "missing_import");
        assert_eq!(FixKind::AddEffect.as_str(), "add_effect");
        assert_eq!(FixKind::Other.as_str(), "other");
    }

    #[test]
    fn unified_diff_single_line() {
        let d = unified_diff("a.mty", 18, &["  old"], &["  new"]);
        let expected = "\
--- a/a.mty
+++ b/a.mty
@@ -18,1 +18,1 @@
-  old
+  new
";
        assert_eq!(d, expected);
    }

    #[test]
    fn unified_diff_insert_before() {
        let f = FixBuilder::new("x", "x", 0.9)
            .insert_before("a.mty", 4, &["use std.fs"])
            .build();
        let expected = "\
--- a/a.mty
+++ b/a.mty
@@ -4,0 +4,1 @@
+use std.fs
";
        assert_eq!(f.diff, expected);
        assert_eq!(f.confidence, 0.9);
    }

    #[test]
    fn offset_to_line_col_basics() {
        let src = "abc\ndefg\nhij";
        assert_eq!(offset_to_line_col(src, 0), (1, 1));
        assert_eq!(offset_to_line_col(src, 2), (1, 3));
        assert_eq!(offset_to_line_col(src, 4), (2, 1));
        assert_eq!(offset_to_line_col(src, 7), (2, 4));
        assert_eq!(offset_to_line_col(src, 9), (3, 1));
    }

    #[test]
    fn span_info_basic() {
        let src = "hello\nworld\nfoo";
        let s = span_info_from("a.mty", src, 6, 11);
        assert_eq!(s.file, "a.mty");
        assert_eq!(s.line, 2);
        assert_eq!(s.col, 1);
        assert_eq!(s.len, 5);
        assert_eq!(s.byte_start, 6);
        assert_eq!(s.byte_end, 11);
    }

    #[test]
    fn snippet_around_middle_line() {
        let src = "a\nb\nc\nd\ne";
        let snip = snippet_around(src, 3, 1);
        assert_eq!(snip.start_line, 2);
        assert_eq!(snip.lines, vec!["b", "c", "d"]);
    }

    #[test]
    fn snippet_around_first_line() {
        let src = "a\nb\nc";
        let snip = snippet_around(src, 1, 1);
        assert_eq!(snip.start_line, 1);
        assert_eq!(snip.lines, vec!["a", "b"]);
    }

    #[test]
    fn snippet_around_last_line() {
        let src = "a\nb\nc";
        let snip = snippet_around(src, 3, 1);
        assert_eq!(snip.start_line, 2);
        assert_eq!(snip.lines, vec!["b", "c"]);
    }

    #[test]
    fn severity_str_round_trip() {
        assert_eq!(severity_str(Severity::Error), "error");
        assert_eq!(severity_str(Severity::Warning), "warning");
        assert_eq!(severity_str(Severity::Note), "note");
        assert_eq!(severity_str(Severity::Help), "help");
    }

    #[test]
    fn envelope_serializes_with_optional_fix() {
        let env = DiagnosticEnvelope {
            code: "MT4099".into(),
            severity: "error".into(),
            span: SpanInfo {
                file: "a.mty".into(),
                line: 1,
                col: 1,
                len: 3,
                byte_start: 0,
                byte_end: 3,
            },
            title: "tainted value".into(),
            prose: "details".into(),
            fix: None,
            see_also: vec![],
            source: None,
        };
        let j = serde_json::to_string(&env).unwrap();
        assert!(j.contains("\"code\":\"MT4099\""));
        assert!(!j.contains("\"fix\""));
        assert!(!j.contains("\"see_also\""));
        assert!(!j.contains("\"source\""));
    }

    #[test]
    fn envelope_round_trip_with_fix() {
        let env = DiagnosticEnvelope {
            code: "MT1001".into(),
            severity: "error".into(),
            span: SpanInfo {
                file: "x.mty".into(),
                line: 5,
                col: 9,
                len: 4,
                byte_start: 40,
                byte_end: 44,
            },
            title: "unresolved name `gtng`".into(),
            prose: "p".into(),
            fix: Some(Fix {
                kind: "rename_to_match_decl".into(),
                confidence: 0.7,
                alternatives: vec![FixAlternative {
                    label: "Rename to `greeting`".into(),
                    diff: "...".into(),
                    rationale: "spell-correction".into(),
                    confidence: 0.7,
                }],
            }),
            see_also: vec!["MT2007".into()],
            source: None,
        };
        let j = serde_json::to_string(&env).unwrap();
        let back: DiagnosticEnvelope = serde_json::from_str(&j).unwrap();
        assert_eq!(env, back);
    }

    #[test]
    fn to_ndjson_emits_one_line_per_diag() {
        let d1 = Diagnostic::error(
            DiagCode::new(1),
            crate::diagnostic::Label {
                start: 0,
                end: 1,
                message: "first".into(),
            },
        );
        let d2 = Diagnostic::error(
            DiagCode::new(1),
            crate::diagnostic::Label {
                start: 2,
                end: 3,
                message: "second".into(),
            },
        );
        let out = to_ndjson(&[d1, d2], "x.mty", "abc", false);
        let lines: Vec<&str> = out.trim_end().split('\n').collect();
        assert_eq!(lines.len(), 2);
        // Each line is a valid envelope.
        for l in &lines {
            let _: DiagnosticEnvelope = serde_json::from_str(l).unwrap();
        }
    }

    #[test]
    fn ndjson_with_include_source() {
        let d = Diagnostic::error(
            DiagCode::new(1001),
            crate::diagnostic::Label {
                start: 0,
                end: 3,
                message: "unresolved name `foo`".into(),
            },
        );
        let src = "let x = foo()\n";
        let out = to_ndjson(&[d], "x.mty", src, true);
        let env: DiagnosticEnvelope = serde_json::from_str(out.trim_end()).unwrap();
        assert!(env.source.is_some());
    }
}
