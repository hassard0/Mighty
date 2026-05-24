use super::{paths, Parser};
use crate::SyntaxKind::{self, *};

pub fn item(p: &mut Parser) -> bool {
    p.skip_trivia();
    // Slice 5: optional attributes/derive prefixes.
    let cp = p.checkpoint();
    // `#[derive(...)]` attributes or `derive Copy` keyword shorthand.
    while p.at(HASH) || p.at(DERIVE_KW) {
        attribute(p);
        p.skip_trivia();
    }
    if p.at(PUB_KW) {
        p.start_node(VISIBILITY);
        p.bump(PUB_KW);
        p.finish_node();
        p.skip_trivia();
    }
    match p.peek() {
        USE_KW => {
            use_decl(p, cp);
            true
        }
        MOD_KW => {
            mod_decl(p, cp);
            true
        }
        PACKAGE_KW => {
            package_decl(p, cp);
            true
        }
        FN_KW => {
            fn_decl_pub(p, cp);
            true
        }
        UNSAFE_KW if next_nontrivia_after(p, p.pos + 1) == FN_KW => {
            fn_decl_pub(p, cp);
            true
        }
        STRUCT_KW => {
            struct_decl(p, cp);
            true
        }
        ENUM_KW => {
            enum_decl(p, cp);
            true
        }
        TYPE_KW => {
            type_alias(p, cp);
            true
        }
        IMPL_KW => {
            impl_block(p, cp);
            true
        }
        TRAIT_KW => {
            trait_decl(p, cp);
            true
        }
        CONST_KW => {
            const_decl(p, cp);
            true
        }
        AGENT_KW => {
            super::agents::agent_decl(p, cp);
            true
        }
        PROTOCOL_KW => {
            super::agents::protocol_decl(p, cp);
            true
        }
        SUP_KW => {
            super::agents::supervisor_decl(p, cp);
            true
        }
        IDENT if p.tokens[p.pos].text == "supervisor" => {
            super::agents::supervisor_decl(p, cp);
            true
        }
        EXTERN_KW => {
            super::extern_::extern_block(p, cp);
            true
        }
        EXPORT_KW => {
            super::extern_::export_decl(p, cp);
            true
        }
        MACRO_KW => {
            super::macros::macro_decl(p, cp);
            true
        }
        SANDBOX_KW => {
            sandbox_decl(p, cp);
            true
        }
        _ => false,
    }
}

/// Parse `#[derive(Copy, Hash)]` or `derive Copy` shorthand.
fn attribute(p: &mut Parser) {
    p.start_node(ATTR);
    if p.eat(HASH) {
        p.expect(L_BRACK);
        // `derive(Foo, Bar)` form is the only attribute slice 5 understands.
        // Note: `derive` is a keyword so we use `name_or_keyword`.
        paths::name_or_keyword(p);
        if p.eat(L_PAREN) {
            while !p.at(R_PAREN) && !p.at(EOF) {
                paths::name_or_keyword(p);
                if !p.eat(COMMA) {
                    break;
                }
            }
            p.expect(R_PAREN);
        }
        p.expect(R_BRACK);
    } else if p.at(DERIVE_KW) {
        // `derive Copy` (and possibly `derive Copy, Hash`).
        // Emit the `derive` keyword as a NAME so the lowerer can find it.
        p.start_node(NAME);
        p.bump_any();
        p.finish_node();
        p.skip_trivia();
        paths::name_or_keyword(p);
        while p.eat(COMMA) {
            paths::name_or_keyword(p);
        }
    }
    p.finish_node();
    p.skip_trivia();
}

/// Parse a top-level sandbox item (spec §16.1). Same body shape as the
/// expression form.
fn sandbox_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, SANDBOX_BLOCK);
    p.bump(SANDBOX_KW);
    p.skip_trivia();
    // Name is optional but spec writes it; accept either.
    if p.at(IDENT) {
        paths::name(p);
    }
    if p.at(WITH_KW) {
        p.bump(WITH_KW);
        p.skip_trivia();
    }
    p.expect(L_BRACE);
    p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        p.start_node(SANDBOX_ENTRY);
        // entry: PATH = EXPR
        paths::path(p);
        if p.eat(EQ) {
            super::exprs::expr(p);
        }
        p.finish_node();
        p.eat(COMMA);
        p.skip_trivia();
    }
    p.expect(R_BRACE);
    p.skip_trivia();
    if p.at(L_BRACE) {
        super::stmts::block(p);
    }
    p.finish_node();
    p.skip_trivia();
}

