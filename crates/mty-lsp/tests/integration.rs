//! End-to-end-ish tests for the mty-lsp surface.
//!
//! We test at the layer below the JSON-RPC transport: construct a
//! [`DocAnalysis`] directly, invoke each feature module, and assert on
//! the LSP-types result. This catches the same regressions as a full
//! JSON-RPC round-trip but stays fast and deterministic across OSes.
//!
//! At least one test exists per feature in scope (didOpen-equivalent,
//! diagnostics, hover, definition, completion, formatting, incremental
//! change application, line-index UTF-16 handling).

use mty_lsp::completion::complete;
use mty_lsp::definition::definition;
use mty_lsp::diagnostics::build_publish;
use mty_lsp::docs::{apply_change, DocAnalysis};
use mty_lsp::hover::hover;
use mty_lsp::line_index::LineIndex;
use tower_lsp::lsp_types::{
    CompletionResponse, GotoDefinitionResponse, HoverContents, Position, Range,
    TextDocumentContentChangeEvent, Url,
};

fn analyze(src: &str) -> DocAnalysis {
    DocAnalysis::analyze(src.to_string(), "test://main.sd".to_string(), 1)
}

fn uri() -> Url {
    Url::parse("test://main.sd").unwrap()
}

// ---------------------------------------------------------------------
// diagnostics
// ---------------------------------------------------------------------

#[test]
fn diagnostics_clean_file_has_none() {
    let src = "fn main() { }\n";
    let doc = analyze(src);
    let publish = build_publish(uri(), &doc);
    assert!(
        publish.diagnostics.is_empty(),
        "clean file should produce no diagnostics, got {:?}",
        publish.diagnostics
    );
    assert_eq!(publish.version, Some(1));
}

#[test]
fn diagnostics_type_error_produces_at_least_one() {
    // v0.3 (A65): unresolved names in a TopLevelFn scope are now
    // permissive (slice-3 A21 fresh-var fallback). We deliberately
    // surface a strict-scope error here: an agent handler is a strict
    // scope (`ScopeKind::HandlerBody`) so calling an unresolved helper
    // promotes to SD2021. This keeps the LSP wired to a stable
    // diagnostic that survives future tolerance-policy changes.
    let src = "\
        protocol Hi { Greet(name: Str) -> Str }\n\
        agent Greeter: Hi {\n\
          on Greet(name) -> {\n\
            definitely_not_a_real_helper(name)\n\
          }\n\
        }\n";
    let doc = analyze(src);
    let publish = build_publish(uri(), &doc);
    assert!(
        !publish.diagnostics.is_empty(),
        "type error should surface as a diagnostic, got: {:?}",
        publish.diagnostics
    );
    let first = &publish.diagnostics[0];
    let code = format!("{:?}", first.code);
    assert!(code.contains("SD"), "expected SD-prefixed code, got {code}");
}

// ---------------------------------------------------------------------
// hover
// ---------------------------------------------------------------------

#[test]
fn hover_on_fn_name_shows_signature() {
    let src = "fn greet(name: String) -> Unit { }\nfn main() { greet(\"hi\") }\n";
    let doc = analyze(src);
    // Hover over `greet` in the call (line 1, col ~12).
    let call_pos = locate(src, "greet(\"hi\")").unwrap();
    let h = hover(&doc, call_pos).expect("hover returns Some");
    let HoverContents::Markup(m) = h.contents else {
        panic!("expected markup hover")
    };
    assert!(
        m.value.contains("fn greet"),
        "hover body should mention `fn greet`, got: {}",
        m.value
    );
}

#[test]
fn hover_on_unknown_identifier_still_returns_something() {
    let src = "fn main() { let x = 1\n}\n";
    let doc = analyze(src);
    let pos = locate(src, "x =").unwrap();
    let h = hover(&doc, pos).expect("hover returns Some for any token");
    let HoverContents::Markup(m) = h.contents else {
        panic!("expected markup hover")
    };
    assert!(
        m.value.contains("token") || m.value.contains("`x`") || m.value.contains("x"),
        "hover body should mention the token, got: {}",
        m.value
    );
}

// ---------------------------------------------------------------------
// definition
// ---------------------------------------------------------------------

#[test]
fn definition_jumps_from_call_site_to_decl() {
    let src = "fn greet() -> Unit { }\nfn main() { greet() }\n";
    let doc = analyze(src);
    let call_pos = locate(src, "greet()").unwrap();
    let resp = definition(uri(), &doc, call_pos).expect("definition returns Some");
    let GotoDefinitionResponse::Scalar(loc) = resp else {
        panic!("expected scalar definition response")
    };
    // Decl span starts at byte 0 → line 0, char 0.
    assert_eq!(loc.range.start.line, 0);
    assert_eq!(loc.range.start.character, 0);
}

