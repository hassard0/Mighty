use super::{paths, Parser};
use crate::SyntaxKind::{self, *};

/// Look at the kind of the next non-trivia token starting at `from`.
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

/// Like `next_nontrivia_kind` but returns EOF if any trivia between `from`
/// and the next non-trivia token contains a newline. Used to disambiguate
/// `expr?` vs `expr?Msg(args)` — the latter requires that `?` and `Msg`
/// be on the same line, mirroring the spec's "no newline before `Msg`"
/// rule for send/ask sugar.
fn next_nontrivia_kind_same_line(p: &Parser, from: usize) -> SyntaxKind {
    let mut i = from;
    while i < p.tokens.len() {
        let k = p.tokens[i].kind;
        if k.is_trivia() {
            if p.tokens[i].text.contains('\n') {
                return SyntaxKind::EOF;
            }
            i += 1;
            continue;
        }
        return k;
    }
    SyntaxKind::EOF
}

/// Index of the next non-trivia token starting at `from`.
fn next_nontrivia_index(p: &Parser, from: usize) -> usize {
    let mut i = from;
    while i < p.tokens.len() && p.tokens[i].kind.is_trivia() {
        i += 1;
    }
    i
}

pub fn expr(p: &mut Parser) -> bool {
    expr_bp(p, 0).is_some()
}

/// v0.42 L20: parse an expression and return its primary shape (so
/// `paren_or_tuple` can decide whether the wrapped expression is itself
/// callable). Returns `NonCallable` on parse failure to be safe.
pub(crate) fn expr_shape(p: &mut Parser) -> PrimaryShape {
    expr_bp(p, 0).unwrap_or(PrimaryShape::NonCallable)
}

/// v0.42 L20: shape of a parsed primary expression for the purpose of
/// the postfix `(` rule. Only `Callable` primaries may be followed by
/// `(args)` and turned into a CALL_EXPR. A `Parsed(NonCallable)` primary
/// (an arithmetic/logical paren group such as `(a + b)`) is **not** a
/// callable, so `(a + b)(c)` no longer mis-parses as a call.
///
/// Postfixes themselves (`.field`, `[i]`, `(args)`, `?Msg(args)`,
/// `!Msg(args)`, `@dur`) always produce a callable-eligible shape
/// (a field-of, indexed element, or call result might hold a function),
/// so once one postfix runs, subsequent `(...)` are accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrimaryShape {
    /// Path / call / method / field / index / lambda / `?` / `!` / `@`,
    /// or a parenthesised primary whose inner expression was itself
    /// callable-shaped. Allows postfix `(args)` as a CALL_EXPR.
    Callable,
    /// Literal, binary, unary, control-flow, struct/map/array/tuple
    /// literal, or a parenthesised primary wrapping any of the above.
    /// Disallows postfix `(args)` as a CALL_EXPR — `(a + b)(c)` is a
    /// parse error rather than a silent mis-call.
    NonCallable,
}

fn expr_bp(p: &mut Parser, min_bp: u8) -> Option<PrimaryShape> {
    p.skip_trivia();
    let cp = p.checkpoint();
    let mut shape = unary_or_primary(p)?;

    loop {
        p.skip_trivia();
        // postfix first (highest precedence)
        if let Some(next_shape) = try_postfix(p, cp, shape) {
            shape = next_shape;
            continue;
        }

        // binary
        let Some(op_bp) = infix_bp(p) else { break };
        if op_bp < min_bp {
            break;
        }
        // `expr as Ty` is parsed at the binary-expression precedence
        // slot but produces a CAST_EXPR whose RHS is a *type expression*,
        // not a value expression. We special-case it here so a bare
        // `x as I64` no longer silently degrades into a BINARY_EXPR
        // with a path on the RHS (which `lower_bin_op` then mistook
        // for `BinOp::Add`).
        if p.peek() == AS_KW {
            p.start_node_at(cp, CAST_EXPR);
            p.bump(AS_KW);
            p.skip_trivia();
            if !super::types::type_expr(p) {
                p.error("expected type after `as`");
            }
            p.finish_node();
            // The cast result is a value of the target type — could be a
            // function pointer in unsafe contexts. Keep callable so
            // existing `(p as fn(...) -> ...)(args)` shapes still parse.
            shape = PrimaryShape::Callable;
            continue;
        }
        let right_bp = if infix_right_assoc(p.peek()) {
            op_bp // right-assoc: same level for RHS
        } else {
            op_bp + 1 // left-assoc
        };
        p.start_node_at(cp, BINARY_EXPR);
        p.bump_any();
        p.skip_trivia();
        expr_bp(p, right_bp);
        p.finish_node();
        // The result of a binary operation is an arithmetic / logical /
        // comparison value — not a callable.
        shape = PrimaryShape::NonCallable;
    }
    Some(shape)
}

