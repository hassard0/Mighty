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
        schema_version: crate::fix::SCHEMA_VERSION.to_string(),
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
        1 => fix_unexpected_token(source_id, source, span),
        2 => fix_unterminated_string(source_id, source, span),
        3 => fix_invalid_escape(source_id, source, span),
        4 => fix_unknown_duration_unit(source_id, source, span),
        10 => fix_expected_item(source_id, source, span),
        11 => fix_expected_expr(source_id, source, span),
        12 => fix_mismatched_delimiter(source_id, source, span),
        20 => fix_duplicate_on_handler(source_id, source, span),
        21 => fix_pub_needs_return_type(source_id, source, span),

        // HIR
        1001 => fix_unresolved_name(diag, source_id, source, span),
        1002 => fix_use_resolves_to_nothing(diag, source_id, source, span),

        // Types
        2001 => fix_type_mismatch(diag, source_id, source, span),
        2002 => fix_unresolved_type(diag, source_id, source, span),
        2003 => fix_cannot_infer_type(source_id, source, span),
        2004 => fix_wrong_generic_arity(diag, source_id, source, span),
        2005 => fix_wrong_arg_count(diag, source_id, source, span),
        2006 => fix_unknown_field(diag, source_id, source, span),
        2007 => fix_unknown_method(diag, source_id, source, span),
        2008 => fix_not_callable(source_id, source, span),
        2009 => fix_unknown_variant(diag, source_id, source, span),
        2010 => fix_question_outside_result(source_id, source, span),
        2011 => fix_question_error_mismatch(source_id, source, span),
        2012 => fix_wrong_variant_arity(diag, source_id, source, span),
        2013 => fix_missing_struct_field(diag, source_id, source, span),
        2014 => fix_duplicate_struct_field(diag, source_id, source, span),
        2015 => fix_non_exhaustive_match(source_id, source, span),
        2016 => fix_unreachable_match_arm(source_id, source, span),
        2017 => fix_binop_type_mismatch(diag, source_id, source, span),
        2018 => fix_if_branch_mismatch(source_id, source, span),
        2019 => fix_return_type_mismatch(source_id, source, span),
        2020 => fix_pub_param_needs_type(source_id, source, span),
        2021 => fix_unresolved_value(diag, source_id, source, span),
        2022 => fix_not_a_struct(source_id, source, span),
        2023 => fix_generic_arg_mismatch(diag, source_id, source, span),
        2024 => fix_lambda_arity_mismatch(source_id, source, span),
        2025 => fix_cannot_take_ref(source_id, source, span),
        2026 => fix_protocol_msg_unknown(diag, source_id, source, span),
        2027 => fix_invalid_cast_int_to_char(source_id, source, span),

        // Borrow
        3001 => fix_use_after_move(source_id, source, span),
        3002 => fix_move_out_of_borrow(source_id, source, span),
        3003 => fix_borrow_after_move(source_id, source, span),
        3004..=3006 => fix_borrow_conflict(source_id, source, span, diag.code.0),
        3007 => fix_borrow_outlives_owner(source_id, source, span),
        3008 => fix_cannot_move_borrowed(source_id, source, span),
        3009 => fix_move_out_of_ref(source_id, source, span),
        3010 => fix_arena_escape(source_id, source, span),
        3011 => fix_non_sendable_message_arg(source_id, source, span),
        3012 => fix_drop_in_const_context(source_id, source, span),
        3013 | 3014 => fix_immut_local(source_id, source, span),
        3015 => fix_use_uninitialized(source_id, source, span),

        // Effects + capabilities + taint
        4001 => fix_effect_undeclared(diag, source_id, source, span),
        4002 => fix_alloc_in_core(source_id, source, span),
        4010 => fix_cap_too_broad(source_id, source, span),
        4020 => fix_method_ambiguous(diag, source_id, source, span),
        4021 => fix_method_not_found(diag, source_id, source, span),
        4022 => fix_trait_coherence_violation(source_id, source, span),
        4023 => fix_dyn_requires_object_safe(source_id, source, span),
        4030 => fix_protocol_arity_mismatch(source_id, source, span),
        4031 => fix_protocol_param_type_mismatch(source_id, source, span),
        4032 => fix_protocol_missing_handler(diag, source_id, source, span),
        4033 => fix_protocol_extra_handler(diag, source_id, source, span),
        4040 => fix_derive_copy_field_not_copy(source_id, source, span),
        4041 => fix_derive_unknown(diag, source_id, source, span),
        4050 | 4059 => fix_row_subsumption_fail(source_id, source, span),
        4051 => fix_row_occurs_check(source_id, source, span),
        4053 => fix_row_var_unbound(diag, source_id, source, span),
        4054 => fix_row_effect_mismatch(source_id, source, span),
        4055 | 4057 => fix_row_var_unused(source_id, source, span),
        4060 => fix_cap_name_unbound(diag, source_id, source, span),
        4061 => fix_cap_family_mismatch(diag, source_id, source, span),
        4062 => fix_cap_scope_violation(source_id, source, span),
        4063 => fix_cap_redeclaration(diag, source_id, source, span),
        4064 => fix_cap_method_unknown(diag, source_id, source, span),
        4099 => fix_tainted_to_sink(diag, source_id, source, span),

        // Macros
        6001 => fix_unknown_macro(diag, source_id, source, span),
        6002 => fix_macro_arity_mismatch(source_id, source, span),
        6009 => fix_macro_format_bad_template(source_id, source, span),
        6010 => fix_macro_format_unsupported_spec(source_id, source, span),
        6017 => fix_computer_use_missing_cap(source_id, source, span),
        6018 => fix_computer_use_malformed_cap(source_id, source, span),
        6019 => fix_computer_use_malformed_dimension(source_id, source, span),
        6020 => fix_computer_use_not_an_agent(source_id, source, span),

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

// =================================================================
// v0.34 T1 — backfill: MT0xxx parser polish
// =================================================================

// -------- MT0001: unexpected token ---------------------------------

