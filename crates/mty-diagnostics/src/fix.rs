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

/// Current envelope schema version. v0.34 T4 introduces an explicit
/// `schema_version` field on every envelope to lock the wire contract
/// down. Bump rules:
///
/// - **Major** (`2.0`, ...): breaking schema change. Consumers MUST
///   check the version and adapt. Removing a field, renaming a field,
///   changing a field's type, or repurposing an existing field all
///   count as breaking.
/// - **Minor** (`1.1`, `1.2`, ...): additive only. Adding a new
///   optional field is a minor bump; existing consumers can ignore it.
///
/// Forward-compatibility rule for consumers: **accept unknown fields**.
/// The envelope is `#[serde(deny_unknown_fields)]`-free precisely so
/// the v1.x line can ship additive fields without breaking older
/// agents.
///
/// See `docs/internals/diagnostic-envelopes.md` for the full policy
/// and the per-version changelog.
pub const SCHEMA_VERSION: &str = "1.0";

fn default_schema_version() -> String {
    SCHEMA_VERSION.to_string()
}

/// The structured envelope an agent (or LSP / IDE / CI dashboard)
/// consumes. Every `Diagnostic` that flows through the v0.33 T4
/// envelope builder lands as exactly one of these.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiagnosticEnvelope {
    /// v0.34 T4 — wire schema version. Always set to [`SCHEMA_VERSION`]
    /// on emit. Consumers parse this BEFORE looking at the rest of the
    /// envelope; a major-version mismatch means the consumer should
    /// either upgrade or fall back to the pretty renderer.
    ///
    /// Deserialised envelopes that omit the field default to `"1.0"`
    /// so back-compat round-trips of pre-v0.34 envelopes continue to
    /// parse cleanly.
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
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

// ---------------------------------------------------------------------------
// v0.45 T3 — structured `mty check --json` result document
// ---------------------------------------------------------------------------
//
// The pre-existing `--format json` route emits one *envelope* per
// diagnostic on its own NDJSON line. That shape is rich (it carries
// fix proposals, prose, see_also, snippet) and has real consumers
// (`mty fix --apply --from-stdin`, the agent-mode protocol).
//
// v0.45 T3 adds a *new*, simpler, single-document shape — the same
// JSON skeleton CLI/LSP/runtime control paths are migrating to under
// the v0.45 "agent command surfaces" track:
//
// ```json
// {
//   "ok": false,
//   "path": "src/main.mty",
//   "diagnostics": [
//     {
//       "code": "MT2001",
//       "severity": "error",
//       "message": "type mismatch: expected Str, found I32",
//       "span": {
//         "file": "src/main.mty",
//         "line": 12, "col": 5,
//         "end_line": 12, "end_col": 18
//       }
//     }
//   ]
// }
// ```
//
// Differences from the NDJSON envelope path, intentional:
//
// * **Single document, not NDJSON.** Agents that want one
//   `serde_json::from_str(&stdout)` call get one. NDJSON consumers
//   keep `--format json`.
// * **Both start AND end** line/col are exposed. The IDE used to
//   default `end_col = col + 1`; T3 threads the real end through
//   when the diagnostic carries an `end > start` byte span, and
//   falls back to `col + 1` when it doesn't.
// * **No fix / prose / see_also.** Those stay on the envelope path.
//   The structured-result shape is the *minimum* every agent
//   surface in v0.45 agrees on; richer needs continue to flow
//   through `--format json`.

/// One diagnostic, as the `mty check --json` result shape sees it.
/// Lean by design: code + severity + message + span.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CheckDiagnostic {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub span: CheckSpan,
}

/// 1-indexed span carrying BOTH start and end positions.
///
/// Falls back to `end_col = col + 1` when the underlying diagnostic
/// didn't carry a real end byte offset (matches the IDE's pre-T6
/// workaround).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CheckSpan {
    pub file: String,
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

/// The whole `mty check --json` document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CheckResult {
    /// True iff every diagnostic is below `Error` severity.
    pub ok: bool,
    /// The path `mty check` was invoked against (the source-id).
    pub path: String,
    /// Every diagnostic emitted, in the order the pipeline produced
    /// them. Warnings/notes/helps included.
    pub diagnostics: Vec<CheckDiagnostic>,
}

/// Build a [`CheckResult`] from a flat diagnostic list + the source
/// the check ran against.
///
/// `path` is used verbatim both as the top-level `path` field and as
/// each diagnostic's `span.file`, matching the pretty/ariadne path's
/// "all spans live in the file we just checked" assumption.
pub fn build_check_result(diags: &[Diagnostic], path: &str, source: &str) -> CheckResult {
    let mut out = Vec::with_capacity(diags.len());
    for d in diags {
        let start = d.primary.start;
        let end = d.primary.end;
        let (line, col) = offset_to_line_col(source, start);
        let (end_line, end_col) = if end > start {
            offset_to_line_col(source, end)
        } else {
            // Fallback matches the IDE's pre-T6 workaround: collapsed
            // span renders as a single-char highlight.
            (line, col + 1)
        };
        out.push(CheckDiagnostic {
            code: d.code.as_str(),
            severity: severity_str(d.severity).to_string(),
            message: d.primary.message.clone(),
            span: CheckSpan {
                file: path.to_string(),
                line,
                col,
                end_line,
                end_col,
            },
        });
    }
    let ok = !diags.iter().any(|d| matches!(d.severity, Severity::Error));
    CheckResult {
        ok,
        path: path.to_string(),
        diagnostics: out,
    }
}

