use super::{paths, Parser};
use crate::SyntaxKind::*;

pub fn type_expr(p: &mut Parser) -> bool {
    p.skip_trivia();
    match p.peek() {
        AMP => borrow(p),
        STAR => ptr(p),
        L_PAREN => tuple(p),
        L_BRACK => array(p),
        FN_KW => fn_type(p),
        DYN_KW => dyn_type(p),
        IDENT => path_type(p),
        _ => return false,
    }
    true
}

fn dyn_type(p: &mut Parser) {
    // `dyn Trait` — slice-5. We accept `dyn IDENT` only (no generic args
    // on the trait — slice-5 doesn't model that).
    p.start_node(TYPE_DYN);
    p.bump(DYN_KW);
    p.skip_trivia();
    if p.at(IDENT) {
        paths::path(p);
    } else {
        p.error("expected trait name after `dyn`");
    }
    p.finish_node();
    p.skip_trivia();
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
            if p.at(R_PAREN) {
                break;
            }
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
        while p.eat(COMMA) {
            if p.at(R_PAREN) {
                break;
            }
            type_expr(p);
        }
    }
    p.expect(R_PAREN);
    if p.eat(THIN_ARROW) {
        type_expr(p);
    }
    p.finish_node();
    p.skip_trivia();
}

fn path_type(p: &mut Parser) {
    let cp = p.checkpoint();
    p.start_node(TYPE_PATH);
    paths::path(p);
    if p.at(L_BRACK) {
        generic_args(p);
    }
    p.finish_node();
    // Result sugar wraps the path-type node.
    //
    // v0.15 RFC-008 effect-row disambiguation: `T!{ ... }` is *either* an
    // anonymous error union (existing behaviour, A11) *or* an effect-row
    // clause introducer (new). We pick based on the brace body:
    //   * contains `|` at depth 0 → effect clause (RFC-008 `!{a | E}` /
    //     `!{a, b | E}`); leave the `!` for `effect_clause` to consume.
    //   * first ident is lowercase (or a keyword like `spawn`) → effect
    //     clause (`!{fs}`, `!{fs, net}`).
    //   * else (first ident is uppercase, e.g. `NetErr`) → legacy error
    //     sugar.
    //
    // Bare `T!IDENT` (no braces) stays error sugar regardless of case —
    // `!FetchErr` is a widely-used form. Users who want a row var on a
    // return type with no error sugar can write `() !E`, which doesn't
    // hit this path (`()` is TYPE_TUPLE), or `Result[T, Err] !E` (same).
    if p.at(BANG) && peeks_as_effect_row_clause(p) {
        // Defer entirely to the outer EffectClause parser.
        p.skip_trivia();
        return;
    }
    if p.at(BANG) {
        p.start_node_at(cp, TYPE_RESULT_SUGAR);
        p.bump(BANG);
        p.skip_trivia();
        if p.eat(L_BRACE) {
            p.start_node(TYPE_UNION);
            type_expr(p);
            while p.eat(COMMA) {
                if p.at(R_BRACE) {
                    break;
                }
                type_expr(p);
            }
            p.expect(R_BRACE);
            p.finish_node();
        } else {
            type_expr(p);
        }
        p.finish_node();
    }
    p.skip_trivia();
}

/// Look ahead at a `!` that immediately follows a path-type and decide
/// whether the body is an RFC-008 effect-row clause (`!{a | E}`,
/// `!{fs, net}`) rather than the legacy anonymous-error-union sugar
/// (`!{NetErr, ParseErr}`). Returns true iff the `!` should be left for
/// the outer `effect_clause` parser to consume.
///
/// Bare `!IDENT` (no braces) is ALWAYS treated as error sugar to
/// preserve back-compat with `!FetchErr` etc.; row vars in that
/// position must be written `() !E` so the type side sees `()` and
/// `effect_clause` sees the `!E`.
fn peeks_as_effect_row_clause(p: &Parser) -> bool {
    debug_assert!(p.at(BANG));
    // Skip past the BANG and any trivia.
    let mut i = p.pos + 1;
    while i < p.tokens.len() && p.tokens[i].kind.is_trivia() {
        i += 1;
    }
    if i >= p.tokens.len() || p.tokens[i].kind != L_BRACE {
        // Bare `!IDENT` form — stay legacy error sugar.
        return false;
    }
    // Walk the brace body. If we see `|` at depth 0, it's an effect
    // clause. Otherwise inspect the first ident's case.
    i += 1;
    let mut depth: i32 = 0;
    let mut first_ident_lower: Option<bool> = None;
    while i < p.tokens.len() {
        let t = &p.tokens[i];
        match t.kind {
            L_BRACE | L_PAREN | L_BRACK => depth += 1,
            R_BRACE if depth == 0 => break,
            R_BRACE | R_PAREN | R_BRACK => depth -= 1,
            PIPE if depth == 0 => return true,
            IDENT if depth == 0 && first_ident_lower.is_none() => {
                let first = t.text.chars().next();
                first_ident_lower = Some(first.is_some_and(|c| c.is_lowercase() || c == '_'));
            }
            // Keywords like `spawn` are valid effect names per A4 — treat
            // as lowercase if they appear first.
            k if depth == 0 && first_ident_lower.is_none() && k.is_keyword() => {
                first_ident_lower = Some(true);
            }
            _ => {}
        }
        if t.kind == EOF {
            break;
        }
        i += 1;
    }
    first_ident_lower.unwrap_or(false)
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
            if p.at(R_BRACK) {
                break;
            }
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
    if !p.at(L_BRACK) {
        return;
    }
    p.start_node(GENERIC_PARAM_LIST);
    p.bump(L_BRACK);
    p.skip_trivia();
    if !p.at(R_BRACK) {
        param(p);
        while p.eat(COMMA) {
            if p.at(R_BRACK) {
                break;
            }
            param(p);
        }
    }
    p.expect(R_BRACK);
    p.finish_node();
    p.skip_trivia();

    fn param(p: &mut Parser) {
        p.start_node(GENERIC_PARAM);
        paths::name(p);
        if p.eat(COLON) {
            type_expr(p);
            while p.eat(PLUS) {
                type_expr(p);
            }
        }
        p.finish_node();
        p.skip_trivia();
    }
}

