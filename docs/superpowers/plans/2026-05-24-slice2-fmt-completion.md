# Slice 2 — Formatter Completion + Syntactic Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close every slice-1 deferral that belongs to the formatter + surface syntax: real Wadler/Lindig per-node formatter, lambdas, if-let, turbofish, keyword method/field names, decimal size suffixes, `run <expr>`, restored examples 19/20, `sdust explain`.

**Architecture:** Extend `sdust-syntax` lexer + parser for the 6 surface additions, mirror them in `sdust-hir` lowering, fill in the 6 stub modules in `sdust-fmt` with per-node `Doc` builders, ship `sdust explain` in `sdust-cli`. Tests via insta snapshots, the existing fmt-sweep, and end-to-end `sdust check` on the canonical examples.

**Tech Stack:** Rust 1.82, logos 0.14, rowan 0.16, la-arena, ariadne, clap, insta. Same workspace as slice 1.

---

## File Structure

**Modify:**
- `crates/sdust-syntax/src/syntax_kind.rs` — add `RUN_EXPR`, `IF_LET_*` (none — reuse IF_EXPR), `LAMBDA_EXPR` (already exists), update `SIZE_LITERAL` regex.
- `crates/sdust-syntax/src/parser/exprs.rs` — lambdas, turbofish, run expr, keyword-tolerant `.` postfix.
- `crates/sdust-syntax/src/parser/stmts.rs` — `if let` branch in `if_expr`.
- `crates/sdust-syntax/src/parser/paths.rs` — `name_or_keyword`, generic args on path segments.
- `crates/sdust-syntax/src/parser/types.rs` — extend `effect_clause` for keyword names.
- `crates/sdust-hir/src/nodes.rs` — `HirExpr::Lambda`, `HirExpr::IfLet`, `HirExpr::Run`, `HirExpr::PathGeneric`.
- `crates/sdust-hir/src/lower/exprs.rs` — lower the 4 new variants.
- `crates/sdust-hir/src/dump.rs` — S-expr dump for new variants.
- `crates/sdust-diagnostics/src/codes.rs` — add `explain(code)` lookup table.
- `crates/sdust-cli/src/main.rs` — `Cmd::Explain`.
- `crates/sdust-cli/src/cmd/mod.rs` — add `explain` module.
- `crates/sdust-fmt/src/lib.rs` — wire real formatter.
- `crates/sdust-fmt/src/fmt/{mod,items,exprs,patterns,types,agents,concurrency}.rs` — per-node printers.
- `crates/sdust-fmt/src/trivia.rs` — leading/trailing trivia collection.
- `examples/19_backend_service.sd` — restore spec syntax.
- `examples/20_frontend_component.sd` — restore spec syntax.
- `examples/11_budget_block.sd`, `18_sandbox.sd` — remove divergence notes (`mb 1k`, `run job(input)`).
- `README.md` — roadmap mark slice 2 shipped.
- `SLICE1.md` — strike closed deferrals.
- `docs/tour/06-agents.md`, `12-extern.md` — update lambda + on-method notes.
- `docs/reference/cli/explain.md` (new), `docs/reference/diagnostics.md`.

**Create:**
- `crates/sdust-cli/src/cmd/explain.rs` — handler.
- `crates/sdust-cli/tests/explain.rs` — CLI test.
- `crates/sdust-fmt/tests/canonical.rs` — golden canonical-form fixtures.
- `tests/fmt/canonical/` — fixture .sd files.
- `docs/spec/v0.1-amendments.md` — document `k`/`m` suffix + `::[T]` turbofish.
- `SLICE2.md` — slice summary.

---

## Task 1: Lexer — add `k`/`m` decimal size suffixes

**Files:**
- Modify: `crates/sdust-syntax/src/syntax_kind.rs:27-28`
- Test: `crates/sdust-syntax/tests/lexer.rs`

- [ ] **Step 1: Write the failing tests**

Append to `crates/sdust-syntax/tests/lexer.rs`:

```rust
#[test]
fn lex_decimal_size_suffix_k() {
    use sdust_syntax::{lex, SyntaxKind};
    let toks = lex("1k");
    assert_eq!(toks[0].kind, SyntaxKind::SIZE_LITERAL);
    assert_eq!(toks[0].text, "1k");
}

#[test]
fn lex_decimal_size_suffix_m() {
    use sdust_syntax::{lex, SyntaxKind};
    let toks = lex("4096m");
    assert_eq!(toks[0].kind, SyntaxKind::SIZE_LITERAL);
    assert_eq!(toks[0].text, "4096m");
}

#[test]
fn lex_binary_size_suffix_still_works() {
    use sdust_syntax::{lex, SyntaxKind};
    let toks = lex("128MiB");
    assert_eq!(toks[0].kind, SyntaxKind::SIZE_LITERAL);
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p sdust-syntax lex_decimal_size_suffix
```

Expected: the first two FAIL because `1k` and `4096m` currently tokenize as INT_LITERAL + IDENT.

- [ ] **Step 3: Extend the SIZE_LITERAL regex**

In `crates/sdust-syntax/src/syntax_kind.rs:27-28`, replace:

```rust
    #[regex(r"[0-9]+(?:B|KiB|MiB|GiB)")]
    SIZE_LITERAL,
```

with:

```rust
    #[regex(r"[0-9]+(?:KiB|MiB|GiB|B|k|m)")]
    SIZE_LITERAL,
```

Note the order: multi-char `KiB`/`MiB`/`GiB` must come before single-char `B`/`k`/`m` so logos matches the longest prefix.

- [ ] **Step 4: Run lexer tests**

```bash
cargo test -p sdust-syntax
```

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/sdust-syntax/src/syntax_kind.rs crates/sdust-syntax/tests/lexer.rs
git commit -m "$(cat <<'EOF'
Lexer: accept decimal k/m as SIZE_LITERAL suffixes

Adds k (=1000) and m (=1000000) as decimal size-literal suffixes,
distinct from the binary KiB/MiB/GiB. Lowercase only — uppercase K/M
remain reserved for future binary aliases. Resolves slice-1 deferral
"Lexer support for 1k/1m size suffixes" so example 11 can drop its
divergence comment.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Parser — lambda expressions

**Files:**
- Modify: `crates/sdust-syntax/src/parser/exprs.rs`
- Test: `crates/sdust-syntax/tests/parse_exprs.rs`

- [ ] **Step 1: Write the failing tests**

Append to `crates/sdust-syntax/tests/parse_exprs.rs`:

```rust
#[test]
fn parse_lambda_nullary() {
    use sdust_syntax::{parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("fn() { 0 }");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    let lam = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::LAMBDA_EXPR);
    assert!(lam.is_some(), "expected LAMBDA_EXPR");
}

#[test]
fn parse_lambda_with_params_and_ret() {
    use sdust_syntax::{parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("fn(x: I32, y) -> I32 { x + y }");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::LAMBDA_EXPR));
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::FN_PARAM_LIST));
    assert!(root.descendants().any(|n| n.kind() == SyntaxKind::RET_TYPE));
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p sdust-syntax parse_lambda
```

Expected: FAIL — `fn` is rejected in expression position.

- [ ] **Step 3: Implement the lambda parser**

In `crates/sdust-syntax/src/parser/exprs.rs`, add `FN_KW` to `primary()`:

```rust
        FN_KW => lambda_expr(p),
```

Add to `can_start_expr`:

```rust
            | FN_KW
```

Then add the function (place near `primary`):

```rust
fn lambda_expr(p: &mut Parser) -> bool {
    p.start_node(LAMBDA_EXPR);
    p.bump(FN_KW);
    p.skip_trivia();
    super::items::fn_param_list(p);
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
```

Check `crates/sdust-syntax/src/parser/items.rs` for the actual name of the param-list helper; rename `fn_param_list` to match (it is `fn_params` or similar). If it's private inside items.rs, change `pub(crate) fn` so exprs can call it.

- [ ] **Step 4: Inspect items.rs and adjust**

```bash
grep -n "fn_param\|FN_PARAM" crates/sdust-syntax/src/parser/items.rs
```

If the existing helper is named `params` and private, expose it as `pub(crate) fn fn_params(p: &mut Parser)`. Reuse it; do not duplicate.

- [ ] **Step 5: Run lambda tests**

```bash
cargo test -p sdust-syntax parse_lambda
```

Expected: PASS. Also run `cargo test -p sdust-syntax` to verify no regressions.

- [ ] **Step 6: Commit**

```bash
git add crates/sdust-syntax/src/parser/exprs.rs crates/sdust-syntax/src/parser/items.rs crates/sdust-syntax/tests/parse_exprs.rs
git commit -m "$(cat <<'EOF'
Parser: lambda expressions in expression position

Accepts `fn() { body }` and `fn(x, y) -> T { body }` in primary
expression position, reusing the item-level FN_PARAM_LIST production.
LAMBDA_EXPR CST node was already declared; this wires up the parser
production. Resolves slice-1 deferral "Lambda expressions".

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Parser — if let

**Files:**
- Modify: `crates/sdust-syntax/src/parser/stmts.rs`
- Test: `crates/sdust-syntax/tests/parse_stmts.rs`

- [ ] **Step 1: Write the failing tests**

Append to `crates/sdust-syntax/tests/parse_stmts.rs`:

```rust
#[test]
fn parse_if_let_some() {
    use sdust_syntax::{parse, SyntaxKind, SyntaxNode};
    let src = "fn f() { if let Some(x) = opt { x } else { 0 } }";
    let r = parse(src);
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    let if_node = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::IF_EXPR)
        .expect("IF_EXPR");
    let has_let = if_node
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|t| t.kind() == SyntaxKind::LET_KW);
    assert!(has_let, "if let should carry LET_KW token");
}

