//! v0.34 T2 — Unified-diff → LSP `TextEdit` conversion.
//!
//! Fix envelopes produced by `mty-diagnostics::fix` carry their edits
//! as small unified diffs (one hunk per alternative, or rarely a
//! multi-hunk diff). LSP clients want `TextEdit { range, newText }`
//! objects directly, so the LSP layer needs to translate.
//!
//! The diffs we generate ourselves (via [`mty_diagnostics::fix::FixBuilder`])
//! are very stylized — single hunk, no surrounding context, headers
//! line up with the diagnostic's source line. The parser here is
//! tolerant enough to handle that plus the common "real" patch shapes
//! (multiple hunks, leading context lines) so we don't lock the fix
//! engines into one diff dialect.
//!
//! # Shape
//!
//! A unified-diff hunk looks like:
//! ```text
//! --- a/file
//! +++ b/file
//! @@ -OLD_LINE,OLD_LEN +NEW_LINE,NEW_LEN @@ optional-section
//! -removed line 1
//! -removed line 2
//! +added line 1
//! +added line 2
//!  context line  (untouched)
//! ```
//!
//! We split on `@@` markers, then for each hunk: collect the `-`,
//! `+`, and ` ` lines, work out the source range they cover, and
//! emit one [`TextEdit`] per hunk.
//!
//! # Multi-hunk
//!
//! Multi-hunk diffs are returned as a `Vec<TextEdit>` in
//! source order. LSP applies all edits in a single `WorkspaceEdit`
//! atomically.

use tower_lsp::lsp_types::{Position, Range, TextEdit};

/// Convert a unified-diff string into a vector of LSP `TextEdit`s
/// keyed off `source`. Returns an empty vector if the diff is
/// malformed or the hunks don't line up with the source — callers
/// then drop the action rather than offering a broken edit.
pub fn unified_diff_to_text_edits(diff: &str, source: &str) -> Vec<TextEdit> {
    let mut out: Vec<TextEdit> = Vec::new();

    // Split into hunks. Each hunk starts at a `@@ ` header line.
    let lines: Vec<&str> = diff.split('\n').collect();
    let mut i = 0usize;
    while i < lines.len() {
        if !lines[i].starts_with("@@") {
            i += 1;
            continue;
        }
        let header = lines[i];
        let Some(spec) = parse_hunk_header(header) else {
            i += 1;
            continue;
        };
        // Collect the body — lines after the header until the next
        // header or the end of the diff.
        let mut body: Vec<&str> = Vec::new();
        let mut j = i + 1;
        while j < lines.len() && !lines[j].starts_with("@@") {
            // Skip stray empty trailing entries from split('\n').
            if j + 1 == lines.len() && lines[j].is_empty() {
                break;
            }
            body.push(lines[j]);
            j += 1;
        }
        if let Some(edit) = hunk_to_text_edit(spec, &body, source) {
            out.push(edit);
        }
        i = j;
    }
    out
}

#[derive(Debug, Clone, Copy)]
struct HunkSpec {
    /// 1-indexed source line where the old block starts.
    /// If `old_len` is 0 (pure insertion), this is the line *before*
    /// which the new lines go (mirroring `git diff` semantics: e.g.
    /// `@@ -4,0 +4,1 @@` means "insert before line 4").
    old_line: u32,
    /// Number of source lines the hunk replaces.
    old_len: u32,
}

/// Parse `@@ -OLD_LINE,OLD_LEN +NEW_LINE,NEW_LEN @@ ...` into a
/// [`HunkSpec`]. The `+...` half is reconstructible from the body so
/// we only need the `-...` half here. Returns `None` for malformed
/// headers (caller silently skips the hunk).
fn parse_hunk_header(s: &str) -> Option<HunkSpec> {
    let rest = s.strip_prefix("@@")?;
    let rest = rest.trim_start();
    // Should start with `-`.
    let rest = rest.strip_prefix('-')?;
    // Take up to whitespace.
    let end = rest.find(' ').unwrap_or(rest.len());
    let lhs = &rest[..end];
    // Split on `,` (length is optional and defaults to 1).
    let (line_part, len_part) = match lhs.split_once(',') {
        Some((l, n)) => (l, n),
        None => (lhs, "1"),
    };
    let old_line: u32 = line_part.parse().ok()?;
    let old_len: u32 = len_part.parse().ok()?;
    Some(HunkSpec { old_line, old_len })
}