fn infix_right_assoc(k: SyntaxKind) -> bool {
    matches!(
        k,
        EQ | PLUS_EQ
            | MINUS_EQ
            | STAR_EQ
            | SLASH_EQ
            | PERCENT_EQ
            | AMP_EQ
            | PIPE_EQ
            | CARET_EQ
            | SHL_EQ
            | SHR_EQ
    )
}

fn infix_bp(p: &Parser) -> Option<u8> {
    let bp = match p.peek() {
        DOT_DOT | DOT_DOT_EQ => 1,
        EQ | PLUS_EQ | MINUS_EQ | STAR_EQ | SLASH_EQ | PERCENT_EQ | AMP_EQ | PIPE_EQ | CARET_EQ
        | SHL_EQ | SHR_EQ => 2,
        PIPE_PIPE => 3,
        AMP_AMP => 4,
        EQ_EQ | BANG_EQ | LT | LT_EQ | GT | GT_EQ => 5,
        PIPE => 6,
        CARET => 7,
        AMP => 8,
        SHL | SHR => 9,
        PLUS | MINUS => 10,
        STAR | SLASH | PERCENT => 11,
        AS_KW => 12,
        _ => return None,
    };
    Some(bp)
}

const PREFIX_BP: u8 = 13;

fn unary_or_primary(p: &mut Parser) -> Option<PrimaryShape> {
    p.skip_trivia();
    match p.peek() {
        MINUS | BANG | STAR => {
            p.start_node(UNARY_EXPR);
            p.bump_any();
            p.skip_trivia();
            expr_bp(p, PREFIX_BP);
            p.finish_node();
            // `-(f)` / `!(g)` / `*(h)` are not callable themselves —
            // `(-f)(x)` should not parse as a call of `-f`.
            Some(PrimaryShape::NonCallable)
        }
        AMP => {
            p.start_node(BORROW_EXPR);
            p.bump(AMP);
            p.skip_trivia();
            p.eat(MUT_KW);
            unary_or_primary(p);
            p.finish_node();
            Some(PrimaryShape::NonCallable)
        }
        MOVE_KW => {
            p.start_node(MOVE_EXPR);
            p.bump(MOVE_KW);
            p.skip_trivia();
            unary_or_primary(p);
            p.finish_node();
            // `move f` is still semantically a function reference — keep
            // it callable so `(move f)(x)` works.
            Some(PrimaryShape::Callable)
        }
        SPAWN_KW => {
            p.start_node(SPAWN_EXPR);
            p.bump(SPAWN_KW);
            p.skip_trivia();
            // spawn task <expr> | spawn <Path>(args)
            p.eat(TASK_KW);
            expr(p);
            p.finish_node();
            Some(PrimaryShape::NonCallable)
        }
        _ => primary(p),
    }
}

