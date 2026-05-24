use super::{paths, Parser};
use crate::SyntaxKind::*;

/// `extern (Name)? { ExternFn* }` — FFI / WASM imports.
/// Optional `Name` is the ABI tag ("c", "js", etc.) and is parsed as an IDENT.
pub fn extern_block(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, EXTERN_BLOCK);
    p.bump(EXTERN_KW);
    p.skip_trivia();
    // Optional ABI name (`extern c { ... }`, `extern js { ... }`).
    if p.at(IDENT) {
        paths::name(p);
    }
    p.expect(L_BRACE);
    p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        extern_fn(p);
        p.skip_trivia();
    }
    p.expect(R_BRACE);
    p.finish_node();
    p.skip_trivia();
}

/// `fn Name FnParams ('->' Type)? EffectClause? ';'?` — extern function signature only.
fn extern_fn(p: &mut Parser) {
    p.start_node(EXTERN_FN);
    p.expect(FN_KW);
    p.skip_trivia();
    paths::name(p);
    super::items::fn_params(p);
    if p.eat(THIN_ARROW) {
        p.start_node(RET_TYPE);
        super::types::type_expr(p);
        p.finish_node();
        p.skip_trivia();
    }
    super::types::effect_clause(p);
    p.eat(SEMI);
    p.finish_node();
    p.skip_trivia();
}

/// `export (Name)? (FnDecl | ComponentDecl)`
/// `Name` is the target ABI tag for the export ("c", "js", etc.).
/// ComponentDecl is library-lowered; its body is consumed brace-balanced.
pub fn export_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, EXPORT_DECL);
    p.bump(EXPORT_KW);
    p.skip_trivia();
    // Optional ABI tag: `export c fn ...`, `export js fn ...`.
    // The tag must NOT be `fn` (that's the function form) and must NOT be
    // `component` (contextual keyword for the component form).
    if p.at(IDENT) && p.tokens[p.pos].text != "component" {
        paths::name(p);
    }
    if p.at(FN_KW) || (p.at(UNSAFE_KW) && next_nontrivia_kind(p, p.pos + 1) == FN_KW) {
        // Reuse the full fn_decl_pub pipeline for the function form.
        let icp = p.checkpoint();
        super::items::fn_decl_pub(p, icp);
    } else if p.at(IDENT) && p.tokens[p.pos].text == "component" {
        component_decl(p);
    } else {
        p.error("expected `fn` or `component` after `export`");
    }
    p.finish_node();
    p.skip_trivia();
}

/// `component Name { ... opaque tokens ... }`
/// Contextual keyword; body is consumed brace-balanced and left as opaque tokens.
fn component_decl(p: &mut Parser) {
    // Consume the contextual `component` keyword as a NAME-like token.
    // It's just an IDENT to the lexer; bump it so it shows up in the CST.
    p.bump_any();
    p.skip_trivia();
    if p.at(IDENT) {
        paths::name(p);
    }
    if p.at(L_BRACE) {
        consume_brace_balanced(p);
    }
}

/// Consume `{ ... }` as opaque tokens, tracking brace depth.
pub(super) fn consume_brace_balanced(p: &mut Parser) {
    if !p.at(L_BRACE) {
        return;
    }
    p.bump(L_BRACE);
    let mut depth: usize = 1;
    while depth > 0 && !p.at(EOF) {
        match p.peek() {
            L_BRACE => {
                depth += 1;
                p.bump_any();
            }
            R_BRACE => {
                depth -= 1;
                p.bump_any();
            }
            _ => p.bump_any(),
        }
    }
    p.skip_trivia();
}

fn next_nontrivia_kind(p: &Parser, from: usize) -> crate::SyntaxKind {
    let mut i = from;
    while i < p.tokens.len() && p.tokens[i].kind.is_trivia() {
        i += 1;
    }
    p.tokens
        .get(i)
        .map(|t| t.kind)
        .unwrap_or(crate::SyntaxKind::EOF)
}
