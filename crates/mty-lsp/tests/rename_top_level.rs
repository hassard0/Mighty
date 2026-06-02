//! Rename regressions for top-level items (fns, structs, enums).
//!
//! v0.5 is single-file: cross-file renames are deferred until the LSP
//! grows a multi-file resolve map. The test below renames a fn whose
//! decl and call sites all live in `main.mty` and confirms every
//! occurrence is rewritten.

use mty_lsp::docs::DocAnalysis;
use mty_lsp::rename::{rename, rename_with_caps};
use tower_lsp::lsp_types::{DocumentChanges, OneOf, Position, Url};

fn analyze(src: &str) -> DocAnalysis {
    DocAnalysis::analyze(src.to_string(), "test://main.mty".to_string(), 1)
}

fn locate(src: &str, needle: &str) -> Position {
    let off = src.find(needle).expect("needle missing") as u32;
    let li = mty_lsp::line_index::LineIndex::new(src);
    let (line, character) = li.offset_to_position(src, off);
    Position { line, character }
}

fn uri() -> Url {
    Url::parse("test://main.mty").unwrap()
}

#[test]
fn rename_top_level_fn_rewrites_decl_and_calls() {
    let src = "fn greet() -> Unit { }\nfn main() { greet()\n greet()\n }\n";
    let doc = analyze(src);
    let pos = locate(src, "greet()");
    let edit = rename(uri(), &doc, pos, "salute").expect("rename ok");
    let changes = edit.changes.expect("changes");
    let edits = changes.values().next().unwrap();
    // 3 occurrences (1 decl + 2 calls).
    assert_eq!(edits.len(), 3);
    for e in edits {
        assert_eq!(e.new_text, "salute");
    }
}

#[test]
fn rename_struct_rewrites_decl_and_usages() {
    let src =
        "struct Point { x: I32, y: I32 }\nfn main() { let p: Point = Point { x: 1, y: 2 } }\n";
    let doc = analyze(src);
    let pos = locate(src, "Point {");
    let edit = rename(uri(), &doc, pos, "Pt").expect("rename ok");
    let changes = edit.changes.expect("changes");
    let edits = changes.values().next().unwrap();
    // 3 occurrences of `Point` (decl + type annot + struct literal).
    assert_eq!(edits.len(), 3);
}

// ---------- v0.47 T5 — documentChanges migration ----------

#[test]
fn rename_with_document_changes_emits_versioned_text_document_edit() {
    let src = "fn greet() -> Unit { }\nfn main() { greet() }\n";
    let doc = analyze(src);
    let pos = locate(src, "greet()");
    // Client advertises documentChanges support.
    let edit = rename_with_caps(uri(), &doc, pos, "salute", None, true).expect("rename ok");
    // Legacy `changes` map MUST be empty.
    assert!(
        edit.changes.is_none(),
        "documentChanges-shaped edits should not also emit `changes`"
    );
    let document_changes = edit.document_changes.expect("documentChanges populated");
    let edits = match document_changes {
        DocumentChanges::Edits(e) => e,
        DocumentChanges::Operations(_) => panic!("expected Edits variant, got Operations"),
    };
    assert_eq!(edits.len(), 1, "single-file rename = one TextDocumentEdit");
    let tde = &edits[0];
    assert_eq!(tde.text_document.uri, uri());
    // Version comes from the analysed buffer (set to `1` by `analyze`).
    assert_eq!(tde.text_document.version, Some(1));
    // Two occurrences: decl + call.
    assert_eq!(tde.edits.len(), 2);
    for oneof in &tde.edits {
        let OneOf::Left(te) = oneof else {
            panic!("expected plain TextEdit, got AnnotatedTextEdit");
        };
        assert_eq!(te.new_text, "salute");
    }
}

#[test]
fn rename_without_document_changes_falls_back_to_legacy_changes_shape() {
    // v0.46 T5 back-compat: a client that does NOT advertise
    // documentChanges should still see the legacy `changes` shape.
    let src = "fn greet() -> Unit { }\nfn main() { greet() }\n";
    let doc = analyze(src);
    let pos = locate(src, "greet()");
    let edit = rename_with_caps(uri(), &doc, pos, "salute", None, false).expect("rename ok");
    assert!(
        edit.document_changes.is_none(),
        "downgraded shape must not populate documentChanges"
    );
    let changes = edit.changes.expect("changes populated");
    let edits = changes.values().next().unwrap();
    assert_eq!(edits.len(), 2);
}