fn primary(p: &mut Parser) -> Option<PrimaryShape> {
    p.skip_trivia();
    match p.peek() {
        INT_LITERAL | HEX_INT_LITERAL | BIN_INT_LITERAL | OCT_INT_LITERAL | FLOAT_LITERAL
        | STRING_LITERAL | CHAR_LITERAL | TRUE_KW | FALSE_KW | DURATION_LITERAL | SIZE_LITERAL => {
            p.start_node(LITERAL_EXPR);
            p.bump_any();
            p.finish_node();
            Some(PrimaryShape::NonCallable)
        }
        HTML_LITERAL => {
            p.start_node(HTML_EXPR);
            p.bump(HTML_LITERAL);
            p.finish_node();
            Some(PrimaryShape::NonCallable)
        }
        L_PAREN => Some(paren_or_tuple(p)),
        L_BRACK => Some(array_lit(p)),
        L_BRACE => Some(block_or_map_or_struct(p)),
        IF_KW => {
            super::stmts::if_expr(p);
            // `if` produces a value but it's a control-flow value, not
            // a function. Disallow `(if c { f } else { g })(x)` for now;
            // users can bind to a let if they want this.
            Some(PrimaryShape::NonCallable)
        }
        MATCH_KW => {
            super::stmts::match_expr(p);
            Some(PrimaryShape::NonCallable)
        }
        FOR_KW => {
            super::stmts::for_expr(p);
            Some(PrimaryShape::NonCallable)
        }
        WHILE_KW => {
            super::stmts::while_expr(p);
            Some(PrimaryShape::NonCallable)
        }
        LOOP_KW => {
            super::stmts::loop_expr(p);
            Some(PrimaryShape::NonCallable)
        }
        RETURN_KW => {
            p.start_node(RETURN_EXPR);
            p.bump(RETURN_KW);
            p.skip_trivia();
            if can_start_expr(p.peek()) {
                expr(p);
            }
            p.finish_node();
            Some(PrimaryShape::NonCallable)
        }
        BREAK_KW => {
            // `break` or `break <expr>`. v0.5 ships without label support;
            // labelled-break (`break 'outer value`) is deferred to v0.6.
            p.start_node(BREAK_EXPR);
            p.bump(BREAK_KW);
            p.skip_trivia();
            if can_start_expr(p.peek()) {
                expr(p);
            }
            p.finish_node();
            Some(PrimaryShape::NonCallable)
        }
        CONTINUE_KW => {
            // Unlabelled `continue`. v0.6 will add label support.
            p.start_node(CONTINUE_EXPR);
            p.bump(CONTINUE_KW);
            p.finish_node();
            Some(PrimaryShape::NonCallable)
        }
        UNSAFE_KW => {
            super::unsafe_::unsafe_block(p);
            Some(PrimaryShape::NonCallable)
        }
        ARENA_KW => {
            super::concurrency::arena_block(p);
            Some(PrimaryShape::NonCallable)
        }
        TASK_KW => {
            // `task_scope_or_call` re-emits `task` as a PATH_EXPR when the
            // next non-trivia token is DOT (so postfix `.method(...)` works);
            // otherwise it parses a TASK_SCOPE block.
            let next_is_dot = next_nontrivia_kind(p, p.pos + 1) == DOT;
            super::concurrency::task_scope_or_call(p);
            Some(if next_is_dot {
                PrimaryShape::Callable
            } else {
                PrimaryShape::NonCallable
            })
        }
        SANDBOX_KW => {
            super::concurrency::sandbox_block(p);
            Some(PrimaryShape::NonCallable)
        }
        DETACH_KW => {
            p.start_node(DETACH_EXPR);
            p.bump(DETACH_KW);
            p.skip_trivia();
            expr(p);
            p.finish_node();
            Some(PrimaryShape::NonCallable)
        }
        JOIN_KW => {
            p.start_node(JOIN_EXPR);
            p.bump(JOIN_KW);
            p.skip_trivia();
            expr(p);
            p.finish_node();
            Some(PrimaryShape::NonCallable)
        }
        FN_KW => {
            lambda_expr(p);
            // Closures ARE callable — `(fn() { 1 })()` and the historical
            // `(|| 1)()` shape should keep working.
            Some(PrimaryShape::Callable)
        }
        RUN_KW => {
            run_expr(p);
            Some(PrimaryShape::NonCallable)
        }
        IDENT => {
            // v0.29 Track E: soft keyword `budget`. When we see
            // `budget { ident expr ...`, dispatch to the budget-block
            // production (which re-tags the IDENT as BUDGET_KW). The
            // lookahead is conservative enough that struct literals like
            // `Foo { x: 1 }` and plain identifiers `let budget = 5` keep
            // the regular IDENT path.
            if p.tokens[p.pos].text == "budget" && lookahead_is_budget_block(p) {
                super::concurrency::budget_block(p);
                return Some(PrimaryShape::NonCallable);
            }
            Some(path_expr_or_call(p))
        }
        SELF_KW => {
            // `self` parses as a single-segment path expression so postfix `.field`,
            // method calls, etc. work uniformly.
            p.start_node(PATH_EXPR);
            p.start_node(PATH);
            p.start_node(PATH_SEGMENT);
            p.start_node(NAME_REF);
            p.bump(SELF_KW);
            p.finish_node();
            p.finish_node();
            p.finish_node();
            p.finish_node();
            p.skip_trivia();
            Some(PrimaryShape::Callable)
        }
        _ => None,
    }
}

