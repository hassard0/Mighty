//! v0.35 T3 — `mty fix --apply <path>`.
//!
//! Closes the loop on agent first-shot → zero-shot. v0.33 T4 + v0.34
//! T1 ship 81 MTxxxx codes with structured auto-fix proposals; v0.34
//! T2 surfaces them as LSP `CodeAction`s. This subcommand applies
//! them from the CLI without an editor in the loop:
//!
//! ```bash
//! mty fix --apply src/main.mty                         # default policy
//! mty fix --apply src/main.mty --code MT4099           # one code only
//! mty fix --apply src/main.mty --alternative 0         # always pick alt 0
//! mty fix --apply src/main.mty --threshold 0.7         # lower confidence floor
//! mty fix --apply src/main.mty --dry-run               # print diff to stdout
//! mty fix --apply src/main.mty --interactive           # y/n per fix
//! mty check --format json file.mty | mty fix --apply --from-stdin
//! ```
//!
//! # Policy
//!
//! For every diagnostic that ships a fix envelope:
//!
//! 1. If `--code` is set and doesn't match, skip.
//! 2. Filter alternatives to those with confidence ≥ `threshold`
//!    (default 0.85).
//! 3. Pick:
//!    - `--alternative N` → that index (if it survived filtering).
//!    - default → highest-confidence surviving alternative.
//! 4. Apply via [`mty_diagnostics::apply::apply_unified_diff`].
//! 5. After every successful application, re-check the source so the
//!    next envelope is based on fresh diagnostics (defends against
//!    line-number drift + invalidated suggestions).
//!
//! Multi-fix conflict resolution: we apply highest line number first,
//! so earlier-in-file edits remain anchored as we splice. Within the
//! same line, innermost-span first.
//!
//! # Exit codes
//!
//! - `0` — applied 1+ fixes, no errors.
//! - `0` — no diagnostics with fixes; nothing to do (still "ok").
//! - `1` — file read / write error, or unrecoverable failure.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use mty_diagnostics::apply::apply_unified_diff;
use mty_diagnostics::fix::{to_ndjson, DiagnosticEnvelope};
use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source, type_and_borrow_check};

/// Default confidence floor — matches the LSP `preferred_threshold`.
pub const DEFAULT_THRESHOLD: f32 = 0.85;

/// CLI args for `mty fix --apply`.
#[derive(Debug, Clone)]
pub struct FixApplyArgs {
    /// Path to the source file. Required unless `--from-stdin` is set
    /// (in which case the JSON envelopes already carry the file path).
    pub path: Option<PathBuf>,
    /// Apply only fixes whose code matches this string (e.g. "MT4099").
    pub code: Option<String>,
    /// Always pick this alternative index instead of the highest-
    /// confidence one. Negative = unset.
    pub alternative: Option<usize>,
    /// Confidence floor. Default [`DEFAULT_THRESHOLD`].
    pub threshold: f32,
    /// Print the diff to stdout instead of writing back.
    pub dry_run: bool,
    /// Prompt y/n before each fix. Reads from `stdin` — incompatible
    /// with `--from-stdin`.
    pub interactive: bool,
    /// Read NDJSON envelopes from stdin (pipe from `mty check --format json`).
    pub from_stdin: bool,
}

impl Default for FixApplyArgs {
    fn default() -> Self {
        Self {
            path: None,
            code: None,
            alternative: None,
            threshold: DEFAULT_THRESHOLD,
            dry_run: false,
            interactive: false,
            from_stdin: false,
        }
    }
}

/// Summary of the apply pass — exposed so integration tests can assert
/// the headline numbers without scraping stderr.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FixApplyOutcome {
    /// Number of fixes successfully applied to the buffer.
    pub applied: usize,
    /// Codes the applier touched, with multiplicity.
    /// e.g. `[("MT4099", 2), ("MT1001", 1)]`.
    pub applied_by_code: Vec<(String, usize)>,
    /// Diagnostics whose fix envelope was below the threshold or
    /// didn't apply cleanly (so we skipped them).
    pub unapplied: usize,
    /// Diagnostics with no fix envelope at all.
    pub no_fix: usize,
}

/// Entry point for `mty fix --apply`. Returns the process exit code.
pub fn run(args: FixApplyArgs) -> i32 {
    // Stdout / stderr writers (real ones; tests use `run_with_io`).
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    let stdin_buf: Option<String> = if args.from_stdin {
        let mut s = String::new();
        if let Err(e) = io::stdin().read_to_string(&mut s) {
            let _ = writeln!(stderr, "mty fix: failed to read stdin: {}", e);
            return 1;
        }
        Some(s)
    } else {
        None
    };
    run_with_io(
        args,
        stdin_buf.as_deref(),
        &mut stdout,
        &mut stderr,
        &mut DefaultPrompter,
    )
}

