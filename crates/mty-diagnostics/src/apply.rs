//! v0.35 T3 — Apply unified-diff fix envelopes to source strings.
//!
//! `mty-lsp::diff_apply` translates unified diffs into LSP `TextEdit`
//! objects (line/character positions, range-based replace). The CLI's
//! `mty fix --apply` needs the same logic but operating on a plain
//! `String`, returning the new `String` directly. This module is the
//! shared, LSP-free, source-string-level applier — used by the CLI
//! (`crates/mty-cli/src/cmd/fix.rs`) and the LSP's bulk-apply path
//! (`source.fixAll.mighty`).
//!
//! The diff format matches what `FixBuilder` emits: single-hunk,
//! optional leading + trailing context, LF line endings, `\n`-terminated.
//! Multi-hunk diffs are supported (each hunk applies in order).
//!
//! # Strategy
//!
//! 1. Parse the diff into hunks (`@@ ... @@` headers).
//! 2. For each hunk, trim leading + trailing context, classify the
//!    body as remove/add/context.
//! 3. Compute the source line range the OLD block covers.
//! 4. Validate the OLD block matches the source — refuse to apply
//!    otherwise (returns `None`).
//! 5. Splice the new lines in.
//!
//! Multi-hunk diffs are applied highest-line-first so each hunk's
//! line-number anchors remain valid as earlier hunks shrink/grow the
//! buffer.

/// Apply a unified-diff string to a source string. Returns the new
/// source on success, or `None` if any hunk fails to validate (its
/// OLD block doesn't match the source at the claimed line).
///
/// On `None`, the source is untouched — there is no partial-apply.
/// Callers (the CLI applier, the LSP bulk-apply path) treat this as
/// "skip the fix, surface a warning".
pub fn apply_unified_diff(source: &str, diff: &str) -> Option<String> {
    let hunks = parse_hunks(diff);
    if hunks.is_empty() {
        return None;
    }
    // Apply hunks highest-line first so later-hunk line offsets remain
    // valid as we splice into the buffer.
    let mut hunks = hunks;
    hunks.sort_by_key(|h| std::cmp::Reverse(h.spec.old_line));

    let mut out = source.to_string();
    for h in &hunks {
        out = apply_hunk(&out, h)?;
    }
    Some(out)
}

/// Test-friendly variant: split a diff into [`Hunk`]s without applying.
pub fn parse_hunks(diff: &str) -> Vec<Hunk> {
    let mut out: Vec<Hunk> = Vec::new();
    let lines: Vec<&str> = diff.split('\n').collect();
    let mut i = 0usize;
    while i < lines.len() {
        if !lines[i].starts_with("@@") {
            i += 1;
            continue;
        }
        let Some(spec) = parse_hunk_header(lines[i]) else {
            i += 1;
            continue;
        };
        let mut body: Vec<String> = Vec::new();
        let mut j = i + 1;
        while j < lines.len() && !lines[j].starts_with("@@") {
            if j + 1 == lines.len() && lines[j].is_empty() {
                break;
            }
            body.push(lines[j].to_string());
            j += 1;
        }
        out.push(Hunk { spec, body });
        i = j;
    }
    out
}