#[test]
fn parse_if_let_ok_no_else() {
    use sdust_syntax::parse;
    let src = "fn f() { if let Ok(n) = parse(s) { use_n(n) } }";
    let r = parse(src);
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
}
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test -p sdust-syntax parse_if_let
```

Expected: FAIL — `let` after `if` is parsed as a stand-alone keyword and aborts.

- [ ] **Step 3: Extend `if_expr` in stmts.rs**

Replace `pub fn if_expr(p: &mut Parser) -> bool` body in `crates/sdust-syntax/src/parser/stmts.rs:43-58` with:

```rust
pub fn if_expr(p: &mut Parser) -> bool {
    p.start_node(IF_EXPR);
    p.bump(IF_KW);
    p.skip_trivia();
    if p.at(LET_KW) {
        p.bump(LET_KW);
        p.skip_trivia();
        patterns::pattern(p);
        p.expect(EQ);
        p.skip_trivia();
    }
    exprs::expr(p);
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
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p sdust-syntax
```

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/sdust-syntax/src/parser/stmts.rs crates/sdust-syntax/tests/parse_stmts.rs
git commit -m "$(cat <<'EOF'
Parser: if let Pattern = expr { ... } else { ... }

Adds the `if let` form to the existing IF_EXPR production. Single CST
shape with an optional leading LET_KW + pattern + EQ keeps the AST
view and the formatter simple. Resolves slice-1 deferral "if let".

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Parser — `run <expr>` and keyword-tolerant method/field names + keyword effects

**Files:**
- Modify: `crates/sdust-syntax/src/parser/exprs.rs`, `paths.rs`, `types.rs`, `syntax_kind.rs`
- Test: `crates/sdust-syntax/tests/parse_exprs.rs`

- [ ] **Step 1: Add RUN_EXPR kind**

In `crates/sdust-syntax/src/syntax_kind.rs`, add to the node-kind block (after `JOIN_EXPR`):

```rust
    RUN_EXPR,
```

- [ ] **Step 2: Write failing tests for run/keyword-method/keyword-effect**

Append to `crates/sdust-syntax/tests/parse_exprs.rs`:

```rust
#[test]
fn parse_run_expr_in_block() {
    use sdust_syntax::{parse, SyntaxKind, SyntaxNode};
    let src = "fn f() { run job(input) }";
    let r = parse(src);
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(root.descendants().any(|n| n.kind() == SyntaxKind::RUN_EXPR));
}

#[test]
fn parse_method_with_keyword_name() {
    use sdust_syntax::{parse, SyntaxKind, SyntaxNode};
    let r = parse("fn f() { dom.on(\"click\", h) }");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::METHOD_CALL_EXPR));
}

#[test]
fn parse_field_with_keyword_name() {
    use sdust_syntax::parse;
    let r = parse("fn f() { x.match }");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
}
```

And in `crates/sdust-syntax/tests/parse_items.rs` add:

```rust
#[test]
fn parse_effect_with_keyword_name() {
    use sdust_syntax::parse;
    let r = parse("fn f() effect net, model, spawn {}");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
}
```

- [ ] **Step 3: Run to confirm failures**

```bash
cargo test -p sdust-syntax parse_run_expr parse_method_with_keyword parse_field_with_keyword parse_effect_with_keyword
```

Expected: all FAIL.

- [ ] **Step 4: Implement `run_expr` and `name_or_keyword`**

In `crates/sdust-syntax/src/parser/paths.rs`, add:

```rust
/// Like [`name`], but also accepts a keyword token in name position.
/// Used after `.` for keyword-tolerant method/field names and in
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
```

In `crates/sdust-syntax/src/parser/exprs.rs`:

1. Add `RUN_KW => run_expr(p)` to the `primary()` match.
2. Add `RUN_KW` to `can_start_expr`.
3. Replace the `DOT` arm in `try_postfix` to also accept keyword tokens. Replace:

```rust
        DOT => {
            // method call vs field access: if followed by IDENT then L_PAREN, it's a method call
            let after_dot = next_nontrivia_index(p, p.pos + 1);
            let name_kind = next_nontrivia_kind(p, p.pos + 1);
            let is_method_call =
                name_kind == IDENT && next_nontrivia_kind(p, after_dot + 1) == L_PAREN;
            if is_method_call {
                p.start_node_at(cp, METHOD_CALL_EXPR);
                p.bump(DOT);
                p.skip_trivia();
                paths::name(p);
                args(p);
                p.finish_node();
            } else {
                p.start_node_at(cp, FIELD_EXPR);
                p.bump(DOT);
                p.skip_trivia();
                paths::name(p);
                p.finish_node();
            }
            true
        }