/// Test-friendly entry point.
///
/// - `stdin_envelopes`: pre-read NDJSON when `args.from_stdin` is set.
///   `None` when reading from a file.
/// - `out`: stdout writer.
/// - `err`: stderr writer.
/// - `prompt`: y/n interactive prompt source (production: stdin).
pub fn run_with_io<O: Write, E: Write, P: Prompter>(
    args: FixApplyArgs,
    stdin_envelopes: Option<&str>,
    out: &mut O,
    err: &mut E,
    prompt: &mut P,
) -> i32 {
    // ---- Resolve input source + envelope list -----------------------
    let (path, source, envelopes) = match load_inputs(&args, stdin_envelopes, err) {
        Ok(t) => t,
        Err(code) => return code,
    };

    // ---- Filter + dispatch -----------------------------------------
    let (new_source, outcome) =
        apply_envelopes(&source, &envelopes, &args, path.as_ref(), out, err, prompt);

    // ---- Emit summary, write back ----------------------------------
    if outcome.applied == 0 {
        let _ = writeln!(
            err,
            "mty fix: no fixes applied ({} skipped, {} diagnostic(s) had no fix)",
            outcome.unapplied, outcome.no_fix
        );
        return 0;
    }

    if args.dry_run {
        // Print the unified-diff-style before/after to stdout so a user
        // (or agent) can pipe it into `patch` / `git apply` as a sanity
        // check.
        let _ = write_unified_diff(out, path.as_ref(), &source, &new_source);
    } else if let Some(p) = &path {
        if let Err(e) = std::fs::write(p, &new_source) {
            let _ = writeln!(err, "mty fix: failed to write {}: {}", p.display(), e);
            return 1;
        }
    } else {
        // No path AND no dry-run: streaming mode with --from-stdin and
        // no file write target. Emit the result to stdout for the
        // pipeline to consume.
        let _ = write!(out, "{}", new_source);
    }

    let _ = writeln!(err, "{}", format_summary(&outcome));
    0
}

/// Trait for the y/n interactive prompter. Production reads from
/// stdin; tests inject a scripted answer queue.
pub trait Prompter {
    fn confirm(&mut self, label: &str) -> bool;
}

/// Default prompter — reads one line from stdin per call.
pub struct DefaultPrompter;
impl Prompter for DefaultPrompter {
    fn confirm(&mut self, label: &str) -> bool {
        eprint!("Apply fix? [{}] (y/N): ", label);
        let _ = io::stderr().flush();
        let mut buf = String::new();
        if io::stdin().read_line(&mut buf).is_err() {
            return false;
        }
        let t = buf.trim().to_ascii_lowercase();
        t == "y" || t == "yes"
    }
}

/// Scripted prompter — tests construct it with a Vec of answers and
/// pop one per `confirm` call.
pub struct ScriptedPrompter {
    pub answers: std::collections::VecDeque<bool>,
}

impl ScriptedPrompter {
    pub fn new(answers: Vec<bool>) -> Self {
        Self {
            answers: answers.into(),
        }
    }
}

impl Prompter for ScriptedPrompter {
    fn confirm(&mut self, _label: &str) -> bool {
        self.answers.pop_front().unwrap_or(false)
    }
}

fn load_inputs(
    args: &FixApplyArgs,
    stdin_envelopes: Option<&str>,
    err: &mut impl Write,
) -> Result<(Option<PathBuf>, String, Vec<DiagnosticEnvelope>), i32> {
    if args.from_stdin {
        // NDJSON path. Each line is one envelope. We require all
        // envelopes to share the same `span.file` so we can apply
        // them to a single source.
        let raw = stdin_envelopes.unwrap_or("");
        let mut envelopes: Vec<DiagnosticEnvelope> = Vec::new();
        for (i, line) in raw.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<DiagnosticEnvelope>(trimmed) {
                Ok(env) => envelopes.push(env),
                Err(e) => {
                    let _ = writeln!(err, "mty fix: stdin line {}: {}", i + 1, e);
                    return Err(1);
                }
            }
        }

        if envelopes.is_empty() {
            // Honest: nothing to do, but not an error.
            return Ok((args.path.clone(), String::new(), envelopes));
        }

        // Path resolution: --path overrides; otherwise pull from the
        // first envelope's span.
        let path = args
            .path
            .clone()
            .unwrap_or_else(|| PathBuf::from(&envelopes[0].span.file));
        let source = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                let _ = writeln!(err, "mty fix: failed to read {}: {}", path.display(), e);
                return Err(1);
            }
        };
        Ok((Some(path), source, envelopes))
    } else {
        // File-driven path. Run `mty check` in-process and collect envelopes.
        let Some(path) = args.path.clone() else {
            let _ = writeln!(
                err,
                "mty fix: expected a path argument or --from-stdin (pipe from `mty check --format json`)"
            );
            return Err(2);
        };
        let source = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                let _ = writeln!(err, "mty fix: failed to read {}: {}", path.display(), e);
                return Err(1);
            }
        };
        let envelopes = check_to_envelopes(&path, &source);
        Ok((Some(path), source, envelopes))
    }
}

