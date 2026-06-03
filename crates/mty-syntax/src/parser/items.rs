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
    // v0.27 Track A: `@<ident>(args...)` attribute prefix. Recognized
    // only immediately preceding a `fn`/`agent`/`protocol`/`pub` item.
    // The check is `AT` + `IDENT` + `L_PAREN` (with optional whitespace
    // between each pair); everything else (e.g. a stray `@` token in
    // mid-stream) falls through to the normal unexpected-token path.
    let mut had_attr_at = false;
    let mut attr_at_span: Option<(usize, usize, String)> = None;
    while p.at(AT) && tool_attr_prefix_ahead(p) {
        let (s, e, name) = attr_at(p);
        had_attr_at = true;
        // The most recent attribute's span is what we report against if
        // the trailing item turns out to be a non-fn.
        attr_at_span = Some((s, e, name));
        p.skip_trivia();
    }
    if p.at(PUB_KW) {
        p.start_node(VISIBILITY);
        p.bump(PUB_KW);
        p.finish_node();
        p.skip_trivia();
    }
    // v0.27 Track A: if `@<attr>(...)` preceded a non-`fn`/`agent`/
    // `protocol` item, emit MT1004 at the most recent attribute span.
    // The attribute is still consumed (CST stays well-formed); the
    // diagnostic surfaces the error cleanly.
    if had_attr_at {
        let k = p.peek();
        let attr_target_ok = matches!(k, FN_KW | AGENT_KW | PROTOCOL_KW)
            || (k == UNSAFE_KW && next_nontrivia_after(p, p.pos + 1) == FN_KW);
        if !attr_target_ok {
            if let Some((s, e, name)) = &attr_at_span {
                p.error_at_code(
                    1004,
                    format!(
                        "`@{}` attribute only decorates `fn`/`agent`/`protocol` items",
                        if name.is_empty() { "<unknown>" } else { name }
                    ),
                    *s,
                    *e,
                );
            }
        }
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
        // v0.5: `proc macro Name(input: TokenStream) -> TokenStream { body }`
        // `proc` lexes as IDENT (not a keyword), so we recognize the two-token
        // prefix here.
        IDENT
            if p.tokens[p.pos].text == "proc" && next_nontrivia_after(p, p.pos + 1) == MACRO_KW =>
        {
            super::macros::proc_macro_decl(p, cp);
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
                let before = p.pos;
                paths::name_or_keyword(p);
                if !p.eat(COMMA) {
                    break;
                }
                // v0.9 non-progress guard (FUZZ_V0_9 audit): break if
                // neither the name nor a trailing comma advanced us.
                if p.pos == before {
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

/// v0.27 Track A: parse a `@<ident>(args...)` attribute. The leading
/// `@` token has already been verified by the caller; we consume the
/// whole prefix including the closing `)`.
///
/// Surface (v0.27 accepts only `@tool`; the parser stays generic so
/// later attribute names can land in `expand_builtin_attribute` without
/// re-parsing work):
///
/// ```mty
/// @tool("desc")
/// @tool("desc", cap: fs.read)
/// @tool("desc", cap: fs.read("./data/**"), streaming: true, name: "rd")
/// ```
///
/// Args parsing:
///   - First positional arg is captured as a generic expression (the
///     caller — the macro expander or HIR preprocessor — checks it's a
///     string literal).
///   - `cap: <expr>` is wrapped in a TOOL_ATTR_CAP_ARG so consumers can
///     pull the inner expression node out without re-walking. The arg
///     value goes through the regular expression sub-parser so dotted
///     paths AND method calls (`fs.read("./data/**")`) both parse.
///   - Other named args (`streaming: true`, `name: "x"`) parse as
///     generic NAMED_ARG nodes.
///
/// Unknown attribute names are not rejected at parse time — the HIR
/// preprocessor emits a clean MT1XXX diagnostic with full context. The
/// parser's job is just to ensure the CST stays well-formed.
fn attr_at(p: &mut Parser) -> (usize, usize, String) {
    let attr_start = p.tokens[p.pos].start;
    p.start_node(TOOL_ATTR);
    p.bump(AT);
    p.skip_trivia();
    // Attribute name (e.g. `tool`). Stored under NAME so the HIR
    // preprocessor can pull the text the same way it does for other
    // ident references.
    let attr_name = if p.at(IDENT) {
        p.tokens[p.pos].text.to_string()
    } else {
        String::new()
    };
    paths::name(p);
    p.skip_trivia();
    // Arg list — required `(`. The caller has already peeked it.
    p.start_node(TOOL_ATTR_ARGS);
    p.expect(L_PAREN);
    p.skip_trivia();
    while !p.at(R_PAREN) && !p.at(EOF) {
        let before = p.pos;
        // `cap:` arg is special-cased so the HIR preprocessor can
        // locate the cap expression directly. Note: `cap` lexes as
        // `CAP_KW` (it's a reserved keyword in the spec), so we
        // recognize the keyword form here — accepting plain `IDENT`
        // would still leave the keyword path unhandled.
        if (p.at(CAP_KW) || (p.at(IDENT) && p.tokens[p.pos].text == "cap"))
            && next_nontrivia_after(p, p.pos + 1) == COLON
        {
            p.start_node(TOOL_ATTR_CAP_ARG);
            // Capture `cap` under NAME so downstream lowering can pull
            // the keyword text uniformly with other named args.
            p.start_node(NAME);
            p.bump_any();
            p.finish_node();
            p.skip_trivia();
            p.bump(COLON);
            p.skip_trivia();
            super::exprs::expr(p);
            p.finish_node();
        } else if p.at(IDENT) && next_nontrivia_after(p, p.pos + 1) == COLON {
            // Generic named arg: `streaming: true`, `name: "x"`, or
            // `description: "..."`.
            p.start_node(NAMED_ARG);
            paths::name(p);
            p.skip_trivia();
            p.bump(COLON);
            p.skip_trivia();
            super::exprs::expr(p);
            p.finish_node();
        } else {
            // Positional arg — wrap in ARG so the HIR side has a
            // stable node to match on.
            p.start_node(ARG);
            super::exprs::expr(p);
            p.finish_node();
        }
        p.skip_trivia();
        if !p.eat(COMMA) {
            break;
        }
        p.skip_trivia();
        // Non-progress guard: matches the enum/struct body shape.
        if p.pos == before {
            p.error("unexpected token in @attribute arguments");
            p.bump_any();
            p.skip_trivia();
        }
    }
    p.expect(R_PAREN);
    p.finish_node(); // TOOL_ATTR_ARGS
    let attr_end = if p.pos < p.tokens.len() {
        p.tokens[p.pos].start
    } else {
        attr_start
    };
    p.finish_node(); // TOOL_ATTR
    p.skip_trivia();
    // v0.27 Track A: unknown-attribute diagnostic. v0.27 accepted ONLY
    // `@tool`; v0.30 Track C extends this to `@computer_use` for
    // Anthropic Computer Use agents. Anything else (`@bogus`,
    // `@route`, ...) is a clean MT1003 at the attribute-name span.
    if attr_name != "tool" && attr_name != "computer_use" && !attr_name.is_empty() {
        p.error_at_code(
            1003,
            format!(
                "unknown attribute `@{}` (v0.30 accepts `@tool`, `@computer_use`)",
                attr_name
            ),
            attr_start,
            attr_end,
        );
    }
    (attr_start, attr_end, attr_name)
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
        let before = p.pos;
        p.start_node(SANDBOX_ENTRY);
        // entry: PATH = EXPR
        paths::path(p);
        if p.eat(EQ) {
            super::exprs::expr(p);
        }
        p.finish_node();
        p.eat(COMMA);
        p.skip_trivia();
        // v0.9 non-progress guard (FUZZ_V0_9 audit): same shape as enum_decl.
        if p.pos == before {
            p.error("unexpected token in sandbox body");
            p.bump_any();
            p.skip_trivia();
        }
    }
    p.expect(R_BRACE);
    p.skip_trivia();
    if p.at(L_BRACE) {
        super::stmts::block(p);
    }
    p.finish_node();
    p.skip_trivia();
}

/// v0.27 Track A: is the next-after-`AT` token-shape `IDENT L_PAREN`?
/// Handles trivia between `@`, the name, and the open paren. Caller has
/// already verified `p.at(AT)`.
fn tool_attr_prefix_ahead(p: &Parser) -> bool {
    // First non-trivia token after `@`:
    let mut i = p.pos + 1;
    while i < p.tokens.len() && p.tokens[i].kind.is_trivia() {
        i += 1;
    }
    if i >= p.tokens.len() || p.tokens[i].kind != SyntaxKind::IDENT {
        return false;
    }
    // First non-trivia token after the ident:
    let mut j = i + 1;
    while j < p.tokens.len() && p.tokens[j].kind.is_trivia() {
        j += 1;
    }
    j < p.tokens.len() && p.tokens[j].kind == SyntaxKind::L_PAREN
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

pub(crate) fn param(p: &mut Parser) {
    p.start_node(FN_PARAM);
    // v0.38 Track T3 — optional per-param attribute prefixes
    // (e.g. `#[ffi_nul_ok]` on extern fn params). Stays generic so
    // future attribute names land without a parser change; the lowerer
    // collects the attribute name strings into `HirParam.attrs` and
    // downstream typeck consults them where it cares (only the
    // extern-c Str→*U8 coercion path today).
    while p.at(HASH) {
        attribute(p);
        p.skip_trivia();
    }
    // v0.47 T1 — optional `mut` prefix on the param name marks a
    // caller-allocated OUT buffer at extern-c sites. The token is
    // preserved as a direct MUT_KW child of FN_PARAM so the AST
    // accessor (`FnParam::is_mut`) and the HIR lowerer pick it up
    // without touching the rest of the param shape. For non-extern-c
    // fns the type checker emits a diagnostic — `mut` is FFI-only.
    p.eat(MUT_KW);
    p.skip_trivia();
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
        let before = p.pos;
        p.start_node(STRUCT_FIELD);
        paths::name(p);
        if p.eat(COLON) {
            super::types::type_expr(p);
        }
        p.finish_node();
        p.eat(COMMA);
        p.skip_trivia();
        // v0.9 non-progress guard (FUZZ_V0_9 audit): same anti-pattern as
        // enum_decl. Avoid infinite STRUCT_FIELD growth on malformed input.
        if p.pos == before {
            p.error("unexpected token in struct body");
            p.bump_any();
            p.skip_trivia();
        }
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
        let before = p.pos;
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
        // v0.9 non-progress guard (FUZZ_V0_9 Bug 1): on malformed input
        // like `enum E { R(F>4)`, the loop body can fail to consume any
        // tokens, growing ENUM_VARIANT green nodes without bound. If we
        // didn't advance, surface an error and bump one token so the
        // outer loop can make progress instead of OOMing.
        if p.pos == before {
            p.error("unexpected token in enum body");
            p.bump_any();
            p.skip_trivia();
        }
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
        let before = p.pos;
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
        // v0.9 non-progress guard (FUZZ_V0_9 audit): defensive — the else
        // branch already bumps, but fn_decl_pub / type_alias could stall
        // on malformed input. Force progress so we never spin.
        if p.pos == before {
            p.error("unexpected token in impl body");
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
        let before = p.pos;
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
        // v0.9 non-progress guard (FUZZ_V0_9 audit): defensive — the else
        // branch already bumps, but fn_decl_pub could in principle stall
        // on malformed input. Force progress so we never spin.
        if p.pos == before {
            p.error("unexpected token in trait body");
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
