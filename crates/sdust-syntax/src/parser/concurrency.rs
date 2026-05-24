use super::Parser;
use crate::SyntaxKind::{self, *};

/// Index of the next non-trivia token starting at `from`.
fn next_nontrivia_kind(p: &Parser, from: usize) -> SyntaxKind {
    let mut i = from;
    while i < p.tokens.len() {
        let k = p.tokens[i].kind;
        if !k.is_trivia() {
            return k;
        }
        i += 1;
    }
    SyntaxKind::EOF
}

/// arena Name (':' Expr | Block)
pub fn arena_block(p: &mut Parser) -> bool {
    p.start_node(ARENA_BLOCK);
    p.bump(ARENA_KW);
    p.skip_trivia();
    super::paths::name(p);
    p.skip_trivia();
    if p.eat(COLON) {
        super::exprs::expr(p);
    } else if p.at(L_BRACE) {
        super::stmts::block(p);
    } else {
        p.error("expected ':' or '{' after arena name");
    }
    p.finish_node();
    true
}

/// Disambiguate `task scope ...` from `task.<method>(...)`.
/// If next non-trivia after TASK_KW is DOT, treat `task` as an identifier
/// (emit PATH_EXPR(NAME_REF(task))) so the Pratt postfix loop in `expr_bp`
/// can consume `.method(...)`.
pub fn task_scope_or_call(p: &mut Parser) -> bool {
    if next_nontrivia_kind(p, p.pos + 1) == DOT {
        // Emit `task` as a single-segment PATH_EXPR.
        p.start_node(PATH_EXPR);
        p.start_node(PATH);
        p.start_node(PATH_SEGMENT);
        p.start_node(NAME_REF);
        p.bump(TASK_KW);
        p.finish_node();
        p.finish_node();
        p.finish_node();
        p.finish_node();
        return true;
    }
    p.start_node(TASK_SCOPE);
    p.bump(TASK_KW);
    p.skip_trivia();
    p.eat(SCOPE_KW);
    p.skip_trivia();
    if p.eat(AT) {
        super::exprs::expr(p);
    }
    if p.at(L_BRACE) {
        super::stmts::block(p);
    } else {
        p.error("expected '{' to start task scope body");
    }
    p.finish_node();
    true
}

/// budget '{' (Name Expr)+ '}' 'run' (Block | Expr)
pub fn budget_block(p: &mut Parser) -> bool {
    p.start_node(BUDGET_BLOCK);
    p.bump(BUDGET_KW);
    p.skip_trivia();
    p.expect(L_BRACE);
    p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        if !p.at(IDENT) {
            // Avoid infinite loops on garbage tokens inside the entries.
            p.error("expected budget entry name");
            p.bump_any();
            p.skip_trivia();
            continue;
        }
        p.start_node(BUDGET_ENTRY);
        super::paths::name(p);
        p.skip_trivia();
        super::exprs::expr(p);
        p.eat(SEMI);
        p.finish_node();
        p.skip_trivia();
    }
    p.expect(R_BRACE);
    p.skip_trivia();
    p.expect(RUN_KW);
    p.skip_trivia();
    if p.at(L_BRACE) {
        super::stmts::block(p);
    } else {
        super::exprs::expr(p);
    }
    p.finish_node();
    true
}

/// sandbox Name 'with' '{' (Path '=' Expr (',' | ';')?)+ '}' Block
pub fn sandbox_block(p: &mut Parser) -> bool {
    p.start_node(SANDBOX_BLOCK);
    p.bump(SANDBOX_KW);
    p.skip_trivia();
    super::paths::name(p);
    p.skip_trivia();
    p.expect(WITH_KW);
    p.skip_trivia();
    p.expect(L_BRACE);
    p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        if !p.at(IDENT) {
            p.error("expected sandbox entry path");
            p.bump_any();
            p.skip_trivia();
            continue;
        }
        p.start_node(SANDBOX_ENTRY);
        super::paths::path(p);
        p.skip_trivia();
        p.expect(EQ);
        p.skip_trivia();
        super::exprs::expr(p);
        // Trailing separator is optional and may be either ',' or ';'.
        if !p.eat(COMMA) {
            p.eat(SEMI);
        }
        p.finish_node();
        p.skip_trivia();
    }
    p.expect(R_BRACE);
    p.skip_trivia();
    if p.at(L_BRACE) {
        super::stmts::block(p);
    } else {
        p.error("expected '{' to start sandbox body");
    }
    p.finish_node();
    true
}
