//! v0.33 T4 — per-MTxxxx fix-proposal engines.
//!
//! Each handler reads a `Diagnostic` + source context and produces a
//! `Fix` (or returns `None` when no high-enough-confidence
//! suggestion exists). Engines are keyed on the diagnostic code.
//!
//! Coverage target: 30+ of the most-common MTxxxx codes. The marquee
//! handler is MT4099 (taint-flow-to-sink) which ships three untaint
//! strategies. See `docs/internals/diagnostic-envelopes.md` for the
//! full code/fix matrix.

use crate::codes::{explain, DiagCode};
use crate::diagnostic::Diagnostic;
use crate::fix::{
    severity_str, span_info_from, DiagnosticEnvelope, Fix, FixAlternative, FixBuilder, FixKind,
};

/// Build a complete envelope for `diag`, dispatching to the per-code
/// fix engine. Always returns an envelope — handlers that can't
/// suggest a fix omit the `fix` field.
pub fn build_envelope(diag: &Diagnostic, source_id: &str, source: &str) -> DiagnosticEnvelope {
    let span = span_info_from(source_id, source, diag.primary.start, diag.primary.end);

    // The prose: explain() text if available, plus per-site notes.
    let base_prose = explain(diag.code).unwrap_or("").trim().to_string();
    let mut prose_parts: Vec<String> = Vec::new();
    if !base_prose.is_empty() {
        prose_parts.push(base_prose);
    }
    for note in &diag.notes {
        prose_parts.push(format!("note: {}", note));
    }
    for help in &diag.helps {
        prose_parts.push(format!("help: {}", help));
    }
    let prose = if prose_parts.is_empty() {
        diag.primary.message.clone()
    } else {
        prose_parts.join("\n\n")
    };

    // Per-code fix engine.
    let fix = fix_for(diag, source_id, source, &span);

    // see_also: related codes.
    let see_also = see_also_for(diag.code);

    DiagnosticEnvelope {
        code: diag.code.as_str(),
        severity: severity_str(diag.severity).to_string(),
        span,
        title: diag.primary.message.clone(),
        prose,
        fix,
        see_also,
        source: None,
    }
}

/// Read the line of source code that contains the diagnostic's
/// primary label. Used by handlers that emit a replacement diff.
fn current_line(source: &str, line_no: u32) -> Option<&str> {
    source.split('\n').nth(line_no.saturating_sub(1) as usize)
}

/// Identifier-ish token at (or near) `start`. Returns the token text
/// and its starting byte offset.
fn identifier_at(source: &str, start: usize) -> Option<(String, usize)> {
    let bytes = source.as_bytes();
    if start >= bytes.len() {
        return None;
    }
    // Walk backward to find the first non-ident byte.
    let mut s = start;
    while s > 0 && is_ident_byte(bytes[s - 1]) {
        s -= 1;
    }
    let mut e = start;
    while e < bytes.len() && is_ident_byte(bytes[e]) {
        e += 1;
    }
    if e == s {
        return None;
    }
    Some((source[s..e].to_string(), s))
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Compute Levenshtein distance between two strings. Used by
/// rename-suggestion fixes. Capped at 8 for performance.
fn levenshtein(a: &str, b: &str) -> usize {
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let (m, n) = (ac.len(), bc.len());
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if ac[i - 1] == bc[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

/// Find the closest identifier in `source` to `target` (by Levenshtein).
/// Returns (candidate, distance). Caller usually filters dist <= 2.
fn closest_ident(source: &str, target: &str) -> Option<(String, usize)> {
    let mut best: Option<(String, usize)> = None;
    let mut current = String::new();
    for ch in source.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch);
        } else if !current.is_empty() {
            try_replace_best(&mut best, &current, target);
            current.clear();
        }
    }
    if !current.is_empty() {
        try_replace_best(&mut best, &current, target);
    }
    best
}

fn try_replace_best(best: &mut Option<(String, usize)>, candidate: &str, target: &str) {
    if candidate == target || candidate.len() < 2 {
        return;
    }
    let d = levenshtein(candidate, target);
    if d == 0 {
        return;
    }
    let take = match best {
        None => true,
        Some((_, bd)) => d < *bd,
    };
    if take {
        *best = Some((candidate.to_string(), d));
    }
}

/// Per-code dispatcher.
fn fix_for(
    diag: &Diagnostic,
    source_id: &str,
    source: &str,
    span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    match diag.code.0 {
        // Lex/parse
        12 => fix_mismatched_delimiter(source_id, source, span),
        21 => fix_pub_needs_return_type(source_id, source, span),

        // HIR
        1001 => fix_unresolved_name(diag, source_id, source, span),
        1002 => fix_use_resolves_to_nothing(diag, source_id, source, span),

        // Types
        2001 => fix_type_mismatch(diag, source_id, source, span),
        2002 => fix_unresolved_type(diag, source_id, source, span),
        2003 => fix_cannot_infer_type(source_id, source, span),
        2005 => fix_wrong_arg_count(diag, source_id, source, span),
        2006 => fix_unknown_field(diag, source_id, source, span),
        2007 => fix_unknown_method(diag, source_id, source, span),
        2010 => fix_question_outside_result(source_id, source, span),
        2013 => fix_missing_struct_field(diag, source_id, source, span),
        2015 => fix_non_exhaustive_match(source_id, source, span),
        2018 => fix_if_branch_mismatch(source_id, source, span),
        2019 => fix_return_type_mismatch(source_id, source, span),
        2020 => fix_pub_param_needs_type(source_id, source, span),
        2021 => fix_unresolved_value(diag, source_id, source, span),
        2026 => fix_protocol_msg_unknown(diag, source_id, source, span),

        // Borrow
        3001 => fix_use_after_move(source_id, source, span),
        3004..=3006 => fix_borrow_conflict(source_id, source, span, diag.code.0),
        3013 | 3014 => fix_immut_local(source_id, source, span),
        3015 => fix_use_uninitialized(source_id, source, span),

        // Effects + capabilities + taint
        4001 => fix_effect_undeclared(diag, source_id, source, span),
        4010 => fix_cap_too_broad(source_id, source, span),
        4032 => fix_protocol_missing_handler(diag, source_id, source, span),
        4050 | 4059 => fix_row_subsumption_fail(source_id, source, span),
        4055 | 4057 => fix_row_var_unused(source_id, source, span),
        4060 => fix_cap_name_unbound(diag, source_id, source, span),
        4099 => fix_tainted_to_sink(diag, source_id, source, span),

        // Macros
        6001 => fix_unknown_macro(diag, source_id, source, span),
        6017 => fix_computer_use_missing_cap(source_id, source, span),

        // Codegen runtime traps fold to "this is a runtime-only
        // diagnostic; no source-level fix to propose".
        _ => None,
    }
}

