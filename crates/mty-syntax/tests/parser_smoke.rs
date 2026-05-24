#[test]
fn empty_file_parses() {
    let r = mty_syntax::parse("");
    assert_eq!(r.errors.len(), 0);
    let root = mty_syntax::SyntaxNode::new_root(r.green);
    assert_eq!(root.kind(), mty_syntax::SyntaxKind::FILE);
}

#[test]
fn junk_input_does_not_panic() {
    // The stub `items::item` returns false, so the parse loop emits errors
    // and bumps tokens one at a time. Should terminate cleanly.
    let r = mty_syntax::parse("@@@???!!!");
    assert!(!r.errors.is_empty());
    let root = mty_syntax::SyntaxNode::new_root(r.green);
    assert_eq!(root.kind(), mty_syntax::SyntaxKind::FILE);
}