/// Convert one hunk body to a single LSP `TextEdit`. The strategy:
///
/// 1. Walk the body, classifying each line as remove/add/context.
/// 2. Trim leading + trailing *context* lines so the resulting
///    range and new_text are minimal. This makes diffs that bracket
///    a 1-line change with context (the usual `git diff` shape)
///    behave the same as a tight `replace_line` envelope.
/// 3. Compute the byte range in `source` corresponding to the
///    remaining old lines, build a `Range` from it, and assemble the
///    new text from the remaining `+` and ` ` lines.
fn hunk_to_text_edit(spec: HunkSpec, body: &[&str], source: &str) -> Option<TextEdit> {
    // Classify lines.
    #[derive(Clone)]
    enum K {
        Rem,
        Add,
        Ctx,
    }
    let mut entries: Vec<(K, String)> = Vec::new();
    for raw in body {
        if raw.is_empty() {
            // An empty body line in unified-diff is illegal (every line
            // is prefixed with one of `-`, `+`, ` `). Treat as a "no
            // change" sentinel so trailing empty entries from split do
            // not break us.
            continue;
        }
        let (tag, rest) = raw.split_at(1);
        let txt = rest.to_string();
        match tag {
            "-" => entries.push((K::Rem, txt)),
            "+" => entries.push((K::Add, txt)),
            " " => entries.push((K::Ctx, txt)),
            "\\" => {
                // `\ No newline at end of file` — ignore.
            }
            _ => {
                // Unknown prefix; treat as context to be safe.
                entries.push((K::Ctx, raw.to_string()));
            }
        }
    }

    // Trim leading and trailing context. The position offset we have
    // (`spec.old_line`) points at the first OLD line in the hunk
    // (which, for a leading-context hunk, is a context line). When we
    // drop a leading context line, advance the start.
    let mut leading_ctx = 0u32;
    while let Some((K::Ctx, _)) = entries.first() {
        entries.remove(0);
        leading_ctx += 1;
    }
    while let Some((K::Ctx, _)) = entries.last() {
        entries.pop();
    }

    // Distinguish pure-insert vs replace/delete:
    //   - Pure insert: `old_len == 0` OR no Rem entries after trimming.
    //   - Replace/delete: at least one Rem.
    let n_rem = entries.iter().filter(|(k, _)| matches!(k, K::Rem)).count();

    let source_lines: Vec<&str> = source.split('\n').collect();

    if n_rem == 0 || spec.old_len == 0 {
        // Pure insertion. The new lines go BEFORE source line
        // `spec.old_line` for `@@ -N,0 @@` shape, or BEFORE the line
        // after the trimmed leading context.
        // For our FixBuilder.insert_before shape, the header is
        // `@@ -N,0 +N,L @@` — meaning "insert before line N".
        let target_line_1 = if spec.old_len == 0 {
            spec.old_line + leading_ctx
        } else {
            // Mixed insert at end of removed block.
            spec.old_line + leading_ctx
        };
        // 0-indexed line.
        let target_line_0 = target_line_1.saturating_sub(1);
        // Position: start of that line. If the source has fewer lines,
        // clamp to end-of-buffer.
        let pos = line_start_position(source, target_line_0, &source_lines);
        let mut new_text = String::new();
        for (k, t) in &entries {
            if matches!(k, K::Add | K::Ctx) {
                new_text.push_str(t);
                new_text.push('\n');
            }
        }
        return Some(TextEdit {
            range: Range {
                start: pos,
                end: pos,
            },
            new_text,
        });
    }

    // Replace/delete. The OLD block covers `n_rem` consecutive source
    // lines starting at (1-indexed) `spec.old_line + leading_ctx`.
    let start_line_1 = spec.old_line + leading_ctx;
    let start_line_0 = start_line_1.saturating_sub(1);
    let end_line_0 = start_line_0 + n_rem as u32; // exclusive

    // Sanity-check: source has enough lines.
    if (end_line_0 as usize) > source_lines.len() + 1 {
        return None;
    }

    // Optional: validate the OLD lines match the source. If they
    // don't, drop the action (caller skips).
    let mut walk = 0usize;
    for entry in &entries {
        if let (K::Rem, want) = entry {
            let line_idx = (start_line_0 as usize) + walk;
            let got = source_lines.get(line_idx).copied().unwrap_or("");
            if got != want.as_str() {
                return None;
            }
            walk += 1;
        }
    }

    let start_pos = line_start_position(source, start_line_0, &source_lines);
    // End position: start of the line AFTER the last removed line.
    // For the very last line in the source (with no trailing \n) we
    // clamp to end-of-buffer.
    let end_pos = line_start_position(source, end_line_0, &source_lines);

    let mut new_text = String::new();
    for (k, t) in &entries {
        if matches!(k, K::Add | K::Ctx) {
            new_text.push_str(t);
            new_text.push('\n');
        }
    }

    Some(TextEdit {
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        new_text,
    })
}