// -------- MT4099: tainted value flows to sink (the marquee fix) ---

fn fix_tainted_to_sink(
    _diag: &Diagnostic,
    source_id: &str,
    source: &str,
    span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    // Pull the line so we can produce three concrete untaint
    // alternatives. We assume the sink call is on one line and the
    // tainted value is the immediately-preceding identifier in the
    // call. The actual untaint strategy is hand-written per
    // alternative; the diff replaces the offending line with a guarded
    // version.
    let line = current_line(source, span.line)?;
    let trimmed_indent = line.len() - line.trim_start().len();
    let indent = &line[..trimmed_indent];

    // Extract the tainted identifier from the diagnostic span if we can.
    let tainted = identifier_at(source, span.byte_start as usize)
        .map(|(s, _)| s)
        .unwrap_or_else(|| "value".to_string());

    let alt_regex = FixBuilder::new(
        "Constrain via a known-safe regex",
        "Extracts the value only if it matches a known-safe character class. \
         Returns Option[Str]; downstream code consumes the Some branch.",
        0.92,
    )
    .replace_lines(
        source_id,
        span.line,
        &[line],
        &[
            &format!(
                "{indent}if let Some(safe) = {tainted}.matches_regex(r\"^[A-Za-z0-9 _.-]+$\") {{"
            ),
            &format!("{indent}  {}", line.trim_start().replace(&tainted, "safe")),
            &format!("{indent}}}"),
        ],
    )
    .build();

    let alt_sanitizer = FixBuilder::new(
        "Apply a provably-correct sanitizer",
        "Routes the tainted value through a typed sanitizer (HtmlEscape / ShellEscape / SqlEscape / PathBoundary) \
         whose post-condition the type system enforces.",
        0.90,
    )
    .replace_line(
        source_id,
        span.line,
        line,
        &format!(
            "{indent}{}",
            line.trim_start().replace(
                &tainted,
                &format!("{tainted}.sanitize_with(HtmlEscape)"),
            )
        ),
    )
    .build();

    let alt_allowlist = FixBuilder::new(
        "Parse against an enum allowlist",
        "Narrows the value to an enum's variants. Anything outside the allowlist is dropped before reaching the sink.",
        0.85,
    )
    .replace_lines(
        source_id,
        span.line,
        &[line],
        &[
            &format!("{indent}if let Some(safe) = {tainted}.in_allowlist[Verdict]() {{"),
            &format!("{indent}  {}", line.trim_start().replace(&tainted, "safe")),
            &format!("{indent}}}"),
        ],
    )
    .build();

    Some(Fix {
        kind: FixKind::Untaint.as_str().to_string(),
        confidence: 0.92,
        alternatives: vec![alt_regex, alt_sanitizer, alt_allowlist],
    })
}

// -------- MT1001 / MT2021: unresolved name / value -----------------

fn fix_unresolved_name(
    _diag: &Diagnostic,
    source_id: &str,
    source: &str,
    span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let line = current_line(source, span.line)?;
    let (ident, _) = identifier_at(source, span.byte_start as usize)?;

    let mut alts: Vec<FixAlternative> = Vec::new();

    // 1. Closest in-scope spelling.
    if let Some((candidate, dist)) = closest_ident(source, &ident) {
        if dist <= 2 {
            let new_line = line.replace(&ident, &candidate);
            let conf = match dist {
                1 => 0.75,
                2 => 0.6,
                _ => 0.5,
            };
            alts.push(
                FixBuilder::new(
                    format!("Rename `{ident}` to `{candidate}`"),
                    format!(
                        "Edit distance {dist} from the identifier you wrote; \
                         most likely a typo."
                    ),
                    conf,
                )
                .replace_line(source_id, span.line, line, &new_line)
                .build(),
            );
        }
    }

    // 2. Propose a `use` for std.* names.
    if is_probable_std_path(&ident) {
        alts.push(
            FixBuilder::new(
                format!("Bring `{ident}` into scope with `use std.{ident}`"),
                "Mighty's stdlib modules need an explicit `use` before their \
                 short names resolve.",
                0.6,
            )
            .insert_before(
                source_id,
                find_first_non_package_line(source),
                &[&format!("use std.{ident}")],
            )
            .build(),
        );
    }

    // 3. Fallback: add a `let` binding above the use-site.
    alts.push(
        FixBuilder::new(
            format!("Declare `{ident}` before this line"),
            "Introduce a binding so the identifier resolves. The expected type \
             depends on the surrounding context.",
            0.5,
        )
        .insert_before(source_id, span.line, &[&format!("  let {ident} = todo()")])
        .build(),
    );

    let best = alts.iter().map(|a| a.confidence).fold(0.0_f32, f32::max);
    Some(Fix {
        kind: FixKind::RenameToMatchDecl.as_str().to_string(),
        confidence: best,
        alternatives: alts,
    })
}

fn fix_unresolved_value(
    diag: &Diagnostic,
    source_id: &str,
    source: &str,
    span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    // Same shape as MT1001 — try the rename + use + declare set.
    fix_unresolved_name(diag, source_id, source, span)
}

fn is_probable_std_path(ident: &str) -> bool {
    matches!(
        ident,
        "fs" | "net" | "process" | "json" | "env" | "time" | "log" | "random" | "swarm"
    )
}

fn find_first_non_package_line(source: &str) -> u32 {
    // After a `package X` line we typically have a blank line; this
    // function returns the line *number* (1-indexed) of the FIRST
    // import-friendly position. v0.33 keeps it simple: line 2 if line 1
    // starts with `package`, else line 1.
    let first = source.split('\n').next().unwrap_or("");
    if first.trim_start().starts_with("package") {
        2
    } else {
        1
    }
}

// -------- MT1002: use resolves to nothing --------------------------

