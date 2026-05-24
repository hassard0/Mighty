use super::{Parser, paths};
use crate::SyntaxKind::*;

pub fn item(p: &mut Parser) -> bool {
    p.skip_trivia();
    // Optional visibility prefix wraps the item that follows.
    let cp = p.checkpoint();
    if p.at(PUB_KW) {
        p.start_node(VISIBILITY);
        p.bump(PUB_KW);
        p.finish_node();
        p.skip_trivia();
    }
    match p.peek() {
        USE_KW => { use_decl(p, cp); true }
        MOD_KW => { mod_decl(p, cp); true }
        PACKAGE_KW => { package_decl(p, cp); true }
        _ => false,
    }
}

fn use_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, USE_DECL);
    p.bump(USE_KW);
    p.skip_trivia();
    paths::path(p);
    // Either `use X.{a, b}` or `use X as Y` or just `use X`
    // After path(), we're past whitespace. Check for `.{` or `as`.
    if p.at(DOT) && p.peek_n(1) == L_BRACE {
        p.bump(DOT);
        p.bump(L_BRACE);
        p.skip_trivia();
        loop {
            if p.at(R_BRACE) { break; }
            paths::name(p);
            if p.eat(AS_KW) { paths::name(p); }
            if !p.eat(COMMA) { break; }
        }
        p.expect(R_BRACE);
    } else if p.eat(AS_KW) {
        paths::name(p);
    }
    p.eat(SEMI);
    p.finish_node();
    p.skip_trivia();
}

fn mod_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, MOD_DECL);
    p.bump(MOD_KW);
    p.skip_trivia();
    paths::path(p);
    p.eat(SEMI);
    p.finish_node();
    p.skip_trivia();
}

fn package_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, PACKAGE_DECL);
    p.bump(PACKAGE_KW);
    p.skip_trivia();
    paths::path(p);
    p.eat(SEMI);
    p.finish_node();
    p.skip_trivia();
}
