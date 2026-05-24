use super::Parser;
use crate::SyntaxKind::*;

/// `unsafe { ... }` — wraps a Block inside an UNSAFE_BLOCK node.
/// Used in expression position. `unsafe fn` is handled separately by items::fn_decl_pub.
pub fn unsafe_block(p: &mut Parser) -> bool {
    if !p.at(UNSAFE_KW) {
        return false;
    }
    p.start_node(UNSAFE_BLOCK);
    p.bump(UNSAFE_KW);
    p.skip_trivia();
    if p.at(L_BRACE) {
        super::stmts::block(p);
    } else {
        p.error("expected `{` after `unsafe`");
    }
    p.finish_node();
    p.skip_trivia();
    true
}