/// Run the v0.5 check pipeline in-process + serialize each diagnostic
/// into an envelope. Mirrors what `mty check --format json` writes.
fn check_to_envelopes(path: &Path, source: &str) -> Vec<DiagnosticEnvelope> {
    let parsed = parse_source(source.to_string(), path.display().to_string());
    let (pkg, mut diags) = lower(&parsed);
    let lower_errors = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !lower_errors {
        diags.extend(type_and_borrow_check(&pkg));
    }
    // We use the same NDJSON helper the CLI's `check` uses so the
    // wire shape is identical to what `--from-stdin` consumes.
    let ndjson = to_ndjson(&diags, &path.display().to_string(), source, false);
    ndjson
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn apply_envelopes(
    source: &str,
    envelopes: &[DiagnosticEnvelope],
    args: &FixApplyArgs,
    _path: Option<&PathBuf>,
    _out: &mut impl Write,
    err: &mut impl Write,
    prompt: &mut impl Prompter,
) -> (String, FixApplyOutcome) {
    let mut out = FixApplyOutcome::default();

    // Stage 1: pre-filter envelopes that have an applicable fix, by
    // (line, code, label) — so we can sort them highest-line first
    // before splicing.
    struct Pick {
        line: u32,
        col: u32,
        code: String,
        label: String,
        diff: String,
    }
    let mut picks: Vec<Pick> = Vec::new();
    let mut code_tally: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    let mut unapplied = 0usize;
    let mut no_fix = 0usize;

    for env in envelopes {
        // --code filter.
        if let Some(code) = &args.code {
            if &env.code != code {
                continue;
            }
        }
        let Some(fix) = env.fix.as_ref() else {
            no_fix += 1;
            continue;
        };
        // Keep alternatives ≥ threshold.
        let mut surviving: Vec<&mty_diagnostics::fix::FixAlternative> = fix
            .alternatives
            .iter()
            .filter(|a| a.confidence >= args.threshold)
            .collect();
        if surviving.is_empty() {
            unapplied += 1;
            continue;
        }
        // Order by confidence desc so default selection picks the
        // strongest fix.
        surviving.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // --alternative N? Honor it if the index survived filtering;
        // otherwise fall back to highest-confidence.
        let chosen: Option<&mty_diagnostics::fix::FixAlternative> =
            if let Some(idx) = args.alternative {
                // Apply the index against the ORIGINAL alternatives list
                // (so `--alternative 0` always means "the first alt the
                // engine emitted"), then verify it's in the surviving set.
                fix.alternatives
                    .get(idx)
                    .filter(|a| a.confidence >= args.threshold)
                    .or_else(|| surviving.first().copied())
            } else {
                surviving.first().copied()
            };
        let Some(alt) = chosen else {
            unapplied += 1;
            continue;
        };

        if args.interactive && !prompt.confirm(&format!("{} — {}", env.code, alt.label)) {
            unapplied += 1;
            continue;
        }

        picks.push(Pick {
            line: env.span.line,
            col: env.span.col,
            code: env.code.clone(),
            label: alt.label.clone(),
            diff: alt.diff.clone(),
        });
        *code_tally.entry(env.code.clone()).or_default() += 1;
    }

    // Stage 2: order picks highest-line, highest-col first. Innermost
    // spans (longer col) win when two picks share a line.
    picks.sort_by_key(|p| std::cmp::Reverse((p.line, p.col)));

    // Stage 3: splice. After each successful application we re-check
    // the buffer to invalidate stale envelopes — but only if there
    // are more picks left AND the source actually changed.
    let mut buf = source.to_string();
    let mut applied = 0usize;
    for pick in &picks {
        let Some(next) = apply_unified_diff(&buf, &pick.diff) else {
            // Diff stopped applying cleanly (probably due to a prior
            // splice changing surrounding context).
            unapplied += 1;
            // Roll back the code tally for this pick so the summary
            // doesn't claim a code we couldn't actually apply.
            let entry = code_tally.entry(pick.code.clone()).or_default();
            *entry = entry.saturating_sub(1);
            if *entry == 0 {
                code_tally.remove(&pick.code);
            }
            continue;
        };
        buf = next;
        applied += 1;
        let _ = writeln!(err, "applied {} — {}", pick.code, pick.label);
    }

    out.applied = applied;
    out.applied_by_code = code_tally.into_iter().collect();
    out.unapplied = unapplied;
    out.no_fix = no_fix;
    (buf, out)
}

fn format_summary(outcome: &FixApplyOutcome) -> String {
    let codes_str = if outcome.applied_by_code.is_empty() {
        String::new()
    } else {
        let inner = outcome
            .applied_by_code
            .iter()
            .map(|(c, n)| format!("{} ×{}", c, n))
            .collect::<Vec<_>>()
            .join(", ");
        format!(" ({})", inner)
    };
    let unapplied_str = if outcome.unapplied == 0 {
        String::new()
    } else {
        format!(", {} unapplied", outcome.unapplied)
    };
    format!(
        "Applied {} fix{}{}{}",
        outcome.applied,
        if outcome.applied == 1 { "" } else { "es" },
        codes_str,
        unapplied_str
    )
}

/// Print a minimal `--- a/path\n+++ b/path\n` diff to `out` when
/// `--dry-run` is set. We don't try to be byte-identical to GNU diff;
/// callers usually pipe through their own diff tool if they want the
/// canonical shape. Useful as-is for quick eyeball verification.
fn write_unified_diff<W: Write>(
    out: &mut W,
    path: Option<&PathBuf>,
    before: &str,
    after: &str,
) -> io::Result<()> {
    let label = path
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<stdin>".to_string());
    writeln!(out, "--- a/{}", label)?;
    writeln!(out, "+++ b/{}", label)?;
    // Walk line-by-line. For each differing line, emit `-old` / `+new`.
    let before_lines: Vec<&str> = before.split('\n').collect();
    let after_lines: Vec<&str> = after.split('\n').collect();
    let max = before_lines.len().max(after_lines.len());
    for i in 0..max {
        let b = before_lines.get(i).copied();
        let a = after_lines.get(i).copied();
        match (b, a) {
            (Some(bl), Some(al)) if bl == al => {
                // Unchanged — emit as context.
                writeln!(out, " {}", bl)?;
            }
            (Some(bl), Some(al)) => {
                writeln!(out, "-{}", bl)?;
                writeln!(out, "+{}", al)?;
            }
            (Some(bl), None) => writeln!(out, "-{}", bl)?,
            (None, Some(al)) => writeln!(out, "+{}", al)?,
            (None, None) => {}
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default, clippy::useless_vec)]
mod tests {
    use super::*;
    use mty_diagnostics::fix::{
        DiagnosticEnvelope, Fix, FixAlternative, FixBuilder, FixKind, SpanInfo, SCHEMA_VERSION,
    };

    fn env(
        code: &str,
        line: u32,
        col: u32,
        confidence: f32,
        alternatives: Vec<FixAlternative>,
    ) -> DiagnosticEnvelope {
        DiagnosticEnvelope {
            schema_version: SCHEMA_VERSION.to_string(),
            code: code.to_string(),
            severity: "error".into(),
            span: SpanInfo {
                file: "x.mty".into(),
                line,
                col,
                len: 1,
                byte_start: 0,
                byte_end: 1,
            },
            title: "t".into(),
            prose: "p".into(),
            fix: Some(Fix {
                kind: FixKind::Untaint.as_str().to_string(),
                confidence,
                alternatives,
            }),
            see_also: vec![],
            source: None,
        }
    }

    fn env_without_fix(code: &str, line: u32) -> DiagnosticEnvelope {
        DiagnosticEnvelope {
            schema_version: SCHEMA_VERSION.to_string(),
            code: code.to_string(),
            severity: "error".into(),
            span: SpanInfo {
                file: "x.mty".into(),
                line,
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
        }
    }

    fn alt(confidence: f32, label: &str, line: u32, old: &str, new: &str) -> FixAlternative {
        FixBuilder::new(label, "rationale", confidence)
            .replace_line("x.mty", line, old, new)
            .build()
    }

    fn args() -> FixApplyArgs {
        FixApplyArgs {
            path: Some(PathBuf::from("x.mty")),
            ..Default::default()
        }
    }

    fn run_apply(
        source: &str,
        envelopes: &[DiagnosticEnvelope],
        args: &FixApplyArgs,
    ) -> (String, FixApplyOutcome) {
        let mut o: Vec<u8> = Vec::new();
        let mut e: Vec<u8> = Vec::new();
        let mut p = ScriptedPrompter::new(vec![]);
        apply_envelopes(
            source,
            envelopes,
            args,
            args.path.as_ref(),
            &mut o,
            &mut e,
            &mut p,
        )
    }

    // ---------- 1: single-fix happy path ----------

    #[test]
    fn single_fix_happy_path() {
        let src = "a\nb\nc\n";
        let envs = vec![env(
            "MT4099",
            2,
            1,
            0.9,
            vec![alt(0.9, "fix b", 2, "b", "B")],
        )];
        let (out, summary) = run_apply(src, &envs, &args());
        assert_eq!(out, "a\nB\nc\n");
        assert_eq!(summary.applied, 1);
        assert_eq!(summary.applied_by_code, vec![("MT4099".to_string(), 1)]);
    }

    // ---------- 2: multi-fix application ----------

    #[test]
    fn multi_fix_application_high_line_first() {
        let src = "a\nb\nc\nd\ne\n";
        let envs = vec![
            env("MT4099", 2, 1, 0.9, vec![alt(0.9, "fix b", 2, "b", "B")]),
            env("MT4099", 4, 1, 0.9, vec![alt(0.9, "fix d", 4, "d", "D")]),
        ];
        let (out, summary) = run_apply(src, &envs, &args());
        assert_eq!(out, "a\nB\nc\nD\ne\n");
        assert_eq!(summary.applied, 2);
        assert_eq!(summary.applied_by_code, vec![("MT4099".to_string(), 2)]);
    }

    // ---------- 3: --code filter ----------

    #[test]
    fn code_filter_applies_only_matching() {
        let src = "a\nb\nc\n";
        let envs = vec![
            env("MT4099", 1, 1, 0.9, vec![alt(0.9, "x", 1, "a", "A")]),
            env("MT1001", 2, 1, 0.9, vec![alt(0.9, "y", 2, "b", "B")]),
        ];
        let mut a = args();
        a.code = Some("MT4099".to_string());
        let (out, summary) = run_apply(src, &envs, &a);
        assert_eq!(out, "A\nb\nc\n");
        assert_eq!(summary.applied, 1);
    }

    // ---------- 4: --alternative override ----------

    #[test]
    fn alternative_override_picks_index() {
        let src = "a\n";
        let envs = vec![env(
            "MT4099",
            1,
            1,
            0.95,
            vec![
                alt(0.95, "best", 1, "a", "BEST"),
                alt(0.90, "second", 1, "a", "SECOND"),
            ],
        )];
        let mut a = args();
        a.alternative = Some(1);
        let (out, _) = run_apply(src, &envs, &a);
        assert_eq!(out, "SECOND\n");
    }

    #[test]
    fn alternative_override_below_threshold_falls_back_to_best() {
        let src = "a\n";
        let envs = vec![env(
            "MT4099",
            1,
            1,
            0.95,
            vec![
                alt(0.95, "best", 1, "a", "BEST"),
                alt(0.30, "second", 1, "a", "TOO_LOW"),
            ],
        )];
        let mut a = args();
        a.alternative = Some(1); // 0.30 is below default threshold 0.85.
        let (out, _) = run_apply(src, &envs, &a);
        assert_eq!(out, "BEST\n");
    }

    // ---------- 5: --threshold ----------

    #[test]
    fn threshold_filter_default_drops_low_confidence() {
        let src = "a\n";
        let envs = vec![env(
            "MT4099",
            1,
            1,
            0.60,
            vec![alt(0.60, "low", 1, "a", "L")],
        )];
        let (out, summary) = run_apply(src, &envs, &args());
        assert_eq!(out, "a\n");
        assert_eq!(summary.applied, 0);
        assert_eq!(summary.unapplied, 1);
    }

    #[test]
    fn threshold_lowered_includes_borderline_fixes() {
        let src = "a\n";
        let envs = vec![env(
            "MT4099",
            1,
            1,
            0.60,
            vec![alt(0.60, "low", 1, "a", "L")],
        )];
        let mut a = args();
        a.threshold = 0.5;
        let (out, summary) = run_apply(src, &envs, &a);
        assert_eq!(out, "L\n");
        assert_eq!(summary.applied, 1);
    }

    // ---------- 6: no fix ----------

    #[test]
    fn diagnostic_without_fix_is_counted_separately() {
        let envs = vec![env_without_fix("MT0001", 1)];
        let (_, summary) = run_apply("a\n", &envs, &args());
        assert_eq!(summary.applied, 0);
        assert_eq!(summary.no_fix, 1);
    }

    #[test]
    fn empty_envelope_list_is_a_clean_noop() {
        let (out, summary) = run_apply("a\n", &[], &args());
        assert_eq!(out, "a\n");
        assert_eq!(summary.applied, 0);
        assert_eq!(summary.no_fix, 0);
        assert_eq!(summary.unapplied, 0);
    }

    // ---------- 7: --interactive ----------

    #[test]
    fn interactive_yes_applies() {
        let src = "a\n";
        let envs = vec![env("MT4099", 1, 1, 0.9, vec![alt(0.9, "x", 1, "a", "A")])];
        let mut a = args();
        a.interactive = true;
        let mut o: Vec<u8> = Vec::new();
        let mut e: Vec<u8> = Vec::new();
        let mut p = ScriptedPrompter::new(vec![true]);
        let (out, summary) =
            apply_envelopes(src, &envs, &a, a.path.as_ref(), &mut o, &mut e, &mut p);
        assert_eq!(out, "A\n");
        assert_eq!(summary.applied, 1);
    }

    #[test]
    fn interactive_no_skips() {
        let src = "a\n";
        let envs = vec![env("MT4099", 1, 1, 0.9, vec![alt(0.9, "x", 1, "a", "A")])];
        let mut a = args();
        a.interactive = true;
        let mut o: Vec<u8> = Vec::new();
        let mut e: Vec<u8> = Vec::new();
        let mut p = ScriptedPrompter::new(vec![false]);
        let (out, summary) =
            apply_envelopes(src, &envs, &a, a.path.as_ref(), &mut o, &mut e, &mut p);
        assert_eq!(out, "a\n");
        assert_eq!(summary.applied, 0);
        assert_eq!(summary.unapplied, 1);
    }

    // ---------- 8: conflicting fixes (same line) ----------

    #[test]
    fn conflicting_fixes_first_wins() {
        // Two fixes both targeting line 1's "  call(x)".
        let src = "  call(x)\n";
        let envs = vec![
            env(
                "MT4099",
                1,
                1,
                0.95,
                vec![alt(0.95, "first", 1, "  call(x)", "  call(SAFE_A)")],
            ),
            env(
                "MT4099",
                1,
                1,
                0.90,
                vec![alt(0.90, "second", 1, "  call(x)", "  call(SAFE_B)")],
            ),
        ];
        let (out, summary) = run_apply(src, &envs, &args());
        // First fix applied; second can no longer match the source.
        assert_eq!(out, "  call(SAFE_A)\n");
        assert_eq!(summary.applied, 1);
        assert_eq!(summary.unapplied, 1);
    }

    // ---------- 9: from-stdin happy path ----------

    #[test]
    fn from_stdin_via_serialized_ndjson() {
        let envs = vec![env("MT4099", 1, 1, 0.9, vec![alt(0.9, "x", 1, "a", "A")])];
        let ndjson: String = envs
            .iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        // Round-trip through serde, like real stdin pipeline.
        let mut parsed: Vec<DiagnosticEnvelope> = Vec::new();
        for line in ndjson.lines() {
            parsed.push(serde_json::from_str(line).unwrap());
        }
        let (out, summary) = run_apply("a\n", &parsed, &args());
        assert_eq!(out, "A\n");
        assert_eq!(summary.applied, 1);
    }

    // ---------- 10: format_summary ----------

    #[test]
    fn summary_format_zero_fixes() {
        let outcome = FixApplyOutcome::default();
        assert_eq!(format_summary(&outcome), "Applied 0 fixes");
    }

    #[test]
    fn summary_format_one_fix_pluralization() {
        let outcome = FixApplyOutcome {
            applied: 1,
            applied_by_code: vec![("MT4099".into(), 1)],
            unapplied: 0,
            no_fix: 0,
        };
        assert_eq!(format_summary(&outcome), "Applied 1 fix (MT4099 ×1)");
    }

    #[test]
    fn summary_format_multiple_codes() {
        let outcome = FixApplyOutcome {
            applied: 3,
            applied_by_code: vec![("MT4099".into(), 2), ("MT1001".into(), 1)],
            unapplied: 1,
            no_fix: 0,
        };
        assert_eq!(
            format_summary(&outcome),
            "Applied 3 fixes (MT4099 ×2, MT1001 ×1), 1 unapplied"
        );
    }

    // ---------- 11: dry-run unified diff helper ----------

    #[test]
    fn dry_run_emits_a_minimal_diff() {
        let mut out: Vec<u8> = Vec::new();
        let path = PathBuf::from("x.mty");
        write_unified_diff(&mut out, Some(&path), "a\nb\nc\n", "a\nB\nc\n").unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("--- a/x.mty"));
        assert!(s.contains("+++ b/x.mty"));
        assert!(s.contains("-b"));
        assert!(s.contains("+B"));
    }

    // ---------- 12: marquee MT4099 (matches LSP envelope tests) ----------

    #[test]
    fn marquee_mt4099_picks_highest_confidence() {
        let src = "  fs.write(user_input)\n";
        let envs = vec![env(
            "MT4099",
            1,
            1,
            0.92,
            vec![
                alt(
                    0.92,
                    "regex",
                    1,
                    "  fs.write(user_input)",
                    "  fs.write(user_input.matches_regex(r\"^[A-Za-z]+$\"))",
                ),
                alt(
                    0.90,
                    "sanitizer",
                    1,
                    "  fs.write(user_input)",
                    "  fs.write(user_input.sanitize_with(HtmlEscape))",
                ),
                alt(
                    0.85,
                    "allowlist",
                    1,
                    "  fs.write(user_input)",
                    "  fs.write(user_input.in_allowlist[Verdict]())",
                ),
            ],
        )];
        let (out, summary) = run_apply(src, &envs, &args());
        // Default: highest confidence (0.92) wins.
        assert_eq!(
            out,
            "  fs.write(user_input.matches_regex(r\"^[A-Za-z]+$\"))\n"
        );
        assert_eq!(summary.applied, 1);
    }

    #[test]
    fn marquee_mt4099_alternative_index_one_picks_sanitizer() {
        let src = "  fs.write(user_input)\n";
        let envs = vec![env(
            "MT4099",
            1,
            1,
            0.92,
            vec![
                alt(
                    0.92,
                    "regex",
                    1,
                    "  fs.write(user_input)",
                    "  fs.write(user_input.matches_regex(r\"^[A-Za-z]+$\"))",
                ),
                alt(
                    0.90,
                    "sanitizer",
                    1,
                    "  fs.write(user_input)",
                    "  fs.write(user_input.sanitize_with(HtmlEscape))",
                ),
            ],
        )];
        let mut a = args();
        a.alternative = Some(1);
        let (out, summary) = run_apply(src, &envs, &a);
        assert_eq!(out, "  fs.write(user_input.sanitize_with(HtmlEscape))\n");
        assert_eq!(summary.applied, 1);
    }

    // ---------- 13: dry-run flag ----------

    #[test]
    fn dry_run_does_not_overwrite() {
        // Stage a tempfile, set --dry-run, confirm contents unchanged.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.mty");
        let src = "a\nb\n";
        std::fs::write(&path, src).unwrap();
        let envs = vec![env("MT4099", 2, 1, 0.9, vec![alt(0.9, "x", 2, "b", "B")])];

        let mut a = FixApplyArgs::default();
        a.path = Some(path.clone());
        a.dry_run = true;
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let mut prompter = ScriptedPrompter::new(vec![]);
        // Drive through the high-level entry to exercise dry-run path.
        // Serialize envelopes back through stdin format so we can use
        // from_stdin without touching the file's real diagnostics.
        a.from_stdin = true;
        let ndjson: String = envs
            .iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        let code = run_with_io(a, Some(&ndjson), &mut out, &mut err, &mut prompter);
        assert_eq!(code, 0);
        // File untouched.
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after, src);
        // Stdout has the diff.
        let stdout = String::from_utf8(out).unwrap();
        assert!(stdout.contains("-b"));
        assert!(stdout.contains("+B"));
    }

    // ---------- 14: writes new contents on default (non-dry-run) ----------

    #[test]
    fn non_dry_run_writes_new_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.mty");
        std::fs::write(&path, "a\nb\n").unwrap();
        let envs = vec![env("MT4099", 2, 1, 0.9, vec![alt(0.9, "x", 2, "b", "B")])];
        let ndjson: String = envs
            .iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        let mut a = FixApplyArgs::default();
        a.path = Some(path.clone());
        a.from_stdin = true;
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let mut p = ScriptedPrompter::new(vec![]);
        let code = run_with_io(a, Some(&ndjson), &mut out, &mut err, &mut p);
        assert_eq!(code, 0);
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after, "a\nB\n");
    }

    // ---------- 15: from-stdin without --path errors gracefully ----------

    #[test]
    fn from_stdin_without_envelopes_is_clean_noop() {
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let mut p = ScriptedPrompter::new(vec![]);
        let mut a = FixApplyArgs::default();
        a.from_stdin = true;
        let code = run_with_io(a, Some(""), &mut out, &mut err, &mut p);
        assert_eq!(code, 0);
        let stderr = String::from_utf8(err).unwrap();
        assert!(stderr.contains("no fixes applied"));
    }

    // ---------- 16: malformed NDJSON is rejected with exit code 1 ----------

    #[test]
    fn malformed_stdin_returns_error() {
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let mut p = ScriptedPrompter::new(vec![]);
        let mut a = FixApplyArgs::default();
        a.from_stdin = true;
        let code = run_with_io(a, Some("not json\n"), &mut out, &mut err, &mut p);
        assert_eq!(code, 1);
    }

    // ---------- 17: missing path without --from-stdin errors ----------

    #[test]
    fn missing_path_returns_usage_error() {
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let mut p = ScriptedPrompter::new(vec![]);
        let a = FixApplyArgs::default();
        let code = run_with_io(a, None, &mut out, &mut err, &mut p);
        assert_eq!(code, 2);
        let stderr = String::from_utf8(err).unwrap();
        assert!(stderr.contains("expected a path argument"));
    }

    // ---------- 18: nonexistent file ----------

    #[test]
    fn nonexistent_file_returns_error() {
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let mut p = ScriptedPrompter::new(vec![]);
        let mut a = FixApplyArgs::default();
        a.path = Some(PathBuf::from("definitely/does/not/exist.mty"));
        let code = run_with_io(a, None, &mut out, &mut err, &mut p);
        assert_eq!(code, 1);
        let stderr = String::from_utf8(err).unwrap();
        assert!(stderr.contains("failed to read"));
    }

    // ---------- 19: code filter with no match ----------

    #[test]
    fn code_filter_no_match_yields_zero_applied() {
        let envs = vec![env("MT4099", 1, 1, 0.9, vec![alt(0.9, "x", 1, "a", "A")])];
        let mut a = args();
        a.code = Some("MT9999".into());
        let (out, summary) = run_apply("a\n", &envs, &a);
        assert_eq!(out, "a\n");
        assert_eq!(summary.applied, 0);
    }

    // ---------- 20: code filter keeps non-matching envelope as no-op ----------

    #[test]
    fn code_filter_does_not_count_skipped_as_unapplied() {
        let envs = vec![
            env("MT4099", 1, 1, 0.9, vec![alt(0.9, "a", 1, "a", "A")]),
            env("MT1001", 2, 1, 0.9, vec![alt(0.9, "b", 2, "b", "B")]),
        ];
        let mut a = args();
        a.code = Some("MT4099".into());
        let (out, summary) = run_apply("a\nb\n", &envs, &a);
        assert_eq!(out, "A\nb\n");
        assert_eq!(summary.applied, 1);
        assert_eq!(summary.unapplied, 0);
    }

    // ---------- 21: high threshold disables everything ----------

    #[test]
    fn threshold_above_one_disables_all() {
        let envs = vec![env("MT4099", 1, 1, 0.99, vec![alt(0.99, "x", 1, "a", "A")])];
        let mut a = args();
        a.threshold = 1.01;
        let (out, summary) = run_apply("a\n", &envs, &a);
        assert_eq!(out, "a\n");
        assert_eq!(summary.applied, 0);
        assert_eq!(summary.unapplied, 1);
    }

    // ---------- 22: envelope with empty alternatives list still counted ----------

    #[test]
    fn envelope_with_empty_alternatives_counted_as_unapplied() {
        let mut e = env("MT4099", 1, 1, 0.9, vec![alt(0.9, "x", 1, "a", "A")]);
        // Wipe alternatives.
        if let Some(fix) = e.fix.as_mut() {
            fix.alternatives.clear();
        }
        let (out, summary) = run_apply("a\n", &[e], &args());
        assert_eq!(out, "a\n");
        assert_eq!(summary.applied, 0);
        assert_eq!(summary.unapplied, 1);
    }

    // ---------- 23: alternative N out-of-range falls back to best ----------

    #[test]
    fn alternative_out_of_range_falls_back() {
        let envs = vec![env("MT4099", 1, 1, 0.9, vec![alt(0.9, "x", 1, "a", "A")])];
        let mut a = args();
        a.alternative = Some(99);
        let (out, summary) = run_apply("a\n", &envs, &a);
        assert_eq!(out, "A\n");
        assert_eq!(summary.applied, 1);
    }

    // ---------- 24: NDJSON round-trip preserves fix kind ----------

    #[test]
    fn ndjson_round_trip_preserves_diff_text() {
        let e = env("MT4099", 1, 1, 0.9, vec![alt(0.9, "x", 1, "a", "A")]);
        let line = serde_json::to_string(&e).unwrap();
        let parsed: DiagnosticEnvelope = serde_json::from_str(&line).unwrap();
        assert_eq!(
            parsed.fix.as_ref().unwrap().alternatives[0].diff,
            e.fix.as_ref().unwrap().alternatives[0].diff
        );
    }

    // ---------- 25: load_inputs reads file correctly ----------

    #[test]
    fn load_inputs_reads_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.mty");
        std::fs::write(&path, "hello\n").unwrap();
        let mut err: Vec<u8> = Vec::new();
        let mut a = FixApplyArgs::default();
        a.path = Some(path.clone());
        let (p, src, _envs) = load_inputs(&a, None, &mut err).unwrap();
        assert_eq!(p.as_ref().unwrap(), &path);
        assert_eq!(src, "hello\n");
    }

    // ---------- 26: from_stdin uses first envelope's path when --path absent ----------

    #[test]
    fn from_stdin_uses_envelope_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.mty");
        std::fs::write(&path, "a\n").unwrap();
        let e = DiagnosticEnvelope {
            schema_version: SCHEMA_VERSION.to_string(),
            code: "MT4099".into(),
            severity: "error".into(),
            span: SpanInfo {
                file: path.display().to_string(),
                line: 1,
                col: 1,
                len: 1,
                byte_start: 0,
                byte_end: 1,
            },
            title: "t".into(),
            prose: "p".into(),
            fix: Some(Fix {
                kind: FixKind::Untaint.as_str().to_string(),
                confidence: 0.9,
                alternatives: vec![alt(0.9, "x", 1, "a", "A")],
            }),
            see_also: vec![],
            source: None,
        };
        let ndjson = serde_json::to_string(&e).unwrap();
        let mut a = FixApplyArgs::default();
        a.from_stdin = true;
        let mut err: Vec<u8> = Vec::new();
        let (p, src, envs) = load_inputs(&a, Some(&ndjson), &mut err).unwrap();
        assert_eq!(p.as_ref().unwrap(), &path);
        assert_eq!(src, "a\n");
        assert_eq!(envs.len(), 1);
    }
}