fn paren_or_tuple(p: &mut Parser) -> PrimaryShape {
    let cp = p.checkpoint();
    p.bump(L_PAREN);
    p.skip_trivia();
    if p.at(R_PAREN) {
        // `()` — empty tuple (unit). Not callable.
        p.start_node_at(cp, TUPLE_EXPR);
        p.bump(R_PAREN);
        p.finish_node();
        return PrimaryShape::NonCallable;
    }
    // Inside parentheses, struct literals are unambiguous again.
    let mut shape = PrimaryShape::NonCallable;
    p.with_struct_literal(|p| {
        // v0.42 L20: capture the shape of the bare inner expression so
        // `(f)(x)` (Callable inner) keeps parsing as a call while
        // `(a + b)(c)` (NonCallable inner) does not.
        let inner_shape = expr_shape(p);
        p.skip_trivia();
        if p.at(COMMA) {
            // Tuple literal `(a, b, ...)` — never callable as a whole.
            p.start_node_at(cp, TUPLE_EXPR);
            p.bump(COMMA);
            p.skip_trivia();
            while !p.at(R_PAREN) && !p.at(EOF) {
                expr(p);
                p.skip_trivia();
                if !p.eat(COMMA) {
                    break;
                }
            }
            p.expect(R_PAREN);
            p.finish_node();
            shape = PrimaryShape::NonCallable;
        } else {
            p.expect(R_PAREN);
            // bare parenthesized expr — leave as the inner expr (no wrapper).
            // The parenthesised group inherits the inner expression's
            // callability so that `(f)(x)` works but `(a + b)(c)` is rejected.
            shape = inner_shape;
        }
    });
    shape
}

fn array_lit(p: &mut Parser) -> PrimaryShape {
    p.start_node(ARRAY_EXPR);
    p.bump(L_BRACK);
    p.skip_trivia();
    if !p.at(R_BRACK) {
        expr(p);
        p.skip_trivia();
        while p.eat(COMMA) {
            if p.at(R_BRACK) {
                break;
            }
            expr(p);
            p.skip_trivia();
        }
    }
    p.expect(R_BRACK);
    p.finish_node();
    // Array literal `[a, b, c]` is not directly callable; the user must
    // index first.
    PrimaryShape::NonCallable
}

