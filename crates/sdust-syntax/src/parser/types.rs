use super::{Parser, paths};
use crate::SyntaxKind::*;

pub fn type_expr(p: &mut Parser) -> bool {
    p.skip_trivia();
    match p.peek() {
        AMP => borrow(p),
        STAR => ptr(p),
        L_PAREN => tuple(p),
        L_BRACK => array(p),
        FN_KW => fn_type(p),
        IDENT => path_type(p),
        _ => return false,
    }
    true
}

fn ptr(p: &mut Parser) {
    // Raw-pointer-style type `*T`. Slice-1 doesn't enforce pointer vs borrow
    // semantics, so reuse TYPE_BORROW as the CST shape.
    p.start_node(TYPE_BORROW);
    p.bump(STAR);
    p.skip_trivia();
    type_expr(p);
    p.finish_node();
    p.skip_trivia();
}

fn borrow(p: &mut Parser) {
    p.start_node(TYPE_BORROW);
    p.bump(AMP);
    p.skip_trivia();
    p.eat(MUT_KW);
    type_expr(p);
    p.finish_node();
    p.skip_trivia();
}

fn tuple(p: &mut Parser) {
    p.start_node(TYPE_TUPLE);
    p.bump(L_PAREN);
    p.skip_trivia();
    if !p.at(R_PAREN) {
        type_expr(p);
        while p.eat(COMMA) {
            if p.at(R_PAREN) { break; }
            type_expr(p);
        }
    }
    p.expect(R_PAREN);
    p.finish_node();
    p.skip_trivia();
}

fn array(p: &mut Parser) {
    p.start_node(TYPE_ARRAY);
    p.bump(L_BRACK);
    p.skip_trivia();
    type_expr(p);
    if p.eat(SEMI) {
        super::exprs::expr(p);
    }
    p.expect(R_BRACK);
    p.finish_node();
    p.skip_trivia();
}

fn fn_type(p: &mut Parser) {
    p.start_node(TYPE_FN);
    p.bump(FN_KW);
    p.skip_trivia();
    p.expect(L_PAREN);
    if !p.at(R_PAREN) {
        type_expr(p);
        while p.eat(COMMA) { if p.at(R_PAREN) { break; } type_expr(p); }
    }
    p.expect(R_PAREN);
    if p.eat(THIN_ARROW) { type_expr(p); }
    p.finish_node();
    p.skip_trivia();
}

fn path_type(p: &mut Parser) {
    let cp = p.checkpoint();
    p.start_node(TYPE_PATH);
    paths::path(p);
    if p.at(L_BRACK) { generic_args(p); }
    p.finish_node();
    // Result sugar wraps the path-type node.
    if p.at(BANG) {
        p.start_node_at(cp, TYPE_RESULT_SUGAR);
        p.bump(BANG);
        p.skip_trivia();
        if p.eat(L_BRACE) {
            p.start_node(TYPE_UNION);
            type_expr(p);
            while p.eat(COMMA) { if p.at(R_BRACE) { break; } type_expr(p); }
            p.expect(R_BRACE);
            p.finish_node();
        } else {
            type_expr(p);
        }
        p.finish_node();
    }
    p.skip_trivia();
}

pub fn generic_args(p: &mut Parser) {
    p.start_node(GENERIC_ARG_LIST);
    p.bump(L_BRACK);
    p.skip_trivia();
    if !p.at(R_BRACK) {
        p.start_node(GENERIC_ARG);
        type_expr(p);
        p.finish_node();
        while p.eat(COMMA) {
            if p.at(R_BRACK) { break; }
            p.start_node(GENERIC_ARG);
            type_expr(p);
            p.finish_node();
        }
    }
    p.expect(R_BRACK);
    p.finish_node();
    p.skip_trivia();
}

pub fn generic_params(p: &mut Parser) {
    if !p.at(L_BRACK) { return; }
    p.start_node(GENERIC_PARAM_LIST);
    p.bump(L_BRACK);
    p.skip_trivia();
    if !p.at(R_BRACK) {
        param(p);
        while p.eat(COMMA) { if p.at(R_BRACK) { break; } param(p); }
    }
    p.expect(R_BRACK);
    p.finish_node();
    p.skip_trivia();

    fn param(p: &mut Parser) {
        p.start_node(GENERIC_PARAM);
        paths::name(p);
        if p.eat(COLON) {
            type_expr(p);
            while p.eat(PLUS) { type_expr(p); }
        }
        p.finish_node();
        p.skip_trivia();
    }
}

pub fn effect_clause(p: &mut Parser) {
    if !p.at(EFFECT_KW) { return; }
    p.start_node(EFFECT_CLAUSE);
    p.bump(EFFECT_KW);
    p.skip_trivia();
    paths::name(p);
    while p.eat(COMMA) { paths::name(p); }
    p.finish_node();
    p.skip_trivia();
}
