use super::{exprs, patterns, types, Parser};
use crate::SyntaxKind::*;

pub fn block(p: &mut Parser) {
    p.start_node(BLOCK);
    p.expect(L_BRACE);
    p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        if p.at(LET_KW) {
            let_stmt(p);
        } else if !exprs::can_start_expr(p.peek()) {
            p.error("expected statement or expression");
            p.bump_any();
        } else {
            p.start_node(EXPR_STMT);
            exprs::expr(p);
            p.eat(SEMI);
            p.finish_node();
        }
        p.skip_trivia();
    }
    p.expect(R_BRACE);
    p.finish_node();
    p.skip_trivia();
}

fn let_stmt(p: &mut Parser) {
    p.start_node(LET_STMT);
    p.bump(LET_KW);
    p.skip_trivia();
    // Optional `mut` makes the binding mutable.
    p.eat(MUT_KW);
    p.skip_trivia();
    patterns::pattern(p);
    if p.eat(COLON) {
        types::type_expr(p);
    }
    if p.eat(EQ) {
        exprs::expr(p);
    }
    p.eat(SEMI);
    p.finish_node();
    p.skip_trivia();
}

pub fn if_expr(p: &mut Parser) -> bool {
    p.start_node(IF_EXPR);
    p.bump(IF_KW);
    p.skip_trivia();
    // `if let Pattern = scrutinee { ... }` — optional leading let-binding.
    // The CST shape is the same IF_EXPR; HIR lowering branches on whether
    // a LET_KW token is present.
    if p.at(LET_KW) {
        p.bump(LET_KW);
        p.skip_trivia();
        patterns::pattern(p);
        p.expect(EQ);
        p.skip_trivia();
    }
    // Disable struct-literal parsing for the condition so `if x { ... }`
    // parses as condition + body, not as `x { ... }` struct expr.
    p.with_no_struct_literal(|p| {
        exprs::expr(p);
    });
    block(p);
    if p.eat(ELSE_KW) {
        if p.at(IF_KW) {
            if_expr(p);
        } else {
            block(p);
        }
    }
    p.finish_node();
    true
}

pub fn match_expr(p: &mut Parser) -> bool {
    p.start_node(MATCH_EXPR);
    p.bump(MATCH_KW);
    p.skip_trivia();
    // Scrutinee uses the no-struct-literal context for the same reason as
    // if/while/for: `match x { ... }` shouldn't parse as `x { ... }`.
    p.with_no_struct_literal(|p| {
        exprs::expr(p);
    });
    p.expect(L_BRACE);
    p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        let before = p.pos;
        match_arm(p);
        p.skip_trivia();
        // v0.9 non-progress guard (FUZZ_V0_9 audit): match_arm can stall
        // when pattern + expect(FAT_ARROW) both no-op on malformed input.
        if p.pos == before {
            p.error("unexpected token in match body");
            p.bump_any();
            p.skip_trivia();
        }
    }
    p.expect(R_BRACE);
    p.finish_node();
    true
}

fn match_arm(p: &mut Parser) {
    p.start_node(MATCH_ARM);
    patterns::pattern(p);
    if p.eat(IF_KW) {
        p.start_node(MATCH_GUARD);
        exprs::expr(p);
        p.finish_node();
    }
    p.expect(FAT_ARROW);
    p.skip_trivia();
    if p.at(L_BRACE) {
        block(p);
    } else {
        exprs::expr(p);
        p.eat(COMMA);
    }
    p.finish_node();
    p.skip_trivia();
}

pub fn for_expr(p: &mut Parser) -> bool {
    p.start_node(FOR_EXPR);
    p.bump(FOR_KW);
    p.skip_trivia();
    patterns::pattern(p);
    p.expect(IN_KW);
    p.skip_trivia();
    p.with_no_struct_literal(|p| {
        exprs::expr(p);
    });
    block(p);
    p.finish_node();
    true
}

pub fn while_expr(p: &mut Parser) -> bool {
    p.start_node(WHILE_EXPR);
    p.bump(WHILE_KW);
    p.skip_trivia();
    // `while let Pattern = scrutinee { ... }` — optional leading
    // let-binding, mirroring the `if let` shape from slice 2. The CST
    // node remains WHILE_EXPR; HIR lowering branches on whether a
    // LET_KW token is present.
    if p.at(LET_KW) {
        p.bump(LET_KW);
        p.skip_trivia();
        patterns::pattern(p);
        p.expect(EQ);
        p.skip_trivia();
    }
    p.with_no_struct_literal(|p| {
        exprs::expr(p);
    });
    block(p);
    p.finish_node();
    true
}

pub fn loop_expr(p: &mut Parser) -> bool {
    p.start_node(LOOP_EXPR);
    p.bump(LOOP_KW);
    p.skip_trivia();
    block(p);
    p.finish_node();
    true
}
