//! End-to-end shape: render `sdust doc <file>` style output as a single
//! function call (mirrors what `crates/sdust-cli/src/cmd/doc.rs` does).

use sdust_doc::{build_doc_package, render};

const FIXTURE: &str = r#"package hello

/// Main entry point.
pub fn main() {
  log("hello, Stardust")
}
"#;

#[test]
fn package_text_render_matches_go_style() {
    let (doc, _) = build_doc_package(FIXTURE, "hello.sd", "hello");
    let out = render::text(&doc);
    assert!(out.starts_with("package hello\n"), "leading line: {}", out);
    assert!(out.contains("FUNCTIONS"));
    assert!(out.contains("pub fn main()"));
    assert!(out.contains("Main entry point."));
}

#[test]
fn item_text_render_includes_full_body() {
    let src = r#"package p

/// Foo does something interesting.
///
/// This is the long body.
pub fn foo() {}
"#;
    let (doc, _) = build_doc_package(src, "p.sd", "p");
    let foo = doc.items.iter().find(|i| i.name == "foo").unwrap();
    let out = render::item_text(&doc, foo);
    assert!(out.contains("package p"));
    assert!(out.contains("pub fn foo()"));
    assert!(out.contains("Foo does something interesting"));
    assert!(out.contains("This is the long body"));
}

#[test]
fn writing_html_tree_succeeds() {
    let (doc, _) = build_doc_package(FIXTURE, "hello.sd", "hello");
    let files = render::html(&doc);
    let dir = tempfile::tempdir().unwrap();
    render::write_tree(dir.path(), &files).unwrap();
    let index = std::fs::read_to_string(dir.path().join("index.html")).unwrap();
    assert!(index.contains("Package <code>hello</code>"));
    let css = std::fs::read_to_string(dir.path().join("style.css")).unwrap();
    assert!(css.contains("--accent"));
    let js = std::fs::read_to_string(dir.path().join("search.js")).unwrap();
    assert!(js.contains("search-index.json"));
}

#[test]
fn agents_and_protocols_are_documented() {
    let src = r#"package svc

/// Echo replies with the same message.
protocol Echo {
  Say(msg: Str) -> Str
}

/// Repeater agent answers Echo messages.
agent Repeater(): Echo {
  on Say(m) {
    return m
  }
}
"#;
    let (doc, _) = build_doc_package(src, "svc.sd", "svc");
    let echo = doc.items.iter().find(|i| i.name == "Echo").expect("Echo");
    assert_eq!(echo.kind, sdust_doc::DocItemKind::Protocol);
    assert!(echo.signature.plain.contains("protocol Echo"));
    let rep = doc
        .items
        .iter()
        .find(|i| i.name == "Repeater")
        .expect("Repeater");
    assert_eq!(rep.kind, sdust_doc::DocItemKind::Agent);
    assert!(rep.signature.plain.contains("agent Repeater"));
    assert!(rep.signature.plain.contains("on Say"));
}
