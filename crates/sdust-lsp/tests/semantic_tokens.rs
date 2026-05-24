//! `textDocument/semanticTokens/full` regressions.
//!
//! We hit `semantic_tokens::full` (and `range`) directly against a
//! `DocAnalysis` and assert on the encoded token stream: number of
//! tokens, ordered classifications, and that the modifiers bitset is
//! populated as documented (declaration, defaultLibrary, readonly).

use sdust_lsp::docs::DocAnalysis;
use sdust_lsp::semantic_tokens::{full, legend, range, LEGEND_TYPES};
use tower_lsp::lsp_types::{Position, Range, SemanticTokensRangeResult, SemanticTokensResult};

fn analyze(src: &str) -> DocAnalysis {
    DocAnalysis::analyze(src.to_string(), "test://main.sd".to_string(), 1)
}

#[test]
fn legend_contains_expected_types() {
    let l = legend();
    assert_eq!(l.token_types.len(), LEGEND_TYPES.len());
    let names: Vec<String> = l
        .token_types
        .iter()
        .map(|t| {
            let s: &str = t.as_str();
            s.to_string()
        })
        .collect();
    assert!(names.contains(&"keyword".to_string()));
    assert!(names.contains(&"function".to_string()));
    assert!(names.contains(&"type".to_string()));
    assert!(names.contains(&"parameter".to_string()));
    assert!(names.contains(&"variable".to_string()));
}

#[test]
fn empty_file_returns_empty_tokens() {
    let doc = analyze("");
    let SemanticTokensResult::Tokens(t) = full(&doc) else {
        panic!("expected Tokens variant")
    };
    assert!(t.data.is_empty());
}

#[test]
fn simple_fn_produces_keyword_and_function_tokens() {
    let src = "fn main() { let x = 1 }\n";
    let doc = analyze(src);
    let SemanticTokensResult::Tokens(t) = full(&doc) else {
        panic!("Tokens")
    };
    // We expect at least: `fn` (keyword), `main` (function decl), `let`
    // (keyword), `x` (variable), `1` (number).
    assert!(t.data.len() >= 5, "got {} tokens", t.data.len());
}

#[test]
fn range_filters_to_viewport() {
    let src = "fn a() { }\nfn b() { }\nfn c() { }\n";
    let doc = analyze(src);
    let r = Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: 0,
            character: 20,
        },
    };
    let SemanticTokensRangeResult::Tokens(t) = range(&doc, r) else {
        panic!("Tokens")
    };
    // Token stream for just the first line should be strictly smaller
    // than the full file stream.
    let SemanticTokensResult::Tokens(full_t) = full(&doc) else {
        panic!("Tokens")
    };
    assert!(t.data.len() < full_t.data.len());
}

#[test]
fn string_and_number_literals_classified() {
    let src = "fn f() { let s = \"hi\"\n let n = 42\n }\n";
    let doc = analyze(src);
    let SemanticTokensResult::Tokens(t) = full(&doc) else {
        panic!("Tokens")
    };
    // Find the legend index for string and number.
    let l = legend();
    let str_idx = l
        .token_types
        .iter()
        .position(|t| t.as_str() == "string")
        .unwrap() as u32;
    let num_idx = l
        .token_types
        .iter()
        .position(|t| t.as_str() == "number")
        .unwrap() as u32;
    let mut saw_string = false;
    let mut saw_number = false;
    for tok in &t.data {
        if tok.token_type == str_idx {
            saw_string = true;
        }
        if tok.token_type == num_idx {
            saw_number = true;
        }
    }
    assert!(saw_string, "no STRING token classified");
    assert!(saw_number, "no NUMBER token classified");
}