fn fix_use_resolves_to_nothing(
    _diag: &Diagnostic,
    source_id: &str,
    source: &str,
    span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let line = current_line(source, span.line)?;
    // Suggest one obvious correction: collapse `pkg::mod::item` to
    // `pkg.mod.item` (this is the most common Rust-vs-Mighty mistake).
    if line.contains("::") {
        let fixed = line.replace("::", ".");
        return Some(Fix {
            kind: FixKind::MissingImport.as_str().to_string(),
            confidence: 0.95,
            alternatives: vec![FixBuilder::new(
                "Use `.` as the module separator",
                "Mighty's path separator is `.`, not `::`. Replacing them resolves the import.",
                0.95,
            )
            .replace_line(source_id, span.line, line, &fixed)
            .build()],
        });
    }
    None
}

// -------- MT2001 / MT2018 / MT2019: type mismatches ----------------

fn fix_type_mismatch(
    diag: &Diagnostic,
    source_id: &str,
    source: &str,
    span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    // Heuristic: if the diagnostic mentions Str vs I32 or similar,
    // propose a `.parse()` / `.to_string()` / explicit cast.
    let msg = diag.primary.message.to_lowercase();
    let line = current_line(source, span.line)?;
    let mut alts: Vec<FixAlternative> = Vec::new();

    if msg.contains("str") && (msg.contains("i32") || msg.contains("i64") || msg.contains("u32")) {
        alts.push(
            FixBuilder::new(
                "Convert with `.parse()`",
                "Parses the string into the expected integer type. Returns Result; \
                 handle the Err branch.",
                0.75,
            )
            .diff(format!(
                "--- a/{f}\n+++ b/{f}\n@@ -{l},1 +{l},1 @@\n-{old}\n+{new}.parse()?\n",
                f = source_id,
                l = span.line,
                old = line,
                new = line.trim_end()
            ))
            .build(),
        );
    }
    if msg.contains("i32") && msg.contains("str") {
        alts.push(
            FixBuilder::new(
                "Convert with `.to_string()`",
                "Converts the integer back to its string representation before use.",
                0.7,
            )
            .diff(format!(
                "--- a/{f}\n+++ b/{f}\n@@ -{l},1 +{l},1 @@\n-{old}\n+{new}.to_string()\n",
                f = source_id,
                l = span.line,
                old = line,
                new = line.trim_end()
            ))
            .build(),
        );
    }
    // Always offer the "fix the annotation" generic fallback.
    alts.push(
        FixBuilder::new(
            "Update the expected type or annotation",
            "Manual: re-read the expected vs found types in the error and adjust \
             either the value's construction or the binding's annotation.",
            0.5,
        )
        .diff(String::new())
        .build(),
    );

    let best = alts.iter().map(|a| a.confidence).fold(0.0_f32, f32::max);
    Some(Fix {
        kind: FixKind::TypeConversion.as_str().to_string(),
        confidence: best,
        alternatives: alts,
    })
}

fn fix_if_branch_mismatch(
    source_id: &str,
    source: &str,
    span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let line = current_line(source, span.line)?;
    Some(Fix {
        kind: FixKind::TypeConversion.as_str().to_string(),
        confidence: 0.55,
        alternatives: vec![FixBuilder::new(
            "Unify the two branch types",
            "Both branches of an `if` expression must produce the same type. \
             Either convert one branch's value or replace the if with a `match`.",
            0.55,
        )
        .replace_line(source_id, span.line, line, line)
        .diff(String::new())
        .build()],
    })
}

