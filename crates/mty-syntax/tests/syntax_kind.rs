use mty_syntax::SyntaxKind;

#[test]
fn keywords_classify() {
    assert!(SyntaxKind::AGENT_KW.is_keyword());
    assert!(SyntaxKind::FN_KW.is_keyword());
    assert!(!SyntaxKind::IDENT.is_keyword());
}

#[test]
fn trivia_classify() {
    assert!(SyntaxKind::WHITESPACE.is_trivia());
    assert!(SyntaxKind::LINE_COMMENT.is_trivia());
    assert!(!SyntaxKind::IDENT.is_trivia());
}

#[test]
fn rowan_conversion() {
    let rk: rowan::SyntaxKind = SyntaxKind::FN_KW.into();
    assert_eq!(rk.0, SyntaxKind::FN_KW as u16);
}