pub fn effect_clause(p: &mut Parser) {
    if p.at(EFFECT_KW) {
        effect_clause_keyword(p);
        return;
    }
    if p.at(BANG) {
        effect_clause_bang(p);
    }
}

/// Legacy v1.0 form: `effect a, b, c`. v0.15 extends it with an
/// optional `| RowVar` tail so a row-poly fn can keep the keyword
/// shape if desired: `effect fs, net | E`.
///
/// CST stability: the concrete-effect names are emitted as bare NAME
/// nodes (NOT wrapped in EFFECT_NAME) so the v0.14 mty-hir lowerer
/// (`lower::items::lower_fn`) — which iterates `EFFECT_CLAUSE`
/// children and `Name::cast`s them — keeps working unchanged. The new
/// `!{...}` form below uses EFFECT_NAME because it's a fresh shape
/// the v0.16 lowerer will walk explicitly.
fn effect_clause_keyword(p: &mut Parser) {
    p.start_node(EFFECT_CLAUSE);
    p.bump(EFFECT_KW);
    p.skip_trivia();
    // Allow keyword names so `effect net, model, spawn` parses (spec §10
    // doesn't restrict effect names to non-reserved words).
    if !p.at(PIPE) {
        paths::name_or_keyword(p);
        while p.eat(COMMA) {
            if p.at(PIPE) {
                break;
            }
            paths::name_or_keyword(p);
        }
    }
    // v0.15 RFC-008: optional `| RowVar` tail on the keyword form.
    if p.at(PIPE) {
        effect_row_tail(p);
    }
    p.finish_node();
    p.skip_trivia();
}

/// v0.15 RFC-008 form: `!{...}` or `!E` after the return type.
///
/// Accepts (per RFC-008 §Syntax):
///   * `!E`               — bare row var
///   * `!{}`              — empty closed row
///   * `!{a, b}`          — concrete closed row
///   * `!{| E}`           — row var only, braced
///   * `!{a, b | E}`      — concrete + row tail
///
/// The `!{a | b}` and `!{Foo, Bar}` disambiguation lives in
/// `peeks_as_effect_row_clause` above — by the time we get here we
/// know this `!` is an effect clause, not error sugar.
fn effect_clause_bang(p: &mut Parser) {
    p.start_node(EFFECT_CLAUSE);
    p.bump(BANG);
    p.skip_trivia();
    if p.at(L_BRACE) {
        p.start_node(EFFECT_SET);
        p.bump(L_BRACE);
        p.skip_trivia();
        // Concrete effects: optional, comma-separated, terminated by `|`
        // or `}`.
        if !p.at(R_BRACE) && !p.at(PIPE) {
            effect_name(p);
            while p.eat(COMMA) {
                if p.at(R_BRACE) || p.at(PIPE) {
                    break;
                }
                effect_name(p);
            }
        }
        if p.at(PIPE) {
            effect_row_tail(p);
        }
        p.expect(R_BRACE);
        p.finish_node();
    } else if p.at(IDENT) || p.peek().is_keyword() {
        // Bare `!E` — single row variable, no braces. v0.15 parser
        // doesn't enforce the uppercase convention (that's MT4023 in
        // v0.16); we emit EFFECT_ROW_VAR unconditionally.
        p.start_node(EFFECT_ROW_VAR);
        paths::name_or_keyword(p);
        p.finish_node();
        p.skip_trivia();
    } else {
        p.error("expected `{` or row variable after `!` in effect clause");
    }
    p.finish_node();
    p.skip_trivia();
}

/// One concrete effect name inside an EFFECT_SET or after the legacy
/// `effect` keyword. Wraps the identifier in an EFFECT_NAME node so
/// the v0.16 lowerer can distinguish concrete effect identifiers from
/// generic NAME tokens elsewhere.
fn effect_name(p: &mut Parser) {
    p.start_node(EFFECT_NAME);
    paths::name_or_keyword(p);
    p.finish_node();
    p.skip_trivia();
}

/// Parse `| RowVar` and emit an EFFECT_ROW_TAIL node. Assumes
/// `p.at(PIPE)`.
fn effect_row_tail(p: &mut Parser) {
    p.start_node(EFFECT_ROW_TAIL);
    p.bump(PIPE);
    p.skip_trivia();
    if p.at(IDENT) || p.peek().is_keyword() {
        p.start_node(EFFECT_ROW_VAR);
        paths::name_or_keyword(p);
        p.finish_node();
        p.skip_trivia();
    } else {
        p.error("expected row variable identifier after `|`");
    }
    p.finish_node();
    p.skip_trivia();
}