fn fix_return_type_mismatch(
    _source_id: &str,
    source: &str,
    span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let line = current_line(source, span.line)?;
    let _ = line;
    Some(Fix {
        kind: FixKind::AddReturnType.as_str().to_string(),
        confidence: 0.6,
        alternatives: vec![
            FixBuilder::new(
                "Update the fn's declared return type",
                "Change the `-> T` annotation to match the body's actual return type.",
                0.6,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Convert the returned value",
                "Insert an explicit conversion (`.to_string()`, `.into()`, `.parse()?`) \
                 so the body matches the declared return type.",
                0.55,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT2002: unresolved type ----------------------------------

fn fix_unresolved_type(
    _diag: &Diagnostic,
    source_id: &str,
    source: &str,
    span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let (ident, _) = identifier_at(source, span.byte_start as usize)?;
    // Common-types nudge: try snapping to one of the standard names.
    let standards = [
        "Str", "I32", "I64", "U32", "U64", "F32", "F64", "Bool", "Bytes", "Unit",
    ];
    let mut best: Option<(&str, usize)> = None;
    for s in standards {
        let d = levenshtein(s, &ident);
        if d <= 2 && d != 0 {
            best = Some(match best {
                None => (s, d),
                Some((_, pd)) if d < pd => (s, d),
                other => other.unwrap(),
            });
        }
    }
    let line = current_line(source, span.line)?;
    let mut alts: Vec<FixAlternative> = Vec::new();
    if let Some((target, _)) = best {
        let new_line = line.replace(&ident, target);
        alts.push(
            FixBuilder::new(
                format!("Rename to `{target}`"),
                format!("`{target}` is a standard type close to the name you wrote."),
                0.85,
            )
            .replace_line(source_id, span.line, line, &new_line)
            .build(),
        );
    }
    alts.push(
        FixBuilder::new(
            format!("Import `{ident}` from another module"),
            "If the type lives in a sibling module, add a `use pkg.mod.Type` near the top of the file.",
            0.55,
        )
        .insert_before(source_id, find_first_non_package_line(source), &[&format!("use pkg.mod.{ident}")])
        .build(),
    );
    let best_conf = alts.iter().map(|a| a.confidence).fold(0.0_f32, f32::max);
    Some(Fix {
        kind: FixKind::MissingImport.as_str().to_string(),
        confidence: best_conf,
        alternatives: alts,
    })
}

// -------- MT2003: cannot infer ------------------------------------

fn fix_cannot_infer_type(
    source_id: &str,
    source: &str,
    span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let line = current_line(source, span.line)?;
    if let Some(ident_end) = line.find('=') {
        let head = line[..ident_end].trim_end();
        // Insert ": Str" before "=" — caller picks the real type.
        let new_line = if head.contains(':') {
            return None;
        } else {
            format!("{head}: T = {}", line[ident_end + 1..].trim_start())
        };
        return Some(Fix {
            kind: FixKind::AddTypeAnnotation.as_str().to_string(),
            confidence: 0.65,
            alternatives: vec![FixBuilder::new(
                "Add an explicit type annotation",
                "When inference has too little context, an explicit `: T` annotation tells the \
                 type checker exactly what you mean. Replace `T` with the intended type.",
                0.65,
            )
            .replace_line(source_id, span.line, line, &new_line)
            .build()],
        });
    }
    None
}

// -------- MT2005: wrong arg count ----------------------------------

fn fix_wrong_arg_count(
    _diag: &Diagnostic,
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.55,
        alternatives: vec![FixBuilder::new(
            "Pass the missing argument (or remove the extra)",
            "Inspect the function's declared signature and align the call to it.",
            0.55,
        )
        .diff(String::new())
        .build()],
    })
}

// -------- MT2006: unknown field ------------------------------------

fn fix_unknown_field(
    _diag: &Diagnostic,
    source_id: &str,
    source: &str,
    span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let line = current_line(source, span.line)?;
    let (ident, _) = identifier_at(source, span.byte_start as usize)?;
    if let Some((candidate, dist)) = closest_ident(source, &ident) {
        if dist <= 2 {
            let new_line = line.replace(&ident, &candidate);
            let conf = if dist == 1 { 0.8 } else { 0.6 };
            return Some(Fix {
                kind: FixKind::RenameToMatchDecl.as_str().to_string(),
                confidence: conf,
                alternatives: vec![FixBuilder::new(
                    format!("Rename to the closest in-scope field `{candidate}`"),
                    "Edit-distance heuristic surfaced a likely typo.",
                    conf,
                )
                .replace_line(source_id, span.line, line, &new_line)
                .build()],
            });
        }
    }
    None
}

// -------- MT2007: unknown method -----------------------------------

fn fix_unknown_method(
    _diag: &Diagnostic,
    source_id: &str,
    source: &str,
    span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    // Same rename + trait-import shape as MT1001.
    let line = current_line(source, span.line)?;
    let (ident, _) = identifier_at(source, span.byte_start as usize)?;
    let mut alts: Vec<FixAlternative> = Vec::new();
    if let Some((candidate, dist)) = closest_ident(source, &ident) {
        if dist <= 2 {
            let new_line = line.replace(&ident, &candidate);
            let conf = if dist == 1 { 0.78 } else { 0.6 };
            alts.push(
                FixBuilder::new(
                    format!("Rename to `{candidate}`"),
                    format!(
                        "Edit distance {dist}; most likely a typo. Method resolution \
                         only searches the trait imports in this file."
                    ),
                    conf,
                )
                .replace_line(source_id, span.line, line, &new_line)
                .build(),
            );
        }
    }
    alts.push(
        FixBuilder::new(
            format!("Import the trait that defines `{ident}`"),
            "If the method lives on a trait, bring the trait into scope with \
             `use pkg.mod.Trait` so the receiver's `impl` is visible.",
            0.55,
        )
        .insert_before(
            source_id,
            find_first_non_package_line(source),
            &["use pkg.mod.Trait"],
        )
        .build(),
    );
    let best = alts.iter().map(|a| a.confidence).fold(0.0_f32, f32::max);
    Some(Fix {
        kind: FixKind::RenameToMatchDecl.as_str().to_string(),
        confidence: best,
        alternatives: alts,
    })
}

// -------- MT2010: `?` outside Result -------------------------------

fn fix_question_outside_result(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::UnpackQuestion.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                "Change the fn return type to `Result[T, E]`",
                "`?` requires the enclosing function to return a Result. \
                 Adding `-> Result[T, E]` makes the operator legal.",
                0.7,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Replace `?` with an explicit `match`",
                "`match expr { Ok(v) => v, Err(e) => return Err(e) }` is the \
                 desugared form; use it when the fn must not return Result.",
                0.6,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT2013: missing struct field -----------------------------

fn fix_missing_struct_field(
    diag: &Diagnostic,
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    // Extract the field name from the message: e.g. "missing field `foo`".
    let msg = &diag.primary.message;
    let field = msg
        .split('`')
        .nth(1)
        .map(|s| s.to_string())
        .unwrap_or_default();
    let mut alts: Vec<FixAlternative> = Vec::new();
    if !field.is_empty() {
        alts.push(
            FixBuilder::new(
                format!("Add the missing field `{field}`"),
                "Provide a value (or wrap in Some if the field is Option-typed).",
                0.7,
            )
            .diff(String::new())
            .build(),
        );
    }
    alts.push(
        FixBuilder::new(
            "Wrap the value in `Some(...)`",
            "If the field is `Option[T]`, the literal needs `Some(...)` around the value.",
            0.5,
        )
        .diff(String::new())
        .build(),
    );
    let best = alts.iter().map(|a| a.confidence).fold(0.0_f32, f32::max);
    Some(Fix {
        kind: FixKind::AddStructField.as_str().to_string(),
        confidence: best,
        alternatives: alts,
    })
}

// -------- MT2015: non-exhaustive match -----------------------------

fn fix_non_exhaustive_match(
    source_id: &str,
    source: &str,
    span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let line = current_line(source, span.line)?;
    let trimmed_indent = line.len() - line.trim_start().len();
    let indent = &line[..trimmed_indent];
    Some(Fix {
        kind: FixKind::AddMatchArm.as_str().to_string(),
        confidence: 0.75,
        alternatives: vec![
            FixBuilder::new(
                "Add a wildcard arm",
                "`_ => ...` matches every unhandled variant. Use this when you \
                 genuinely want a catch-all default.",
                0.75,
            )
            .insert_before(source_id, span.line + 1, &[&format!("{indent}_ => todo()")])
            .build(),
            FixBuilder::new(
                "Enumerate each missing variant",
                "Spelling out every variant lets the compiler re-check exhaustiveness \
                 when the enum gains a new case.",
                0.65,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT2020: pub param needs type -----------------------------

fn fix_pub_param_needs_type(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::AddTypeAnnotation.as_str().to_string(),
        confidence: 0.85,
        alternatives: vec![FixBuilder::new(
            "Annotate every parameter",
            "`pub` fn parameters must be explicitly typed so downstream callers \
             can rely on the signature.",
            0.85,
        )
        .diff(String::new())
        .build()],
    })
}

// -------- MT2026: protocol message unknown -------------------------

fn fix_protocol_msg_unknown(
    _diag: &Diagnostic,
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::DeclareProtocolMessage.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                "Declare the message in a protocol",
                "Add `protocol P { Msg(args: T) }` (or a matching `on Msg` in an \
                 imported protocol) so the handler's signature is known.",
                0.7,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Compose an existing protocol",
                "If another protocol already declares the message, list it in the \
                 agent's `: Proto1 + Proto2` clause.",
                0.6,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT0012: mismatched delimiter -----------------------------

fn fix_mismatched_delimiter(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::BalanceDelimiters.as_str().to_string(),
        confidence: 0.6,
        alternatives: vec![FixBuilder::new(
            "Balance the delimiters",
            "Walk back from the highlighted token to find the unmatched `(`, `[`, or `{` \
             and add the corresponding closer (or remove the stray opener).",
            0.6,
        )
        .diff(String::new())
        .build()],
    })
}

// -------- MT0021: pub needs return type ----------------------------

fn fix_pub_needs_return_type(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::AddReturnType.as_str().to_string(),
        confidence: 0.85,
        alternatives: vec![
            FixBuilder::new(
                "Annotate `-> Unit` if the fn returns nothing",
                "Explicit `-> Unit` documents the intent and satisfies the `pub` requirement.",
                0.85,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Annotate the actual return type",
                "If the body produces a value, declare its type (`-> Str`, `-> I32`, ...) \
                 so callers in other modules can rely on the signature.",
                0.75,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT3001: use after move -----------------------------------

fn fix_use_after_move(source_id: &str, source: &str, span: &crate::fix::SpanInfo) -> Option<Fix> {
    let line = current_line(source, span.line)?;
    Some(Fix {
        kind: FixKind::AddClone.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                "Clone the value before the move",
                "Adds `.clone()` so the move and the subsequent read both have valid owners.",
                0.7,
            )
            .replace_line(
                source_id,
                span.line,
                line,
                &format!("{}.clone()", line.trim_end()),
            )
            .build(),
            FixBuilder::new(
                "Pass a reference instead of moving",
                "Replace the move with `&` so the original binding keeps ownership.",
                0.65,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT3004 / MT3005 / MT3006: borrow conflicts ---------------

fn fix_borrow_conflict(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
    code: u16,
) -> Option<Fix> {
    let label = match code {
        3004 => "Move the `&mut` after the last shared use",
        3005 => "Drop or last-use the `&mut` before the shared borrow",
        3006 => "Sequence the two mutations",
        _ => "Re-order the borrows",
    };
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.6,
        alternatives: vec![FixBuilder::new(
            label,
            "Mighty enforces exclusive-XOR-shared borrowing at all times. \
             Sequence the conflicting borrow past the highlighted use, \
             or split the data into field-disjoint borrows.",
            0.6,
        )
        .diff(String::new())
        .build()],
    })
}

// -------- MT3013 / MT3014: immutable local -------------------------

fn fix_immut_local(source_id: &str, source: &str, span: &crate::fix::SpanInfo) -> Option<Fix> {
    let line = current_line(source, span.line)?;
    // Naïve "add mut" fix: requires the user to manually find the
    // `let` site. We emit a no-diff alternative for them to apply.
    Some(Fix {
        kind: FixKind::AddMutability.as_str().to_string(),
        confidence: 0.75,
        alternatives: vec![FixBuilder::new(
            "Declare the binding with `let mut`",
            "Find the `let <name> = ...` line for this binding and replace it with `let mut <name> = ...`.",
            0.75,
        )
        .replace_line(source_id, span.line, line, line)
        .diff(String::new())
        .build()],
    })
}

// -------- MT3015: use of uninitialized -----------------------------

fn fix_use_uninitialized(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::InitializeBinding.as_str().to_string(),
        confidence: 0.75,
        alternatives: vec![FixBuilder::new(
            "Assign before first use",
            "Move the assignment above the highlighted read, or initialize the binding inline.",
            0.75,
        )
        .diff(String::new())
        .build()],
    })
}

// -------- MT4001: effect undeclared --------------------------------

fn fix_effect_undeclared(
    diag: &Diagnostic,
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    // Extract the effect name from the message if it's quoted with backticks.
    let msg = &diag.primary.message;
    let effect = msg.split('`').nth(1).unwrap_or("io").to_string();
    Some(Fix {
        kind: FixKind::AddEffect.as_str().to_string(),
        confidence: 0.8,
        alternatives: vec![
            FixBuilder::new(
                format!("Add `effect {effect}` to the fn"),
                "Effects are a contract with downstream packages. Declaring the missing \
                 effect makes the call legal at this site.",
                0.8,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Lift the capability into a parameter",
                "Pass the capability as an argument so the effect is local to the caller, \
                 not part of this fn's signature.",
                0.55,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT4010: capability too broad -----------------------------

fn fix_cap_too_broad(_source_id: &str, _source: &str, _span: &crate::fix::SpanInfo) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::AddCapability.as_str().to_string(),
        confidence: 0.65,
        alternatives: vec![FixBuilder::new(
            "Narrow at the call site",
            "Use the family's narrowing constructor (`fs.ro(...)`, `net.host(...)`, ...) \
             to produce a constraint compatible with the parameter's type.",
            0.65,
        )
        .diff(String::new())
        .build()],
    })
}

// -------- MT4032: protocol missing handler -------------------------

fn fix_protocol_missing_handler(
    diag: &Diagnostic,
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let msg = &diag.primary.message;
    let _name = msg.split('`').nth(1).unwrap_or("Msg").to_string();
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                "Implement the missing `on Msg(...)` handler",
                "Every protocol message declared in the agent's protocol clause needs a matching handler.",
                0.7,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Remove the protocol from the agent's declaration",
                "If the agent doesn't actually handle that message, drop the protocol from \
                 the `: ProtoA + ProtoB` clause.",
                0.55,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT4050 / MT4059: row subsumption fail --------------------

fn fix_row_subsumption_fail(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::AddEffect.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                "Add the missing effect to the caller's `effect` clause",
                "The closure's effect set was rejected by the caller's row constraint. \
                 Widening the caller's declared effects accepts the closure.",
                0.7,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Use a pure closure",
                "Removing the side-effect from the closure body avoids the constraint entirely.",
                0.55,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT4055 / MT4057: row var unused --------------------------

fn fix_row_var_unused(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::AddEffect.as_str().to_string(),
        confidence: 0.6,
        alternatives: vec![
            FixBuilder::new(
                "Drop the row variable; use a concrete row",
                "Replace `!{a, b | E}` with `!{a, b}`. Concrete rows have no binding site \
                 issues.",
                0.6,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Add a closure parameter that carries the row",
                "Adding `fn(...) -> _` lets the row variable be bound at the call site.",
                0.55,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT4060: capability name unbound --------------------------

fn fix_cap_name_unbound(
    diag: &Diagnostic,
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let msg = &diag.primary.message;
    let name = msg.split('`').nth(1).unwrap_or("cap").to_string();
    Some(Fix {
        kind: FixKind::AddCapability.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                format!("Declare `cap {name}` in scope"),
                "Adds the capability so the resolver can see it; the use-site is then valid.",
                0.7,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Pass the capability as a parameter",
                "Threading the cap through as an argument keeps the call site honest without \
                 widening the scope.",
                0.6,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT6001: unknown macro ------------------------------------

fn fix_unknown_macro(
    _diag: &Diagnostic,
    source_id: &str,
    source: &str,
    span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let line = current_line(source, span.line)?;
    let (ident, _) = identifier_at(source, span.byte_start as usize)?;
    // Known builtins.
    let knowns = ["log", "print", "format", "dbg", "todo", "panic", "assert"];
    let mut alts: Vec<FixAlternative> = Vec::new();
    for k in &knowns {
        let d = levenshtein(k, &ident);
        if d != 0 && d <= 2 {
            let new_line = line.replace(&ident, k);
            let conf = if d == 1 { 0.78 } else { 0.6 };
            alts.push(
                FixBuilder::new(
                    format!("Rename to `{k}`"),
                    format!("Built-in macro `{k}!` is the closest match (edit distance {d})."),
                    conf,
                )
                .replace_line(source_id, span.line, line, &new_line)
                .build(),
            );
        }
    }
    if alts.is_empty() {
        alts.push(
            FixBuilder::new(
                format!("Declare `macro {ident}(...) => {{ ... }}`"),
                "If you meant to define a new macro, add a declaration above the call site \
                 (or `use otherpkg.name` if it lives in another file).",
                0.6,
            )
            .insert_before(
                source_id,
                span.line,
                &[&format!("macro {ident}() => {{ /* body */ }}")],
            )
            .build(),
        );
    }
    let best = alts.iter().map(|a| a.confidence).fold(0.0_f32, f32::max);
    Some(Fix {
        kind: FixKind::CorrectMacroAttr.as_str().to_string(),
        confidence: best,
        alternatives: alts,
    })
}

// -------- MT6017: computer_use missing cap -------------------------

fn fix_computer_use_missing_cap(
    source_id: &str,
    source: &str,
    span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let line = current_line(source, span.line)?;
    // Insert `cap: computer.screen + computer.input` at a reasonable spot.
    let new_line = if line.contains("@computer_use(") {
        line.replace(
            "@computer_use(",
            "@computer_use(cap: computer.screen + computer.input, ",
        )
    } else {
        line.to_string()
    };
    Some(Fix {
        kind: FixKind::CorrectMacroAttr.as_str().to_string(),
        confidence: 0.88,
        alternatives: vec![FixBuilder::new(
            "Add `cap: computer.screen + computer.input`",
            "Computer Use without a capability is never safe. The cap argument bounds \
             what the dispatcher can do and is required.",
            0.88,
        )
        .replace_line(source_id, span.line, line, &new_line)
        .build()],
    })
}

// -------- see_also: per-code related codes -------------------------

fn see_also_for(code: DiagCode) -> Vec<String> {
    match code.0 {
        4099 => vec!["MT4001".into(), "docs/internals/taint-types.md".into()],
        1001 => vec!["MT1002".into(), "MT2002".into(), "MT2021".into()],
        1002 => vec!["MT1001".into()],
        2001 => vec!["MT2018".into(), "MT2019".into()],
        2002 => vec!["MT1002".into()],
        2007 => vec!["MT4020".into(), "MT4021".into()],
        2010 => vec!["MT2011".into()],
        2026 => vec!["MT4031".into(), "MT4032".into(), "MT4033".into()],
        3001 => vec!["MT3002".into(), "MT3008".into()],
        3004..=3006 => vec!["MT3001".into()],
        4001 => vec!["MT4050".into()],
        4050 => vec!["MT4055".into(), "MT4059".into()],
        4060 => vec!["MT4061".into(), "MT4062".into(), "MT4063".into()],
        6001 => vec!["MT6002".into(), "MT6003".into()],
        6017 => vec!["MT6018".into(), "MT6019".into(), "MT6020".into()],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codes;
    use crate::diagnostic::Label;
    use crate::fix::ToEnvelope;

    fn d(code: DiagCode, start: usize, end: usize, msg: &str) -> Diagnostic {
        Diagnostic::error(
            code,
            Label {
                start,
                end,
                message: msg.into(),
            },
        )
    }

    #[test]
    fn envelope_mt4099_yields_three_untaint_alternatives() {
        let src =
            "fn main() {\n  let user_input = ask()\n  std.fs.write(\"log.txt\", user_input)\n}\n";
        let start = src.find("user_input)").unwrap();
        let end = start + "user_input".len();
        let diag = d(
            codes::TAINTED_VALUE_TO_SINK,
            start,
            end,
            "tainted value flows to fs.write",
        );
        let env = diag.to_envelope("a.mty", src);
        assert_eq!(env.code, "MT4099");
        let fix = env.fix.unwrap();
        assert_eq!(fix.kind, "untaint");
        assert_eq!(fix.alternatives.len(), 3);
        assert!(fix.confidence >= 0.85);
        assert!(env.see_also.contains(&"MT4001".to_string()));
    }

    #[test]
    fn envelope_mt1001_offers_rename_when_close() {
        // "grting" should match "greeting" (one transposition).
        let src = "fn main() {\n  let greeting = \"hi\"\n  log(grting)\n}\n";
        let start = src.find("grting").unwrap();
        let end = start + "grting".len();
        let diag = d(
            codes::UNRESOLVED_NAME,
            start,
            end,
            "unresolved name `grting`",
        );
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.expect("MT1001 always proposes some fix");
        assert!(fix
            .alternatives
            .iter()
            .any(|a| a.label.contains("greeting")));
    }

    #[test]
    fn envelope_mt1002_collapses_double_colon() {
        let src = "use std::fs\n";
        let start = 0;
        let end = src.len() - 1;
        let diag = d(
            codes::USE_RESOLVES_TO_NOTHING,
            start,
            end,
            "use resolves to nothing",
        );
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.kind, "missing_import");
        assert!(fix.alternatives[0].diff.contains("use std.fs"));
    }

    #[test]
    fn envelope_mt2002_renames_to_standard_type() {
        let src = "fn f(x: Stng) -> Unit {}\n";
        let start = src.find("Stng").unwrap();
        let end = start + 4;
        let diag = d(codes::UNRESOLVED_TYPE, start, end, "unresolved type `Stng`");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives.iter().any(|a| a.label.contains("Str")));
    }

    #[test]
    fn envelope_mt2003_adds_type_annotation() {
        let src = "fn main() {\n  let x = mkArray()\n}\n";
        let start = src.find("x").unwrap();
        let diag = d(
            codes::CANNOT_INFER_TYPE,
            start,
            start + 1,
            "cannot infer type",
        );
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.kind, "add_type_annotation");
    }

    #[test]
    fn envelope_mt2005_offers_arity_fix() {
        let src = "fn main() { f(1, 2) }\n";
        let start = src.find("f(").unwrap();
        let diag = d(codes::WRONG_ARG_COUNT, start, start + 1, "wrong arg count");
        let env = diag.to_envelope("a.mty", src);
        assert!(env.fix.is_some());
    }

    #[test]
    fn envelope_mt2006_renames_misspelled_field() {
        let src = "struct S { name: Str }\nfn f(s: S) -> Str { s.naem }\n";
        let start = src.find(".naem").unwrap() + 1;
        let end = start + 4;
        let diag = d(codes::UNKNOWN_FIELD, start, end, "unknown field `naem`");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives[0].label.contains("name"));
    }

    #[test]
    fn envelope_mt2007_offers_rename_or_import() {
        let src = "fn main() {\n  let len = 3\n  log(\"hi\".lengt())\n}\n";
        let start = src.find("lengt").unwrap();
        let end = start + 5;
        let diag = d(codes::UNKNOWN_METHOD, start, end, "unknown method `lengt`");
        let env = diag.to_envelope("a.mty", src);
        assert!(env.fix.is_some());
    }

    #[test]
    fn envelope_mt2010_question_outside_result_two_alts() {
        let src = "fn main() { f()? }\n";
        let start = src.find('?').unwrap();
        let diag = d(
            codes::QUESTION_OUTSIDE_RESULT,
            start,
            start + 1,
            "`?` outside Result fn",
        );
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.alternatives.len(), 2);
    }

    #[test]
    fn envelope_mt2013_extracts_field_name() {
        let src = "fn main() {\n  let s = S { name: \"x\" }\n}\n";
        let start = src.find("S {").unwrap();
        let diag = d(
            codes::MISSING_STRUCT_FIELD,
            start,
            start + 1,
            "missing field `age`",
        );
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives[0].label.contains("`age`"));
    }

    #[test]
    fn envelope_mt2015_inserts_wildcard_arm() {
        let src = "fn main() {\n  match x {\n    A => 1\n  }\n}\n";
        let start = src.find("match").unwrap();
        let diag = d(
            codes::NON_EXHAUSTIVE_MATCH,
            start,
            start + 5,
            "non-exhaustive match",
        );
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives[0].diff.contains("_ => todo()"));
    }

    #[test]
    fn envelope_mt2018_if_branch_mismatch() {
        let src = "fn f() -> I32 { if true { 1 } else { \"x\" } }\n";
        let start = src.find("if").unwrap();
        let diag = d(
            codes::IF_BRANCH_MISMATCH,
            start,
            start + 2,
            "branch mismatch",
        );
        let env = diag.to_envelope("a.mty", src);
        assert!(env.fix.is_some());
    }

    #[test]
    fn envelope_mt2019_return_type_mismatch_offers_two_alts() {
        let src = "fn f() -> I32 { \"hi\" }\n";
        let start = src.find("\"hi\"").unwrap();
        let diag = d(
            codes::RETURN_TYPE_MISMATCH,
            start,
            start + 4,
            "return type mismatch",
        );
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.alternatives.len(), 2);
    }

    #[test]
    fn envelope_mt2020_pub_param_needs_type() {
        let src = "pub fn f(x) -> Unit {}\n";
        let start = src.find("x").unwrap();
        let diag = d(
            codes::PUB_PARAM_NEEDS_TYPE,
            start,
            start + 1,
            "pub fn param needs type",
        );
        let env = diag.to_envelope("a.mty", src);
        assert_eq!(env.fix.unwrap().kind, "add_type_annotation");
    }

    #[test]
    fn envelope_mt2021_unresolved_value_handled() {
        let src = "agent A: P {\n  on Tick() -> {\n    counter = 1\n  }\n}\n";
        let start = src.find("counter").unwrap();
        let diag = d(
            codes::UNRESOLVED_VALUE,
            start,
            start + 7,
            "unresolved value",
        );
        let env = diag.to_envelope("a.mty", src);
        assert!(env.fix.is_some());
    }

    #[test]
    fn envelope_mt2026_protocol_msg_unknown_two_alts() {
        let src = "agent A: P { on UnknownMsg() -> {} }\n";
        let start = src.find("UnknownMsg").unwrap();
        let diag = d(
            codes::PROTOCOL_MSG_UNKNOWN,
            start,
            start + 10,
            "protocol msg unknown",
        );
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.kind, "declare_protocol_message");
        assert_eq!(fix.alternatives.len(), 2);
    }

    #[test]
    fn envelope_mt0012_balance_delimiters() {
        let src = "fn main() { (1 }\n";
        let start = src.find('}').unwrap();
        let diag = d(
            codes::MISMATCHED_DELIMITER,
            start,
            start + 1,
            "mismatched delim",
        );
        let env = diag.to_envelope("a.mty", src);
        assert_eq!(env.fix.unwrap().kind, "balance_delimiters");
    }

    #[test]
    fn envelope_mt0021_pub_needs_return_type_two_alts() {
        let src = "pub fn f() {}\n";
        let start = 0;
        let diag = d(codes::PUB_NEEDS_RETURN_TYPE, start, 3, "pub fn needs -> T");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.alternatives.len(), 2);
        assert_eq!(fix.kind, "add_return_type");
    }

    #[test]
    fn envelope_mt3001_use_after_move_offers_clone() {
        let src = "fn main() {\n  let s = mk()\n  let t = move s\n  log(s)\n}\n";
        let start = src.find("log(s)").unwrap() + 4;
        let diag = d(codes::USE_AFTER_MOVE, start, start + 1, "use after move");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.kind, "add_clone");
    }

    #[test]
    fn envelope_mt3004_borrow_conflict_handled() {
        let src = "fn f() {}\n";
        let diag = d(
            codes::MUT_BORROW_WHILE_SHARED,
            0,
            2,
            "mut borrow while shared",
        );
        let env = diag.to_envelope("a.mty", src);
        assert!(env.fix.is_some());
    }

    #[test]
    fn envelope_mt3013_immut_local() {
        let src = "fn f() { let x = 1; x = 2 }\n";
        let start = src.rfind("x").unwrap();
        let diag = d(
            codes::MUT_BORROW_OF_IMMUT_LOCAL,
            start,
            start + 1,
            "immut local",
        );
        let env = diag.to_envelope("a.mty", src);
        assert_eq!(env.fix.unwrap().kind, "add_mutability");
    }

    #[test]
    fn envelope_mt3015_uninitialized() {
        let src = "fn f() { let x: I32; log(x) }\n";
        let start = src.rfind("x").unwrap();
        let diag = d(codes::USE_OF_UNINITIALIZED, start, start + 1, "uninit");
        let env = diag.to_envelope("a.mty", src);
        assert_eq!(env.fix.unwrap().kind, "initialize_binding");
    }

    #[test]
    fn envelope_mt4001_extracts_effect_name() {
        let src = "pub fn save() {}\n";
        let diag = d(codes::EFFECT_UNDECLARED, 0, 3, "missing `io` effect");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives[0].label.contains("io"));
    }

    #[test]
    fn envelope_mt4010_cap_too_broad() {
        let src = "fn f() {}\n";
        let diag = d(codes::CAPABILITY_TOO_BROAD, 0, 2, "cap too broad");
        let env = diag.to_envelope("a.mty", src);
        assert_eq!(env.fix.unwrap().kind, "add_capability");
    }

    #[test]
    fn envelope_mt4032_protocol_missing_handler() {
        let src = "agent A: P {}\n";
        let diag = d(
            codes::PROTOCOL_MISSING_HANDLER,
            0,
            5,
            "missing handler `Tick`",
        );
        let env = diag.to_envelope("a.mty", src);
        assert!(env.fix.is_some());
    }

    #[test]
    fn envelope_mt4050_row_subsumption_fail() {
        let src = "fn f() { xs.map(|x| log(x)) }\n";
        let diag = d(codes::ROW_SUBSUMPTION_FAIL, 0, 5, "row subsumption fail");
        let env = diag.to_envelope("a.mty", src);
        assert_eq!(env.fix.unwrap().kind, "add_effect");
    }

    #[test]
    fn envelope_mt4055_row_var_unused() {
        let src = "fn f[E]() !E {}\n";
        let diag = d(codes::ROW_VAR_UNUSED, 0, 4, "row var unused");
        let env = diag.to_envelope("a.mty", src);
        assert!(env.fix.is_some());
    }

    #[test]
    fn envelope_mt4060_cap_name_unbound() {
        let src = "fn f() { net.get(\"x\") }\n";
        let diag = d(codes::CAP_NAME_UNBOUND, 0, 3, "capability `net` is unbound");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        // Extracted name appears in at least one alternative label.
        assert!(fix.alternatives.iter().any(|a| a.label.contains("net")));
        assert_eq!(fix.kind, "add_capability");
    }

    #[test]
    fn envelope_mt6001_renames_to_known_builtin() {
        let src = "fn main() { logg!(\"hi\") }\n";
        let start = src.find("logg").unwrap();
        let diag = d(
            codes::UNKNOWN_MACRO,
            start,
            start + 4,
            "unknown macro `logg`",
        );
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives.iter().any(|a| a.label.contains("log")));
    }

    #[test]
    fn envelope_mt6017_computer_use_missing_cap() {
        let src = "@computer_use(width: 1280, height: 800)\nagent A {}\n";
        let start = src.find("@computer_use").unwrap();
        let diag = d(
            codes::COMPUTER_USE_MISSING_CAP,
            start,
            start + 13,
            "missing cap",
        );
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives[0].diff.contains("computer.screen"));
    }

    #[test]
    fn envelope_records_all_severity_strings() {
        // The diagnostic builder gives us Error; ensure warning rounds in
        // when constructed manually.
        let mut d = d(codes::PROTOCOL_MSG_UNKNOWN, 0, 1, "warn");
        d.severity = crate::diagnostic::Severity::Warning;
        let env = d.to_envelope("a.mty", "x");
        assert_eq!(env.severity, "warning");
    }

    #[test]
    fn envelope_low_confidence_codes_still_produce_envelope() {
        let src = "fn main() {}\n";
        let diag = d(DiagCode::new(9999), 0, 1, "unknown code");
        let env = diag.to_envelope("a.mty", src);
        // No fix for unknown code, but envelope still exists.
        assert!(env.fix.is_none());
        assert_eq!(env.code, "MT9999");
        assert_eq!(env.span.line, 1);
    }

    #[test]
    fn envelope_includes_notes_and_helps_in_prose() {
        let mut diag = d(codes::TAINTED_VALUE_TO_SINK, 0, 1, "tainted");
        diag = diag.with_note("originates from agent.ask()");
        diag = diag.with_help("untaint via .matches_regex(...)");
        let env = diag.to_envelope("a.mty", "x");
        assert!(env.prose.contains("note: originates"));
        assert!(env.prose.contains("help: untaint"));
    }
}