/// Serialize a [`CheckResult`] as a single JSON document with a
/// trailing newline. Used by `mty check --json` and the `mty agent`
/// `check` op when called with `structured: true`.
pub fn to_check_result_json(diags: &[Diagnostic], path: &str, source: &str) -> String {
    let result = build_check_result(diags, path, source);
    let mut s = serde_json::to_string(&result).expect("check result serializes");
    s.push('\n');
    s
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
            schema_version: SCHEMA_VERSION.to_string(),
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
        assert!(j.contains("\"schema_version\":\"1.0\""));
        assert!(!j.contains("\"fix\""));
        assert!(!j.contains("\"see_also\""));
        assert!(!j.contains("\"source\""));
    }

    #[test]
    fn envelope_round_trip_with_fix() {
        let env = DiagnosticEnvelope {
            schema_version: SCHEMA_VERSION.to_string(),
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

    // ----------------------------------------------------------------
    // v0.34 T4 — schema_version field tests
    // ----------------------------------------------------------------

    #[test]
    fn schema_version_constant_is_one_zero() {
        // Lock the wire constant. Changing this constant requires a
        // doc update + a v0.34 changelog note + a coordinated consumer
        // upgrade per `docs/internals/diagnostic-envelopes.md`.
        assert_eq!(SCHEMA_VERSION, "1.0");
    }

    #[test]
    fn schema_version_appears_in_serialized_envelope() {
        let env = DiagnosticEnvelope {
            schema_version: SCHEMA_VERSION.to_string(),
            code: "MT0001".into(),
            severity: "error".into(),
            span: SpanInfo {
                file: "a.mty".into(),
                line: 1,
                col: 1,
                len: 1,
                byte_start: 0,
                byte_end: 1,
            },
            title: "t".into(),
            prose: "p".into(),
            fix: None,
            see_also: vec![],
            source: None,
        };
        let j = serde_json::to_string(&env).unwrap();
        assert!(
            j.contains("\"schema_version\":\"1.0\""),
            "serialized envelope must include schema_version, got: {}",
            j
        );
    }

    #[test]
    fn schema_version_round_trips_through_serde() {
        // Build → JSON → parse → JSON: schema_version must survive.
        let env = DiagnosticEnvelope {
            schema_version: SCHEMA_VERSION.to_string(),
            code: "MT0001".into(),
            severity: "error".into(),
            span: SpanInfo {
                file: "a.mty".into(),
                line: 1,
                col: 1,
                len: 1,
                byte_start: 0,
                byte_end: 1,
            },
            title: "t".into(),
            prose: "p".into(),
            fix: None,
            see_also: vec![],
            source: None,
        };
        let j = serde_json::to_string(&env).unwrap();
        let back: DiagnosticEnvelope = serde_json::from_str(&j).unwrap();
        assert_eq!(back.schema_version, "1.0");
        assert_eq!(back, env);
    }

    #[test]
    fn schema_version_missing_field_defaults_to_one_zero() {
        // Back-compat: pre-v0.34 envelopes (no schema_version field)
        // must still parse, defaulting to "1.0". Documents the
        // forward-compat rule for consumers in
        // `docs/internals/diagnostic-envelopes.md`.
        let legacy = r#"{
            "code": "MT0001",
            "severity": "error",
            "span": {
                "file": "a.mty", "line": 1, "col": 1, "len": 1,
                "byte_start": 0, "byte_end": 1
            },
            "title": "t",
            "prose": "p"
        }"#;
        let env: DiagnosticEnvelope = serde_json::from_str(legacy).unwrap();
        assert_eq!(env.schema_version, "1.0");
    }

    #[test]
    fn build_envelope_populates_schema_version() {
        // Driver-style smoke: the build_envelope helper that every CLI
        // consumer ultimately calls must stamp the field.
        let d = Diagnostic::error(
            DiagCode::new(1),
            crate::diagnostic::Label {
                start: 0,
                end: 1,
                message: "x".into(),
            },
        );
        let env = crate::codes_fix::build_envelope(&d, "f.mty", "abc");
        assert_eq!(env.schema_version, "1.0");
    }

    #[test]
    fn ndjson_emits_schema_version_on_every_line() {
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
        for l in &lines {
            let env: DiagnosticEnvelope = serde_json::from_str(l).unwrap();
            assert_eq!(env.schema_version, "1.0");
        }
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

    // ---- v0.45 T3: structured `mty check --json` result document ----

    #[test]
    fn check_result_clean_file_is_ok() {
        let result = build_check_result(&[], "src/main.mty", "fn main() {}\n");
        assert!(result.ok);
        assert_eq!(result.path, "src/main.mty");
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn check_result_threads_real_end_span() {
        // `let x: Str = 42;` — point at the `42` literal on line 2.
        // The diagnostic carries the start AND end byte of the literal.
        let src = "fn demo() {\n    let x: Str = 42;\n}\n";
        let start = src.find("42").unwrap();
        let end = start + 2;
        let d = Diagnostic::error(
            DiagCode::new(2001),
            crate::diagnostic::Label {
                start,
                end,
                message: "type mismatch".into(),
            },
        );
        let r = build_check_result(&[d], "demo.mty", src);
        assert!(!r.ok);
        assert_eq!(r.diagnostics.len(), 1);
        let dg = &r.diagnostics[0];
        assert_eq!(dg.code, "MT2001");
        assert_eq!(dg.severity, "error");
        assert_eq!(dg.span.line, 2);
        assert_eq!(dg.span.end_line, 2);
        // end_col MUST be col + 2 (the literal is 2 chars wide).
        assert_eq!(dg.span.end_col, dg.span.col + 2);
    }

    #[test]
    fn check_result_falls_back_to_col_plus_one_when_no_end() {
        // Diagnostic with start == end (no real range). end_col must
        // default to col + 1.
        let d = Diagnostic::error(
            DiagCode::new(2021),
            crate::diagnostic::Label {
                start: 4,
                end: 4,
                message: "undefined".into(),
            },
        );
        let r = build_check_result(&[d], "x.mty", "abc\nfoo\n");
        let dg = &r.diagnostics[0];
        assert_eq!(dg.span.end_line, dg.span.line);
        assert_eq!(dg.span.end_col, dg.span.col + 1);
    }

    #[test]
    fn check_result_two_errors_distinct_positions() {
        // Pre-v0.42 T6 both errors would collapse to 1:1. Make sure
        // T3 surfaces them as two distinct line:col positions.
        let src = "fn demo() {\n    let x: I32 = \"hello\";\n    let y: Str = 42;\n}\n";
        let s1 = src.find("\"hello\"").unwrap();
        let e1 = s1 + "\"hello\"".len();
        let s2 = src.find("42;").unwrap();
        let e2 = s2 + 2;
        let d1 = Diagnostic::error(
            DiagCode::new(2001),
            crate::diagnostic::Label {
                start: s1,
                end: e1,
                message: "expected I32, found Str".into(),
            },
        );
        let d2 = Diagnostic::error(
            DiagCode::new(2001),
            crate::diagnostic::Label {
                start: s2,
                end: e2,
                message: "expected Str, found I32".into(),
            },
        );
        let r = build_check_result(&[d1, d2], "demo.mty", src);
        assert!(!r.ok);
        assert_eq!(r.diagnostics.len(), 2);
        let (a, b) = (&r.diagnostics[0], &r.diagnostics[1]);
        assert_eq!(a.span.line, 2);
        assert_eq!(b.span.line, 3);
        assert!(
            a.span.col != b.span.col || a.span.line != b.span.line,
            "two errors must report distinct line:col"
        );
        // Neither anchors at the fn header.
        for d in &r.diagnostics {
            assert!(!(d.span.line == 1 && d.span.col == 1));
        }
    }

    #[test]
    fn check_result_json_is_single_document() {
        let d = Diagnostic::error(
            DiagCode::new(2001),
            crate::diagnostic::Label {
                start: 0,
                end: 3,
                message: "boom".into(),
            },
        );
        let out = to_check_result_json(&[d], "x.mty", "abc\n");
        // Must parse as ONE document, not NDJSON.
        let parsed: CheckResult = serde_json::from_str(out.trim_end()).expect("single document");
        assert!(!parsed.ok);
        assert_eq!(parsed.path, "x.mty");
        assert_eq!(parsed.diagnostics.len(), 1);
        // Trailing newline so `tee`/`jq` pipelines stay tidy.
        assert!(out.ends_with('\n'));
        // Exactly one newline at the very end — i.e. no NDJSON splits.
        let nl_count = out.matches('\n').count();
        assert_eq!(nl_count, 1, "result must be single-line JSON, got: {out}");
    }

    #[test]
    fn check_result_only_warnings_is_ok() {
        // ok is true iff there are no error-severity diagnostics.
        let mut d = Diagnostic::error(
            DiagCode::new(2015),
            crate::diagnostic::Label {
                start: 0,
                end: 1,
                message: "non-exhaustive".into(),
            },
        );
        d.severity = Severity::Warning;
        let r = build_check_result(&[d], "x.mty", "ab\n");
        assert!(r.ok, "ok must be true when only warnings are present");
        assert_eq!(r.diagnostics.len(), 1);
        assert_eq!(r.diagnostics[0].severity, "warning");
    }
}