fn block_or_map_or_struct(p: &mut Parser) -> PrimaryShape {
    // Disambiguate by peeking past `{` and the first non-trivia token.
    // Map literal: { IDENT : ... } or { } we treat as block.
    // Otherwise: plain block (for now an inline brace-balanced consumer; TODO(task-11): replace with stmts::block).
    let cp = p.checkpoint();
    let after_brace = next_nontrivia_index(p, p.pos + 1);
    let first_kind = next_nontrivia_kind(p, p.pos + 1);
    let second_kind = if first_kind == EOF {
        EOF
    } else {
        next_nontrivia_kind(p, after_brace + 1)
    };
    let looks_like_map = first_kind == IDENT && second_kind == COLON;

    if looks_like_map {
        p.start_node_at(cp, MAP_EXPR);
        p.bump(L_BRACE);
        p.skip_trivia();
        while !p.at(R_BRACE) && !p.at(EOF) {
            p.start_node(MAP_ENTRY);
            paths::name(p);
            p.expect(COLON);
            expr(p);
            p.skip_trivia();
            p.finish_node();
            if !p.eat(COMMA) {
                break;
            }
        }
        p.expect(R_BRACE);
        p.finish_node();
    } else {
        super::stmts::block(p);
    }
    // A block expression yields its tail expression's value, but we don't
    // know its shape here; conservatively non-callable. Map literals are
    // never callable.
    PrimaryShape::NonCallable
}

fn path_expr_or_call(p: &mut Parser) -> PrimaryShape {
    let cp = p.checkpoint();
    p.start_node(PATH_EXPR);
    paths::path(p);
    p.finish_node();
    p.skip_trivia();
    // v0.5 macros: `Path!(args)` invocation. Peek for BANG immediately
    // followed by L_PAREN (no newline between them — guards against the
    // postfix `!Msg(args)` send-sugar which requires same-line IDENT).
    if p.at(BANG) && next_nontrivia_kind(p, p.pos + 1) == L_PAREN {
        p.start_node_at(cp, MACRO_CALL);
        p.bump(BANG);
        p.skip_trivia();
        token_tree(p);
        p.finish_node();
        // Macro invocations expand to arbitrary expressions; conservatively
        // non-callable so `(some_macro!(...))(x)` doesn't silently flip
        // shape.
        return PrimaryShape::NonCallable;
    }
    // struct literal: Path { field: expr, ... }
    // Suppress when parsing control-flow conditions (`if`/`while`/`for`) so
    // `if x { ... }` parses as condition + body, not as a struct literal.
    if !p.no_struct_literal && p.at(L_BRACE) && lookahead_is_struct_literal(p) {
        p.start_node_at(cp, STRUCT_EXPR);
        p.bump(L_BRACE);
        p.skip_trivia();
        while !p.at(R_BRACE) && !p.at(EOF) {
            p.start_node(STRUCT_FIELD_EXPR);
            paths::name(p);
            if p.eat(COLON) {
                expr(p);
                p.skip_trivia();
            }
            p.finish_node();
            if !p.eat(COMMA) {
                break;
            }
        }
        p.expect(R_BRACE);
        p.finish_node();
        return PrimaryShape::NonCallable;
    }
    // Bare path expression — callable if it later resolves to a function
    // or callable type. The typechecker has the final say.
    PrimaryShape::Callable
}

