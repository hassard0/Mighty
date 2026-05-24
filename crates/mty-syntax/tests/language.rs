use rowan::GreenNodeBuilder;
use mty_syntax::{SyntaxKind, SyntaxNode};

#[test]
fn build_minimal_tree() {
    let mut b = GreenNodeBuilder::new();
    b.start_node(SyntaxKind::FILE.into());
    b.token(SyntaxKind::FN_KW.into(), "fn");
    b.token(SyntaxKind::WHITESPACE.into(), " ");
    b.token(SyntaxKind::IDENT.into(), "main");
    b.finish_node();
    let green = b.finish();
    let root = SyntaxNode::new_root(green);
    assert_eq!(root.kind(), SyntaxKind::FILE);
    assert_eq!(root.text().to_string(), "fn main");
}