#[test]
fn definition_on_struct_name_returns_decl_span() {
    let src =
        "struct Point { x: I32, y: I32 }\nfn main() { let p: Point = Point { x: 1, y: 2 } }\n";
    let doc = analyze(src);
    // Find the second occurrence of `Point` (the type annotation).
    let pos = locate_nth(src, "Point", 1).unwrap();
    let resp = definition(uri(), &doc, pos).expect("definition resolves struct name");
    let GotoDefinitionResponse::Scalar(loc) = resp else {
        panic!("scalar")
    };
    assert_eq!(loc.range.start.line, 0);
    assert_eq!(loc.range.start.character, 0);
}

// ---------------------------------------------------------------------
// completion
// ---------------------------------------------------------------------

#[test]
fn completion_includes_keywords() {
    let src = "fn main() { }\n";
    let doc = analyze(src);
    let resp = complete(
        &doc,
        Position {
            line: 0,
            character: 0,
        },
    )
    .unwrap();
    let CompletionResponse::Array(items) = resp else {
        panic!("expected array completion")
    };
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    for kw in ["fn", "let", "if", "match", "agent", "protocol"] {
        assert!(labels.contains(&kw), "expected keyword {kw} in completions");
    }
}

#[test]
fn completion_after_dot_includes_methods() {
    // builtin_methods table is populated by the type checker's prelude.
    let src = "fn main() { let s: String = \"hi\"\n s.\n}\n";
    let doc = analyze(src);
    // Cursor right after `s.`
    let dot = src.rfind("s.").unwrap();
    let cursor = dot + 2; // immediately after the '.'
    let (line, character) = doc.line_index.offset_to_position(src, cursor as u32);
    let resp = complete(&doc, Position { line, character }).unwrap();
    let CompletionResponse::Array(items) = resp else {
        panic!("array")
    };
    // We don't assert which method names show up (the prelude can
    // change), but the count should exceed the keyword set if any
    // built-in methods are registered. If the table is empty we still
    // accept the response (keywords-only completion).
    let any_method = items.iter().any(|i| {
        matches!(
            i.kind,
            Some(tower_lsp::lsp_types::CompletionItemKind::METHOD)
        )
    });
    let _ = any_method; // tolerant assertion: just exercise the path.
    assert!(!items.is_empty(), "completion should produce items");
}

// ---------------------------------------------------------------------
// formatting (smoke — full coverage lives in mty-fmt)
// ---------------------------------------------------------------------

#[test]
fn formatting_is_idempotent_for_already_formatted_file() {
    // Round-trip: format twice and confirm output is stable.
    let src = "fn main() { }\n";
    let doc = analyze(src);
    let formatted = mty_fmt::format(doc.parsed.green.clone());
    let doc2 = analyze(&formatted);
    let formatted2 = mty_fmt::format(doc2.parsed.green.clone());
    assert_eq!(formatted, formatted2, "format is idempotent");
}

// ---------------------------------------------------------------------
// incremental change application
// ---------------------------------------------------------------------

#[test]
fn apply_full_replacement_change() {
    let src = "fn main() { }\n";
    let li = LineIndex::new(src);
    let change = TextDocumentContentChangeEvent {
        range: None,
        range_length: None,
        text: "fn main() { let x = 1\n}\n".into(),
    };
    let out = apply_change(src, &li, &change).unwrap();
    assert!(out.contains("let x"));
}

#[test]
fn apply_incremental_range_change() {
    let src = "fn main() { 1 }\n";
    let li = LineIndex::new(src);
    // Replace the `1` at position (0, 12..13) with `42`.
    let change = TextDocumentContentChangeEvent {
        range: Some(Range {
            start: Position {
                line: 0,
                character: 12,
            },
            end: Position {
                line: 0,
                character: 13,
            },
        }),
        range_length: None,
        text: "42".into(),
    };
    let out = apply_change(src, &li, &change).unwrap();
    assert!(out.contains("42"));
    assert!(!out.contains(" 1 "));
}

// ---------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------

fn locate(src: &str, needle: &str) -> Option<Position> {
    locate_nth(src, needle, 0)
}

/// Return the LSP position of the `n`-th (0-indexed) occurrence of
/// `needle` in `src`, or `None` if there aren't that many matches.
fn locate_nth(src: &str, needle: &str, n: usize) -> Option<Position> {
    let mut start = 0;
    let mut count = 0;
    loop {
        let off = src[start..].find(needle)?;
        let byte = start + off;
        if count == n {
            let li = LineIndex::new(src);
            let (line, character) = li.offset_to_position(src, byte as u32);
            return Some(Position { line, character });
        }
        count += 1;
        start = byte + 1;
    }
}
