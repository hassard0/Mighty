//! Golden tests for markdown rendering.

use mty_doc::{build_doc_package, render};

const SAMPLE: &str = r#"//! Calculator package.
//!
//! Provides basic arithmetic.

package calc

/// Add returns the sum of `a` and `b`.
///
/// Used by every caller in the package.
pub fn add(a: I32, b: I32) -> I32 {
  a + b
}

/// Sub returns `a - b`.
pub fn sub(a: I32, b: I32) -> I32 {
  a - b
}

/// Pair groups two integers.
pub struct Pair {
  lo: I32,
  hi: I32,
}
"#;

#[test]
fn markdown_index_contains_sections_and_anchors() {
    let (doc, _) = build_doc_package(SAMPLE, "calc.mty", "calc");
    let files = render::markdown(&doc);
    let idx = files.get("index.md").expect("index.md");
    assert!(
        idx.contains("# Package `calc`"),
        "missing pkg header: {idx}"
    );
    assert!(idx.contains("Calculator package."), "missing pkg synopsis");
    assert!(idx.contains("## FUNCTIONS"), "missing functions section");
    assert!(idx.contains("## TYPES"), "missing types section");
    assert!(idx.contains("[`add`](fn.add.md)"), "missing add link");
    assert!(
        idx.contains("[`Pair`](struct.Pair.md)"),
        "missing pair link"
    );
}

#[test]
fn markdown_per_item_has_signature_and_back_link() {
    let (doc, _) = build_doc_package(SAMPLE, "calc.mty", "calc");
    let files = render::markdown(&doc);
    let add = files.get("fn.add.md").expect("fn.add.md");
    assert!(add.contains("# `add` (fn)"));
    assert!(add.contains("pub fn add(a: I32, b: I32) -> I32"));
    assert!(add.contains("Used by every caller"));
    assert!(add.contains("[Back to index](index.md)"));
}

#[test]
fn markdown_struct_signature_is_pretty_printed() {
    let (doc, _) = build_doc_package(SAMPLE, "calc.mty", "calc");
    let files = render::markdown(&doc);
    let pair = files.get("struct.Pair.md").expect("struct.Pair.md");
    assert!(pair.contains("pub struct Pair"));
    assert!(pair.contains("lo: I32"));
    assert!(pair.contains("hi: I32"));
}