/// v0.29 Track E: lookahead for the soft-keyword `budget` block.
///
/// Precondition: `p.peek()` is an IDENT with text `"budget"`.
///
/// We are in expression position. The shapes we must accept as a budget
/// block:
///   `budget { cpu 150ms wall 2s } run job(input)`
///   `budget {} run job()`              (empty body)
/// The shapes we must REJECT (so `budget` keeps working as a plain ident):
///   `budget`                            (bare ident — let, fn param, ...)
///   `budget = 5.0`                      (assignment / let-init RHS)
///   `budget()`                          (function call)
///   `budget.field`                      (field access)
///   `budget + 1`                        (arithmetic)
///   `Foo { budget: 5 }`                 (struct literal where `budget` is a field name — different path)
///   `budget { x: 1 }`                   (no struct literal because `budget` is not capitalised, but a hypothetical struct named `budget` would surface as struct-literal NOT budget-block; we resolve by requiring `IDENT EXPR` shape inside the braces, not `IDENT COLON ...`)
///
/// Decision rule: an IDENT "budget" starts a budget block iff the next
/// non-trivia token is `{` AND either
///   (a) the brace body is empty (`{}`), OR
///   (b) the first interior token is an IDENT NOT followed by `:`, `,`,
///       or `}` (the struct-literal shape). A `budget` block entry is
///       `IDENT EXPR`, so any token starting an expression (numeric /
///       string / duration / size literal, IDENT path, `(`, `[`, ...)
///       qualifies. We approximate via "second token is not COLON / COMMA
///       / R_BRACE".
fn lookahead_is_budget_block(p: &Parser) -> bool {
    let after_budget = next_nontrivia_index(p, p.pos + 1);
    if p.tokens.get(after_budget).map(|t| t.kind) != Some(L_BRACE) {
        return false;
    }
    let after_brace = next_nontrivia_index(p, after_budget + 1);
    let first = p.tokens.get(after_brace).map(|t| t.kind).unwrap_or(EOF);
    // Empty `budget {}` — accept (matches the existing grammar; even
    // though the runtime rejects it, the parser shape is unambiguous).
    if first == R_BRACE {
        return true;
    }
    // `budget { ident EXPR ... }` — entry is IDENT followed by an
    // expression token, NOT colon/comma/r_brace (those are struct-literal
    // shapes and route to `path_expr_or_call`).
    if first != IDENT {
        return false;
    }
    let second_idx = next_nontrivia_index(p, after_brace + 1);
    let second = p.tokens.get(second_idx).map(|t| t.kind).unwrap_or(EOF);
    !matches!(second, COLON | COMMA | R_BRACE)
}

fn lookahead_is_struct_literal(p: &Parser) -> bool {
    // Inside `Path { ... }`, treat as struct literal only if the immediate body looks like
    // fields (IDENT followed by COLON/COMMA/R_BRACE) or empty. Trivia-aware.
    let after_brace = next_nontrivia_index(p, p.pos + 1);
    let first = next_nontrivia_kind(p, p.pos + 1);
    if first == R_BRACE {
        return true;
    }
    if first == IDENT {
        let second = next_nontrivia_kind(p, after_brace + 1);
        return matches!(second, COLON | COMMA | R_BRACE);
    }
    false
}

