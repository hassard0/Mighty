//! Signature-help regressions for fn-call sites.

use sdust_lsp::docs::DocAnalysis;
use sdust_lsp::signature_help::signature_help;
use tower_lsp::lsp_types::Position;

fn analyze(src: &str) -> DocAnalysis {
    DocAnalysis::analyze(src.to_string(), "test://main.sd".to_string(), 1)
}

#[test]
fn signature_help_in_call_active_param_zero() {
    let src = "fn greet(name: String, age: I32) -> Unit { }\nfn main() { greet(\"hi\", 1) }\n";
    let doc = analyze(src);
    // Position cursor right after the `(` in the CALL `greet(` (the
    // second occurrence, not the decl).
    let off = src.rfind("greet(").unwrap() + "greet(".len();
    let li = sdust_lsp::line_index::LineIndex::new(src);
    let (line, character) = li.offset_to_position(src, off as u32);
    let h = signature_help(&doc, Position { line, character }).expect("Some");
    assert_eq!(h.signatures.len(), 1);
    assert!(h.signatures[0].label.contains("greet"));
    assert_eq!(h.active_parameter, Some(0));
}

#[test]
fn signature_help_after_comma_bumps_active_param() {
    let src = "fn greet(name: String, age: I32) -> Unit { }\nfn main() { greet(\"hi\", 1) }\n";
    let doc = analyze(src);
    // Cursor immediately after the `, ` between the args of the CALL.
    let call_open = src.rfind("greet(").unwrap() + "greet(".len();
    let off = src[call_open..].find(", ").unwrap() + call_open + 2;
    let li = sdust_lsp::line_index::LineIndex::new(src);
    let (line, character) = li.offset_to_position(src, off as u32);
    let h = signature_help(&doc, Position { line, character }).expect("Some");
    assert_eq!(h.active_parameter, Some(1));
}

#[test]
fn signature_help_outside_call_returns_none() {
    let src = "fn greet() -> Unit { }\nfn main() { }\n";
    let doc = analyze(src);
    let h = signature_help(
        &doc,
        Position {
            line: 0,
            character: 0,
        },
    );
    assert!(h.is_none());
}
