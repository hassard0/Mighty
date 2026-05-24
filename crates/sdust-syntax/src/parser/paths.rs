use super::Parser;
use crate::SyntaxKind::*;

pub fn path(p: &mut Parser) -> bool {
    if !p.at(IDENT) {
        return false;
    }
    p.start_node(PATH);
    p.start_node(PATH_SEGMENT);
    p.start_node(NAME_REF);
    p.bump(IDENT);
    p.finish_node();
    p.finish_node();
    p.skip_trivia();
    while p.at(DOT) && p.peek_n(1) == IDENT {
        p.bump(DOT);
        p.skip_trivia();
        p.start_node(PATH_SEGMENT);
        p.start_node(NAME_REF);
        p.bump(IDENT);
        p.finish_node();
        p.finish_node();
        p.skip_trivia();
    }
    p.finish_node();
    true
}

pub fn name(p: &mut Parser) -> bool {
    if !p.at(IDENT) {
        return false;
    }
    p.start_node(NAME);
    p.bump(IDENT);
    p.finish_node();
    p.skip_trivia();
    true
}
