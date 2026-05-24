use super::Parser;
use crate::SyntaxKind::*;

pub fn path(p: &mut Parser) -> bool {
    if !p.at(IDENT) {
        return false;
    }
    p.start_node(PATH);
    segment(p);
    p.skip_trivia();
    while p.at(DOT) && p.peek_n(1) == IDENT {
        p.bump(DOT);
        p.skip_trivia();
        segment(p);
        p.skip_trivia();
    }
    p.finish_node();
    true
}

/// One path segment: `IDENT` optionally followed by a turbofish
/// `::[T1, T2]` generic-args list. The `::` disambiguates from
/// `IDENT[index]` (index expression).
fn segment(p: &mut Parser) {
    p.start_node(PATH_SEGMENT);
    p.start_node(NAME_REF);
    p.bump(IDENT);
    p.finish_node();
    if p.at(COLON_COLON) && p.peek_n(1) == L_BRACK {
        p.bump(COLON_COLON);
        super::types::generic_args(p);
    }
    p.finish_node();
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

/// Like [`name`], but also accepts a keyword token in name position.
/// Used after `.` for keyword-tolerant method/field names and inside
/// `effect` clauses where reserved words (e.g. `spawn`) can appear.
pub fn name_or_keyword(p: &mut Parser) -> bool {
    let k = p.peek();
    if k != IDENT && !k.is_keyword() {
        return false;
    }
    p.start_node(NAME);
    p.bump_any();
    p.finish_node();
    p.skip_trivia();
    true
}