#[derive(Debug, Clone)]
pub struct Hunk {
    pub spec: HunkSpec,
    pub body: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct HunkSpec {
    /// 1-indexed source line where the OLD block starts.
    pub old_line: u32,
    /// Number of source lines the hunk replaces.
    pub old_len: u32,
}

fn parse_hunk_header(s: &str) -> Option<HunkSpec> {
    let rest = s.strip_prefix("@@")?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('-')?;
    let end = rest.find(' ').unwrap_or(rest.len());
    let lhs = &rest[..end];
    let (line_part, len_part) = match lhs.split_once(',') {
        Some((l, n)) => (l, n),
        None => (lhs, "1"),
    };
    let old_line: u32 = line_part.parse().ok()?;
    let old_len: u32 = len_part.parse().ok()?;
    Some(HunkSpec { old_line, old_len })
}

fn apply_hunk(source: &str, hunk: &Hunk) -> Option<String> {
    // Classify entries.
    #[derive(Clone)]
    enum K {
        Rem,
        Add,
        Ctx,
    }
    let mut entries: Vec<(K, String)> = Vec::new();
    for raw in &hunk.body {
        if raw.is_empty() {
            continue;
        }
        let (tag, rest) = raw.split_at(1);
        let txt = rest.to_string();
        match tag {
            "-" => entries.push((K::Rem, txt)),
            "+" => entries.push((K::Add, txt)),
            " " => entries.push((K::Ctx, txt)),
            "\\" => {} // "\ No newline at end of file"
            _ => entries.push((K::Ctx, raw.clone())),
        }
    }

    let mut leading_ctx = 0u32;
    while let Some((K::Ctx, _)) = entries.first() {
        entries.remove(0);
        leading_ctx += 1;
    }
    while let Some((K::Ctx, _)) = entries.last() {
        entries.pop();
    }

    let n_rem = entries.iter().filter(|(k, _)| matches!(k, K::Rem)).count();

    // Preserve trailing newline shape: most Mighty sources end with a
    // single `\n`. `split('\n')` on `"a\nb\n"` yields `["a", "b", ""]`
    // — we keep that empty trailing entry so re-joining round-trips.
    let source_lines: Vec<&str> = source.split('\n').collect();

    if n_rem == 0 || hunk.spec.old_len == 0 {
        // Pure insert. Lines go before line `old_line + leading_ctx`.
        let target_line_1 = hunk.spec.old_line + leading_ctx;
        let target_line_0 = target_line_1.saturating_sub(1) as usize;
        if target_line_0 > source_lines.len() {
            return None;
        }
        let mut result_lines: Vec<String> = source_lines.iter().map(|s| (*s).to_string()).collect();
        // The new chunk is all Add+Ctx entries (post-trim, leading ctx
        // was dropped). We must preserve the original ordering.
        let mut chunk: Vec<String> = Vec::new();
        for (k, t) in &entries {
            if matches!(k, K::Add | K::Ctx) {
                chunk.push(t.clone());
            }
        }
        for (offset, line) in chunk.into_iter().enumerate() {
            result_lines.insert(target_line_0 + offset, line);
        }
        return Some(result_lines.join("\n"));
    }

    // Replace block.
    let start_line_1 = hunk.spec.old_line + leading_ctx;
    let start_line_0 = start_line_1.saturating_sub(1) as usize;
    let end_line_0 = start_line_0 + n_rem;

    if end_line_0 > source_lines.len() {
        return None;
    }

    // Validate the OLD block matches the source verbatim.
    let mut walk = 0usize;
    for entry in &entries {
        if let (K::Rem, want) = entry {
            let line_idx = start_line_0 + walk;
            let got = *source_lines.get(line_idx)?;
            if got != want.as_str() {
                return None;
            }
            walk += 1;
        }
    }

    let mut new_chunk: Vec<String> = Vec::new();
    for (k, t) in &entries {
        if matches!(k, K::Add | K::Ctx) {
            new_chunk.push(t.clone());
        }
    }

    let mut result_lines: Vec<String> = Vec::with_capacity(source_lines.len() + new_chunk.len());
    for (idx, l) in source_lines.iter().enumerate() {
        if idx == start_line_0 {
            for c in &new_chunk {
                result_lines.push(c.clone());
            }
        }
        if !(idx >= start_line_0 && idx < end_line_0) {
            result_lines.push((*l).to_string());
        }
    }
    // Edge case: the source had fewer lines than `start_line_0` — should
    // already be caught above, but be defensive.
    Some(result_lines.join("\n"))
}

/// Apply a list of fix alternatives to a source, picking the first
/// alternative that applies cleanly. Returns `(new_source, applied)`
/// where `applied` is `true` iff some alternative succeeded.
///
/// Used by the CLI happy-path when the caller has already filtered
/// to one fix per diagnostic; the LSP bulk-apply path constructs its
/// own TextEdit collection via `mty-lsp::diff_apply`.
pub fn try_apply_alternatives<'a, I>(source: &str, alternatives: I) -> (String, bool)
where
    I: IntoIterator<Item = &'a str>,
{
    for diff in alternatives {
        if let Some(s) = apply_unified_diff(source, diff) {
            return (s, true);
        }
    }
    (source.to_string(), false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fix::{unified_diff, FixBuilder};

    // ---------- header parse ----------

    #[test]
    fn parses_header_basic() {
        let h = parse_hunk_header("@@ -5,3 +5,2 @@").unwrap();
        assert_eq!(h.old_line, 5);
        assert_eq!(h.old_len, 3);
    }

    #[test]
    fn parses_header_no_length() {
        let h = parse_hunk_header("@@ -7 +9 @@").unwrap();
        assert_eq!(h.old_line, 7);
        assert_eq!(h.old_len, 1);
    }

    #[test]
    fn rejects_non_header() {
        assert!(parse_hunk_header("xxx").is_none());
    }

    // ---------- single-line replace ----------

    #[test]
    fn single_line_replace() {
        let src = "a\nb\nc\n";
        let diff = unified_diff("x.mty", 2, &["b"], &["B"]);
        let out = apply_unified_diff(src, &diff).unwrap();
        assert_eq!(out, "a\nB\nc\n");
    }

    #[test]
    fn replace_via_builder() {
        let src = "fn main() {\n  let x = foo\n}\n";
        let f = FixBuilder::new("t", "r", 0.9)
            .replace_line("x.mty", 2, "  let x = foo", "  let x = bar")
            .build();
        let out = apply_unified_diff(src, &f.diff).unwrap();
        assert_eq!(out, "fn main() {\n  let x = bar\n}\n");
    }

    // ---------- multi-line replace ----------

    #[test]
    fn multi_line_replace() {
        let src = "header\nold1\nold2\nfooter\n";
        let diff = unified_diff("x.mty", 2, &["old1", "old2"], &["new1", "new2", "new3"]);
        let out = apply_unified_diff(src, &diff).unwrap();
        assert_eq!(out, "header\nnew1\nnew2\nnew3\nfooter\n");
    }

    // ---------- pure deletion ----------

    #[test]
    fn pure_deletion() {
        let src = "a\nb\nc\n";
        let diff = unified_diff("x.mty", 2, &["b"], &[]);
        let out = apply_unified_diff(src, &diff).unwrap();
        assert_eq!(out, "a\nc\n");
    }

    // ---------- pure insertion ----------

    #[test]
    fn insert_before_line() {
        let src = "a\nb\nc\n";
        let f = FixBuilder::new("t", "r", 0.8)
            .insert_before("x.mty", 2, &["use std.foo"])
            .build();
        let out = apply_unified_diff(src, &f.diff).unwrap();
        assert_eq!(out, "a\nuse std.foo\nb\nc\n");
    }

    #[test]
    fn insert_at_start_of_file() {
        let src = "first\nsecond\n";
        let f = FixBuilder::new("t", "r", 0.8)
            .insert_before("x.mty", 1, &["// header"])
            .build();
        let out = apply_unified_diff(src, &f.diff).unwrap();
        assert_eq!(out, "// header\nfirst\nsecond\n");
    }

    #[test]
    fn insert_multiple_lines() {
        let src = "a\nb\n";
        let f = FixBuilder::new("t", "r", 0.8)
            .insert_before("x.mty", 2, &["one", "two"])
            .build();
        let out = apply_unified_diff(src, &f.diff).unwrap();
        assert_eq!(out, "a\none\ntwo\nb\n");
    }

    // ---------- multi-hunk ----------

    #[test]
    fn multi_hunk_applied_high_to_low() {
        let src = "a\nb\nc\nd\ne\n";
        let diff = "\
--- a/x.mty
+++ b/x.mty
@@ -2,1 +2,1 @@
-b
+B
@@ -4,1 +4,1 @@
-d
+D
";
        let out = apply_unified_diff(src, diff).unwrap();
        assert_eq!(out, "a\nB\nc\nD\ne\n");
    }

    // ---------- validation ----------

    #[test]
    fn refuses_to_apply_when_old_does_not_match() {
        let src = "actual\n";
        let diff = unified_diff("x.mty", 1, &["wrong"], &["new"]);
        assert!(apply_unified_diff(src, &diff).is_none());
    }

    #[test]
    fn empty_diff_yields_none() {
        assert!(apply_unified_diff("anything\n", "").is_none());
    }

    // ---------- context-bracketed (git diff shape) ----------

    #[test]
    fn handles_context_brackets() {
        let src = "header\nbody\nfooter\n";
        let diff = "\
--- a/x.mty
+++ b/x.mty
@@ -1,3 +1,3 @@
 header
-body
+BODY
 footer
";
        let out = apply_unified_diff(src, diff).unwrap();
        assert_eq!(out, "header\nBODY\nfooter\n");
    }

    // ---------- try_apply_alternatives ----------

    #[test]
    fn try_apply_alternatives_picks_first_clean_one() {
        let src = "  fs.write(user_input)\n";
        let bad = unified_diff("x.mty", 1, &["  wrong_line"], &["  fixed"]);
        let good = unified_diff(
            "x.mty",
            1,
            &["  fs.write(user_input)"],
            &["  fs.write(user_input.matches_regex(r\"^[a-z]+$\"))"],
        );
        let (out, applied) = try_apply_alternatives(src, [bad.as_str(), good.as_str()]);
        assert!(applied);
        assert_eq!(out, "  fs.write(user_input.matches_regex(r\"^[a-z]+$\"))\n");
    }

    #[test]
    fn try_apply_alternatives_no_match() {
        let src = "actual\n";
        let bad = unified_diff("x.mty", 1, &["wrong"], &["new"]);
        let (out, applied) = try_apply_alternatives(src, [bad.as_str()]);
        assert!(!applied);
        assert_eq!(out, "actual\n");
    }

    #[test]
    fn marquee_mt4099_three_alternatives_only_first_applies() {
        // Mirrors the LSP marquee test: three alternatives for the
        // same line, applier picks the first whose OLD matches.
        let src = "  fs.write(user_input)\n";
        let alts = [
            FixBuilder::new("regex", "r", 0.92)
                .replace_line(
                    "x.mty",
                    1,
                    "  fs.write(user_input)",
                    "  fs.write(user_input.matches_regex(r\"^[A-Za-z]+$\"))",
                )
                .build(),
            FixBuilder::new("sanitizer", "s", 0.90)
                .replace_line(
                    "x.mty",
                    1,
                    "  fs.write(user_input)",
                    "  fs.write(user_input.sanitize_with(HtmlEscape))",
                )
                .build(),
            FixBuilder::new("allowlist", "a", 0.85)
                .replace_line(
                    "x.mty",
                    1,
                    "  fs.write(user_input)",
                    "  fs.write(user_input.in_allowlist[Verdict]())",
                )
                .build(),
        ];
        // First alt applies cleanly.
        let out = apply_unified_diff(src, &alts[0].diff).unwrap();
        assert_eq!(
            out,
            "  fs.write(user_input.matches_regex(r\"^[A-Za-z]+$\"))\n"
        );
    }
}
