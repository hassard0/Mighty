//! v0.27 Track A: parser tests for the `@tool(...)` attribute prefix.
//!
//! Surface (closes the v0.26 demo-07 gap that had to ship the three
//! tool fns as plain `fn` decls):
//!
//! ```mty
//! @tool("Read a text file", cap: fs.read("./data/**"))
//! fn read_doc(path: String) -> Result[String, FsError] !{fs} {
//!   std.fs.read_to_string(path)
//! }
//! ```
//!
//! - `@<ident>(args...)` — attribute prefix, recognized only when
//!   immediately preceding a `fn`, `agent`, or `protocol` decl.
//! - For v0.27, ONLY `@tool` is accepted; unknown names produce a clean
//!   MT1003 diagnostic.
//! - Attribute on a non-fn item produces MT1004.

use mty_syntax::SyntaxKind;

fn parse(src: &str) -> (mty_syntax::SyntaxNode, Vec<mty_syntax::parser::ParseError>) {
    let r = mty_syntax::parse(src);
    let node = mty_syntax::SyntaxNode::new_root(r.green);
    (node, r.errors)
}

/// Walk the CST and count TOOL_ATTR nodes.
fn count_tool_attrs(root: &mty_syntax::SyntaxNode) -> usize {
    root.descendants()
        .filter(|n| n.kind() == SyntaxKind::TOOL_ATTR)
        .count()
}

#[test]
fn parse_tool_with_desc_only() {
    let (root, errors) = parse("@tool(\"Description\") fn foo() {}");
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(count_tool_attrs(&root), 1);
    // FN_DECL must still be present (the attribute is a sibling, not a parent).
    let fns: Vec<_> = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::FN_DECL)
        .collect();
    assert_eq!(fns.len(), 1, "expected one fn decl, got {}", fns.len());
}

#[test]
fn parse_tool_with_cap() {
    let (root, errors) = parse("@tool(\"d\", cap: fs.read) fn read_doc() {}");
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(count_tool_attrs(&root), 1);
    // The cap arg must surface as a TOOL_ATTR_CAP_ARG child.
    let cap_args: Vec<_> = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::TOOL_ATTR_CAP_ARG)
        .collect();
    assert_eq!(cap_args.len(), 1, "expected one cap arg");
}

#[test]
fn parse_tool_with_cap_expr() {
    let (root, errors) = parse(
        "@tool(\"d\", cap: fs.read(\"./data/**\")) fn read_doc(path: String) -> String { path }",
    );
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(count_tool_attrs(&root), 1);
    // The cap arg should hold a method-call shape inside.
    let cap_arg = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::TOOL_ATTR_CAP_ARG)
        .expect("cap arg present");
    // Inside the cap arg, the call expression must be reachable.
    let has_call = cap_arg.descendants().any(|n| {
        matches!(
            n.kind(),
            SyntaxKind::CALL_EXPR | SyntaxKind::METHOD_CALL_EXPR
        )
    });
    assert!(has_call, "cap expr should contain a call/method call");
}

#[test]
fn parse_tool_with_multiple_named_args() {
    let (root, errors) =
        parse("@tool(\"d\", cap: fs.read, streaming: true, name: \"rd\") fn read_doc() {}");
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(count_tool_attrs(&root), 1);
    // One cap arg + two NAMED_ARG children (streaming, name).
    let cap_args = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::TOOL_ATTR_CAP_ARG)
        .count();
    assert_eq!(cap_args, 1);
    // NAMED_ARG count INSIDE the TOOL_ATTR_ARGS only. NAMED_ARG can
    // also show up elsewhere (call sites), so scope to the attr.
    let args_node = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::TOOL_ATTR_ARGS)
        .expect("tool attr args present");
    let named_args = args_node
        .children()
        .filter(|n| n.kind() == SyntaxKind::NAMED_ARG)
        .count();
    assert_eq!(named_args, 2, "expected streaming + name as NAMED_ARG");
}

#[test]
fn parse_tool_on_non_fn_errors_clean() {
    // `@tool(...)` immediately preceding a `let` is invalid. The
    // parser still consumes the attribute, then surfaces MT1004 at the
    // attribute span. The `let` part is at file scope which by itself
    // isn't an item — but the point is the attribute-on-non-fn error.
    let (_root, errors) = parse("@tool(\"d\") struct S { x: I32 }");
    assert!(!errors.is_empty(), "expected an MT1004 diagnostic");
    let has_mt1004 = errors.iter().any(|e| e.code == 1004);
    assert!(has_mt1004, "expected MT1004 in errors, got: {:?}", errors);
}

#[test]
fn parse_unknown_attribute_errors_clean() {
    let (_root, errors) = parse("@bogus(\"d\") fn x() {}");
    assert!(!errors.is_empty(), "expected an MT1003 diagnostic");
    let has_mt1003 = errors.iter().any(|e| e.code == 1003);
    assert!(has_mt1003, "expected MT1003 in errors, got: {:?}", errors);
}

#[test]
fn parse_tool_with_full_signature_no_errors() {
    // End-to-end: the exact surface the v0.27 demo 08 expects.
    let src = "\
@tool(\"Read a text file from the corpus\", cap: fs.read(\"./data/**\"))
fn read_doc(path: String) -> Result[String, FsError] !{fs} {
  std.fs.read_to_string(path)
}
";
    let (root, errors) = parse(src);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(count_tool_attrs(&root), 1);
    // The fn decl must carry both its effect clause and its body.
    let fn_decl = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::FN_DECL)
        .expect("fn decl present");
    assert!(
        fn_decl
            .descendants()
            .any(|n| n.kind() == SyntaxKind::EFFECT_CLAUSE),
        "expected effect clause"
    );
    assert!(
        fn_decl.descendants().any(|n| n.kind() == SyntaxKind::BLOCK),
        "expected fn body block"
    );
}

#[test]
fn parse_tool_attr_pub_fn_works() {
    // `pub fn` must still be valid after the attribute.
    let (root, errors) = parse("@tool(\"d\") pub fn read_doc() {}");
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(count_tool_attrs(&root), 1);
    let fns: Vec<_> = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::FN_DECL)
        .collect();
    assert_eq!(fns.len(), 1);
}
