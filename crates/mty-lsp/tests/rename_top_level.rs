//! Rename regressions for top-level items (fns, structs, enums).
//!
//! v0.5 is single-file: cross-file renames are deferred until the LSP
//! grows a multi-file resolve map. The test below renames a fn whose
//! decl and call sites all live in `main.sd` and confirms every
//! occurrence is rewritten.

use mty_lsp::docs::DocAnalysis;
use mty_lsp::rename::rename;
use tower_lsp::lsp_types::{Position, Url};

fn analyze(src: &str) -> DocAnalysis {
    DocAnalysis::analyze(src.to_string(), "test://main.sd".to_string(), 1)
}

fn locate(src: &str, needle: &str) -> Position {
    let off = src.find(needle).expect("needle missing") as u32;
    let li = mty_lsp::line_index::LineIndex::new(src);
    let (line, character) = li.offset_to_position(src, off);
    Position { line, character }
}

fn uri() -> Url {
    Url::parse("test://main.sd").unwrap()
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