fn fix_unexpected_token(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.55,
        alternatives: vec![
            FixBuilder::new(
                "Remove or replace the unexpected token",
                "The parser was in the middle of a known construct (let/fn/match/...) \
                 when this token appeared. Either delete the token or replace it with \
                 the expected one.",
                0.55,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Close the surrounding construct first",
                "An earlier missing closer (`)`, `]`, `}`) often produces a misleading \
                 \"unexpected token\" further down. Re-check the preceding lines.",
                0.5,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT0002: unterminated string ------------------------------

fn fix_unterminated_string(
    source_id: &str,
    source: &str,
    span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let line = current_line(source, span.line)?;
    // Append a closing quote at end of line.
    let new_line = if line.trim_end().ends_with('"') {
        line.to_string()
    } else {
        format!("{}\"", line.trim_end())
    };
    Some(Fix {
        kind: FixKind::BalanceDelimiters.as_str().to_string(),
        confidence: 0.85,
        alternatives: vec![
            FixBuilder::new(
                "Close the string literal",
                "The lexer reached end-of-line / end-of-input before the closing quote. \
                 Adding a trailing `\"` closes it.",
                0.85,
            )
            .replace_line(source_id, span.line, line, &new_line)
            .build(),
            FixBuilder::new(
                "Escape the embedded quote",
                "If the string contains a literal `\"`, escape it as `\\\"` so the lexer \
                 doesn't mis-read it as the closer.",
                0.6,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT0003: invalid escape -----------------------------------

fn fix_invalid_escape(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                "Use a supported escape (`\\n`, `\\t`, `\\r`, `\\\\`, `\\\"`)",
                "Mighty's string escapes are the standard short set plus `\\u{NNNN}` for \
                 unicode code-points. Other backslash sequences are rejected.",
                0.7,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Escape the backslash with `\\\\`",
                "If the literal `\\` is intentional (e.g. Windows paths), double it.",
                0.6,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT0004: unknown duration unit ----------------------------

fn fix_unknown_duration_unit(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.8,
        alternatives: vec![FixBuilder::new(
            "Use one of the known duration suffixes",
            "Mighty accepts `ns`, `us`, `ms`, `s`, `m`, `h`, `d` as duration suffixes. \
             Compound durations (`1h30m`) are written `1.h + 30.m` in this version.",
            0.8,
        )
        .diff(String::new())
        .build()],
    })
}

// -------- MT0010: expected item ------------------------------------

fn fix_expected_item(_source_id: &str, _source: &str, _span: &crate::fix::SpanInfo) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.55,
        alternatives: vec![
            FixBuilder::new(
                "Start the file with a top-level item",
                "Mighty files must contain items: `fn`, `struct`, `enum`, `agent`, \
                 `protocol`, `use`, or `package`. The parser saw something else here.",
                0.55,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Move expression code into an `fn`",
                "Free-standing statements aren't legal at the top level; wrap them in \
                 `fn main() { ... }` or another fn.",
                0.6,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT0011: expected expr ------------------------------------

fn fix_expected_expr(_source_id: &str, _source: &str, _span: &crate::fix::SpanInfo) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.55,
        alternatives: vec![FixBuilder::new(
            "Supply an expression here",
            "The parser was inside a position that requires an expression (right-hand \
             side of `=`, inside a call, after `return`, ...). Fill it in or remove the \
             surrounding construct.",
            0.55,
        )
        .diff(String::new())
        .build()],
    })
}

// -------- MT0020: duplicate on-handler -----------------------------

fn fix_duplicate_on_handler(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::RemoveUnreachable.as_str().to_string(),
        confidence: 0.8,
        alternatives: vec![
            FixBuilder::new(
                "Delete the duplicate `on` handler",
                "An agent may declare each protocol message exactly once. Keep the \
                 handler whose body you actually want and delete the other.",
                0.8,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Merge the two handlers into one",
                "If both handlers should run, combine their bodies in a single `on Msg` \
                 (use `match` on payload fields when behavior depends on input).",
                0.65,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// =================================================================
// v0.34 T1 — backfill: MT2xxx full coverage
// =================================================================

// -------- MT2004: wrong generic arity ------------------------------

fn fix_wrong_generic_arity(
    diag: &Diagnostic,
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let msg = &diag.primary.message;
    let expected = extract_first_number(msg).unwrap_or_else(|| "N".to_string());
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                format!("Provide exactly {expected} generic argument(s)"),
                "The type expects a specific number of generic parameters; supply the \
                 missing ones or drop the extras.",
                0.7,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Use `_` placeholders to let inference fill the rest",
                "When you only want to pin some generics, leave the rest as `_` and the \
                 checker will infer them from context.",
                0.6,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT2008: not callable -------------------------------------

fn fix_not_callable(source_id: &str, source: &str, span: &crate::fix::SpanInfo) -> Option<Fix> {
    let line = current_line(source, span.line)?;
    // Drop the `()` that immediately follows the identifier at the
    // diagnostic's span, if any. Falls back to leaving the line alone.
    let new_line = drop_parens_after_ident(line, source, span.byte_start as usize);
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                "Remove the call parens — it's a value, not a fn",
                "The expression bound here is a value (struct field, constant, ...). \
                 Drop the `()` to use it directly.",
                0.7,
            )
            .replace_line(source_id, span.line, line, &new_line)
            .build(),
            FixBuilder::new(
                "Call its method instead",
                "If the value has a callable method (e.g. `.call()`, `.invoke()`), use \
                 method-call syntax: `value.method(args)`.",
                0.55,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT2009: unknown variant ----------------------------------

fn fix_unknown_variant(
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
            let conf = if dist == 1 { 0.78 } else { 0.6 };
            return Some(Fix {
                kind: FixKind::RenameToMatchDecl.as_str().to_string(),
                confidence: conf,
                alternatives: vec![FixBuilder::new(
                    format!("Rename variant to `{candidate}`"),
                    "Edit-distance heuristic surfaced a likely typo against an existing variant.",
                    conf,
                )
                .replace_line(source_id, span.line, line, &new_line)
                .build()],
            });
        }
    }
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.55,
        alternatives: vec![FixBuilder::new(
            format!("Add the variant `{ident}` to the enum declaration"),
            "If the variant is genuinely new, declare it on the enum where the other \
             variants live.",
            0.55,
        )
        .diff(String::new())
        .build()],
    })
}

// -------- MT2011: `?` error-type mismatch --------------------------

fn fix_question_error_mismatch(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::TypeConversion.as_str().to_string(),
        confidence: 0.72,
        alternatives: vec![
            FixBuilder::new(
                "Map the error with `.map_err(|e| MyError::from(e))?`",
                "When `?` propagates a `Result[T, E1]` into a fn that returns `Result[T, E2]`, \
                 convert the error type explicitly.",
                0.72,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Add a `From[E1] for E2` impl",
                "An automatic `From` conversion lets `?` work transparently.",
                0.65,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Widen the fn's error type",
                "Change `-> Result[T, E2]` to `-> Result[T, E1]` if the caller can absorb \
                 the broader error.",
                0.55,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT2012: wrong variant arity ------------------------------

fn fix_wrong_variant_arity(
    diag: &Diagnostic,
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let msg = &diag.primary.message;
    let expected = extract_first_number(msg).unwrap_or_else(|| "N".to_string());
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                format!("Pass exactly {expected} argument(s) to the variant"),
                "The variant's declared arity is fixed. Check the enum and match the count.",
                0.7,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Use the unit form `Variant` if it takes no payload",
                "If you're constructing a payload-less variant, drop the parens entirely.",
                0.6,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT2014: duplicate struct field ---------------------------

fn fix_duplicate_struct_field(
    diag: &Diagnostic,
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let msg = &diag.primary.message;
    let field = msg.split('`').nth(1).unwrap_or("field").to_string();
    Some(Fix {
        kind: FixKind::RemoveUnreachable.as_str().to_string(),
        confidence: 0.85,
        alternatives: vec![
            FixBuilder::new(
                format!("Remove the second `{field}: ...` occurrence"),
                "Struct literals must mention each field exactly once.",
                0.85,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Rename one of the keys",
                "If the two values were meant for different fields, rename the second \
                 occurrence to the right key.",
                0.6,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT2016: unreachable match arm ----------------------------

fn fix_unreachable_match_arm(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::RemoveUnreachable.as_str().to_string(),
        confidence: 0.8,
        alternatives: vec![
            FixBuilder::new(
                "Delete the unreachable arm",
                "A prior arm already covers this case, so the highlighted body can never \
                 execute.",
                0.8,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Move the arm above the catch-all",
                "If you want this specific case handled differently, reorder it before the \
                 broader pattern (typically the `_` wildcard).",
                0.7,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT2017: binop type mismatch ------------------------------

fn fix_binop_type_mismatch(
    _diag: &Diagnostic,
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::TypeConversion.as_str().to_string(),
        confidence: 0.65,
        alternatives: vec![
            FixBuilder::new(
                "Convert one operand to the other's type",
                "Use `.to_string()`, `.parse()?`, `as I32`, etc. so both sides agree.",
                0.65,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Use the type-specific operator",
                "Mighty's `+` is type-uniform; for string concatenation use `++` or \
                 `format!(\"{}{}\", a, b)`.",
                0.6,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT2022: not a struct -------------------------------------

fn fix_not_a_struct(_source_id: &str, _source: &str, _span: &crate::fix::SpanInfo) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.6,
        alternatives: vec![
            FixBuilder::new(
                "Use the enum's variant constructor instead",
                "The path points to an enum or alias, not a struct. Construct via \
                 `EnumName.Variant { ... }` or call the alias's underlying constructor.",
                0.6,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Define the struct",
                "If you meant a new struct type, add `struct Name { field: T }` and use \
                 it here.",
                0.55,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT2023: generic arg mismatch -----------------------------

fn fix_generic_arg_mismatch(
    _diag: &Diagnostic,
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::TypeConversion.as_str().to_string(),
        confidence: 0.62,
        alternatives: vec![
            FixBuilder::new(
                "Align the generic argument with the parameter's bound",
                "The supplied type doesn't satisfy the parameter's constraint. Either \
                 substitute a conforming type or widen the bound on the declaration.",
                0.62,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Add an `impl` for the required trait",
                "If you control the type, implementing the missing trait makes it \
                 acceptable as the generic argument.",
                0.55,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT2024: lambda arity mismatch ----------------------------

fn fix_lambda_arity_mismatch(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                "Match the callback's expected parameter count",
                "Higher-order fns (`map`, `filter`, `fold`, ...) define the lambda's arity. \
                 Add or remove parameters until they match.",
                0.7,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Use `_` for parameters you don't need",
                "If the callback supplies arguments you ignore, accept them with `_` to \
                 keep the arity right.",
                0.65,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT2025: cannot take ref ----------------------------------

fn fix_cannot_take_ref(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::TakeReference.as_str().to_string(),
        confidence: 0.6,
        alternatives: vec![
            FixBuilder::new(
                "Bind the temporary to a `let` first",
                "Mighty refuses to take a reference to an expression with no home. \
                 Introduce `let x = expr` then borrow `&x`.",
                0.6,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Pass by value (clone if needed)",
                "If the callee can take ownership, drop the `&` and pass the value \
                 directly — clone first when the binding must survive.",
                0.55,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// -------- MT2027 (v0.40 T3): `<int> as Char` -> `Char.from_u32(...)?` --

/// v0.40 T3 — propose `Char.from_u32(expr)?` when the rejected cast is
/// `<int-expr> as Char`. Detects the shape with a tolerant scan of the
/// offending source line:
///
///   `let c: Char = expr as Char`
///                 ^^^^^^^^^^^^^
///
/// becomes
///
///   `let c: Char = Char.from_u32(expr)?`
///
/// Confidence is high (0.85) when the line contains exactly one
/// ` as Char` occurrence and the chunk before it parses as a single
/// balanced expression; otherwise we skip (return `None`) and let the
/// caller render the diagnostic prose-only.
fn fix_invalid_cast_int_to_char(
    source_id: &str,
    source: &str,
    span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let line = current_line(source, span.line)?;
    // The rejected shape: ... <expr> as Char ...
    // We split on the LAST ` as Char` so an inline `let c: Char = ... as Char`
    // takes the trailing cast, not the type annotation.
    let cast_pat = " as Char";
    let cast_at = line.rfind(cast_pat)?;
    let before = &line[..cast_at];
    let after = &line[cast_at + cast_pat.len()..];

    // Extract the source expression: walk backwards from `cast_at`
    // collapsing matched brackets/braces/parens and stopping at the
    // first unbalanced delimiter or at the start of the line. A leading
    // `=` (from `let x = foo as Char`) or `,` / `(` (from `f(foo as Char)`)
    // is the natural stop.
    let bytes = before.as_bytes();
    let mut depth_paren: i32 = 0;
    let mut depth_bracket: i32 = 0;
    let mut depth_brace: i32 = 0;
    let mut start = before.len();
    while start > 0 {
        let b = bytes[start - 1];
        match b {
            b')' => depth_paren += 1,
            b'(' => {
                if depth_paren == 0 {
                    break;
                }
                depth_paren -= 1;
            }
            b']' => depth_bracket += 1,
            b'[' => {
                if depth_bracket == 0 {
                    break;
                }
                depth_bracket -= 1;
            }
            b'}' => depth_brace += 1,
            b'{' => {
                if depth_brace == 0 {
                    break;
                }
                depth_brace -= 1;
            }
            b'=' | b',' | b';' if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 => {
                break;
            }
            _ => {}
        }
        start -= 1;
    }
    // Trim leading/trailing whitespace inside the captured expression.
    let mut expr_start = start;
    while expr_start < before.len() && (bytes[expr_start] as char).is_whitespace() {
        expr_start += 1;
    }
    let expr = before[expr_start..].trim_end();
    if expr.is_empty() {
        return None;
    }
    let lead = &before[..expr_start];

    // Build the rewrite. Two alternatives:
    //   1. Use `?` (forces the surrounding fn to return `Option[Char]`).
    //   2. Use `match` / `if let Some(c) = ...` for a panic-free fallback.
    let new_with_question = format!("{lead}Char.from_u32({expr})?{after}");
    let alt_question = FixBuilder::new(
        "Replace cast with `Char.from_u32(...)?`",
        "`Char.from_u32` returns `Option[Char]`; `?` propagates `None` to \
         the surrounding fn (which must therefore return `Option[Char]` or \
         a compatible type).",
        0.85,
    )
    .replace_line(source_id, span.line, line, &new_with_question)
    .build();

    let new_with_unwrap = format!("{lead}Char.from_u32({expr}).unwrap_or('?'){after}");
    let alt_unwrap = FixBuilder::new(
        "Use `Char.from_u32(...).unwrap_or('?')` as a replacement-character fallback",
        "Returns U+003F ('?') for out-of-range codepoints. Pick a different \
         replacement character if your domain has a more meaningful default.",
        0.7,
    )
    .replace_line(source_id, span.line, line, &new_with_unwrap)
    .build();

    Some(Fix {
        kind: FixKind::TypeConversion.as_str().to_string(),
        confidence: 0.85,
        alternatives: vec![alt_question, alt_unwrap],
    })
}

// =================================================================
// v0.34 T1 — backfill: MT3xxx borrow polish
// =================================================================

fn fix_move_out_of_borrow(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::AddClone.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                "Clone before moving",
                "Borrowed data can't be moved out. `.clone()` copies the value so the \
                 borrow remains valid.",
                0.7,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Restructure to consume the owner",
                "If you can take ownership earlier, the borrow doesn't need to exist at \
                 this point.",
                0.55,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_borrow_after_move(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::TakeReference.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                "Borrow before the move",
                "Reorder so the `&`/`&mut` is taken while the binding is still live; the \
                 move happens after the last borrow use.",
                0.7,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Clone instead of moving",
                "If the move was for handing ownership somewhere else, `.clone()` keeps \
                 the original binding usable by the borrow site.",
                0.65,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_borrow_outlives_owner(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.6,
        alternatives: vec![
            FixBuilder::new(
                "Extend the owner's lifetime",
                "Bind the owner one scope earlier so it lives at least as long as the \
                 borrow.",
                0.6,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Return an owned value, not a reference",
                "Functions that return `&T` over locals leak the local's lifetime. \
                 Returning `T` (cloned/owned) sidesteps the constraint.",
                0.6,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Add an explicit `'a` annotation",
                "When the inferred lifetime is wrong, an explicit `'a` documents the \
                 intent and forces the checker to use it.",
                0.55,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_cannot_move_borrowed(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::AddClone.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                "Wait for the borrow to end before moving",
                "End the borrow (drop the reference) and then move; or restructure the \
                 borrow to a smaller scope.",
                0.7,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Clone so the borrow stays valid",
                "`.clone()` produces a new owned copy you can move while the original \
                 stays borrowed.",
                0.65,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_move_out_of_ref(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::AddClone.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                "Dereference + clone",
                "Replace `(*r)` with `(*r).clone()` (or just `r.clone()` if `T: Clone`).",
                0.7,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Take the reference by value",
                "Change the parameter type from `&T` to `T` and let the caller move.",
                0.55,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_arena_escape(_source_id: &str, _source: &str, _span: &crate::fix::SpanInfo) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.6,
        alternatives: vec![
            FixBuilder::new(
                "Move the binding into the same arena scope",
                "Arena-allocated values can't escape the arena. Confine the use-site to \
                 within `arena { ... }`.",
                0.6,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Allocate in the parent arena (or heap)",
                "If the value needs to outlive the inner scope, allocate it in the \
                 outer arena or use the heap allocator.",
                0.55,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_non_sendable_message_arg(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.65,
        alternatives: vec![
            FixBuilder::new(
                "Use a Send-safe payload type",
                "Agent messages must implement `Send`. Replace `Rc[T]` with `Arc[T]`, \
                 `&T` with `T` (owned), or drop the cap from the payload.",
                0.65,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Derive Send if your type qualifies",
                "If every field is Send, add `derive Send` on the type so the agent can \
                 carry it.",
                0.55,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_drop_in_const_context(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.6,
        alternatives: vec![FixBuilder::new(
            "Move the value out of the const context",
            "Const evaluation can't run Drop glue. Compute the value at runtime, or \
             use a `Copy` type that has no Drop.",
            0.6,
        )
        .diff(String::new())
        .build()],
    })
}

// =================================================================
// v0.34 T1 — backfill: MT4xxx effect/cap remaining
// =================================================================

fn fix_alloc_in_core(_source_id: &str, _source: &str, _span: &crate::fix::SpanInfo) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                "Move the allocation outside `#[core]`",
                "`#[core]` modules forbid heap allocation. Lift the call into a non-core \
                 fn that the core fn calls back into.",
                0.7,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Use a stack-allocated container",
                "Replace `Vec[T]` with `[T; N]` (fixed-size) or a pre-allocated arena.",
                0.6,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_method_ambiguous(
    _diag: &Diagnostic,
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                "Disambiguate with `<T as Trait>::method(self, ...)`",
                "Two traits in scope define the same method name. Fully-qualified syntax \
                 picks the one you want.",
                0.7,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Remove one of the trait imports",
                "Often only one of the conflicting traits is actually used; dropping the \
                 stray `use` resolves the ambiguity.",
                0.65,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_method_not_found(
    diag: &Diagnostic,
    source_id: &str,
    source: &str,
    span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    // Reuse MT2007's rename-or-import shape.
    fix_unknown_method(diag, source_id, source, span)
}

fn fix_trait_coherence_violation(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.6,
        alternatives: vec![
            FixBuilder::new(
                "Wrap the type in a newtype you own",
                "Orphan-rule coherence forbids impls for foreign-type/foreign-trait \
                 pairs. A newtype struct gives you a local type for the impl.",
                0.6,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Move the impl into the upstream package",
                "If you maintain the package that owns the trait or the type, add the \
                 impl there.",
                0.5,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_dyn_requires_object_safe(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.65,
        alternatives: vec![
            FixBuilder::new(
                "Use a generic `T: Trait` parameter instead of `dyn Trait`",
                "`dyn` requires object-safe traits. Monomorphic generics sidestep the \
                 constraint.",
                0.65,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Split the trait into object-safe and non-object-safe halves",
                "Methods returning `Self` or using generic parameters break object \
                 safety. Move them to a separate trait.",
                0.55,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_protocol_arity_mismatch(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                "Match the protocol's declared arity",
                "The `protocol P { Msg(a, b) }` declaration fixes the arity. Add or \
                 remove parameters until your `on Msg(...)` lines up.",
                0.7,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Change the protocol declaration",
                "If the protocol is yours and the handler shape is the right one, edit \
                 the protocol to match.",
                0.6,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_protocol_param_type_mismatch(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::TypeConversion.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![FixBuilder::new(
            "Annotate `on Msg(arg: T)` with the declared type",
            "The protocol fixes each argument's type. Use that type verbatim in your \
             handler (or remove the annotation and let it infer).",
            0.7,
        )
        .diff(String::new())
        .build()],
    })
}

fn fix_protocol_extra_handler(
    diag: &Diagnostic,
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let msg = &diag.primary.message;
    let name = msg.split('`').nth(1).unwrap_or("Msg").to_string();
    Some(Fix {
        kind: FixKind::RemoveUnreachable.as_str().to_string(),
        confidence: 0.75,
        alternatives: vec![
            FixBuilder::new(
                format!("Delete the stray `on {name}` handler"),
                "The agent's protocol clause doesn't include this message; the handler \
                 is dead code.",
                0.75,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                format!("Add `{name}` to a protocol the agent uses"),
                "If the message is intentional, declare it in the protocol and add the \
                 protocol to the agent's clause.",
                0.65,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_derive_copy_field_not_copy(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                "Replace the non-Copy field with a Copy alternative",
                "Common fix: `String` → `Str` (Copy), `Vec[T]` → `[T; N]`. The struct \
                 only derives Copy when every field is Copy.",
                0.7,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Drop `Copy` from the derive list",
                "If the struct doesn't need to be `Copy`, derive only `Clone` (or remove \
                 the derive altogether).",
                0.7,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_derive_unknown(
    diag: &Diagnostic,
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let msg = &diag.primary.message;
    let name = msg.split('`').nth(1).unwrap_or("Trait").to_string();
    let knowns = [
        "Copy",
        "Clone",
        "Debug",
        "Eq",
        "PartialEq",
        "Hash",
        "Send",
        "Default",
        "Display",
    ];
    let mut alts: Vec<FixAlternative> = Vec::new();
    for k in &knowns {
        let d = levenshtein(k, &name);
        if d != 0 && d <= 2 {
            alts.push(
                FixBuilder::new(
                    format!("Rename to `{k}`"),
                    format!("Built-in derivable `{k}` is the closest match (edit distance {d})."),
                    if d == 1 { 0.78 } else { 0.6 },
                )
                .diff(String::new())
                .build(),
            );
            break;
        }
    }
    if alts.is_empty() {
        alts.push(
            FixBuilder::new(
                format!("Import a `derive_{name}` proc macro"),
                "Custom derives must come from a proc-macro crate brought into scope \
                 with `use`.",
                0.55,
            )
            .diff(String::new())
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

fn fix_row_occurs_check(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.55,
        alternatives: vec![
            FixBuilder::new(
                "Break the recursive row constraint",
                "A row variable can't appear inside its own row. Add an indirection (Box, \
                 closure, or named protocol) so the recursion is structural rather than \
                 by-row.",
                0.55,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Use a concrete row instead",
                "Replace the row variable with the concrete set of effects you mean.",
                0.55,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_row_var_unbound(
    diag: &Diagnostic,
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let msg = &diag.primary.message;
    let name = msg.split('`').nth(1).unwrap_or("E").to_string();
    Some(Fix {
        kind: FixKind::AddEffect.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                format!("Bind `{name}` on the enclosing fn"),
                "Add the row variable to the fn's generic list: `fn f[{name}](...)`.",
                0.7,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Replace it with a concrete row",
                "If you don't need polymorphism over effects, write the concrete row \
                 directly.",
                0.6,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_row_effect_mismatch(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::AddEffect.as_str().to_string(),
        confidence: 0.65,
        alternatives: vec![
            FixBuilder::new(
                "Add the missing effect to the row",
                "The caller's row needs to mention every effect the callee performs. \
                 Widen the row at the caller's signature.",
                0.65,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Remove the surplus effect from the callee",
                "If the effect isn't actually performed, drop it from the callee's row.",
                0.55,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_cap_family_mismatch(
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
                format!("Use a capability from the `{name}` family"),
                "Capability families are nominal — `fs.ro` is not `net.host`. Match the \
                 family the callee expects.",
                0.7,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Convert via the family's adapter",
                "Some families expose `.into[OtherFamily]()` adapters; use one if available.",
                0.55,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_cap_scope_violation(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::AddCapability.as_str().to_string(),
        confidence: 0.65,
        alternatives: vec![
            FixBuilder::new(
                "Pass the capability into the inner scope",
                "Capabilities can't escape sandbox boundaries implicitly. Thread the cap \
                 through as a parameter.",
                0.65,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Move the operation outside the sandbox",
                "If the call must use the outer cap, perform it before entering the \
                 sandboxed block.",
                0.6,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_cap_redeclaration(
    diag: &Diagnostic,
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let msg = &diag.primary.message;
    let name = msg.split('`').nth(1).unwrap_or("cap").to_string();
    Some(Fix {
        kind: FixKind::RemoveUnreachable.as_str().to_string(),
        confidence: 0.8,
        alternatives: vec![
            FixBuilder::new(
                format!("Delete the duplicate `cap {name}` declaration"),
                "Each capability name must be declared at most once in a scope.",
                0.8,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Rename one of the declarations",
                "If both declarations are intentional, give them distinct names.",
                0.6,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_cap_method_unknown(
    diag: &Diagnostic,
    source_id: &str,
    source: &str,
    span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    let line = current_line(source, span.line)?;
    let (ident, _) = identifier_at(source, span.byte_start as usize)?;
    if let Some((candidate, dist)) = closest_ident(source, &ident) {
        if dist <= 2 {
            let new_line = line.replace(&ident, &candidate);
            let conf = if dist == 1 { 0.78 } else { 0.6 };
            return Some(Fix {
                kind: FixKind::RenameToMatchDecl.as_str().to_string(),
                confidence: conf,
                alternatives: vec![FixBuilder::new(
                    format!("Rename to capability method `{candidate}`"),
                    "Edit-distance heuristic surfaced a likely typo against the \
                     capability's method set.",
                    conf,
                )
                .replace_line(source_id, span.line, line, &new_line)
                .build()],
            });
        }
    }
    let _ = diag;
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.55,
        alternatives: vec![FixBuilder::new(
            "Check the capability's family for available methods",
            "Each cap family declares a finite method set. Consult `mty explain` on the \
             family for the full list.",
            0.55,
        )
        .diff(String::new())
        .build()],
    })
}

// =================================================================
// v0.34 T1 — backfill: MT6xxx macro polish
// =================================================================

fn fix_macro_arity_mismatch(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.7,
        alternatives: vec![
            FixBuilder::new(
                "Match the macro's declared arity",
                "Macros enforce their parameter count at expansion time. Add or remove \
                 arguments until the call matches the declaration.",
                0.7,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Use the variadic form (if the macro supports it)",
                "If the macro has a variadic pattern (`$($x:expr),+`), check whether \
                 you accidentally split the args.",
                0.55,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_macro_format_bad_template(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.75,
        alternatives: vec![
            FixBuilder::new(
                "Balance `{` and `}` in the format string",
                "Use `{{` and `}}` to write literal braces. Every `{...}` placeholder \
                 needs a matching closer.",
                0.75,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Switch to positional placeholders",
                "If named placeholders are giving trouble, use `{}` and supply args in \
                 order.",
                0.65,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

fn fix_macro_format_unsupported_spec(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::Other.as_str().to_string(),
        confidence: 0.65,
        alternatives: vec![FixBuilder::new(
            "Use a supported format spec",
            "Mighty's v0.34 `format!` supports `{}`, `{:?}`, `{:x}`, `{:o}`, `{:b}`, \
             `{:0Nd}`. Other specs aren't implemented yet.",
            0.65,
        )
        .diff(String::new())
        .build()],
    })
}

fn fix_computer_use_malformed_cap(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::CorrectMacroAttr.as_str().to_string(),
        confidence: 0.8,
        alternatives: vec![FixBuilder::new(
            "Use the canonical cap expression",
            "`cap: computer.screen + computer.input + computer.dom` — the `+` combines \
             family members. Anything else is rejected.",
            0.8,
        )
        .diff(String::new())
        .build()],
    })
}

fn fix_computer_use_malformed_dimension(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::CorrectMacroAttr.as_str().to_string(),
        confidence: 0.8,
        alternatives: vec![FixBuilder::new(
            "Pass `width` and `height` as positive integers",
            "Computer-use dimensions are pixels; non-integer or non-positive values are \
             rejected.",
            0.8,
        )
        .diff(String::new())
        .build()],
    })
}

fn fix_computer_use_not_an_agent(
    _source_id: &str,
    _source: &str,
    _span: &crate::fix::SpanInfo,
) -> Option<Fix> {
    Some(Fix {
        kind: FixKind::CorrectMacroAttr.as_str().to_string(),
        confidence: 0.8,
        alternatives: vec![
            FixBuilder::new(
                "Apply `@computer_use(...)` to an `agent` item",
                "The decorator only attaches to agents. Move it above the `agent` block.",
                0.8,
            )
            .diff(String::new())
            .build(),
            FixBuilder::new(
                "Wrap the fn in an agent",
                "Computer-use sessions need an agent's mailbox; promote the fn into a \
                 one-shot agent.",
                0.6,
            )
            .diff(String::new())
            .build(),
        ],
    })
}

// =================================================================
// Helpers shared across v0.34 backfill handlers
// =================================================================

/// Drop the empty `()` call-parens that immediately follow the
/// identifier at `byte_start` in `source`. Returns the modified line.
/// If the identifier is missing or not followed by `()`, returns the
/// line unchanged.
fn drop_parens_after_ident(line: &str, source: &str, byte_start: usize) -> String {
    let Some((ident, ident_start)) = identifier_at(source, byte_start) else {
        return line.to_string();
    };
    let ident_with_parens = format!("{ident}()");
    // Locate the identifier within the line. We use the column-from-
    // line-start by finding the line containing ident_start, then
    // searching the line for the exact ident-with-parens occurrence.
    // Caller has already guaranteed `line` is the right line.
    let _ = ident_start;
    if let Some(pos) = line.find(&ident_with_parens) {
        let mut out = String::with_capacity(line.len());
        out.push_str(&line[..pos]);
        out.push_str(&ident);
        out.push_str(&line[pos + ident_with_parens.len()..]);
        out
    } else {
        line.to_string()
    }
}

/// Extract the first run of ASCII digits from `s`. Used by codes that
/// encode "expected N, got M" in the primary message.
fn extract_first_number(s: &str) -> Option<String> {
    let mut buf = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            buf.push(ch);
        } else if !buf.is_empty() {
            break;
        }
    }
    if buf.is_empty() {
        None
    } else {
        Some(buf)
    }
}

// -------- see_also: per-code related codes -------------------------

fn see_also_for(code: DiagCode) -> Vec<String> {
    match code.0 {
        4099 => vec!["MT4001".into(), "docs/internals/taint-types.md".into()],
        1001 => vec!["MT1002".into(), "MT2002".into(), "MT2021".into()],
        1002 => vec!["MT1001".into()],
        2001 => vec!["MT2018".into(), "MT2019".into()],
        2002 => vec!["MT1002".into()],
        2004 => vec!["MT2023".into()],
        2007 => vec!["MT4020".into(), "MT4021".into()],
        2008 => vec!["MT2007".into()],
        2009 => vec!["MT2012".into()],
        2010 => vec!["MT2011".into()],
        2011 => vec!["MT2010".into()],
        2012 => vec!["MT2009".into()],
        2014 => vec!["MT2013".into()],
        2015 => vec!["MT2016".into()],
        2016 => vec!["MT2015".into()],
        2022 => vec!["MT2002".into()],
        2023 => vec!["MT2004".into()],
        2024 => vec!["MT2005".into()],
        2026 => vec!["MT4031".into(), "MT4032".into(), "MT4033".into()],
        3001 => vec!["MT3002".into(), "MT3008".into()],
        3002 => vec!["MT3001".into(), "MT3003".into()],
        3003 => vec!["MT3001".into(), "MT3002".into()],
        3004..=3006 => vec!["MT3001".into()],
        3007 => vec!["MT3008".into()],
        3008 => vec!["MT3001".into(), "MT3007".into()],
        3009 => vec!["MT3002".into()],
        3010 => vec!["MT5007".into()],
        3011 => vec!["MT3001".into()],
        4001 => vec!["MT4050".into()],
        4002 => vec!["MT4001".into()],
        4020 => vec!["MT4021".into(), "MT2007".into()],
        4021 => vec!["MT4020".into(), "MT2007".into()],
        4030 => vec!["MT4031".into(), "MT4032".into(), "MT4033".into()],
        4031 => vec!["MT4030".into(), "MT2026".into()],
        4032 => vec!["MT4030".into(), "MT4033".into()],
        4033 => vec!["MT4032".into(), "MT2026".into()],
        4040 => vec!["MT4041".into()],
        4041 => vec!["MT4040".into()],
        4050 => vec!["MT4055".into(), "MT4059".into()],
        4051 => vec!["MT4050".into()],
        4053 => vec!["MT4055".into(), "MT4057".into()],
        4054 => vec!["MT4050".into()],
        4060 => vec!["MT4061".into(), "MT4062".into(), "MT4063".into()],
        4061 => vec!["MT4060".into()],
        4062 => vec!["MT4060".into(), "MT5015".into()],
        4063 => vec!["MT4060".into()],
        4064 => vec!["MT4060".into(), "MT4061".into()],
        6001 => vec!["MT6002".into(), "MT6003".into()],
        6002 => vec!["MT6001".into()],
        6009 => vec!["MT6010".into(), "MT6011".into(), "MT6012".into()],
        6010 => vec!["MT6009".into()],
        6017 => vec!["MT6018".into(), "MT6019".into(), "MT6020".into()],
        6018 => vec!["MT6017".into()],
        6019 => vec!["MT6017".into()],
        6020 => vec!["MT6017".into()],
        1 | 10 | 11 => vec!["MT0012".into()],
        2 => vec!["MT0003".into()],
        3 => vec!["MT0002".into()],
        20 => vec!["MT4032".into()],
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

    // =============================================================
    // v0.34 T1 — backfill tests
    // =============================================================

    #[test]
    fn envelope_mt0001_unexpected_token() {
        let src = "fn f() { let = 1 }\n";
        let diag = d(codes::UNEXPECTED_TOKEN, 11, 12, "unexpected token");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.alternatives.len(), 2);
    }

    #[test]
    fn envelope_mt0002_unterminated_string_appends_quote() {
        let src = "let x = \"hi\n";
        let diag = d(codes::UNTERMINATED_STRING, 8, 11, "unterminated string");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.kind, "balance_delimiters");
        assert!(fix.alternatives[0].diff.contains("\""));
    }

    #[test]
    fn envelope_mt0003_invalid_escape_offers_known_set() {
        let src = "let x = \"\\q\"\n";
        let diag = d(codes::INVALID_ESCAPE, 9, 11, "invalid escape `\\q`");
        let env = diag.to_envelope("a.mty", src);
        assert!(env.fix.is_some());
        assert_eq!(env.see_also, vec!["MT0002".to_string()]);
    }

    #[test]
    fn envelope_mt0004_unknown_duration_unit() {
        let src = "let t = 5.zorps\n";
        let diag = d(codes::UNKNOWN_DURATION_UNIT, 10, 15, "unknown unit `zorps`");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives[0].rationale.contains("duration"));
    }

    #[test]
    fn envelope_mt0010_expected_item() {
        let src = "x = 1\n";
        let diag = d(codes::EXPECTED_ITEM, 0, 1, "expected item");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.alternatives.len(), 2);
    }

    #[test]
    fn envelope_mt0011_expected_expr() {
        let src = "fn f() { let x = }\n";
        let diag = d(codes::EXPECTED_EXPR, 17, 18, "expected expression");
        let env = diag.to_envelope("a.mty", src);
        assert!(env.fix.is_some());
    }

    #[test]
    fn envelope_mt0020_duplicate_on_handler() {
        let src = "agent A: P { on Tick() -> {} on Tick() -> {} }\n";
        let diag = d(codes::DUPLICATE_ON_HANDLER, 28, 30, "duplicate `on Tick`");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.kind, "remove_unreachable");
        assert_eq!(fix.alternatives.len(), 2);
    }

    #[test]
    fn envelope_mt2004_wrong_generic_arity_extracts_number() {
        let src = "let x: Map[I32] = mk()\n";
        let diag = d(
            codes::WRONG_GENERIC_ARITY,
            7,
            10,
            "expected 2 generic args, got 1",
        );
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives[0].label.contains("2"));
    }

    #[test]
    fn envelope_mt2008_not_callable_removes_parens() {
        let src = "fn main() { let x = 1; x() }\n";
        let start = src.find("x()").unwrap();
        let diag = d(codes::NOT_CALLABLE, start, start + 1, "value not callable");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        let diff = &fix.alternatives[0].diff;
        // Diff removes the old line (containing `x()`) and adds the
        // new line (containing `x` without parens).
        assert!(diff.contains("-fn main() { let x = 1; x() }"));
        assert!(diff.contains("+fn main() { let x = 1; x }"));
    }

    #[test]
    fn envelope_mt2009_unknown_variant_offers_rename() {
        let src = "enum E { Alpha, Beta }\nfn f() { let x = E.Alpa }\n";
        let start = src.rfind("Alpa").unwrap();
        let diag = d(
            codes::UNKNOWN_VARIANT,
            start,
            start + 4,
            "unknown variant `Alpa`",
        );
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives.iter().any(|a| a.label.contains("Alpha")));
    }

    #[test]
    fn envelope_mt2011_question_error_mismatch_three_alts() {
        let src = "fn f() -> Result[I32, MyErr] { g()? }\n";
        let start = src.find('?').unwrap();
        let diag = d(
            codes::QUESTION_ERROR_MISMATCH,
            start,
            start + 1,
            "error mismatch",
        );
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.alternatives.len(), 3);
        assert_eq!(fix.kind, "type_conversion");
    }

    #[test]
    fn envelope_mt2012_wrong_variant_arity() {
        let src = "fn f() { E.Some(1, 2) }\n";
        let diag = d(
            codes::WRONG_VARIANT_ARITY,
            9,
            13,
            "variant `Some` expects 1 arg, got 2",
        );
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives[0].label.contains("1"));
    }

    #[test]
    fn envelope_mt2014_duplicate_struct_field() {
        let src = "fn f() { S { name: \"a\", name: \"b\" } }\n";
        let diag = d(
            codes::DUPLICATE_STRUCT_FIELD,
            0,
            1,
            "duplicate field `name`",
        );
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives[0].label.contains("name"));
        assert_eq!(fix.kind, "remove_unreachable");
    }

    #[test]
    fn envelope_mt2016_unreachable_match_arm_two_alts() {
        let src = "match x { _ => 0, A => 1 }\n";
        let diag = d(
            codes::UNREACHABLE_MATCH_ARM,
            18,
            19,
            "unreachable match arm",
        );
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.alternatives.len(), 2);
        assert_eq!(fix.kind, "remove_unreachable");
    }

    #[test]
    fn envelope_mt2017_binop_type_mismatch() {
        let src = "let x = 1 + \"hi\"\n";
        let diag = d(codes::BINOP_TYPE_MISMATCH, 10, 11, "binop mismatch");
        let env = diag.to_envelope("a.mty", src);
        assert_eq!(env.fix.unwrap().kind, "type_conversion");
    }

    #[test]
    fn envelope_mt2022_not_a_struct() {
        let src = "let x = E { v: 1 }\n";
        let diag = d(codes::NOT_A_STRUCT, 8, 9, "not a struct");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.alternatives.len(), 2);
    }

    #[test]
    fn envelope_mt2023_generic_arg_mismatch() {
        let src = "let x: Vec[NoOrd] = mk()\n";
        let diag = d(
            codes::GENERIC_ARG_MISMATCH,
            11,
            16,
            "generic arg fails bound",
        );
        let env = diag.to_envelope("a.mty", src);
        assert!(env.fix.is_some());
    }

    #[test]
    fn envelope_mt2024_lambda_arity_mismatch() {
        let src = "xs.map(|a, b| a)\n";
        let diag = d(codes::LAMBDA_ARITY_MISMATCH, 3, 6, "lambda arity");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.alternatives.len(), 2);
    }

    #[test]
    fn envelope_mt2025_cannot_take_ref() {
        let src = "let r = &(1 + 2)\n";
        let diag = d(codes::CANNOT_TAKE_REF, 9, 10, "cannot take ref");
        let env = diag.to_envelope("a.mty", src);
        assert_eq!(env.fix.unwrap().kind, "take_reference");
    }

    #[test]
    fn envelope_mt3002_move_out_of_borrow() {
        let src = "fn f(r: &S) -> S { *r }\n";
        let diag = d(codes::MOVE_OUT_OF_BORROW, 19, 21, "move out of borrow");
        let env = diag.to_envelope("a.mty", src);
        assert_eq!(env.fix.unwrap().kind, "add_clone");
    }

    #[test]
    fn envelope_mt3003_borrow_after_move() {
        let src = "fn f() { let s = mk(); let t = move s; let r = &s }\n";
        let diag = d(codes::BORROW_AFTER_MOVE, 0, 2, "borrow after move");
        let env = diag.to_envelope("a.mty", src);
        assert_eq!(env.fix.unwrap().kind, "take_reference");
    }

    #[test]
    fn envelope_mt3007_borrow_outlives_owner_three_alts() {
        let src = "fn f() -> &S { let s = mk(); &s }\n";
        let diag = d(codes::BORROW_OUTLIVES_OWNER, 0, 2, "borrow outlives owner");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.alternatives.len(), 3);
    }

    #[test]
    fn envelope_mt3008_cannot_move_borrowed() {
        let src = "fn f() { let r = &s; let t = move s }\n";
        let diag = d(codes::CANNOT_MOVE_BORROWED, 0, 2, "cannot move borrowed");
        let env = diag.to_envelope("a.mty", src);
        assert!(env.fix.is_some());
    }

    #[test]
    fn envelope_mt3009_move_out_of_ref() {
        let src = "fn f(r: &T) -> T { *r }\n";
        let diag = d(codes::MOVE_OUT_OF_REF, 0, 2, "move out of ref");
        let env = diag.to_envelope("a.mty", src);
        assert_eq!(env.fix.unwrap().kind, "add_clone");
    }

    #[test]
    fn envelope_mt3010_arena_escape() {
        let src = "arena { let x = mk(); leak(x) }\n";
        let diag = d(codes::ARENA_ESCAPE, 0, 5, "arena escape");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.alternatives.len(), 2);
    }

    #[test]
    fn envelope_mt3011_non_sendable_message_arg() {
        let src = "agent A: P { on Msg(x: Rc[T]) -> {} }\n";
        let diag = d(codes::NON_SENDABLE_MESSAGE_ARG, 0, 5, "not Send");
        let env = diag.to_envelope("a.mty", src);
        assert!(env.fix.is_some());
    }

    #[test]
    fn envelope_mt3012_drop_in_const_context() {
        let src = "const X: S = S {}\n";
        let diag = d(codes::DROP_IN_CONST_CONTEXT, 0, 5, "drop in const");
        let env = diag.to_envelope("a.mty", src);
        assert!(env.fix.is_some());
    }

    #[test]
    fn envelope_mt4002_alloc_in_core() {
        let src = "#[core]\nfn f() { Vec.new() }\n";
        let diag = d(codes::ALLOC_IN_CORE, 0, 7, "alloc in core");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.alternatives.len(), 2);
    }

    #[test]
    fn envelope_mt4020_method_ambiguous() {
        let src = "fn f(x: T) { x.foo() }\n";
        let diag = d(codes::METHOD_AMBIGUOUS, 13, 16, "ambiguous method `foo`");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives[0].rationale.contains("Fully"));
    }

    #[test]
    fn envelope_mt4021_method_not_found() {
        let src = "fn f(x: T) { x.bar() }\n";
        let start = src.find("bar").unwrap();
        let diag = d(
            codes::METHOD_NOT_FOUND,
            start,
            start + 3,
            "method `bar` not found",
        );
        let env = diag.to_envelope("a.mty", src);
        assert!(env.fix.is_some());
    }

    #[test]
    fn envelope_mt4022_trait_coherence_violation() {
        let src = "impl ForeignTrait for ForeignType {}\n";
        let diag = d(codes::TRAIT_COHERENCE_VIOLATION, 0, 4, "coherence");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives[0].label.contains("newtype"));
    }

    #[test]
    fn envelope_mt4023_dyn_requires_object_safe() {
        let src = "fn f(x: dyn T) {}\n";
        let diag = d(codes::DYN_REQUIRES_OBJECT_SAFE, 0, 3, "dyn requires safe");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.alternatives.len(), 2);
    }

    #[test]
    fn envelope_mt4030_protocol_arity_mismatch() {
        let src = "agent A: P { on Tick(x, y) -> {} }\n";
        let diag = d(codes::PROTOCOL_ARITY_MISMATCH, 0, 5, "arity mismatch");
        let env = diag.to_envelope("a.mty", src);
        assert!(env.fix.is_some());
    }

    #[test]
    fn envelope_mt4031_protocol_param_type_mismatch() {
        let src = "agent A: P { on Tick(x: Str) -> {} }\n";
        let diag = d(
            codes::PROTOCOL_PARAM_TYPE_MISMATCH,
            0,
            5,
            "param type mismatch",
        );
        let env = diag.to_envelope("a.mty", src);
        assert_eq!(env.fix.unwrap().kind, "type_conversion");
    }

    #[test]
    fn envelope_mt4033_protocol_extra_handler() {
        let src = "agent A: P { on Stray() -> {} }\n";
        let diag = d(
            codes::PROTOCOL_EXTRA_HANDLER,
            0,
            5,
            "handler `Stray` not in protocol",
        );
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives[0].label.contains("Stray"));
        assert_eq!(fix.kind, "remove_unreachable");
    }

    #[test]
    fn envelope_mt4040_derive_copy_field_not_copy() {
        let src = "#[derive(Copy)] struct S { v: Vec[I32] }\n";
        let diag = d(codes::DERIVE_COPY_FIELD_NOT_COPY, 0, 7, "field not copy");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.alternatives.len(), 2);
    }

    #[test]
    fn envelope_mt4041_derive_unknown_renames_to_known() {
        let src = "#[derive(Cloen)] struct S {}\n";
        let diag = d(codes::DERIVE_UNKNOWN, 0, 7, "unknown derive `Cloen`");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives[0].label.contains("Clone"));
    }

    #[test]
    fn envelope_mt4051_row_occurs_check() {
        let src = "fn f[E]() !{a | E} {}\n";
        let diag = d(codes::ROW_OCCURS_CHECK, 0, 4, "occurs check");
        let env = diag.to_envelope("a.mty", src);
        assert!(env.fix.is_some());
    }

    #[test]
    fn envelope_mt4053_row_var_unbound_uses_name() {
        let src = "fn f() !{a | R} {}\n";
        let diag = d(codes::ROW_VAR_UNBOUND, 0, 4, "row var `R` is unbound");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives[0].label.contains('R'));
    }

    #[test]
    fn envelope_mt4054_row_effect_mismatch() {
        let src = "fn f() !{a} { g() }\n";
        let diag = d(codes::ROW_EFFECT_MISMATCH, 0, 4, "row effect mismatch");
        let env = diag.to_envelope("a.mty", src);
        assert_eq!(env.fix.unwrap().kind, "add_effect");
    }

    #[test]
    fn envelope_mt4061_cap_family_mismatch_uses_name() {
        let src = "fn f(c: cap fs.ro) { net.get(c) }\n";
        let diag = d(
            codes::CAP_FAMILY_MISMATCH,
            0,
            4,
            "cap family `net` expected",
        );
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives[0].label.contains("net"));
    }

    #[test]
    fn envelope_mt4062_cap_scope_violation() {
        let src = "sandbox { fs.write(\"x\") }\n";
        let diag = d(codes::CAP_SCOPE_VIOLATION, 0, 7, "cap scope");
        let env = diag.to_envelope("a.mty", src);
        assert_eq!(env.fix.unwrap().kind, "add_capability");
    }

    #[test]
    fn envelope_mt4063_cap_redeclaration_extracts_name() {
        let src = "cap c = fs.ro; cap c = fs.rw\n";
        let diag = d(codes::CAP_REDECLARATION, 0, 5, "duplicate cap `c`");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives[0].label.contains('c'));
        assert_eq!(fix.kind, "remove_unreachable");
    }

    #[test]
    fn envelope_mt4064_cap_method_unknown_renames() {
        let src = "fn f(c: cap fs.ro) { c.reed() }\n";
        let start = src.find("reed").unwrap();
        let diag = d(
            codes::CAP_METHOD_UNKNOWN,
            start,
            start + 4,
            "unknown cap method",
        );
        let env = diag.to_envelope("a.mty", src);
        assert!(env.fix.is_some());
    }

    #[test]
    fn envelope_mt6002_macro_arity_mismatch() {
        let src = "log!()\n";
        let diag = d(codes::MACRO_ARITY_MISMATCH, 0, 4, "macro arity");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.alternatives.len(), 2);
    }

    #[test]
    fn envelope_mt6009_macro_format_bad_template() {
        let src = "format!(\"{\")\n";
        let diag = d(codes::MACRO_FORMAT_BAD_TEMPLATE, 0, 7, "bad template");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives[0].label.contains("{"));
    }

    #[test]
    fn envelope_mt6010_macro_format_unsupported_spec() {
        let src = "format!(\"{:Q}\", x)\n";
        let diag = d(
            codes::MACRO_FORMAT_UNSUPPORTED_SPEC,
            0,
            7,
            "unsupported spec",
        );
        let env = diag.to_envelope("a.mty", src);
        assert!(env.fix.is_some());
    }

    #[test]
    fn envelope_mt6018_computer_use_malformed_cap() {
        let src = "@computer_use(cap: nonsense)\nagent A {}\n";
        let diag = d(codes::COMPUTER_USE_MALFORMED_CAP, 0, 13, "malformed cap");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert!(fix.alternatives[0].rationale.contains("computer.screen"));
    }

    #[test]
    fn envelope_mt6019_computer_use_malformed_dimension() {
        let src = "@computer_use(width: \"x\")\nagent A {}\n";
        let diag = d(
            codes::COMPUTER_USE_MALFORMED_DIMENSION,
            0,
            13,
            "malformed dim",
        );
        let env = diag.to_envelope("a.mty", src);
        assert!(env.fix.is_some());
    }

    #[test]
    fn envelope_mt6020_computer_use_not_an_agent() {
        let src = "@computer_use()\nfn f() {}\n";
        let diag = d(codes::COMPUTER_USE_NOT_AN_AGENT, 0, 13, "not an agent");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.alternatives.len(), 2);
    }

    // -- snapshot-ish tests for the 5 most-common new codes -----

    #[test]
    fn snapshot_mt2008_not_callable_diff_drops_parens() {
        let src = "fn main() {\n  let x = 1\n  x()\n}\n";
        let start = src.find("x()").unwrap();
        let diag = d(codes::NOT_CALLABLE, start, start + 1, "not callable");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        // First alternative is the parens-removal diff.
        let diff = &fix.alternatives[0].diff;
        assert!(diff.contains("--- a/a.mty"));
        assert!(diff.contains("+++ b/a.mty"));
        assert!(diff.contains("-  x()"));
        assert!(diff.contains("+  x"));
    }

    #[test]
    fn snapshot_mt2016_unreachable_arm_has_remove_then_reorder() {
        let src = "fn f(x: E) -> I32 {\n  match x {\n    _ => 0,\n    A => 1,\n  }\n}\n";
        let diag = d(codes::UNREACHABLE_MATCH_ARM, 0, 5, "unreachable");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.alternatives[0].label, "Delete the unreachable arm");
        assert_eq!(
            fix.alternatives[1].label,
            "Move the arm above the catch-all"
        );
    }

    #[test]
    fn snapshot_mt2014_duplicate_field_kind_is_remove_unreachable() {
        let src = "fn f() { S { name: \"a\", name: \"b\" } }\n";
        let diag = d(
            codes::DUPLICATE_STRUCT_FIELD,
            0,
            1,
            "duplicate field `name`",
        );
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(fix.kind, "remove_unreachable");
        assert!((fix.confidence - 0.85).abs() < 1e-4);
    }

    #[test]
    fn snapshot_mt0002_unterminated_string_diff_adds_quote() {
        let src = "let x = \"hi\n";
        let diag = d(codes::UNTERMINATED_STRING, 8, 11, "unterminated");
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        let diff = &fix.alternatives[0].diff;
        assert!(diff.contains("+let x = \"hi\""));
        assert!(diff.contains("@@ -1,1 +1,1 @@"));
    }

    #[test]
    fn snapshot_mt4033_protocol_extra_handler_label_includes_name() {
        let src = "agent A: P { on Stray() -> {} }\n";
        let diag = d(
            codes::PROTOCOL_EXTRA_HANDLER,
            0,
            5,
            "handler `Stray` not in protocol",
        );
        let env = diag.to_envelope("a.mty", src);
        let fix = env.fix.unwrap();
        assert_eq!(
            fix.alternatives[0].label,
            "Delete the stray `on Stray` handler"
        );
        assert_eq!(
            env.see_also,
            vec!["MT4032".to_string(), "MT2026".to_string()]
        );
    }

    // -- helper tests --------------------------------------------

    #[test]
    fn extract_first_number_finds_first_run() {
        assert_eq!(extract_first_number("expected 12 got 4"), Some("12".into()));
        assert_eq!(extract_first_number("no digits"), None);
        assert_eq!(extract_first_number("3"), Some("3".into()));
    }

    // -- end-to-end NDJSON validity check ------------------------

    #[test]
    fn ndjson_round_trip_for_v034_codes() {
        // One diagnostic per new code class — make sure they all
        // produce valid envelopes that serialize + deserialize cleanly.
        let src = "fn main() {\n  let x = 1\n}\n";
        let diags = vec![
            d(codes::UNEXPECTED_TOKEN, 0, 1, "u"),
            d(codes::NOT_CALLABLE, 0, 1, "n"),
            d(codes::DUPLICATE_STRUCT_FIELD, 0, 1, "d `f`"),
            d(codes::UNREACHABLE_MATCH_ARM, 0, 1, "u"),
            d(codes::MOVE_OUT_OF_REF, 0, 1, "m"),
            d(codes::PROTOCOL_EXTRA_HANDLER, 0, 1, "p `M`"),
            d(codes::CAP_FAMILY_MISMATCH, 0, 1, "f `c`"),
            d(codes::COMPUTER_USE_MALFORMED_CAP, 0, 1, "m"),
        ];
        let out = crate::fix::to_ndjson(&diags, "a.mty", src, false);
        let lines: Vec<&str> = out.trim_end().split('\n').collect();
        assert_eq!(lines.len(), diags.len());
        for l in &lines {
            let env: DiagnosticEnvelope = serde_json::from_str(l).unwrap();
            assert!(env.fix.is_some(), "code {} should have a fix", env.code);
        }
    }
}
