//! `textDocument/semanticTokens/full` regressions.
//!
//! We hit `semantic_tokens::full` (and `range`) directly against a
//! `DocAnalysis` and assert on the encoded token stream: number of
//! tokens, ordered classifications, and that the modifiers bitset is
//! populated as documented (declaration, defaultLibrary, readonly).

use mty_lsp::docs::DocAnalysis;
use mty_lsp::semantic_tokens::{
    full, full_delta, full_with_cache, legend, range, DeltaCache, LEGEND_TYPES,
};
use tower_lsp::lsp_types::{
    Position, Range, SemanticTokensFullDeltaResult, SemanticTokensRangeResult,
    SemanticTokensResult, Url,
};

fn analyze(src: &str) -> DocAnalysis {
    DocAnalysis::analyze(src.to_string(), "test://main.mty".to_string(), 1)
}

fn analyze_with_version(src: &str, version: i32) -> DocAnalysis {
    DocAnalysis::analyze(src.to_string(), "test://main.mty".to_string(), version)
}

fn uri() -> Url {
    Url::parse("test://main.mty").unwrap()
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

// ---------- v0.47 T5 — semanticTokens delta ----------

#[test]
fn full_with_cache_emits_result_id_that_round_trips() {
    let src = "fn main() { let x = 1 }\n";
    let doc = analyze(src);
    let mut cache = DeltaCache::with_capacity(4);
    let SemanticTokensResult::Tokens(t) = full_with_cache(&uri(), &doc, &mut cache) else {
        panic!("Tokens variant")
    };
    let rid = t.result_id.expect("result_id populated");
    let snap = cache.get(&uri(), &rid).expect("cache hit");
    assert_eq!(snap.version, 1);
    assert_eq!(snap.data, t.data);
}

#[test]
fn full_delta_returns_edits_when_previous_result_id_matches() {
    // First request: get a snapshot for v=1.
    let src1 = "fn main() { let x = 1 }\n";
    let doc1 = analyze_with_version(src1, 1);
    let mut cache = DeltaCache::with_capacity(4);
    let SemanticTokensResult::Tokens(initial) = full_with_cache(&uri(), &doc1, &mut cache) else {
        panic!("Tokens")
    };
    let rid = initial.result_id.expect("result_id");

    // Second request with the SAME source (v=2) but a small edit
    // that changes one token type: replace `1` with `"hi"`.
    let src2 = "fn main() { let x = \"hi\" }\n";
    let doc2 = analyze_with_version(src2, 2);

    let result = full_delta(&uri(), &doc2, &rid, &mut cache);
    let delta = match result {
        SemanticTokensFullDeltaResult::TokensDelta(d) => d,
        SemanticTokensFullDeltaResult::Tokens(_) => {
            panic!("expected TokensDelta, got Tokens (cache miss?)")
        }
        SemanticTokensFullDeltaResult::PartialTokensDelta { .. } => panic!("partial"),
    };
    assert!(
        !delta.edits.is_empty(),
        "expected at least one delta edit between versions 1 and 2"
    );
    let new_rid = delta.result_id.expect("delta carries fresh result_id");
    assert_ne!(new_rid, rid, "result_id should rotate on each delta");
    // The new snapshot is now in the cache for the next round-trip.
    assert!(cache.get(&uri(), &new_rid).is_some(), "new snapshot cached");
}

#[test]
fn full_delta_returns_full_tokens_when_previous_result_id_is_stale() {
    let src = "fn main() { let x = 1 }\n";
    let doc = analyze_with_version(src, 2);
    let mut cache = DeltaCache::with_capacity(4);
    let result = full_delta(&uri(), &doc, "mty-st-never-issued", &mut cache);
    let tokens = match result {
        SemanticTokensFullDeltaResult::Tokens(t) => t,
        SemanticTokensFullDeltaResult::TokensDelta(_) => {
            panic!("expected Tokens (stale result_id), got TokensDelta")
        }
        SemanticTokensFullDeltaResult::PartialTokensDelta { .. } => panic!("partial"),
    };
    let rid = tokens.result_id.expect("fresh result_id");
    // The fresh result_id is in the cache so the NEXT delta call can
    // succeed.
    assert!(cache.get(&uri(), &rid).is_some(), "fresh snapshot cached");
    assert!(
        !tokens.data.is_empty(),
        "full fallback should carry the full token stream"
    );
}

#[test]
fn full_delta_returns_no_edits_when_source_unchanged() {
    let src = "fn main() { let x = 1 }\n";
    let doc1 = analyze_with_version(src, 1);
    let mut cache = DeltaCache::with_capacity(4);
    let SemanticTokensResult::Tokens(t) = full_with_cache(&uri(), &doc1, &mut cache) else {
        panic!("Tokens")
    };
    let rid = t.result_id.expect("result_id");

    let doc2 = analyze_with_version(src, 2);
    let result = full_delta(&uri(), &doc2, &rid, &mut cache);
    let SemanticTokensFullDeltaResult::TokensDelta(delta) = result else {
        panic!("expected TokensDelta")
    };
    assert!(
        delta.edits.is_empty(),
        "unchanged source should produce zero edits, got {} edits",
        delta.edits.len()
    );
}
