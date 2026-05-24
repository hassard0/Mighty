//! Inlay-hint regressions.
//!
//! Confirms that `let x = <int>` without a type annotation produces an
//! inferred-type hint, and that the hint is suppressed when the user
//! has written `let x: I32 = ...`.

use mty_lsp::docs::DocAnalysis;
use mty_lsp::inlay_hints::inlay_hints;
use tower_lsp::lsp_types::{Position, Range};

fn analyze(src: &str) -> DocAnalysis {
    DocAnalysis::analyze(src.to_string(), "test://main.sd".to_string(), 1)
}

fn whole_file_range() -> Range {
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: 9999,
            character: 0,
        },
    }
}

#[test]
fn inferred_let_emits_type_hint() {
    let src = "fn main() { let x = 1_i32\n }\n";
    let doc = analyze(src);
    let hints = inlay_hints(&doc, whole_file_range());
    // We expect at least one hint for `x`.
    assert!(
        !hints.is_empty(),
        "expected at least one inlay hint, got none"
    );
}

#[test]
fn annotated_let_suppresses_hint() {
    let src = "fn main() { let x: I32 = 1\n }\n";
    let doc = analyze(src);
    let hints = inlay_hints(&doc, whole_file_range());
    // Should have no hint for `x` because the user annotated it.
    let any_let_hint = hints
        .iter()
        .filter(|h| matches!(&h.label, tower_lsp::lsp_types::InlayHintLabel::String(s) if s.starts_with(": ")))
        .count();
    assert_eq!(any_let_hint, 0, "expected no hints, got: {:?}", hints);
}

#[test]
fn viewport_filters_hints() {
    let src = "fn main() {\n let x = 1\n let y = 2\n let z = 3\n }\n";
    let doc = analyze(src);
    // Tight viewport that only contains line 1.
    let r = Range {
        start: Position {
            line: 1,
            character: 0,
        },
        end: Position {
            line: 1,
            character: 99,
        },
    };
    let hints = inlay_hints(&doc, r);
    assert!(hints.len() <= 1);
}