fn try_postfix(p: &mut Parser, cp: rowan::Checkpoint, prev: PrimaryShape) -> Option<PrimaryShape> {
    p.skip_trivia();
    match p.peek() {
        DOT => {
            // Method-call vs field-access. After `.`, accept either an IDENT
            // or any keyword in name position so library APIs can use
            // reserved words (`dom.on(...)`, `x.match`, etc.).
            let after_dot = next_nontrivia_index(p, p.pos + 1);
            let name_kind = next_nontrivia_kind(p, p.pos + 1);
            let name_is_word = name_kind == IDENT || name_kind.is_keyword();
            if !name_is_word {
                return None;
            }
            let is_method_call = next_nontrivia_kind(p, after_dot + 1) == L_PAREN;
            if is_method_call {
                p.start_node_at(cp, METHOD_CALL_EXPR);
                p.bump(DOT);
                p.skip_trivia();
                paths::name_or_keyword(p);
                args(p);
                p.finish_node();
            } else {
                p.start_node_at(cp, FIELD_EXPR);
                p.bump(DOT);
                p.skip_trivia();
                paths::name_or_keyword(p);
                p.finish_node();
            }
            // Both `x.method(...)` results and `x.field` (which may hold a
            // closure) are callable-eligible for the next iteration.
            Some(PrimaryShape::Callable)
        }
        L_PAREN => {
            // v0.42 L20: only treat `expr(...)` as a CALL_EXPR when `expr`
            // is callable-shaped (path / call / field / index / lambda /
            // parens around any of those). For arithmetic/logical/literal
            // expressions, emit a clearer parse error than the downstream
            // MT2008 "value is not callable" the typechecker used to raise
            // (the user's intent is ambiguous: was a binary op meant or did
            // they really want to call this value?).
            if prev != PrimaryShape::Callable {
                p.error(
                    "expected operator before `(` — to call an arithmetic \
                     or boolean expression, bind it to a `let` first \
                     (Mighty does not auto-apply parens to non-callable values)",
                );
                return None;
            }
            p.start_node_at(cp, CALL_EXPR);
            args(p);
            p.finish_node();
            Some(PrimaryShape::Callable)
        }
        L_BRACK => {
            p.start_node_at(cp, INDEX_EXPR);
            p.bump(L_BRACK);
            p.skip_trivia();
            expr(p);
            p.skip_trivia();
            p.expect(R_BRACK);
            p.finish_node();
            // Indexed element might be a closure stored in an array.
            Some(PrimaryShape::Callable)
        }
        QUESTION => {
            // Disambiguate: `?Msg(args)` is ask; bare `?` is propagate.
            // Trivia-aware lookahead for the next token after `?` — but
            // require the identifier to be on the same line so that
            // `let body = fetch(url)?\n  parse(body)?` doesn't get glued
            // together as one `?parse(...)` ask call.
            let next = next_nontrivia_kind_same_line(p, p.pos + 1);
            if next == IDENT {
                p.start_node_at(cp, ASK_EXPR);
                p.bump(QUESTION);
                p.skip_trivia();
                paths::name(p);
                if p.at(L_PAREN) {
                    args(p);
                }
                p.finish_node();
            } else {
                p.start_node_at(cp, QUESTION_EXPR);
                p.bump(QUESTION);
                p.finish_node();
            }
            // `expr?` unwraps a Result/Option — the unwrapped value may
            // itself be callable.
            Some(PrimaryShape::Callable)
        }
        BANG => {
            // `!Msg(args)` is send; `!expr` is boolean-not (handled in unary, not postfix).
            // Same-line rule applies for the same reason as QUESTION (above).
            let next = next_nontrivia_kind_same_line(p, p.pos + 1);
            if next == IDENT {
                p.start_node_at(cp, SEND_EXPR);
                p.bump(BANG);
                p.skip_trivia();
                paths::name(p);
                if p.at(L_PAREN) {
                    args(p);
                }
                p.finish_node();
                Some(PrimaryShape::NonCallable)
            } else {
                None
            }
        }
        AT => {
            // @duration deadline applies to the preceding expression.
            p.start_node_at(cp, DEADLINE_EXPR);
            p.bump(AT);
            p.skip_trivia();
            // Accept DURATION_LITERAL primarily; allow any expr (compile-time const).
            if p.at(DURATION_LITERAL) {
                p.start_node(LITERAL_EXPR);
                p.bump(DURATION_LITERAL);
                p.finish_node();
            } else {
                expr(p);
            }
            p.finish_node();
            // Deadline wraps an expr — preserve the previous callability so
            // `(f @2s)(x)` still works if `f` was callable.
            Some(prev)
        }
        _ => None,
    }
}

fn args(p: &mut Parser) {
    p.start_node(ARG_LIST);
    p.bump(L_PAREN);
    p.skip_trivia();
    if !p.at(R_PAREN) {
        arg(p);
        p.skip_trivia();
        while p.eat(COMMA) {
            if p.at(R_PAREN) {
                break;
            }
            arg(p);
            p.skip_trivia();
        }
    }
    p.expect(R_PAREN);
    p.finish_node();
}