```

with:

```rust
        DOT => {
            let after_dot = next_nontrivia_index(p, p.pos + 1);
            let name_kind = next_nontrivia_kind(p, p.pos + 1);
            let name_is_word = name_kind == IDENT || name_kind.is_keyword();
            if !name_is_word {
                return false;
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
            true
        }
```

4. Add the `run_expr` helper:

```rust
fn run_expr(p: &mut Parser) -> bool {
    p.start_node(RUN_EXPR);
    p.bump(RUN_KW);
    p.skip_trivia();
    expr(p);
    p.finish_node();
    true
}
```

In `crates/sdust-syntax/src/parser/types.rs`, change `effect_clause` (line ~180):

```rust
pub fn effect_clause(p: &mut Parser) {
    if !p.at(EFFECT_KW) {
        return;
    }
    p.start_node(EFFECT_CLAUSE);
    p.bump(EFFECT_KW);
    p.skip_trivia();
    paths::name_or_keyword(p);
    while p.eat(COMMA) {
        paths::name_or_keyword(p);
    }
    p.finish_node();
    p.skip_trivia();
}
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p sdust-syntax
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add crates/sdust-syntax/src/syntax_kind.rs crates/sdust-syntax/src/parser/exprs.rs crates/sdust-syntax/src/parser/paths.rs crates/sdust-syntax/src/parser/types.rs crates/sdust-syntax/tests/parse_exprs.rs crates/sdust-syntax/tests/parse_items.rs
git commit -m "$(cat <<'EOF'
Parser: run <expr>, keyword-tolerant .method/.field, keyword effects

Three related slice-1 deferrals:

- RUN_EXPR as a leading-keyword expression form so `run job(input)`
  parses inside a sandbox body (spec §16.1).
- After `.`, any keyword token may stand in name position so library
  APIs like `dom.on(...)` work even though `on` is reserved.
- `effect` clauses accept keyword names like `spawn` so example 19 can
  declare `effect net, model, spawn`.

A single new paths helper (`name_or_keyword`) underpins both keyword
relaxations.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Parser — turbofish on path segments

**Files:**
- Modify: `crates/sdust-syntax/src/parser/paths.rs`
- Test: `crates/sdust-syntax/tests/parse_exprs.rs`

- [ ] **Step 1: Write the failing tests**

Append to `crates/sdust-syntax/tests/parse_exprs.rs`:

```rust
#[test]
fn parse_turbofish_method_call() {
    use sdust_syntax::{parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("Map::[Str, Json].new()");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::GENERIC_ARG_LIST));
}

#[test]
fn parse_turbofish_constructor() {
    use sdust_syntax::parse_expr;
    let r = parse_expr("Some::[I32](42)");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
}

#[test]
fn parse_turbofish_struct_literal() {
    use sdust_syntax::{parse_expr, SyntaxKind, SyntaxNode};
    let r = parse_expr("Map::[Str, Json]{}");
    assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    let root = SyntaxNode::new_root(r.green);
    assert!(root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::STRUCT_EXPR));
}
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test -p sdust-syntax parse_turbofish
```

Expected: FAIL — `::` is not consumed inside a path.

- [ ] **Step 3: Extend path parser**

Replace `crates/sdust-syntax/src/parser/paths.rs` with:

```rust
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

fn segment(p: &mut Parser) {
    p.start_node(PATH_SEGMENT);
    p.start_node(NAME_REF);
    p.bump(IDENT);
    p.finish_node();
    // Turbofish: `::[T1, T2]` after the segment name.
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
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p sdust-syntax
```

Expected: all pass. Check that the `parse_turbofish_struct_literal` test passes — the existing `lookahead_is_struct_literal` checks for `{` after the path; since the path now consumes `::[...]` first, the `{` lookahead still fires at the right cursor position.

- [ ] **Step 5: Commit**

```bash
git add crates/sdust-syntax/src/parser/paths.rs crates/sdust-syntax/tests/parse_exprs.rs
git commit -m "$(cat <<'EOF'
Parser: turbofish ::[T1, T2] on expression-position paths

Per-segment generic-args list, reusing the existing
types::generic_args production. `::` disambiguates from index-expr.
Works for method calls (`Map::[Str, Json].new()`), constructors
(`Some::[I32](x)`), and struct literals (`Map::[Str, Json]{}`).
Resolves slice-1 deferral "Generic args in expression position".

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: HIR — lower lambdas, if-let, run, path-generic

**Files:**
- Modify: `crates/sdust-hir/src/nodes.rs`, `lower/exprs.rs`, `dump.rs`
- Test: `crates/sdust-hir/tests/lower_items.rs`

- [ ] **Step 1: Extend HirExpr**

In `crates/sdust-hir/src/nodes.rs`, after the `Cast { ... }` arm (around line 343), add:

```rust
    Lambda {
        params: Vec<HirParam>,
        ret: Option<TypeId>,
        body: BlockId,
    },
    IfLet {
        pat: PatId,
        scrutinee: ExprId,
        then: BlockId,
        else_: Option<ExprId>,
    },
    Run(ExprId),
    PathGeneric {
        segments: Vec<String>,
        generics: Vec<TypeId>,
    },
```

- [ ] **Step 2: Write failing HIR tests**

Append to `crates/sdust-hir/tests/lower_items.rs`:

```rust
#[test]
fn lower_lambda_expr() {
    use sdust_driver::pipeline;
    let p = pipeline::compile("fn f() { let g = fn(x: I32) -> I32 { x + 1 } }");
    assert!(p.diagnostics.is_empty(), "diags: {:?}", p.diagnostics);
    let has_lambda = p.package.exprs.iter().any(|(_, e)| {
        matches!(e, sdust_hir::nodes::HirExpr::Lambda { .. })
    });
    assert!(has_lambda, "expected HirExpr::Lambda");
}

#[test]
fn lower_if_let_expr() {
    use sdust_driver::pipeline;
    let p = pipeline::compile("fn f() { if let Some(x) = opt { x } else { 0 } }");
    assert!(p.diagnostics.is_empty(), "diags: {:?}", p.diagnostics);
    let has_iflet = p.package.exprs.iter().any(|(_, e)| {
        matches!(e, sdust_hir::nodes::HirExpr::IfLet { .. })
    });
    assert!(has_iflet, "expected HirExpr::IfLet");
}

#[test]
fn lower_run_expr() {
    use sdust_driver::pipeline;
    let p = pipeline::compile("fn f() { run g() }");
    assert!(p.diagnostics.is_empty(), "diags: {:?}", p.diagnostics);
    let has_run = p.package.exprs.iter().any(|(_, e)| {
        matches!(e, sdust_hir::nodes::HirExpr::Run(_))
    });
    assert!(has_run, "expected HirExpr::Run");
}

#[test]
fn lower_turbofish_path() {
    use sdust_driver::pipeline;
    let p = pipeline::compile("fn f() { Some::[I32](1) }");
    assert!(p.diagnostics.is_empty(), "diags: {:?}", p.diagnostics);
    let has_pg = p.package.exprs.iter().any(|(_, e)| {
        matches!(e, sdust_hir::nodes::HirExpr::PathGeneric { .. })
    });
    assert!(has_pg, "expected HirExpr::PathGeneric");
}
```

If `pipeline::compile` has a different signature, look at `crates/sdust-driver/src/pipeline.rs` and adjust. Expected: it's `pub fn compile(src: &str) -> Compilation` returning a struct with `package: Package` and `diagnostics: Vec<...>`.

- [ ] **Step 3: Run to confirm failure**

```bash
cargo test -p sdust-hir lower_lambda_expr lower_if_let_expr lower_run_expr lower_turbofish_path
```

Expected: FAIL — the lowerer doesn't produce these variants yet.

- [ ] **Step 4: Lower LAMBDA_EXPR, IF_LET (via IF_EXPR), RUN_EXPR**

In `crates/sdust-hir/src/lower/exprs.rs`:

1. Add `LAMBDA_EXPR` and `RUN_EXPR` to the `is_expr_node` matches list. The existing list already has `LAMBDA_EXPR`; add `RUN_EXPR`:

```rust
            | LAMBDA_EXPR
            | RUN_EXPR
            | CAST_EXPR
```

2. In the big `lower_expr` match, add (before the `_ => HirExpr::Error` catch-all):

```rust
        SyntaxKind::LAMBDA_EXPR => {
            // Params: FN_PARAM_LIST -> FN_PARAM nodes
            let params: Vec<HirParam> = n
                .children()
                .find(|c| c.kind() == SyntaxKind::FN_PARAM_LIST)
                .map(|pl| {
                    pl.children()
                        .filter(|c| c.kind() == SyntaxKind::FN_PARAM)
                        .map(|param| {
                            let name = param
                                .children()
                                .find_map(sdust_ast::Name::cast)
                                .map(|nm| nm.text())
                                .unwrap_or_default();
                            let ty = param
                                .children()
                                .find(|c| super::items::is_type_node(c.kind()))
                                .map(|t| super::types::lower_type(ctx, t));
                            let start = param.text_range().start().into();
                            let end = param.text_range().end().into();
                            HirParam {
                                name,
                                ty,
                                span: SourceSpan { start, end },
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let ret = n
                .children()
                .find(|c| c.kind() == SyntaxKind::RET_TYPE)
                .and_then(|rt| rt.children().find(|c| super::items::is_type_node(c.kind())))
                .map(|t| super::types::lower_type(ctx, t));
            let body = n
                .children()
                .find(|c| c.kind() == SyntaxKind::BLOCK)
                .map(|b| lower_block_node(ctx, b))
                .unwrap_or_else(|| ctx.alloc_block(HirBlock { stmts: vec![], tail: None }));
            HirExpr::Lambda { params, ret, body }
        }
        SyntaxKind::RUN_EXPR => HirExpr::Run(first_child_expr_id(ctx, &n)),
```

3. Replace the existing `IF_EXPR` arm to detect `LET_KW`:

```rust
        SyntaxKind::IF_EXPR => {
            let has_let = n
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .any(|t| t.kind() == SyntaxKind::LET_KW);
            if has_let {
                // if let pat = scrutinee { then } else { else_ }
                let pat = n
                    .children()
                    .find(|c| super::patterns::is_pat_node(c.kind()))
                    .map(|p| super::patterns::lower_pat(ctx, p))
                    .unwrap_or_else(|| ctx.alloc_pat(HirPat::Wildcard));
                let mut kids: Vec<SyntaxNode> = n.children().collect();
                let scrutinee_idx = kids.iter().position(|c| is_expr_node(c.kind()));
                let scrutinee = if let Some(i) = scrutinee_idx {
                    lower_expr(ctx, kids.remove(i))
                } else {
                    ctx.alloc_expr(HirExpr::Error)
                };
                let then_idx = kids.iter().position(|c| c.kind() == SyntaxKind::BLOCK);
                let then = if let Some(i) = then_idx {
                    lower_block_node(ctx, kids.remove(i))
                } else {
                    ctx.alloc_block(HirBlock { stmts: vec![], tail: None })
                };
                let else_ = kids
                    .into_iter()
                    .find(|c| c.kind() == SyntaxKind::IF_EXPR || c.kind() == SyntaxKind::BLOCK)
                    .map(|c| {
                        if c.kind() == SyntaxKind::BLOCK {
                            let bid = lower_block_node(ctx, c);
                            ctx.alloc_expr(HirExpr::Block(bid))
                        } else {
                            lower_expr(ctx, c)
                        }
                    });
                HirExpr::IfLet { pat, scrutinee, then, else_ }
            } else {
                // (preserve the existing IF_EXPR lowering body here unchanged)
                let mut kids: Vec<SyntaxNode> = n.children().collect();
                let cond_idx = kids.iter().position(|c| is_expr_node(c.kind()));
                let cond = if let Some(i) = cond_idx {
                    lower_expr(ctx, kids.remove(i))
                } else {
                    ctx.alloc_expr(HirExpr::Error)
                };
                let then_idx = kids.iter().position(|c| c.kind() == SyntaxKind::BLOCK);
                let then = if let Some(i) = then_idx {
                    lower_block_node(ctx, kids.remove(i))
                } else {
                    ctx.alloc_block(HirBlock { stmts: vec![], tail: None })
                };
                let else_ = kids
                    .into_iter()
                    .find(|c| c.kind() == SyntaxKind::IF_EXPR || c.kind() == SyntaxKind::BLOCK)
                    .map(|c| {
                        if c.kind() == SyntaxKind::BLOCK {
                            let bid = lower_block_node(ctx, c);
                            ctx.alloc_expr(HirExpr::Block(bid))
                        } else {
                            lower_expr(ctx, c)
                        }
                    });
                HirExpr::If { cond, then, else_ }
            }
        }
```

4. Replace the `PATH_EXPR` arm to detect generic args:

```rust
        SyntaxKind::PATH_EXPR => {
            let segs = path_segments(&n);
            // Collect generics from the last segment's GENERIC_ARG_LIST, if any.
            let generics: Vec<TypeId> = n
                .descendants()
                .find(|d| d.kind() == SyntaxKind::GENERIC_ARG_LIST)
                .map(|gl| {
                    gl.children()
                        .filter(|c| c.kind() == SyntaxKind::GENERIC_ARG)
                        .filter_map(|g| g.children().find(|c| super::items::is_type_node(c.kind())))
                        .map(|t| super::types::lower_type(ctx, t))
                        .collect()
                })
                .unwrap_or_default();
            if generics.is_empty() {
                HirExpr::Path(segs)
            } else {
                HirExpr::PathGeneric { segments: segs, generics }
            }
        }
```

- [ ] **Step 5: Update dump.rs**

In `crates/sdust-hir/src/dump.rs`, find the `HirExpr` match and add cases for the new variants. Look at the existing structure first:

```bash
grep -n "HirExpr::" crates/sdust-hir/src/dump.rs
```

Then add to the match (after `Cast` case):

```rust
        HirExpr::Lambda { params, ret, body } => format!(
            "(lambda ({}) {} {})",
            params.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(" "),
            ret.map(|t| dump_type(pkg, t)).unwrap_or_else(|| "()".into()),
            dump_block(pkg, *body),
        ),
        HirExpr::IfLet { pat, scrutinee, then, else_ } => format!(
            "(if-let {} {} {}{})",
            dump_pat(pkg, *pat),
            dump_expr(pkg, *scrutinee),
            dump_block(pkg, *then),
            else_.map(|e| format!(" {}", dump_expr(pkg, e))).unwrap_or_default(),
        ),
        HirExpr::Run(e) => format!("(run {})", dump_expr(pkg, *e)),
        HirExpr::PathGeneric { segments, generics } => format!(
            "(path-g [{}] [{}])",
            segments.join("."),
            generics.iter().map(|t| dump_type(pkg, *t)).collect::<Vec<_>>().join(" "),
        ),
```

Match the exact function-name conventions already in `dump.rs` (`dump_pat`, `dump_expr`, `dump_block`, `dump_type` — adjust if names differ).

- [ ] **Step 6: Run HIR tests**

```bash
cargo test -p sdust-hir
```

Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add crates/sdust-hir/
git commit -m "$(cat <<'EOF'
HIR: lower Lambda, IfLet, Run, PathGeneric

Adds four new HirExpr variants and their lowering paths:

- Lambda { params, ret, body } from LAMBDA_EXPR
- IfLet { pat, scrutinee, then, else_ } from IF_EXPR with LET_KW
- Run(inner) from RUN_EXPR
- PathGeneric { segments, generics } from PATH_EXPR with turbofish

Dump output extended for snapshot tests. The plain `If` and `Path`
arms continue to handle their original (no-let, no-generics) forms.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Restore examples 19 and 20, simplify 11 and 18

**Files:**
- Modify: `examples/19_backend_service.sd`, `examples/20_frontend_component.sd`, `examples/11_budget_block.sd`, `examples/18_sandbox.sd`

- [ ] **Step 1: Restore example 11 (budget)**

Replace `examples/11_budget_block.sd` with:

```
fn run_job(input: Bytes) -> Result!RunErr {
  budget {
    cpu 150ms
    wall 2s
    mem 128MiB
    mb 1k
  } run {
    job(input)?
  }
}
```

- [ ] **Step 2: Restore example 18 (sandbox with run inside body)**

Replace `examples/18_sandbox.sd` with:

```
// Note: spec §16.1 places `sandbox` at top level; slice 2 keeps it
// in expression position (wrapped in fn tool_run) and uses the
// real `run` body form. Top-level sandbox items are a slice-3 item.
fn tool_run(input: Bytes) -> Unit!RunErr {
  sandbox ToolRun with {
    fs.read = ["/models", "/tmp/input.json"]
    fs.write = ["/tmp/out"]
    net = ["api.example.com:443"]
    cpu = 150ms
    wall = 2s
    memory = 128MiB
    mailbox = 1k
  } {
    run job(input)?
  }
}
```

- [ ] **Step 3: Restore example 19 (backend service)**

Replace `examples/19_backend_service.sd` with:

```
package search_api

use std.http
use std.json
use std.trace

protocol Search {
  Query(q: Str) -> Json!SearchErr
}

agent Searcher(net, model): Search {
  cache = Map::[Str, Json]{}

  on Query(q) {
    if let Some(hit) = cache.get(q) {
      return Ok(hit)
    }

    arena turn {
      let emb = model.embed(q) @500ms?
      let docs = net.post("https://idx.local/search", emb) @1s?
      let out = json.encode(docs)?
      cache[q] = out
      Ok(out)
    }
  }
}

agent Api(searcher): http.Handler {
  on Request(req) {
    let q = req.query("q").ok_or(SearchErr.BadReq)?
    let body = searcher?Query(q) @2s?
    http.ok(body)
  }
}

fn main(net: Net, model: Model) -> Unit!MainErr effect net, model, spawn {
  let searcher = spawn Searcher(net, model)
  let api = spawn Api(searcher)
  http.serve(":8080", api)?
}
```

- [ ] **Step 4: Restore example 20 (frontend component)**

Replace `examples/20_frontend_component.sd` with:

```
package counter_web

use std.dom

protocol CounterUi {
  Click() -> Unit
}

agent Counter(dom): CounterUi {
  n = 0

  fn draw() {
    dom.set_text("#count", n.to_str())
  }

  on Click() {
    n += 1
    draw()
  }
}

export fn mount(dom: Dom) {
  let c = spawn Counter(dom)
  dom.on("#inc", "click", fn() { c!Click() })
}
```

- [ ] **Step 5: Run sdust check on each**

```bash
cargo run -p sdust-cli -- check examples/11_budget_block.sd
cargo run -p sdust-cli -- check examples/18_sandbox.sd
cargo run -p sdust-cli -- check examples/19_backend_service.sd
cargo run -p sdust-cli -- check examples/20_frontend_component.sd
```

Expected: each exits 0 with no diagnostics.

- [ ] **Step 6: Run sweep tests**

```bash
cargo test -p sdust-fmt
cargo test --workspace
```

Expected: all pass. The idempotence/round-trip sweeps still rely on the identity-passthrough formatter (real per-node formatting lands in Task 9–12), so they should be unaffected.

- [ ] **Step 7: Commit**

```bash
git add examples/
git commit -m "$(cat <<'EOF'
Examples: restore spec-original syntax for 11, 18, 19, 20

Now that slice-2 parser additions (turbofish, if-let, lambda, run,
keyword-tolerant methods, keyword effects, k/m size suffix) have
landed, the four divergent examples are rewritten to match the spec:

- 11: `mb 1024` -> `mb 1k`
- 18: `{ job(input) }` -> `{ run job(input)? }` (keeps fn-wrapper)
- 19: Map.new() -> Map::[Str, Json]{}, match->if-let, +spawn effect
- 20: dom.listen(...) -> dom.on(..., fn() { c!Click() })

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: CLI — `sdust explain <CODE>`

**Files:**
- Modify: `crates/sdust-diagnostics/src/codes.rs`, `crates/sdust-cli/src/main.rs`, `crates/sdust-cli/src/cmd/mod.rs`
- Create: `crates/sdust-cli/src/cmd/explain.rs`, `crates/sdust-cli/tests/explain.rs`

- [ ] **Step 1: Add explain lookup to codes.rs**

Append to `crates/sdust-diagnostics/src/codes.rs`:

```rust
/// Returns a 2-4 sentence human-readable explanation for a diagnostic
/// code, suitable for `sdust explain SDxxxx`. Returns None for unknown
/// codes.
pub fn explain(code: DiagCode) -> Option<&'static str> {
    Some(match code.0 {
        1 => "SD0001: Unexpected token. The lexer or parser found a token \
              that doesn't fit the current grammar context. Check for typos, \
              missing punctuation, or a misplaced keyword.",
        2 => "SD0002: Unterminated string literal. A string starts with \" \
              but never closes before end-of-line or end-of-file. Add the \
              closing quote, or escape any embedded \" as \\\".",
        3 => "SD0003: Invalid escape sequence. The character after \\ in a \
              string or char literal is not a recognized escape. Valid \
              escapes: \\n, \\t, \\r, \\\\, \\\", \\', \\x{HH}, \\u{HHHH}.",
        4 => "SD0004: Unknown duration unit. Stardust duration literals use \
              one of `ns`, `us`, `ms`, `s`, `m`, `h` as the trailing unit.",
        10 => "SD0010: Expected an item. At the top level (or inside a mod), \
               the parser expected one of: fn, struct, enum, type, use, mod, \
               package, agent, protocol, supervisor, extern, export, impl, \
               trait, const, macro.",
        11 => "SD0011: Expected an expression. The parser reached a position \
               where an expression must appear but found something else \
               (such as a closing delimiter or a statement keyword).",
        12 => "SD0012: Mismatched delimiter. An opening `(`, `[`, or `{` was \
               not paired with the matching closing delimiter, or they were \
               crossed.",
        20 => "SD0020: Duplicate `on` handler. An agent body declared two \
               handlers for the same message. Each protocol message may have \
               at most one `on Message` handler per agent.",
        21 => "SD0021: `pub` function needs a return type. Public functions \
               must declare an explicit return type (`-> T`) so callers in \
               other modules can rely on the signature. Add `-> Unit` if the \
               function returns nothing.",
        30 => "SD0030: Recursion depth limit exceeded. The parser nested \
               deeper than the configured limit. This usually indicates \
               adversarial or accidentally pathological input; refactor the \
               source to reduce nesting.",
        1001 => "SD1001: Unresolved name. The HIR lowerer could not resolve \
                 a name reference to any binding in scope. Check the spelling \
                 and ensure the binding's `use` or declaration is visible.",
        1002 => "SD1002: `use` resolves to nothing. The path on the right of \
                 `use` does not name any importable item. Verify the package \
                 and module path; remember that paths use `.` as the \
                 separator.",
        _ => return None,
    })
}
```

- [ ] **Step 2: Write the failing CLI test**

Create `crates/sdust-cli/tests/explain.rs`:

```rust
use std::process::Command;

fn sdust(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_sdust"))
        .args(args)
        .output()
        .expect("run sdust");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn explain_known_code_succeeds() {
    let (code, stdout, _stderr) = sdust(&["explain", "SD0001"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Unexpected token"), "stdout: {}", stdout);
}

#[test]
fn explain_unknown_code_fails() {
    let (code, _stdout, stderr) = sdust(&["explain", "SD9999"]);
    assert_ne!(code, 0);
    assert!(stderr.to_lowercase().contains("unknown"), "stderr: {}", stderr);
}

#[test]
fn explain_bad_format_fails() {
    let (code, _stdout, stderr) = sdust(&["explain", "wat"]);
    assert_ne!(code, 0);
    assert!(stderr.to_lowercase().contains("expected"), "stderr: {}", stderr);
}
```

- [ ] **Step 3: Run to confirm failure**

```bash
cargo test -p sdust-cli explain
```

Expected: FAIL — `explain` subcommand doesn't exist.

- [ ] **Step 4: Add the CLI subcommand and handler**

Create `crates/sdust-cli/src/cmd/explain.rs`:

```rust
use sdust_diagnostics::codes;

pub fn run(arg: &str) -> i32 {
    // Accept formats: SD0001, sd0001, 1, 0001
    let num = if let Some(rest) = arg.strip_prefix("SD").or_else(|| arg.strip_prefix("sd")) {
        rest.parse::<u16>()
    } else {
        arg.parse::<u16>()
    };
    let n = match num {
        Ok(n) => n,
        Err(_) => {
            eprintln!("error: expected diagnostic code like `SD0001`, got `{}`", arg);
            return 2;
        }
    };
    let code = codes::DiagCode::new(n);
    match codes::explain(code) {
        Some(text) => {
            println!("{}", text);
            0
        }
        None => {
            eprintln!("error: unknown diagnostic code {}", code.as_str());
            1
        }
    }
}
```

In `crates/sdust-cli/src/cmd/mod.rs`, add `pub mod explain;`.

In `crates/sdust-cli/src/main.rs`, extend `Cmd`:

```rust
    /// Print a human-readable explanation of a diagnostic code.
    Explain { code: String },
```

And in `main()`:

```rust
        Cmd::Explain { code } => cmd::explain::run(&code),
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p sdust-cli
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add crates/sdust-diagnostics/src/codes.rs crates/sdust-cli/
git commit -m "$(cat <<'EOF'
CLI: add `sdust explain <CODE>` subcommand

`codes::explain(DiagCode) -> Option<&'static str>` ships a static
lookup table of human-readable explanations for every assigned code.
`sdust explain SD0001` prints the body and exits 0; unknown codes
exit 1; malformed input exits 2.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Formatter — types & patterns printers

**Files:**
- Modify: `crates/sdust-fmt/src/fmt/types.rs`, `crates/sdust-fmt/src/fmt/patterns.rs`, `crates/sdust-fmt/src/fmt/mod.rs`
- Test: `crates/sdust-fmt/tests/printer.rs`

The formatter strategy: each module exports `pub fn <node_kind_lower>(n: &SyntaxNode) -> Doc`. Fall back to `Doc::text(n.text().to_string())` for anything not yet implemented. This keeps the slice-1 sweeps green while we add per-node printers incrementally.

- [ ] **Step 1: Add a shared fallback helper in fmt/mod.rs**

Replace `crates/sdust-fmt/src/fmt/mod.rs` with:

```rust
use crate::doc::Doc;
use sdust_syntax::SyntaxNode;

pub mod agents;
pub mod concurrency;
pub mod exprs;
pub mod items;
pub mod patterns;
pub mod types;

/// Verbatim fallback: emit the node's source text unchanged. Used as the
/// "conservative" branch in per-node formatters until each kind has a
/// canonical printer.
pub fn verbatim(n: &SyntaxNode) -> Doc {
    Doc::text(n.text().to_string())
}

/// Format a parsed source file as a [`Doc`].
pub fn file(node: &SyntaxNode) -> Doc {
    items::file(node)
}
```

- [ ] **Step 2: Implement types.rs printers**

Replace `crates/sdust-fmt/src/fmt/types.rs` with:

```rust
use crate::doc::Doc;
use sdust_syntax::{SyntaxKind, SyntaxNode};

/// Format any type-level node. Falls back to verbatim for nodes we
/// don't yet canonicalize.
pub fn type_expr(n: &SyntaxNode) -> Doc {
    match n.kind() {
        SyntaxKind::TYPE_PATH => type_path(n),
        SyntaxKind::TYPE_BORROW => type_borrow(n),
        SyntaxKind::TYPE_TUPLE => type_tuple(n),
        SyntaxKind::TYPE_ARRAY => type_array(n),
        SyntaxKind::TYPE_FN => type_fn(n),
        SyntaxKind::TYPE_RESULT_SUGAR => type_result_sugar(n),
        SyntaxKind::TYPE_UNION => type_union(n),
        _ => super::verbatim(n),
    }
}

fn type_path(n: &SyntaxNode) -> Doc {
    // PATH possibly followed by GENERIC_ARG_LIST
    let mut parts = Vec::new();
    for child in n.children() {
        match child.kind() {
            SyntaxKind::PATH => parts.push(path_node(&child)),
            SyntaxKind::GENERIC_ARG_LIST => parts.push(generic_args(&child)),
            _ => {}
        }
    }
    Doc::concat_all(parts)
}

fn path_node(n: &SyntaxNode) -> Doc {
    // PATH_SEGMENT (NAME_REF) joined with `.`
    let mut segs = Vec::new();
    for seg in n.children().filter(|c| c.kind() == SyntaxKind::PATH_SEGMENT) {
        let name = seg
            .children()
            .find(|c| c.kind() == SyntaxKind::NAME_REF)
            .map(|nr| Doc::text(nr.text().to_string()))
            .unwrap_or(Doc::nil());
        // Turbofish on this segment, if any
        let gen = seg
            .children()
            .find(|c| c.kind() == SyntaxKind::GENERIC_ARG_LIST)
            .map(|gl| Doc::concat(Doc::text("::"), generic_args(&gl)));
        let mut seg_doc = name;
        if let Some(g) = gen {
            seg_doc = Doc::concat(seg_doc, g);
        }
        segs.push(seg_doc);
    }
    Doc::join(Doc::text("."), segs)
}

fn generic_args(n: &SyntaxNode) -> Doc {
    let args: Vec<Doc> = n
        .children()
        .filter(|c| c.kind() == SyntaxKind::GENERIC_ARG)
        .filter_map(|g| g.children().next().map(|t| type_expr(&t)))
        .collect();
    Doc::concat(
        Doc::text("["),
        Doc::concat(Doc::join(Doc::text(", "), args), Doc::text("]")),
    )
}

fn type_borrow(n: &SyntaxNode) -> Doc {
    let mut head = Doc::text("&");
    let has_mut = n
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|t| t.kind() == SyntaxKind::MUT_KW);
    if has_mut {
        head = Doc::concat(head, Doc::text("mut "));
    }
    let inner = n
        .children()
        .next()
        .map(|c| type_expr(&c))
        .unwrap_or(Doc::nil());
    Doc::concat(head, inner)
}

fn type_tuple(n: &SyntaxNode) -> Doc {
    let parts: Vec<Doc> = n.children().map(|c| type_expr(&c)).collect();
    Doc::concat(
        Doc::text("("),
        Doc::concat(Doc::join(Doc::text(", "), parts), Doc::text(")")),
    )
}

fn type_array(n: &SyntaxNode) -> Doc {
    // [T] or [T; N]
    let mut children = n.children();
    let elem = children.next().map(|c| type_expr(&c)).unwrap_or(Doc::nil());
    let len = children.next().map(|c| super::verbatim(&c));
    let inner = match len {
        Some(l) => Doc::concat(elem, Doc::concat(Doc::text("; "), l)),
        None => elem,
    };
    Doc::concat(Doc::text("["), Doc::concat(inner, Doc::text("]")))
}

fn type_fn(n: &SyntaxNode) -> Doc {
    let params: Vec<Doc> = n.children().map(|c| type_expr(&c)).collect();
    // Last child is the return type if THIN_ARROW token is present.
    let has_ret = n
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|t| t.kind() == SyntaxKind::THIN_ARROW);
    let (param_docs, ret_doc) = if has_ret && !params.is_empty() {
        let mut p = params;
        let ret = p.pop();
        (p, ret)
    } else {
        (params, None)
    };
    let body = Doc::concat(
        Doc::text("fn("),
        Doc::concat(
            Doc::join(Doc::text(", "), param_docs),
            Doc::text(")"),
        ),
    );
    match ret_doc {
        Some(r) => Doc::concat(body, Doc::concat(Doc::text(" -> "), r)),
        None => body,
    }
}

fn type_result_sugar(n: &SyntaxNode) -> Doc {
    // ok ! err  (or  ok ! { A, B, C })
    let mut children = n.children();
    let ok = children.next().map(|c| type_expr(&c)).unwrap_or(Doc::nil());
    let err = children.next().map(|c| type_expr(&c)).unwrap_or(Doc::nil());
    Doc::concat(ok, Doc::concat(Doc::text("!"), err))
}

fn type_union(n: &SyntaxNode) -> Doc {
    let parts: Vec<Doc> = n.children().map(|c| type_expr(&c)).collect();
    Doc::concat(
        Doc::text("{"),
        Doc::concat(Doc::join(Doc::text(", "), parts), Doc::text("}")),
    )
}
```

- [ ] **Step 3: Implement patterns.rs printers**

Replace `crates/sdust-fmt/src/fmt/patterns.rs` with:

```rust
use crate::doc::Doc;
use sdust_syntax::{SyntaxKind, SyntaxNode};

pub fn pattern(n: &SyntaxNode) -> Doc {
    match n.kind() {
        SyntaxKind::WILDCARD_PAT => Doc::text("_"),
        SyntaxKind::LITERAL_PAT => super::verbatim(n),
        SyntaxKind::IDENT_PAT => super::verbatim(n),
        SyntaxKind::BINDING_PAT => super::verbatim(n),
        SyntaxKind::REF_PAT => super::verbatim(n),
        SyntaxKind::TUPLE_PAT => tuple_pat(n),
        SyntaxKind::STRUCT_PAT => super::verbatim(n),
        SyntaxKind::ENUM_PAT => enum_pat(n),
        SyntaxKind::RANGE_PAT => super::verbatim(n),
        _ => super::verbatim(n),
    }
}

fn tuple_pat(n: &SyntaxNode) -> Doc {
    let parts: Vec<Doc> = n.children().map(|c| pattern(&c)).collect();
    Doc::concat(
        Doc::text("("),
        Doc::concat(Doc::join(Doc::text(", "), parts), Doc::text(")")),
    )
}

fn enum_pat(n: &SyntaxNode) -> Doc {
    // ENUM_PAT: a PATH followed by parenthesized inner patterns.
    let path = n
        .children()
        .find(|c| c.kind() == SyntaxKind::PATH)
        .map(|p| super::verbatim(&p))
        .unwrap_or(Doc::nil());
    let inner: Vec<Doc> = n
        .children()
        .filter(|c| c.kind() != SyntaxKind::PATH)
        .map(|c| pattern(&c))
        .collect();
    if inner.is_empty() {
        path
    } else {
        Doc::concat(
            path,
            Doc::concat(
                Doc::text("("),
                Doc::concat(Doc::join(Doc::text(", "), inner), Doc::text(")")),
            ),
        )
    }
}
```

- [ ] **Step 4: Verify slice-1 sweep still passes**

```bash
cargo test -p sdust-fmt
```

Expected: all pass — since `items::file` still doesn't exist, `mod.rs::file` will not compile. Fix this by having `items.rs` initially export a trivial passthrough:

```rust
// crates/sdust-fmt/src/fmt/items.rs
use crate::doc::Doc;
use sdust_syntax::SyntaxNode;

/// Top-level: emit verbatim for now (Task 10-12 add per-item printers).
pub fn file(n: &SyntaxNode) -> Doc {
    Doc::text(n.text().to_string())
}
```

Now the workspace builds. Tests pass because the file formatter is still verbatim.

- [ ] **Step 5: Commit**

```bash
git add crates/sdust-fmt/src/fmt/
git commit -m "$(cat <<'EOF'
fmt: canonical printers for types and patterns

First slice of the real per-node formatter: types (path, borrow,
tuple, array, fn, result-sugar, union) and patterns (wildcard,
tuple, enum). Other nodes still fall back to verbatim via the
shared fmt::verbatim helper. Items::file remains a passthrough so
the slice-1 sweep tests keep passing while the rest of the
formatter lands incrementally.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Formatter — expression printers (literals, paths, ops, calls, blocks)

**Files:**
- Modify: `crates/sdust-fmt/src/fmt/exprs.rs`
- Test: `crates/sdust-fmt/tests/canonical.rs` (create)

- [ ] **Step 1: Write canonical-form fixture test**

Create `crates/sdust-fmt/tests/canonical.rs`:

```rust
use sdust_syntax::parse;

fn fmt(src: &str) -> String {
    sdust_fmt::format(parse(src).green)
}

#[test]
fn fmt_simple_binary_ops_normalizes_spacing() {
    let out = fmt("fn f(){1+2*3}");
    assert!(out.contains("1 + 2 * 3"), "got: {}", out);
}

#[test]
fn fmt_idempotent_on_examples() {
    use std::fs;
    use std::path::PathBuf;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = root.join("examples");
    for entry in fs::read_dir(&dir).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().and_then(|s| s.to_str()) != Some("sd") {
            continue;
        }
        let src = fs::read_to_string(&p).unwrap();
        let once = fmt(&src);
        let twice = fmt(&once);
        assert_eq!(once, twice, "not idempotent: {}", p.display());
    }
}
```

- [ ] **Step 2: Implement expression printers**

Replace `crates/sdust-fmt/src/fmt/exprs.rs` with a per-node printer that handles the common shapes and falls back to verbatim. This intentionally avoids restructuring multi-line constructs (those stay verbatim) so round-trip stays trivially safe:

```rust
use crate::doc::Doc;
use sdust_syntax::{SyntaxKind, SyntaxNode};

pub fn expr(n: &SyntaxNode) -> Doc {
    match n.kind() {
        SyntaxKind::LITERAL_EXPR => Doc::text(n.text().to_string()),
        SyntaxKind::PATH_EXPR => path_expr(n),
        SyntaxKind::BINARY_EXPR => binary_expr(n),
        SyntaxKind::UNARY_EXPR => unary_expr(n),
        SyntaxKind::CALL_EXPR => call_expr(n),
        SyntaxKind::METHOD_CALL_EXPR => method_call_expr(n),
        SyntaxKind::FIELD_EXPR => field_expr(n),
        SyntaxKind::INDEX_EXPR => index_expr(n),
        SyntaxKind::TUPLE_EXPR => tuple_or_paren(n),
        SyntaxKind::ARRAY_EXPR => array_expr(n),
        SyntaxKind::SEND_EXPR => send_or_ask(n, '!'),
        SyntaxKind::ASK_EXPR => send_or_ask(n, '?'),
        SyntaxKind::QUESTION_EXPR => Doc::concat(
            expr_or_verbatim_first(n),
            Doc::text("?"),
        ),
        SyntaxKind::DEADLINE_EXPR => deadline_expr(n),
        SyntaxKind::RUN_EXPR => run_expr(n),
        _ => super::verbatim(n),
    }
}

fn path_expr(n: &SyntaxNode) -> Doc {
    n.children()
        .find(|c| c.kind() == SyntaxKind::PATH)
        .map(|p| super::types::type_path_inner(&p))
        .unwrap_or_else(|| super::verbatim(n))
}

fn binary_expr(n: &SyntaxNode) -> Doc {
    // Children: lhs, rhs. The operator is a token sibling.
    let kids: Vec<SyntaxNode> = n.children().collect();
    if kids.len() != 2 {
        return super::verbatim(n);
    }
    // Find the operator token between them.
    let op = n
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| !t.kind().is_trivia())
        .map(|t| t.text().to_string())
        .unwrap_or_default();
    Doc::concat(
        expr(&kids[0]),
        Doc::concat(
            Doc::concat(Doc::text(" "), Doc::text(op)),
            Doc::concat(Doc::text(" "), expr(&kids[1])),
        ),
    )
}

fn unary_expr(n: &SyntaxNode) -> Doc {
    let op = n
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| !t.kind().is_trivia())
        .map(|t| t.text().to_string())
        .unwrap_or_default();
    let inner = n.children().next().map(|c| expr(&c)).unwrap_or(Doc::nil());
    Doc::concat(Doc::text(op), inner)
}

fn call_expr(n: &SyntaxNode) -> Doc {
    let mut kids = n.children();
    let callee = kids.next().map(|c| expr(&c)).unwrap_or(Doc::nil());
    let args = n
        .children()
        .find(|c| c.kind() == SyntaxKind::ARG_LIST)
        .map(arg_list)
        .unwrap_or(Doc::text("()"));
    Doc::concat(callee, args)
}

fn method_call_expr(n: &SyntaxNode) -> Doc {
    let mut iter = n.children();
    let receiver = iter
        .next()
        .map(|c| expr(&c))
        .unwrap_or(Doc::nil());
    let name = n
        .children()
        .find(|c| c.kind() == SyntaxKind::NAME)
        .map(|nm| Doc::text(nm.text().to_string()))
        .unwrap_or(Doc::nil());
    let args = n
        .children()
        .find(|c| c.kind() == SyntaxKind::ARG_LIST)
        .map(arg_list)
        .unwrap_or(Doc::text("()"));
    Doc::concat(receiver, Doc::concat(Doc::text("."), Doc::concat(name, args)))
}

fn field_expr(n: &SyntaxNode) -> Doc {
    let mut iter = n.children();
    let receiver = iter
        .next()
        .map(|c| expr(&c))
        .unwrap_or(Doc::nil());
    let name = n
        .children()
        .find(|c| c.kind() == SyntaxKind::NAME)
        .map(|nm| Doc::text(nm.text().to_string()))
        .unwrap_or(Doc::nil());
    Doc::concat(receiver, Doc::concat(Doc::text("."), name))
}

fn index_expr(n: &SyntaxNode) -> Doc {
    let mut kids = n.children();
    let recv = kids.next().map(|c| expr(&c)).unwrap_or(Doc::nil());
    let idx = kids.next().map(|c| expr(&c)).unwrap_or(Doc::nil());
    Doc::concat(recv, Doc::concat(Doc::text("["), Doc::concat(idx, Doc::text("]"))))
}

fn tuple_or_paren(n: &SyntaxNode) -> Doc {
    let parts: Vec<Doc> = n.children().map(|c| expr(&c)).collect();
    Doc::concat(
        Doc::text("("),
        Doc::concat(Doc::join(Doc::text(", "), parts), Doc::text(")")),
    )
}

fn array_expr(n: &SyntaxNode) -> Doc {
    let parts: Vec<Doc> = n.children().map(|c| expr(&c)).collect();
    Doc::concat(
        Doc::text("["),
        Doc::concat(Doc::join(Doc::text(", "), parts), Doc::text("]")),
    )
}

fn send_or_ask(n: &SyntaxNode, sigil: char) -> Doc {
    let mut iter = n.children();
    let target = iter.next().map(|c| expr(&c)).unwrap_or(Doc::nil());
    let msg = n
        .children()
        .find(|c| c.kind() == SyntaxKind::NAME)
        .map(|nm| Doc::text(nm.text().to_string()))
        .unwrap_or(Doc::nil());
    let args = n
        .children()
        .find(|c| c.kind() == SyntaxKind::ARG_LIST)
        .map(arg_list)
        .unwrap_or(Doc::nil());
    Doc::concat(target, Doc::concat(Doc::text(sigil.to_string()), Doc::concat(msg, args)))
}

fn deadline_expr(n: &SyntaxNode) -> Doc {
    let mut kids = n.children();
    let inner = kids.next().map(|c| expr(&c)).unwrap_or(Doc::nil());
    let dur = kids.next().map(|c| expr(&c)).unwrap_or(Doc::nil());
    Doc::concat(inner, Doc::concat(Doc::text(" @"), dur))
}

fn run_expr(n: &SyntaxNode) -> Doc {
    let inner = n.children().next().map(|c| expr(&c)).unwrap_or(Doc::nil());
    Doc::concat(Doc::text("run "), inner)
}

fn expr_or_verbatim_first(n: &SyntaxNode) -> Doc {
    n.children().next().map(|c| expr(&c)).unwrap_or(Doc::nil())
}

fn arg_list(n: SyntaxNode) -> Doc {
    let parts: Vec<Doc> = n
        .children()
        .map(|c| match c.kind() {
            SyntaxKind::NAMED_ARG => named_arg(&c),
            SyntaxKind::ARG => c.children().next().map(|e| expr(&e)).unwrap_or(Doc::nil()),
            _ => super::verbatim(&c),
        })
        .collect();
    Doc::concat(
        Doc::text("("),
        Doc::concat(Doc::join(Doc::text(", "), parts), Doc::text(")")),
    )
}

fn named_arg(n: &SyntaxNode) -> Doc {
    let name = n
        .children()
        .find(|c| c.kind() == SyntaxKind::NAME)
        .map(|nm| Doc::text(nm.text().to_string()))
        .unwrap_or(Doc::nil());
    let val = n
        .children()
        .find(|c| c.kind() != SyntaxKind::NAME)
        .map(|e| expr(&e))
        .unwrap_or(Doc::nil());
    Doc::concat(name, Doc::concat(Doc::text(": "), val))
}
```

- [ ] **Step 3: Expose types::type_path_inner**

Add to `crates/sdust-fmt/src/fmt/types.rs` (at the end):

```rust
/// Exposed for exprs.rs to render PATH children of PATH_EXPR uniformly.
pub fn type_path_inner(n: &SyntaxNode) -> Doc {
    path_node(n)
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p sdust-fmt
```

These new printers are only invoked when `items::file` dispatches to them — that's the next task. For now this task just compiles & the existing verbatim file printer still drives idempotence/round-trip sweeps. Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/sdust-fmt/src/fmt/exprs.rs crates/sdust-fmt/src/fmt/types.rs crates/sdust-fmt/tests/canonical.rs
git commit -m "$(cat <<'EOF'
fmt: per-node printers for the common expression shapes

Literal, path, binary, unary, call, method-call, field, index, tuple,
array, send, ask, deadline, run, and arg-list printers. Block-bearing
expressions (if, match, for, while, loop, blocks themselves) and the
agent/protocol/supervisor families still fall back to verbatim — they
land in Task 11/12 once items::file dispatches per-item.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: Formatter — items dispatch + canonical blank-line normalization

**Files:**
- Modify: `crates/sdust-fmt/src/fmt/items.rs`, `mod.rs`, `lib.rs`

The strategy: `items::file` walks the FILE node's children and emits each as verbatim, separated by exactly one blank line (canonical). This is the **safe** real-formatter step: it normalizes inter-item spacing without restructuring any item internals (each item still emits its source text).

- [ ] **Step 1: Implement items::file**

Replace `crates/sdust-fmt/src/fmt/items.rs` with:

```rust
use crate::doc::Doc;
use sdust_syntax::{SyntaxKind, SyntaxNode};

/// Format a FILE node: dispatch each top-level item, then separate
/// with exactly one blank line between non-trivia items, plus exactly
/// one trailing newline at EOF.
pub fn file(n: &SyntaxNode) -> Doc {
    // Preserve original leading trivia (so file-leading comments survive).
    let mut parts: Vec<Doc> = Vec::new();

    // Collect (leading-trivia, item-doc) pairs by walking children_with_tokens.
    let mut current_trivia = String::new();
    let mut first_emitted = false;
    for child in n.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(t) => {
                if t.kind().is_trivia() {
                    current_trivia.push_str(t.text());
                }
            }
            rowan::NodeOrToken::Node(item) => {
                // Determine if leading trivia contains any non-whitespace
                // (i.e. comments) or a blank-line marker.
                let trivia_text = std::mem::take(&mut current_trivia);
                let has_comment = trivia_text
                    .lines()
                    .any(|l| l.trim_start().starts_with("//") || l.contains("/*"));
                let blank_before = trivia_text.matches('\n').count() >= 2;

                if has_comment {
                    // Emit comments verbatim, then a newline, then the item.
                    if first_emitted {
                        parts.push(Doc::text("\n\n"));
                    }
                    let comments: String = trivia_text
                        .lines()
                        .filter(|l| l.trim_start().starts_with("//") || l.contains("/*") || l.trim().is_empty())
                        .collect::<Vec<_>>()
                        .join("\n");
                    let comments = comments.trim_matches('\n').to_string();
                    if !comments.is_empty() {
                        parts.push(Doc::text(comments));
                        parts.push(Doc::text("\n"));
                    }
                } else if first_emitted && blank_before {
                    parts.push(Doc::text("\n\n"));
                } else if first_emitted {
                    parts.push(Doc::text("\n"));
                }

                parts.push(item_doc(&item));
                first_emitted = true;
            }
        }
    }

    parts.push(Doc::text("\n"));
    Doc::concat_all(parts)
}

fn item_doc(item: &SyntaxNode) -> Doc {
    match item.kind() {
        // Items we have not yet canonicalized: emit verbatim. The token
        // stream of the item is preserved exactly, so round-trip + idempotence
        // hold by construction.
        _ => super::verbatim(item),
    }
}
```

- [ ] **Step 2: Wire items::file into lib.rs**

`crates/sdust-fmt/src/lib.rs` already calls `fmt::file(&root)` which currently delegates to `items::file`. No change needed if Task 9 already updated `mod.rs` to call `items::file`. Verify:

```bash
grep -n "items::file" crates/sdust-fmt/src/fmt/mod.rs
```

Should show one match.

- [ ] **Step 3: Run sweep tests**

```bash
cargo test -p sdust-fmt
```

Expected: all pass. Both idempotence and round-trip sweeps should remain green because per-item content stays verbatim and we only normalize inter-item whitespace (which the parser preserves losslessly: a re-parse produces the same item kinds).

If they fail, the most likely cause is trailing-trivia handling. Inspect a failure with:

```bash
cargo run -p sdust-cli -- fmt --stdin < examples/01_hello.sd
```

and compare to the file content.

- [ ] **Step 4: Commit**

```bash
git add crates/sdust-fmt/
git commit -m "$(cat <<'EOF'
fmt: dispatch per-item with canonical inter-item spacing

items::file now walks the FILE node and emits each top-level item
verbatim, separated by exactly one blank line (canonical). File-
leading comments survive; trailing blank lines collapse to exactly
one EOF newline. Per-item canonicalization stays deferred to keep
round-trip safe; inter-item normalization is the visible change.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 12: Formatter — sweep regression hardening

**Files:**
- Modify: `crates/sdust-fmt/tests/idempotence.rs`, `crates/sdust-fmt/tests/round_trip.rs`

- [ ] **Step 1: Strengthen the idempotence sweep**

Add a token-text-stream equality check. Append a second test to `crates/sdust-fmt/tests/idempotence.rs`:

```rust
#[test]
fn fmt_preserves_non_trivia_token_stream() {
    use sdust_syntax::{lex, SyntaxKind};
    let files = collect_sd_files();
    let mut failed = Vec::new();
    for path in files {
        let src = fs::read_to_string(&path).unwrap();
        let formatted = sdust_fmt::format(sdust_syntax::parse(&src).green);
        let orig_toks: Vec<(SyntaxKind, String)> = lex(&src)
            .into_iter()
            .filter(|t| !t.kind.is_trivia() && t.kind != SyntaxKind::EOF)
            .map(|t| (t.kind, t.text.to_string()))
            .collect();
        let new_toks: Vec<(SyntaxKind, String)> = lex(&formatted)
            .into_iter()
            .filter(|t| !t.kind.is_trivia() && t.kind != SyntaxKind::EOF)
            .map(|t| (t.kind, t.text.to_string()))
            .collect();
        if orig_toks != new_toks {
            failed.push(format!("{}: non-trivia token stream changed", path.display()));
        }
    }
    assert!(failed.is_empty(), "{} files: {}", failed.len(), failed.join("\n"));
}
```

- [ ] **Step 2: Run**

```bash
cargo test -p sdust-fmt
```

Expected: all pass. If a file fails, it indicates the formatter is dropping/reordering tokens — debug by printing both streams for the failing file.

- [ ] **Step 3: Commit**

```bash
git add crates/sdust-fmt/tests/
git commit -m "$(cat <<'EOF'
fmt: token-stream-equality sweep test

Strengthens the idempotence sweep with a non-trivia token-stream
equality check. Catches any formatter regression that drops or
reorders source tokens (as distinct from the existing round-trip
shape check, which only verifies item-kind sequences).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 13: Gates green sweep — clippy + fmt + workspace tests

**Files:** none (verification)

- [ ] **Step 1: Run the gate**

```bash
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- [ ] **Step 2: Fix any warnings or failures inline**

Common: an unused import in fmt/mod.rs, a missing `pub(crate)` after the items.rs refactor, a stale snapshot in sdust-hir. Address each in place, commit fixes per-issue:

```bash
git add -p
git commit -m "Slice-2 gate fixes: clippy/fmt/snapshot updates"
```

- [ ] **Step 3: Verify all examples check clean**

```bash
for f in examples/*.sd; do
    echo "=== $f ==="
    cargo run --quiet -p sdust-cli -- check "$f" || exit 1
done
```

Expected: every example exits 0.

- [ ] **Step 4: Commit any final fixes**

```bash
git status
# if anything outstanding:
git commit -am "Slice-2: final gate cleanup"
```

---

## Task 14: Docs — amendments, CLI ref, tour updates, README, SLICE1/SLICE2

**Files:**
- Create: `docs/spec/v0.1-amendments.md`, `docs/reference/cli/explain.md`, `SLICE2.md`
- Modify: `docs/reference/diagnostics.md`, `docs/tour/06-agents.md`, `docs/tour/12-extern.md` (if they reference deferrals), `README.md`, `SLICE1.md`

- [ ] **Step 1: Create the amendments doc**

Create `docs/spec/v0.1-amendments.md`:

```markdown
# Stardust v0.1 Spec Amendments

This file tracks decisions made during implementation that extend or
clarify the v0.1 language specification. Each amendment carries the
slice in which it was adopted.

## A1 — Decimal size-literal suffixes `k` and `m` (slice 2)

Spec §3.4 defines `SIZE_LITERAL` as `\d+(B|KiB|MiB|GiB)`. Slice 2 adds
two **decimal** (base-10) suffixes for count-style contexts (e.g.
mailbox depth, queue length):

| Suffix | Meaning | Example |
|--------|---------|---------|
| `k`    | × 1000        | `1k` = 1000 |
| `m`    | × 1_000_000   | `2m` = 2_000_000 |

The lowercase choice deliberately distinguishes them from `KiB`/`MiB`
(which remain binary, × 1024 and × 1_048_576). Uppercase `K` and `M`
are reserved for a future amendment (likely as aliases of `KiB`/`MiB`).

Tokenization: both `k` and `m` lex as `SIZE_LITERAL`. Semantic
interpretation lives in the consumer (e.g. the sandbox/budget block
loader).

## A2 — Expression-position turbofish `Path::[T1, T2]` (slice 2)

Spec §6.2 uses `Path[T1, T2]` in **type** position (e.g.
`Map[Str, Json]`). Expression position is ambiguous because `Map[k]`
already denotes index access.

Slice 2 adopts the Rust-flavored turbofish: in expression position,
`Path::[T1, T2]` carries the generic arguments. Examples:

```sdust
let m = Map::[Str, Json]{}
let s = Some::[I32](42)
```

The `::` disambiguates from `Path[index]`. Type position is
unchanged — `Result[T, E]`, `Map[K, V]` etc. retain bracket-only form.
```

- [ ] **Step 2: Create CLI explain reference**

Create `docs/reference/cli/explain.md`:

```markdown
# sdust explain

Print a human-readable explanation of a Stardust diagnostic code.

## Synopsis

```
sdust explain <CODE>
```

## Arguments

- `<CODE>` — the diagnostic code, in any of these forms:
  - `SD0001`
  - `sd0001`
  - `0001`
  - `1`

## Examples

```sh
$ sdust explain SD0001
SD0001: Unexpected token. The lexer or parser found a token that
doesn't fit the current grammar context. Check for typos, missing
punctuation, or a misplaced keyword.

$ sdust explain SD9999
error: unknown diagnostic code SD9999
$ echo $?
1
```

## Exit codes

| Code | Meaning |
|------|---------|
| 0    | The code was recognized; explanation printed to stdout. |
| 1    | The code is well-formed but not a known Stardust diagnostic. |
| 2    | The argument is not a valid diagnostic-code string. |

## See also

- `docs/reference/diagnostics.md` — full list of assigned codes
- `crates/sdust-diagnostics/src/codes.rs` — source of truth
```

- [ ] **Step 3: Update docs/reference/diagnostics.md**

Open `docs/reference/diagnostics.md` and add a paragraph at the bottom (or near the top) noting that `sdust explain <CODE>` is available. Brief:

```markdown
## Discovering explanations

For any code below, `sdust explain <CODE>` prints a short paragraph
describing the diagnostic, suggested fixes, and related codes. See
`docs/reference/cli/explain.md` for the full command reference.
```

(Read the current file first; if it already enumerates codes 1..30 and 1001..1002, no other change is needed — the explain output mirrors that list.)

- [ ] **Step 4: Update tour pages**

Open `docs/tour/06-agents.md` — if it contains any slice-1 disclaimer about lambdas or `.on`-style keyword methods, remove the disclaimer (these now work). Similarly check tour pages 04/05/13 for mentions of `if let`, turbofish, etc., and remove obsolete caveats.

Search before edit:

```bash
grep -l "slice 1\|slice-1\|TBD\|deferred\|divergen" docs/tour/
```

Edit each match to remove the obsolete notes.

- [ ] **Step 5: Update README roadmap**

In `README.md`, find the roadmap table (under "Roadmap" or "Slices"). Mark slice 2 as shipped:

```
| Slice | Title | Status |
|-------|-------|--------|
| 1 | Parser + Fmt + HIR | Shipped (v0.1.0-phase1) |
| 2 | Fmt completion + syntax polish | Shipped (v0.2.0-phase1-polish) |
| 3 | Type checker | Next |
```

(If the table uses a different schema, preserve it and just flip the status cell + add the tag.)

- [ ] **Step 6: Update SLICE1.md deferrals**

In `SLICE1.md`, replace the "Deferred to slice 2" list with:

```markdown
## Deferred to slice 2 — closed by v0.2.0-phase1-polish

The following slice-1 deferrals shipped in slice 2:

- Real per-node formatter (Wadler/Lindig)
- Lambda expressions
- `if let` patterns
- Generic args in expression position (turbofish `::[T]`)
- Keyword-tolerant method/field names
- Lexer support for `k`/`m` size suffixes
- Sandbox body `run <expr>` keyword form

## Still deferred to slice 3+

- Type checker (slice 3)
- Borrow / ownership / affine checking (slice 3)
- Effect / capability checking (slice 3)
- Top-level `sandbox` items per spec §16.1 (slice 3)
```

- [ ] **Step 7: Create SLICE2.md**

Create `SLICE2.md`:

```markdown
# Stardust Slice 2 — Complete

**Tag:** `v0.2.0-phase1-polish`
**HEAD:** _(filled at tag time)_
**Date:** 2026-05-24

## What landed

- **Real per-node formatter** (`sdust-fmt`): Wadler/Lindig printers for
  types, patterns, common expression shapes; canonical inter-item
  blank-line normalization; verbatim fallback for not-yet-canonicalized
  nodes. All 20 examples remain idempotent and round-trip stable, with
  a new non-trivia token-stream equality sweep guarding regressions.
- **Lambdas**: `fn() { body }` / `fn(x, y) -> T { body }` in expression
  position. HIR: `HirExpr::Lambda { params, ret, body }`.
- **`if let`**: extends `IF_EXPR` with optional `LET_KW`+pattern+`=`.
  HIR: `HirExpr::IfLet { pat, scrutinee, then, else_ }`.
- **Turbofish**: `Path::[T1, T2]` on expression-position path segments.
  HIR: `HirExpr::PathGeneric { segments, generics }`.
- **Keyword-tolerant `.method` / `.field`**: any keyword token may stand
  in name position after a `.`. Parser change only; HIR text capture
  unchanged.
- **Keyword-tolerant effect names**: `effect net, model, spawn` parses.
- **Decimal size suffixes**: `1k` (×1000), `2m` (×1 000 000) lex as
  `SIZE_LITERAL`. See `docs/spec/v0.1-amendments.md` A1.
- **`run <expr>`**: `RUN_EXPR` CST node + HIR `HirExpr::Run(_)`.
  Parseable anywhere an expression is allowed.
- **Spec-original examples**: 11, 18, 19, 20 restored to the syntax in
  spec §16.1/§34/§35. Divergence comments removed.
- **`sdust explain <CODE>`**: ships a static explanation table for
  every assigned diagnostic code.

## Spec interpretation calls (validate in slice 3)

- Turbofish chose `::[T1, T2]` over `[T]` to avoid the index-expression
  ambiguity in expression position. Documented as amendment A2.
- `k`/`m` adopted as **decimal** suffixes (lowercase only); uppercase
  `K`/`M` reserved. Amendment A1.
- `if let` represented as a single CST shape (`IF_EXPR` with optional
  `LET_KW`) rather than a separate `IF_LET_EXPR` node, keeping the AST
  view simpler.
- `RUN_EXPR` is parseable in any expression position. Slice 3's type
  checker is expected to restrict it to sandbox/budget bodies.

## Stats

_(Filled at tag time. Expect: ~150 tests, ~9k lines of Rust.)_

## Still deferred to slice 3+

- Type checker, inference (slice 3)
- Borrow / ownership / affine checking (slice 3)
- Effect / capability checking (slice 3)
- Top-level `sandbox` items per spec §16.1 (slice 3)
- HIR `tail` semantics for `if let` (slice 3 will revisit alongside
  type checking)
- HTML template `{expr}` interpolation parsing (library-level, no
  current consumer)
```

- [ ] **Step 8: Commit docs**

```bash
git add docs/ README.md SLICE1.md SLICE2.md
git commit -m "$(cat <<'EOF'
Docs: slice-2 amendments, CLI ref, tour updates, slice summary

- docs/spec/v0.1-amendments.md: A1 k/m size suffix, A2 turbofish
- docs/reference/cli/explain.md: sdust explain reference
- docs/reference/diagnostics.md: discovery note
- docs/tour/: remove obsolete slice-1 disclaimers
- README.md: roadmap marks slice 2 shipped
- SLICE1.md: deferrals list closed
- SLICE2.md: slice summary

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 15: Tag and push

**Files:** none

- [ ] **Step 1: Final gate**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: all green.

- [ ] **Step 2: Fill in SLICE2.md HEAD and stats**

```bash
HEAD=$(git rev-parse HEAD)
TESTS=$(cargo test --workspace 2>&1 | grep -E "test result.*passed" | awk '{ s += $4 } END { print s }')
LOC=$(find crates -name "*.rs" -exec wc -l {} + | tail -1 | awk '{print $1}')
```

Then edit SLICE2.md to insert the HEAD and the stats. Commit:

```bash
git add SLICE2.md
git commit -m "SLICE2.md: stamp HEAD and stats"
```

- [ ] **Step 3: Tag**

```bash
git tag -a v0.2.0-phase1-polish -m "Slice 2 — formatter completion + syntactic polish"
git push origin main
git push origin v0.2.0-phase1-polish
```

- [ ] **Step 4: Verify**

```bash
git log --oneline -5
git tag -l "v0.2*"
```

Expected: `v0.2.0-phase1-polish` listed at HEAD.
