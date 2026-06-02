//! Signature-help regressions for fn-call sites.

use mty_lsp::docs::DocAnalysis;
use mty_lsp::signature_help::signature_help;
use tower_lsp::lsp_types::{ParameterLabel, Position};

fn analyze(src: &str) -> DocAnalysis {
    DocAnalysis::analyze(src.to_string(), "test://main.mty".to_string(), 1)
}

#[test]
fn signature_help_in_call_active_param_zero() {
    let src = "fn greet(name: String, age: I32) -> Unit { }\nfn main() { greet(\"hi\", 1) }\n";
    let doc = analyze(src);
    // Position cursor right after the `(` in the CALL `greet(` (the
    // second occurrence, not the decl).
    let off = src.rfind("greet(").unwrap() + "greet(".len();
    let li = mty_lsp::line_index::LineIndex::new(src);
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
    let li = mty_lsp::line_index::LineIndex::new(src);
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

// ----------------------------------------------------------------------
// v0.46 T5 — structured parameter labels.
//
// Pre-T5 the LSP emitted `ParameterLabel::Simple("p0")`,
// `ParameterLabel::Simple("p1")` placeholders. The IDE L31 client
// worked around the placeholders by substring-locating `a: I32` inside
// the signature label. T5 promotes per-parameter labels to
// `LabelOffsets` pointing at the real `a: I32` slice, AND verifies the
// offsets land on substrings that read back as `a: I32`/`b: I32`.
// ----------------------------------------------------------------------

#[test]
fn signature_help_emits_real_param_labels() {
    let src = "fn add(a: I32, b: I32) -> I32 { a + b }\nfn main() { add(1, 2) }\n";
    let doc = analyze(src);
    let off = src.rfind("add(").unwrap() + "add(".len();
    let li = mty_lsp::line_index::LineIndex::new(src);
    let (line, character) = li.offset_to_position(src, off as u32);
    let h = signature_help(&doc, Position { line, character }).expect("Some");
    let sig = &h.signatures[0];
    let label = &sig.label;
    let params = sig.parameters.as_ref().expect("parameters present");
    assert_eq!(params.len(), 2, "two params expected");

    // Walk every ParameterLabel; for offset-form labels, the slice the
    // offsets describe must read back as `a: I32` / `b: I32`. For
    // simple-form labels (back-compat fallback), the string itself must
    // be the substring.
    let expected = ["a: I32", "b: I32"];
    for (i, p) in params.iter().enumerate() {
        match &p.label {
            ParameterLabel::LabelOffsets([start, end]) => {
                let chars: Vec<char> = label.chars().collect();
                let slice: String = chars[*start as usize..*end as usize].iter().collect();
                assert_eq!(
                    slice, expected[i],
                    "offset-form label slice mismatch for param {i}"
                );
            }
            ParameterLabel::Simple(s) => {
                assert_eq!(s, expected[i], "simple-form label mismatch for param {i}");
            }
        }
    }

    // Sanity: no `p0` / `p1` placeholders survive.
    for p in params {
        if let ParameterLabel::Simple(s) = &p.label {
            assert!(!s.starts_with('p'), "stale `p{{n}}` placeholder: {s}");
        }
    }
}
