//! v0.8 Task 3 — cross-file workspace resolve / rename.
//!
//! Build a temp workspace with two `.mty` files referring to a single
//! top-level function. Rename the function via the LSP rename path and
//! verify edits land in BOTH files.

use mty_lsp::docs::DocAnalysis;
use mty_lsp::rename::rename_with_workspace;
use mty_lsp::workspace::{path_to_uri, WorkspaceRegistry};
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp::lsp_types::Position;

fn tmpdir(prefix: &str) -> PathBuf {
    let pid = std::process::id();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let p = std::env::temp_dir().join(format!("mty-lsp-test-{prefix}-{pid}-{nonce}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn cross_file_rename_propagates_to_all_files() {
    let dir = tmpdir("rename");
    let a = dir.join("a.mty");
    let b = dir.join("b.mty");
    let c = dir.join("c.mty");
    std::fs::write(&a, "pub fn shared() -> i32 { 1 }\n").unwrap();
    std::fs::write(&b, "fn caller() -> i32 { shared() }\n").unwrap();
    std::fs::write(&c, "fn unrelated() -> i32 { 0 }\n").unwrap();

    let registry = WorkspaceRegistry::new();
    registry.add_folder(dir.clone());

    // Now perform the rename starting from file A on `shared`.
    let a_uri = path_to_uri(&a).unwrap();
    let a_src = std::fs::read_to_string(&a).unwrap();
    let doc = Arc::new(DocAnalysis::analyze(a_src.clone(), a_uri.to_string(), 0));
    // Position cursor on `shared` (line 0, col 7 — right after `pub fn `).
    let pos = Position {
        line: 0,
        character: 7,
    };

    let we = rename_with_workspace(a_uri.clone(), &doc, pos, "renamed", Some(&registry))
        .expect("rename ok");
    let changes = we.changes.expect("workspace changes");
    let a_changes = changes.get(&a_uri).expect("a.mty had no edits");
    assert!(!a_changes.is_empty(), "a.mty had zero edits");

    let b_uri = path_to_uri(&b).unwrap();
    let b_changes = changes.get(&b_uri).expect("b.mty had no edits");
    assert!(!b_changes.is_empty(), "b.mty had zero edits");

    // c.mty should NOT appear (no reference to `shared`).
    let c_uri = path_to_uri(&c).unwrap();
    assert!(
        !changes.contains_key(&c_uri),
        "c.mty should have no edits but got: {:?}",
        changes.get(&c_uri)
    );
}
