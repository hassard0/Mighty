use super::{paths, Parser};
use crate::SyntaxKind::{self, *};

/// Look at the kind of the next non-trivia token after position `from`.
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

pub fn pattern(p: &mut Parser) -> bool {
    p.skip_trivia();
    let cp = p.checkpoint();
    let ok = match p.peek() {
        // Wildcard: `_` lexes as IDENT per our regex.
        IDENT if p.tokens[p.pos].text == "_" => {
            wildcard(p);
            true
        }
        // Path-headed: struct/enum disambiguation by lookahead past trivia.
        IDENT if matches!(next_nontrivia_kind(p, p.pos + 1), L_PAREN | L_BRACE | DOT) => {
            path_headed(p);
            true
        }
        IDENT => {
            binding(p);
            true
        }
        INT_LITERAL | FLOAT_LITERAL | STRING_LITERAL | CHAR_LITERAL | TRUE_KW | FALSE_KW => {
            literal(p);
            true
        }
        AMP => {
            ref_pat(p);
            true
        }
        L_PAREN => {
            tuple(p);
            true
        }
        _ => false,
    };
    if !ok {
        return false;
    }
    if p.at(DOT_DOT) || p.at(DOT_DOT_EQ) {
        p.start_node_at(cp, RANGE_PAT);
        p.bump_any();
        p.skip_trivia();
        pattern(p);
        p.finish_node();
    }
    true
}

fn literal(p: &mut Parser) {
    p.start_node(LITERAL_PAT);
    p.bump_any();
    p.finish_node();
    p.skip_trivia();
}

fn binding(p: &mut Parser) {
    p.start_node(BINDING_PAT);
    paths::name(p);
    if p.eat(AT) {
        pattern(p);
    }
    p.finish_node();
    p.skip_trivia();
}

fn wildcard(p: &mut Parser) {
    p.start_node(WILDCARD_PAT);
    p.bump(IDENT); // `_` lexes as IDENT
    p.finish_node();
    p.skip_trivia();
}

fn ref_pat(p: &mut Parser) {
    p.start_node(REF_PAT);
    p.bump(AMP);
    p.skip_trivia();
    p.eat(MUT_KW);
    pattern(p);
    p.finish_node();
    p.skip_trivia();
}

fn tuple(p: &mut Parser) {
    p.start_node(TUPLE_PAT);
    p.bump(L_PAREN);
    p.skip_trivia();
    if !p.at(R_PAREN) {
        pattern(p);
        while p.eat(COMMA) {
            if p.at(R_PAREN) {
                break;
            }
            pattern(p);
        }
    }
    p.expect(R_PAREN);
    p.finish_node();
    p.skip_trivia();
}

fn path_headed(p: &mut Parser) {
    let cp = p.checkpoint();
    paths::path(p);
    p.skip_trivia();
    if p.eat(L_PAREN) {
        p.start_node_at(cp, ENUM_PAT);
        if !p.at(R_PAREN) {
            pattern(p);
            while p.eat(COMMA) {
                if p.at(R_PAREN) {
                    break;
                }
                pattern(p);
            }
        }
        p.expect(R_PAREN);
        p.finish_node();
    } else if p.eat(L_BRACE) {
        p.start_node_at(cp, STRUCT_PAT);
        if !p.at(R_BRACE) {
            field_pat(p);
            while p.eat(COMMA) {
                if p.at(R_BRACE) {
                    break;
                }
                field_pat(p);
            }
        }
        p.expect(R_BRACE);
        p.finish_node();
    } else {
        // Path used as a unit variant pattern; wrap as ENUM_PAT with no args.
        p.start_node_at(cp, ENUM_PAT);
        p.finish_node();
    }
    p.skip_trivia();
}

fn field_pat(p: &mut Parser) {
    p.skip_trivia();
    paths::name(p);
    if p.eat(COLON) {
        pattern(p);
    }
}