/// Kind of the next non-trivia token at index `from` (inclusive).
fn next_nontrivia_after(p: &Parser, from: usize) -> SyntaxKind {
    let mut i = from;
    while i < p.tokens.len() && p.tokens[i].kind.is_trivia() {
        i += 1;
    }
    p.tokens.get(i).map(|t| t.kind).unwrap_or(SyntaxKind::EOF)
}

fn use_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, USE_DECL);
    p.bump(USE_KW);
    p.skip_trivia();
    paths::path(p);
    // Either `use X.{a, b}` or `use X as Y` or just `use X`
    // After path(), we're past whitespace. Check for `.{` or `as`.
    if p.at(DOT) && p.peek_n(1) == L_BRACE {
        p.bump(DOT);
        p.bump(L_BRACE);
        p.skip_trivia();
        loop {
            if p.at(R_BRACE) {
                break;
            }
            paths::name(p);
            if p.eat(AS_KW) {
                paths::name(p);
            }
            if !p.eat(COMMA) {
                break;
            }
        }
        p.expect(R_BRACE);
    } else if p.eat(AS_KW) {
        paths::name(p);
    }
    p.eat(SEMI);
    p.finish_node();
    p.skip_trivia();
}

fn mod_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, MOD_DECL);
    p.bump(MOD_KW);
    p.skip_trivia();
    paths::path(p);
    p.eat(SEMI);
    p.finish_node();
    p.skip_trivia();
}

fn package_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, PACKAGE_DECL);
    p.bump(PACKAGE_KW);
    p.skip_trivia();
    paths::path(p);
    p.eat(SEMI);
    p.finish_node();
    p.skip_trivia();
}

/// Public so other item modules (agents, extern blocks) can re-use it.
pub(super) fn fn_decl_pub(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, FN_DECL);
    p.eat(UNSAFE_KW);
    p.expect(FN_KW);
    p.skip_trivia();
    paths::name(p);
    super::types::generic_params(p);
    fn_params(p);
    if p.eat(THIN_ARROW) {
        p.start_node(RET_TYPE);
        super::types::type_expr(p);
        p.finish_node();
        p.skip_trivia();
    }
    super::types::effect_clause(p);
    // `requires <expr>` clauses precede the body (used by unsafe-fn contracts in Task 15).
    while p.eat(REQUIRES_KW) {
        super::exprs::expr(p);
        p.skip_trivia();
    }
    p.skip_trivia();
    if p.eat(EQ) {
        super::exprs::expr(p);
        p.eat(SEMI);
    } else if p.at(L_BRACE) {
        super::stmts::block(p);
    } else {
        // Trait method signature without body, or fn declaration in extern block.
        p.eat(SEMI);
    }
    p.finish_node();
    p.skip_trivia();
}

/// Public to siblings so extern blocks (and the lambda parser in exprs.rs)
/// can parse function parameter lists.
pub(crate) fn fn_params(p: &mut Parser) {
    p.start_node(FN_PARAM_LIST);
    p.expect(L_PAREN);
    p.skip_trivia();
    if !p.at(R_PAREN) {
        param(p);
        while p.eat(COMMA) {
            if p.at(R_PAREN) {
                break;
            }
            param(p);
        }
    }
    p.expect(R_PAREN);
    p.finish_node();
    p.skip_trivia();
}

fn param(p: &mut Parser) {
    p.start_node(FN_PARAM);
    if p.at(SELF_KW) {
        // `self` parameter in trait/impl methods; no type annotation required.
        p.bump(SELF_KW);
        p.skip_trivia();
        if p.eat(COLON) {
            super::types::type_expr(p);
        }
    } else {
        paths::name(p);
        if p.eat(COLON) {
            super::types::type_expr(p);
        }
    }
    p.finish_node();
    p.skip_trivia();
}

