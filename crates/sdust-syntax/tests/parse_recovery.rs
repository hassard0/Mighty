use sdust_syntax::{parse, SyntaxKind, SyntaxNode};

fn parse_ok_shape(src: &str) {
    let r = parse(src);
    let root = SyntaxNode::new_root(r.green);
    assert_eq!(root.kind(), SyntaxKind::FILE, "src: {:?}", src);
}

#[test]
fn empty_input() {
    parse_ok_shape("");
}

#[test]
fn whitespace_only() {
    parse_ok_shape("   \n\t\n  ");
}

#[test]
fn lone_keyword() {
    parse_ok_shape("fn");
}

#[test]
fn unterminated_string() {
    let r = parse(r#""hello"#);
    assert!(!r.errors.is_empty(), "unterminated string should error");
}

#[test]
fn unbalanced_brace() {
    let r = parse("fn main() {");
    assert!(!r.errors.is_empty(), "unbalanced brace should error");
}

#[test]
fn random_punct() {
    parse_ok_shape("@@@???!!!");
}

#[test]
fn agent_missing_brace() {
    let r = parse("agent X: Y { on Foo() ->");
    assert!(!r.errors.is_empty());
}

#[test]
fn extern_missing_body() {
    let r = parse("extern c {");
    assert!(!r.errors.is_empty());
}

#[test]
fn recovers_after_error() {
    // After a broken item, the parser should still parse the next item.
    let r = parse("fn broken( ;\nfn good() {}\n");
    let root = SyntaxNode::new_root(r.green);
    let fns: Vec<_> = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::FN_DECL)
        .collect();
    assert!(
        fns.len() >= 1,
        "should recover and parse at least the second fn"
    );
}

#[test]
fn deep_nesting_terminates() {
    // 100 nested parens — should not stack overflow on a modest depth.
    let src: String = std::iter::repeat('(').take(100).collect();
    let r = parse(&src);
    let root = SyntaxNode::new_root(r.green);
    assert_eq!(root.kind(), SyntaxKind::FILE);
    // Errors expected (unmatched parens). Just confirm it terminates.
    let _ = r.errors;
}
