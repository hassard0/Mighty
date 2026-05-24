use super::{paths, Parser};
use crate::SyntaxKind::*;

/// `macro Name (Param (, Param)*)? => { opaque tokens }`
/// Macro bodies are opaque token sequences; real macro expansion is deferred to v0.2.
pub fn macro_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, MACRO_DECL);
    p.bump(MACRO_KW);
    p.skip_trivia();
    paths::name(p);
    // Parameter list.
    p.expect(L_PAREN);
    p.skip_trivia();
    if !p.at(R_PAREN) {
        paths::name(p);
        while p.eat(COMMA) {
            if p.at(R_PAREN) {
                break;
            }
            paths::name(p);
        }
    }
    p.expect(R_PAREN);
    p.skip_trivia();
    p.expect(FAT_ARROW);
    p.skip_trivia();
    // Body: opaque brace-balanced tokens.
    super::extern_::consume_brace_balanced(p);
    p.finish_node();
    p.skip_trivia();
}
