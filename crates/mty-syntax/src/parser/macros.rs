use super::{paths, Parser};
use crate::SyntaxKind::*;

/// `macro Name (Param (, Param)*)? => { opaque tokens }`
/// Macro bodies are opaque token sequences; the macro expander interprets them.
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

/// `proc macro Name(input: TokenStream) -> TokenStream { body }` — v0.5.
///
/// Parses but does not execute. The body is captured as opaque
/// brace-balanced tokens; the registry stores it as a procedural macro,
/// and call sites emit MT6006 until v0.6's sandboxed interpreter ships.
pub fn proc_macro_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, PROC_MACRO_DECL);
    // Consume `proc` (IDENT in lexer) then `macro` keyword.
    // We arrive here when the items dispatcher sees the `proc` IDENT.
    // Use bump_any for the `proc` token (it lexes as IDENT) and then
    // expect MACRO_KW.
    p.bump_any();
    p.skip_trivia();
    p.expect(MACRO_KW);
    p.skip_trivia();
    paths::name(p);
    p.skip_trivia();
    // Parameter list: `(input: TokenStream)` — single positional arg only.
    p.expect(L_PAREN);
    p.skip_trivia();
    if !p.at(R_PAREN) {
        // Capture the param IDENT (e.g. `input`) under NAME.
        paths::name(p);
        p.skip_trivia();
        // Optional `: TokenStream` annotation; we accept any type expr.
        if p.eat(COLON) {
            p.skip_trivia();
            super::types::type_expr(p);
            p.skip_trivia();
        }
        // Trailing comma allowed.
        let _ = p.eat(COMMA);
        p.skip_trivia();
    }
    p.expect(R_PAREN);
    p.skip_trivia();
    // Optional `-> TokenStream` return type.
    if p.eat(THIN_ARROW) {
        p.start_node(RET_TYPE);
        p.skip_trivia();
        super::types::type_expr(p);
        p.finish_node();
        p.skip_trivia();
    }
    // Body: opaque brace-balanced tokens.
    super::extern_::consume_brace_balanced(p);
    p.finish_node();
    p.skip_trivia();
}
