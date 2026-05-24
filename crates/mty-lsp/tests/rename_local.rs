//! Rename regressions for local bindings inside a single fn body.
//!
//! v0.5: locals are renamed within the smallest enclosing fn / handler
//! body; shadowing across blocks is renamed together (the editor's
//! preview lets the user reject if not desired).

use mty_lsp::docs::DocAnalysis;
use mty_lsp::rename::{prepare, rename};
use tower_lsp::lsp_types::{Position, PrepareRenameResponse, Url};

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
fn prepare_rename_returns_range_for_ident() {
    let src = "fn main() { let foo = 1 }\n";
    let doc = analyze(src);
    let pos = locate(src, "foo");
    let prep = prepare(&doc, pos).expect("prepare returns Some");
    match prep {
        PrepareRenameResponse::Range(r) => {
            assert_eq!(r.start.line, 0);
            assert_eq!(r.end.character, r.start.character + 3);
        }
        _ => panic!("expected Range variant"),
    }
}

#[test]
fn rename_local_rewrites_all_occurrences() {
    let src = "fn main() { let x = 1\n let y = x + x\n }\n";
    let doc = analyze(src);
    let pos = locate(src, "x =");
    let edit = rename(uri(), &doc, pos, "z").expect("rename ok");
    let changes = edit.changes.expect("has changes");
    let edits = changes.values().next().unwrap();
    // We expect 3 occurrences of `x` to be renamed to `z`.
    assert_eq!(edits.len(), 3, "expected 3 edits, got {}", edits.len());
    for e in edits {
        assert_eq!(e.new_text, "z");
    }
}

#[test]
fn rename_rejects_keyword() {
    let src = "fn main() { let x = 1 }\n";
    let doc = analyze(src);
    let pos = locate(src, "x =");
    let err = rename(uri(), &doc, pos, "fn").unwrap_err();
    assert!(format!("{:?}", err).contains("not a valid"));
}

#[test]
fn rename_rejects_invalid_ident() {
    let src = "fn main() { let x = 1 }\n";
    let doc = analyze(src);
    let pos = locate(src, "x =");
    let err = rename(uri(), &doc, pos, "1bad").unwrap_err();
    assert!(format!("{:?}", err).contains("not a valid"));
}

#[test]
fn prepare_rename_returns_none_for_keyword_token() {
    let src = "fn main() { let x = 1 }\n";
    let doc = analyze(src);
    // Cursor over `fn` keyword.
    let pos = Position {
        line: 0,
        character: 0,
    };
    let prep = prepare(&doc, pos);
    assert!(prep.is_none(), "expected None for keyword token");
}