fn arg(p: &mut Parser) {
    // Named argument: IDENT COLON expr (trivia-aware).
    if p.at(IDENT) && next_nontrivia_kind(p, p.pos + 1) == COLON {
        p.start_node(NAMED_ARG);
        paths::name(p);
        p.bump(COLON);
        p.skip_trivia();
        expr(p);
        p.finish_node();
    } else {
        p.start_node(ARG);
        expr(p);
        p.finish_node();
    }
}

pub fn can_start_expr(k: SyntaxKind) -> bool {
    matches!(
        k,
        INT_LITERAL
            | HEX_INT_LITERAL
            | BIN_INT_LITERAL
            | OCT_INT_LITERAL
            | FLOAT_LITERAL
            | STRING_LITERAL
            | CHAR_LITERAL
            | TRUE_KW
            | FALSE_KW
            | DURATION_LITERAL
            | SIZE_LITERAL
            | HTML_LITERAL
            | IDENT
            | SELF_KW
            | L_PAREN
            | L_BRACK
            | L_BRACE
            | MINUS
            | BANG
            | STAR
            | AMP
            | MOVE_KW
            | SPAWN_KW
            | IF_KW
            | MATCH_KW
            | FOR_KW
            | WHILE_KW
            | LOOP_KW
            | RETURN_KW
            | BREAK_KW
            | CONTINUE_KW
            | UNSAFE_KW
            | ARENA_KW
            | TASK_KW
            | SANDBOX_KW
            | DETACH_KW
            | JOIN_KW
            | FN_KW
            | RUN_KW
    )
}

/// `fn` lambda expression: `fn() { body }` or `fn(x: T, y) -> R { body }`.
/// LAMBDA_EXPR node kind already exists; this is the parser production.
/// Item-level `fn` is handled in `items::fn_decl_pub`; `lambda_expr` is
/// only reached from expression position via `primary`.
fn lambda_expr(p: &mut Parser) -> bool {
    p.start_node(LAMBDA_EXPR);
    p.bump(FN_KW);
    p.skip_trivia();
    super::items::fn_params(p);
    p.skip_trivia();
    if p.eat(THIN_ARROW) {
        p.start_node(RET_TYPE);
        super::types::type_expr(p);
        p.finish_node();
        p.skip_trivia();
    }
    if p.at(L_BRACE) {
        super::stmts::block(p);
    } else {
        p.error("expected '{' to start lambda body");
    }
    p.finish_node();
    true
}

/// `run <expr>` — leading-keyword expression form. Used in sandbox bodies
/// per spec §16.1. RUN_EXPR node kind is declared in syntax_kind.rs.
fn run_expr(p: &mut Parser) -> bool {
    p.start_node(RUN_EXPR);
    p.bump(RUN_KW);
    p.skip_trivia();
    expr(p);
    p.finish_node();
    true
}

/// v0.5: a paren-balanced opaque token tree, used as the arguments of a
/// `Path!(...)` macro invocation. Tokens are stored verbatim under the
/// TOKEN_TREE node; the macro expander interprets them. Nested parens,
/// brackets, and braces are tracked for depth but their contents are
/// otherwise unparsed.
pub(crate) fn token_tree(p: &mut Parser) {
    p.start_node(TOKEN_TREE);
    if !p.at(L_PAREN) {
        p.error("expected '(' to start macro argument token tree");
        p.finish_node();
        return;
    }
    p.bump(L_PAREN);
    let mut depth: i32 = 1;
    while depth > 0 && !p.at(EOF) {
        match p.peek() {
            L_PAREN | L_BRACK | L_BRACE => {
                depth += 1;
                p.bump_any();
            }
            R_PAREN => {
                depth -= 1;
                if depth == 0 {
                    p.bump(R_PAREN);
                } else {
                    p.bump_any();
                }
            }
            R_BRACK | R_BRACE => {
                depth -= 1;
                p.bump_any();
            }
            _ => {
                p.bump_any();
            }
        }
    }
    if depth > 0 {
        p.error("unterminated macro argument token tree (missing ')')");
    }
    p.finish_node();
}