fn struct_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, STRUCT_DECL);
    p.bump(STRUCT_KW);
    p.skip_trivia();
    paths::name(p);
    super::types::generic_params(p);
    p.expect(L_BRACE);
    p.skip_trivia();
    p.start_node(STRUCT_FIELD_LIST);
    while !p.at(R_BRACE) && !p.at(EOF) {
        p.start_node(STRUCT_FIELD);
        paths::name(p);
        if p.eat(COLON) {
            super::types::type_expr(p);
        }
        p.finish_node();
        p.eat(COMMA);
        p.skip_trivia();
    }
    p.finish_node();
    p.expect(R_BRACE);
    p.finish_node();
    p.skip_trivia();
}

fn enum_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, ENUM_DECL);
    p.bump(ENUM_KW);
    p.skip_trivia();
    paths::name(p);
    super::types::generic_params(p);
    p.expect(L_BRACE);
    p.skip_trivia();
    p.start_node(ENUM_VARIANT_LIST);
    while !p.at(R_BRACE) && !p.at(EOF) {
        p.start_node(ENUM_VARIANT);
        paths::name(p);
        if p.eat(L_PAREN) {
            super::types::type_expr(p);
            while p.eat(COMMA) {
                if p.at(R_PAREN) {
                    break;
                }
                super::types::type_expr(p);
            }
            p.expect(R_PAREN);
        }
        p.finish_node();
        p.eat(COMMA);
        p.skip_trivia();
    }
    p.finish_node();
    p.expect(R_BRACE);
    p.finish_node();
    p.skip_trivia();
}

fn type_alias(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, TYPE_ALIAS);
    p.bump(TYPE_KW);
    p.skip_trivia();
    paths::name(p);
    super::types::generic_params(p);
    p.expect(EQ);
    super::types::type_expr(p);
    p.eat(SEMI);
    p.finish_node();
    p.skip_trivia();
}

fn impl_block(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, IMPL_BLOCK);
    p.bump(IMPL_KW);
    p.skip_trivia();
    super::types::generic_params(p);
    // Either `Trait for Type` or just `Type`. Parse first type; if `for`, parse second.
    super::types::type_expr(p);
    if p.eat(FOR_KW) {
        super::types::type_expr(p);
    }
    p.expect(L_BRACE);
    p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        let icp = p.checkpoint();
        if p.at(PUB_KW) {
            p.start_node(VISIBILITY);
            p.bump(PUB_KW);
            p.finish_node();
            p.skip_trivia();
        }
        if p.at(FN_KW) || (p.at(UNSAFE_KW) && next_nontrivia_after(p, p.pos + 1) == FN_KW) {
            fn_decl_pub(p, icp);
        } else if p.at(TYPE_KW) {
            type_alias(p, icp);
        } else {
            p.error("expected fn or type alias in impl");
            p.bump_any();
            p.skip_trivia();
        }
    }
    p.expect(R_BRACE);
    p.finish_node();
    p.skip_trivia();
}

fn trait_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, TRAIT_DECL);
    p.bump(TRAIT_KW);
    p.skip_trivia();
    paths::name(p);
    super::types::generic_params(p);
    p.expect(L_BRACE);
    p.skip_trivia();
    while !p.at(R_BRACE) && !p.at(EOF) {
        let cp2 = p.checkpoint();
        if p.at(PUB_KW) {
            p.start_node(VISIBILITY);
            p.bump(PUB_KW);
            p.finish_node();
            p.skip_trivia();
        }
        if p.at(FN_KW) || (p.at(UNSAFE_KW) && next_nontrivia_after(p, p.pos + 1) == FN_KW) {
            p.start_node_at(cp2, TRAIT_METHOD);
            fn_decl_pub(p, cp2);
            p.finish_node();
        } else {
            p.error("expected fn in trait");
            p.bump_any();
            p.skip_trivia();
        }
    }
    p.expect(R_BRACE);
    p.finish_node();
    p.skip_trivia();
}

fn const_decl(p: &mut Parser, cp: rowan::Checkpoint) {
    p.start_node_at(cp, CONST_DECL);
    p.bump(CONST_KW);
    p.skip_trivia();
    paths::name(p);
    p.expect(COLON);
    super::types::type_expr(p);
    p.expect(EQ);
    super::exprs::expr(p);
    p.eat(SEMI);
    p.finish_node();
    p.skip_trivia();
}