/// Position of the start of a 0-indexed line in `source`. If
/// `line_idx` is past the end, returns the end-of-buffer position.
fn line_start_position(_source: &str, line_idx: u32, source_lines: &[&str]) -> Position {
    let idx = line_idx as usize;
    if idx >= source_lines.len() {
        // End-of-buffer. Find total line count + character of last line.
        let total = source_lines.len() as u32;
        let last_line = source_lines.last().copied().unwrap_or("");
        return Position {
            line: total.saturating_sub(1),
            character: utf16_len(last_line),
        };
    }
    Position {
        line: line_idx,
        character: 0,
    }
}

fn utf16_len(s: &str) -> u32 {
    s.chars().map(|c| c.len_utf16() as u32).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mty_diagnostics::fix::{unified_diff, FixBuilder};

    fn pos(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    fn range(s: Position, e: Position) -> Range {
        Range { start: s, end: e }
    }

    // ---------- header parsing ----------

    #[test]
    fn parses_header_with_lengths() {
        let h = parse_hunk_header("@@ -5,3 +5,2 @@").unwrap();
        assert_eq!(h.old_line, 5);
        assert_eq!(h.old_len, 3);
    }

    #[test]
    fn parses_header_with_default_length() {
        let h = parse_hunk_header("@@ -7 +9 @@").unwrap();
        assert_eq!(h.old_line, 7);
        assert_eq!(h.old_len, 1);
    }

    #[test]
    fn parses_header_with_section_label() {
        let h = parse_hunk_header("@@ -1,0 +1,2 @@ section").unwrap();
        assert_eq!(h.old_line, 1);
        assert_eq!(h.old_len, 0);
    }

    #[test]
    fn rejects_non_header() {
        assert!(parse_hunk_header("not a header").is_none());
        assert!(parse_hunk_header("--- a/x").is_none());
    }

    // ---------- single-line replacement ----------

    #[test]
    fn single_line_replacement() {
        let src = "fn main() {\n  let x = foo\n}\n";
        let diff = unified_diff("x.mty", 2, &["  let x = foo"], &["  let x = bar"]);
        let edits = unified_diff_to_text_edits(&diff, src);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range, range(pos(1, 0), pos(2, 0)));
        assert_eq!(edits[0].new_text, "  let x = bar\n");
    }

    #[test]
    fn replace_line_with_builder() {
        let src = "a\nb\nc\n";
        let f = FixBuilder::new("t", "r", 0.9)
            .replace_line("test.mty", 2, "b", "B")
            .build();
        let edits = unified_diff_to_text_edits(&f.diff, src);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range, range(pos(1, 0), pos(2, 0)));
        assert_eq!(edits[0].new_text, "B\n");
    }

    // ---------- multi-line replacement ----------

    #[test]
    fn multi_line_replacement() {
        let src = "header\nold1\nold2\nfooter\n";
        let diff = unified_diff("x.mty", 2, &["old1", "old2"], &["new1", "new2", "new3"]);
        let edits = unified_diff_to_text_edits(&diff, src);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range, range(pos(1, 0), pos(3, 0)));
        assert_eq!(edits[0].new_text, "new1\nnew2\nnew3\n");
    }

    #[test]
    fn replace_with_more_lines_than_original() {
        let src = "x\ntarget\ny\n";
        let diff = unified_diff("x.mty", 2, &["target"], &["a", "b", "c"]);
        let edits = unified_diff_to_text_edits(&diff, src);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range, range(pos(1, 0), pos(2, 0)));
        assert_eq!(edits[0].new_text, "a\nb\nc\n");
    }

    // ---------- deletion ----------

    #[test]
    fn pure_deletion_single_line() {
        let src = "keep\ndrop\nkeep2\n";
        let diff = unified_diff("x.mty", 2, &["drop"], &[]);
        let edits = unified_diff_to_text_edits(&diff, src);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range, range(pos(1, 0), pos(2, 0)));
        assert_eq!(edits[0].new_text, "");
    }

    #[test]
    fn pure_deletion_multi_line() {
        let src = "a\nb\nc\nd\n";
        let diff = unified_diff("x.mty", 2, &["b", "c"], &[]);
        let edits = unified_diff_to_text_edits(&diff, src);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range, range(pos(1, 0), pos(3, 0)));
        assert_eq!(edits[0].new_text, "");
    }

    // ---------- insertion ----------

    #[test]
    fn insert_before_line_with_builder() {
        let src = "a\nb\nc\n";
        let f = FixBuilder::new("t", "r", 0.8)
            .insert_before("x.mty", 2, &["new"])
            .build();
        let edits = unified_diff_to_text_edits(&f.diff, src);
        assert_eq!(edits.len(), 1);
        // `insert_before` creates `@@ -2,0 +2,1 @@` — insert before line 2.
        assert_eq!(edits[0].range, range(pos(1, 0), pos(1, 0)));
        assert_eq!(edits[0].new_text, "new\n");
    }

    #[test]
    fn insert_multiple_lines() {
        let src = "a\nb\n";
        let f = FixBuilder::new("t", "r", 0.8)
            .insert_before("x.mty", 2, &["one", "two"])
            .build();
        let edits = unified_diff_to_text_edits(&f.diff, src);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range, range(pos(1, 0), pos(1, 0)));
        assert_eq!(edits[0].new_text, "one\ntwo\n");
    }

    #[test]
    fn insert_at_start_of_file() {
        let src = "first\nsecond\n";
        let f = FixBuilder::new("t", "r", 0.8)
            .insert_before("x.mty", 1, &["// header"])
            .build();
        let edits = unified_diff_to_text_edits(&f.diff, src);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range, range(pos(0, 0), pos(0, 0)));
        assert_eq!(edits[0].new_text, "// header\n");
    }

    // ---------- multi-hunk ----------

    #[test]
    fn multi_hunk_yields_one_edit_per_hunk() {
        let src = "a\nb\nc\nd\ne\n";
        // Two hunks: replace line 2, replace line 4.
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
        let edits = unified_diff_to_text_edits(diff, src);
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].range, range(pos(1, 0), pos(2, 0)));
        assert_eq!(edits[0].new_text, "B\n");
        assert_eq!(edits[1].range, range(pos(3, 0), pos(4, 0)));
        assert_eq!(edits[1].new_text, "D\n");
    }

    // ---------- mismatch / safety ----------

    #[test]
    fn returns_no_edits_for_empty_diff() {
        assert!(unified_diff_to_text_edits("", "src\n").is_empty());
    }

    #[test]
    fn returns_no_edits_when_old_lines_dont_match_source() {
        let src = "actual\n";
        // Diff claims line 1 is "wrong" but source says "actual".
        let diff = unified_diff("x.mty", 1, &["wrong"], &["new"]);
        let edits = unified_diff_to_text_edits(&diff, src);
        assert!(
            edits.is_empty(),
            "should refuse to apply when OLD doesn't match"
        );
    }

    #[test]
    fn handles_diff_with_context_lines() {
        // Real `git diff` shape with leading + trailing context.
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
        let edits = unified_diff_to_text_edits(diff, src);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range, range(pos(1, 0), pos(2, 0)));
        assert_eq!(edits[0].new_text, "BODY\n");
    }

    #[test]
    fn marquee_mt4099_three_alternatives_yield_three_edits() {
        // Simulate a 3-alternative untaint diff by parsing the
        // FixBuilder output for each of MT4099's three strategies.
        // Each individual diff yields exactly one TextEdit; the LSP
        // emits 3 separate CodeActions, one per alternative.
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
        for alt in &alts {
            let edits = unified_diff_to_text_edits(&alt.diff, src);
            assert_eq!(edits.len(), 1, "each alt yields a single edit");
            assert_eq!(edits[0].range, range(pos(0, 0), pos(1, 0)));
        }
    }
}
