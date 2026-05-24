//! Doc-comment extraction unit tests.

use mty_doc::{build_doc_package, DocItemKind, DocVisibility};

const FIXTURE: &str = r#"//! search_api — minimal in-memory search service.
//!
//! Provides a tiny query / index surface used by the smoke tests.
//! Real production code lives in `crates/search-server`.

package search_api

/// Add returns the sum of `a` and `b`.
///
/// This is the second paragraph. It should NOT be in the synopsis.
pub fn add(a: I32, b: I32) -> I32 {
  a + b
}

/// User is a registered user of the system.
///
/// # Since
/// 0.2.0
///
/// ```mty
/// let u = User { id: 1, name: "ada" }
/// ```
pub struct User {
  id: I32,
  name: Str,
}

/// Greet returns a friendly hello.
fn greet(name: Str) -> Str {
  "hi"
}

// not a doc comment — should attach to nothing
pub fn undocumented() {}
"#;

#[test]
fn extracts_package_doc() {
    let (doc, diags) = build_doc_package(FIXTURE, "fixture.mty", "fixture");
    assert!(
        diags
            .iter()
            .all(|d| !matches!(d.severity, mty_diagnostics::Severity::Error)),
        "unexpected errors: {:?}",
        diags
    );
    assert_eq!(doc.name, "search_api");
    assert!(
        doc.synopsis.starts_with("search_api"),
        "synopsis = {:?}",
        doc.synopsis
    );
    assert!(doc.body.contains("smoke tests"));
}

#[test]
fn extracts_fn_with_synopsis_and_body() {
    let (doc, _) = build_doc_package(FIXTURE, "fixture.mty", "fixture");
    let add = doc.items.iter().find(|i| i.name == "add").expect("add");
    assert_eq!(add.kind, DocItemKind::Fn);
    assert_eq!(add.visibility, DocVisibility::Public);
    assert_eq!(add.synopsis, "Add returns the sum of `a` and `b`.");
    assert!(add.body.contains("second paragraph"));
    assert!(add.signature.plain.contains("pub fn add"));
    assert!(add.signature.plain.contains("-> I32"));
}

#[test]
fn extracts_struct_with_since_and_example() {
    let (doc, _) = build_doc_package(FIXTURE, "fixture.mty", "fixture");
    let user = doc.items.iter().find(|i| i.name == "User").expect("User");
    assert_eq!(user.kind, DocItemKind::Struct);
    assert_eq!(user.visibility, DocVisibility::Public);
    assert_eq!(user.since.as_deref(), Some("0.2.0"));
    assert_eq!(user.examples.len(), 1);
    assert_eq!(user.examples[0].language, "mty");
    assert!(user.examples[0].code.contains("User { id: 1"));
    assert!(user.signature.plain.contains("pub struct User"));
    assert!(user.signature.plain.contains("id: I32"));
}

#[test]
fn private_items_are_marked_private() {
    let (doc, _) = build_doc_package(FIXTURE, "fixture.mty", "fixture");
    let greet = doc.items.iter().find(|i| i.name == "greet").expect("greet");
    assert_eq!(greet.visibility, DocVisibility::Private);
}

#[test]
fn undocumented_item_has_empty_synopsis() {
    let (doc, _) = build_doc_package(FIXTURE, "fixture.mty", "fixture");
    let u = doc
        .items
        .iter()
        .find(|i| i.name == "undocumented")
        .expect("undocumented");
    assert_eq!(u.synopsis, "");
    assert_eq!(u.body, "");
}

#[test]
fn comment_separated_by_blank_line_does_not_attach() {
    // A blank line between doc comment block and item breaks attachment.
    let src = r#"package p

/// detached comment

pub fn foo() {}
"#;
    let (doc, _) = build_doc_package(src, "blank.mty", "blank");
    let foo = doc.items.iter().find(|i| i.name == "foo").expect("foo");
    assert_eq!(foo.synopsis, "");
}

#[test]
fn backlinks_credit_calling_fn() {
    let src = r#"package p

/// Inner helper.
pub fn inner() -> I32 { 1 }

/// Outer caller — invokes [inner].
pub fn outer() -> I32 { inner() }
"#;
    let (doc, _) = build_doc_package(src, "bl.mty", "bl");
    let inner = doc.items.iter().find(|i| i.name == "inner").expect("inner");
    assert!(
        inner.used_by.iter().any(|s| s == "outer"),
        "inner.used_by = {:?}",
        inner.used_by
    );
}
