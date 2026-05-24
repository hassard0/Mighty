use super::{exprs, paths, stmts, types, Parser};
use crate::SyntaxKind::{self, *};

/// Kind of the next non-trivia token at offset `offset` from current `pos`.
fn next_nontrivia_kind(p: &Parser, offset: usize) -> SyntaxKind {
    let mut i = p.pos + offset;
    while i < p.tokens.len() && p.tokens[i].kind.is_trivia() {
        i += 1;
    }
    p.tokens.get(i).map(|t| t.kind).unwrap_or(SyntaxKind::EOF)
}

pub fn agent_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, AGENT_DECL);
    p.bump(AGENT_KW);
    p.skip_trivia();
    paths::name(p);
    if p.at(L_PAREN) {
        ctor_params(p);
    }
    if p.eat(COLON) {
        p.start_node(AGENT_PROTOCOL_LIST);
        types::type_expr(p);
        while p.eat(PLUS) {
            types::type_expr(p);
        }
        p.finish_node();
    }
    p.expect(L_BRACE);
    p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        agent_member(p);
        p.skip_trivia();
    }
    p.expect(R_BRACE);
    p.finish_node();
    p.skip_trivia();
}

fn ctor_params(p: &mut Parser) {
    p.start_node(AGENT_CTOR_PARAMS);
    p.bump(L_PAREN);
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
    p.finish_node();
    p.skip_trivia();
}

fn agent_member(p: &mut Parser) {
    let cp = p.checkpoint();
    if p.at(ON_KW) {
        on_handler(p, cp);
        return;
    }
    if p.at(FN_KW) || (p.at(UNSAFE_KW) && next_nontrivia_kind(p, 1) == FN_KW) {
        super::items::fn_decl_pub(p, cp);
        return;
    }
    if p.at(STATE_KW)
        || (p.at(IDENT)
            && (next_nontrivia_kind(p, 1) == EQ || next_nontrivia_kind(p, 1) == COLON))
    {
        state_decl(p, cp);
        return;
    }
    p.error("expected agent member (`on`, `fn`, or state field)");
    p.bump_any();
}

fn state_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, AGENT_STATE_DECL);
    p.eat(STATE_KW);
    paths::name(p);
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

fn on_handler(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, ON_HANDLER);
    p.bump(ON_KW);
    p.skip_trivia();
    paths::name(p);
    if p.eat(L_PAREN) {
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
    }
    p.skip_trivia();
    if p.eat(THIN_ARROW) {
        exprs::expr(p);
    } else if p.at(L_BRACE) {
        stmts::block(p);
    }
    p.finish_node();
    p.skip_trivia();
}

pub fn protocol_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, PROTOCOL_DECL);
    p.bump(PROTOCOL_KW);
    p.skip_trivia();
    paths::name(p);
    // optional generic parameters: `protocol Stream[T] { ... }`
    super::types::generic_params(p);
    // optional version tag: contextual `v\d+` IDENT
    if p.at(IDENT) {
        let text = p.tokens[p.pos].text;
        if text.starts_with('v') && text.len() > 1 && text[1..].chars().all(|c| c.is_ascii_digit())
        {
            paths::name(p);
        }
    }
    if p.eat(EQ) {
        // composition: protocol Web = Fetch + Cache + Health
        types::type_expr(p);
        while p.eat(PLUS) {
            types::type_expr(p);
        }
    } else if p.at(L_BRACE) {
        p.bump(L_BRACE);
        p.skip_trivia();
        while !p.at(R_BRACE) && !p.at(EOF) {
            protocol_msg(p);
            p.skip_trivia();
        }
        p.expect(R_BRACE);
    }
    p.finish_node();
    p.skip_trivia();
}

fn protocol_msg(p: &mut Parser) {
    p.start_node(PROTOCOL_MSG);
    paths::name(p);
    p.expect(L_PAREN);
    if !p.at(R_PAREN) {
        proto_param(p);
        while p.eat(COMMA) {
            if p.at(R_PAREN) {
                break;
            }
            proto_param(p);
        }
    }
    p.expect(R_PAREN);
    if p.eat(THIN_ARROW) {
        types::type_expr(p);
    }
    p.finish_node();
    p.skip_trivia();
}

fn proto_param(p: &mut Parser) {
    p.start_node(FN_PARAM);
    paths::name(p);
    if p.eat(COLON) {
        types::type_expr(p);
    }
    p.finish_node();
    p.skip_trivia();
}

pub fn supervisor_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, SUPERVISOR_DECL);
    // either `supervisor Name(...)` (IDENT) or `sup Name ...` (SUP_KW)
    if p.at(SUP_KW) {
        p.bump(SUP_KW);
    } else {
        // contextual "supervisor" IDENT
        p.bump_any();
    }
    p.skip_trivia();
    paths::name(p);
    // optional strategy or constructor-style args; allow named args like `strategy: one_for_one`
    if p.eat(L_PAREN) {
        if !p.at(R_PAREN) {
            sup_arg(p);
            while p.eat(COMMA) {
                if p.at(R_PAREN) {
                    break;
                }
                sup_arg(p);
            }
        }
        p.expect(R_PAREN);
    } else if p.at(IDENT) {
        paths::name(p);
    }
    p.expect(L_BRACE);
    p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        sup_body(p);
        p.skip_trivia();
    }
    p.expect(R_BRACE);
    p.finish_node();
    p.skip_trivia();
}

fn sup_body(p: &mut Parser) {
    if p.at(ON_FAIL_KW) {
        p.start_node(ON_FAIL_CLAUSE);
        p.bump(ON_FAIL_KW);
        p.skip_trivia();
        p.expect(L_PAREN);
        paths::name(p);
        p.expect(R_PAREN);
        p.expect(L_BRACE);
        p.skip_trivia();
        while !p.at(R_BRACE) && !p.at(EOF) {
            sup_action(p);
            p.eat(SEMI);
            p.skip_trivia();
        }
        p.expect(R_BRACE);
        p.finish_node();
        p.skip_trivia();
        return;
    }
    p.start_node(SUP_CHILD);
    p.eat(CHILD_KW);
    paths::name(p);
    p.expect(EQ);
    exprs::expr(p);
    p.eat(SEMI);
    p.finish_node();
    p.skip_trivia();
}

fn sup_arg(p: &mut Parser) {
    // Accept `name: expr` (named) or bare `expr` (positional).
    if p.at(IDENT) && next_nontrivia_kind(p, 1) == COLON {
        p.start_node(NAMED_ARG);
        paths::name(p);
        p.bump(COLON);
        p.skip_trivia();
        exprs::expr(p);
        p.finish_node();
    } else {
        p.start_node(ARG);
        exprs::expr(p);
        p.finish_node();
    }
    p.skip_trivia();
}

fn sup_action(p: &mut Parser) {
    // restart [up_to N in DUR] | backoff DUR..DUR
    if p.eat(RESTART_KW) {
        if p.eat(UP_TO_KW) {
            exprs::expr(p);
            p.expect(IN_KW);
            exprs::expr(p);
        }
    } else if p.eat(BACKOFF_KW) {
        // backoff D1..D2 — single range expression via expr (handles DOT_DOT)
        exprs::expr(p);
    } else {
        p.error("expected restart or backoff");
        p.bump_any();
    }
}
