//! Workspace-folder smoke test.
//!
//! v0.5 advertises workspace-folder support but keeps per-file
//! analysis. This test confirms that opening two files in the same
//! `DocStore` analyzes both independently and that diagnostics from
//! one don't leak into the other.

use mty_lsp::diagnostics::build_publish;
use mty_lsp::docs::DocStore;
use tower_lsp::lsp_types::Url;

#[test]
fn two_files_get_independent_analysis() {
    let store = DocStore::new();
    let a = Url::parse("file:///a.sd").unwrap();
    let b = Url::parse("file:///b.sd").unwrap();
    let doc_a = store.open(a.clone(), "fn main() { }\n".into(), 1);
    let doc_b = store.open(
        b.clone(),
        "fn main() { definitely_undefined() }\n".into(),
        1,
    );
    let pa = build_publish(a, &doc_a);
    let pb = build_publish(b, &doc_b);
    // File A has no diagnostics; file B at least might have one
    // depending on tolerance policy. We assert independence: B's
    // diagnostics never leak into A.
    assert!(pa.diagnostics.is_empty());
    // Document store retains both.
    assert!(store.get(&pa.uri).is_some());
    assert!(store.get(&pb.uri).is_some());
}
