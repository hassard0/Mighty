# Mighty Slice 2 Design — Formatter Completion + Syntactic Polish

**Date:** 2026-05-24
**Status:** Approved (autonomous build — user away, slice-leader = Claude)
**Source spec:** `C:\Users\ihass\Downloads\stardust_language_spec_v0_1.md` (Mighty Language Specification v0.1)
**Slice maps to:** Spec §31.2 Phase 1 polish — close the slice-1 deferrals before slice 3 (the type checker).
**Prior slice:** `v0.1.0-phase1` (commit `308feb4`), summary in `SLICE1.md`.
**Repo:** `C:\Users\ihass\mighty` (remote `hassard0/stardust`).

---

## 1. Goal

Close every slice-1 deferral that is in scope for "complete the surface syntax + canonical formatter," so slice 3 can begin the type checker on a stable, fully-formed front end. Concretely:

- Real per-node Wadler/Lindig formatter that emits canonical source per spec §11 + §28.1.
- Lambda expressions, `if let`, turbofish, keyword-tolerant method/field names, decimal size suffixes, `run <expr>` in sandbox body.
- Restore examples 19 and 20 to spec-original syntax.
- `mty explain <CODE>` for diagnostic discoverability.

The acceptance gate is:

- `cargo test --workspace` green.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `cargo fmt --all -- --check` clean.
- All 20 examples `mty check` clean (including restored 19 & 20).
- All 20 examples idempotent under `mty fmt`.
- All 20 examples round-trip-stable (CST item-shape preserved).

## 2. Non-goals for slice 2

These still belong to later slices:

- Type checker, inference, borrow/affine checking, effect/capability enforcement. (Slice 3.)
- HIR `tail` semantics beyond what slice 1 already infers. (Slice 3 will revisit.)
- MtyIR lowering, runtime, codegen, LSP. (Slices 4–6.)
- Procedural macros, dependent types, distributed agents. (v0.2+ per spec §30.2.)
- HTML template interpolation parsing. (Library-level, deferred.)

## 3. Surface-syntax additions

### 3.1 Lambda expressions

**Syntax** (spec §4 web example, §29):

```
fn() { body }
fn(x: T, y) -> R { body }
```

**CST node:** `LAMBDA_EXPR` already exists. Children:

```
LAMBDA_EXPR
├── FN_KW
├── FN_PARAM_LIST            # reuses items.rs production
│   └── FN_PARAM*
├── RET_TYPE?                # optional `-> T`
└── BLOCK                    # body
```

Each `FN_PARAM` inside a lambda may omit its type (inference deferred to slice 3 — slice 2 lowers `None` for the type).

**Parser entry point:** add `FN_KW` to `primary()` in `exprs.rs`. Currently `FN_KW` is only seen at item level; we disambiguate by context (in expression position, `fn` always introduces a lambda — item-level `fn` is reached through `items::item()`, which runs from `parse_file()` only).

**HIR:** add

```rust
HirExpr::Lambda {
    params: Vec<HirParam>,
    ret: Option<TypeId>,
    body: BlockId,
}
```

Lower with the existing param-list helpers from `items.rs`.

### 3.2 `if let`

**Syntax** (spec §13):

```
if let Pattern = expr { ... } else { ... }
```

**CST:** extend `IF_EXPR` to optionally carry a leading `LET_KW`, a pattern, and an `=` before the scrutinee expression. We do **not** introduce a separate `IF_LET_EXPR` kind — keeping the same wrapper simplifies the formatter, the AST view, and the HIR lowerer (single branch with a `Option<PatId>` annotation).

**Parser change in `stmts.rs::if_expr`:**

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
    if p.eat(ELSE_KW) { ... }
    ...
}
```

**HIR:** add

```rust
HirExpr::IfLet {
    pat: PatId,
    scrutinee: ExprId,
    then: BlockId,
    else_: Option<ExprId>,
}
```

The HIR lowerer for `IF_EXPR` detects the `let` token and branches on it.

### 3.3 Turbofish / expression-position generic args

**Choice (per dispatch recommendation):** `Path::[T1, T2]` — Rust-flavored turbofish, reusing the type-position bracket-generics shape (`Map[Str, Json]` in type position becomes `Map::[Str, Json]` in expression position). The `::` disambiguator is necessary because `Map[k]` in expression position is already an index expression.

**Examples:**

```
let m = Map::[Str, Json].new()
let s = Some::[I32](42)
```

**CST:** `PATH_SEGMENT` gains an optional `GENERIC_ARG_LIST` child after the `NAME_REF`, preceded by `COLON_COLON`. Existing type-position parser stays unchanged (uses `Path[Args]`); only expression-position paths gain the `::[...]` form.

**Parser change in `paths.rs::path`:** after each `NAME_REF`, look ahead — if `COLON_COLON L_BRACK` follows, consume `COLON_COLON`, then call `types::generic_args(p)` to fill the `GENERIC_ARG_LIST`.

**HIR:** path lowering already stores `Vec<String>` segments. We extend `HirExpr::Path` is **not** changed for slice 2; instead, the segments stay strings and the generics annotation is dropped on the floor at HIR (the formatter still sees them in CST). This matches the existing pattern in `nodes.rs::HirType::Path { segments, generics }` — we'll add `generics_per_segment: Vec<Vec<TypeId>>` to `HirExpr::Path`. Actually simpler: introduce a sibling

```rust
HirExpr::PathGeneric {
    segments: Vec<String>,
    generics: Vec<TypeId>,   // applied to the final segment
}
```

Lowering: only emit this variant when generics were present. Existing `HirExpr::Path` stays the no-generic case.

**Note:** in **type position** the spec already uses `Path[Args]` (e.g. `Result[T, E]`, `Map[K, V]`, `Option[T]`). This stays unchanged. Turbofish is expression-only.

### 3.4 Keyword-tolerant method/field names

**Problem:** `dom.on("click", ...)` fails because `on` is `ON_KW`. The parser path in `try_postfix` for `DOT` checks for `IDENT`.

**Choice:** after a `DOT`, treat any `*_KW` token whose text is the source-form keyword as a valid method/field name (i.e., bump the keyword token as the body of a `NAME` node). This is the same trick rustc uses for raw identifiers in expression position, minus the `r#` prefix.

**Parser change:** in `try_postfix`'s `DOT` branch, the lookahead becomes:

```rust
let name_kind = next_nontrivia_kind(p, p.pos + 1);
let is_name = name_kind == IDENT || name_kind.is_keyword();
```

and `paths::name` learns to accept a keyword token in field/method-name position via a new helper `paths::name_or_keyword`.

We only relax this **after a `.`** — top-level identifiers, struct fields, function names, etc. continue to require `IDENT`.

**HIR:** no change — `first_name_text` already takes the token text verbatim.

### 3.5 Lexer size-literal suffixes

**Choice (per dispatch recommendation):** add `k` and `m` (lowercase only) as **decimal** multipliers, distinct from `KiB`/`MiB` (binary). Treat them as `SIZE_LITERAL` tokens so they slot into the same syntactic role.

```
SIZE_LITERAL: \d+(B|KiB|MiB|GiB|k|m)
```

`1k` = 1000, `1m` = 1,000,000. Lowercase only — uppercase `K`/`M` are reserved for future use (probably as aliases of `KiB`/`MiB`, matching spec §3.4 binary intent).

**Document in** `docs/spec/v0.1-amendments.md` (new file), referenced from `docs/reference/manifest.md`.

**HIR `HirLiteral::Size { value, unit }`:** unit string becomes `"k"`/`"m"` accordingly. Numeric conversion stays in the consumer's hands (no semantic interpretation in slice 2).

### 3.6 Sandbox body `run <expr>`

**Problem:** spec §16.1 places `sandbox` at top level and uses

```
sandbox ToolRun with { ... } {
    run job(input)
}
```

In slice 1 we treat `run` only as the budget-block separator. We need to accept `run <expr>` as a statement form inside `BLOCK` (or inside a sandbox body specifically).

**Choice:** add `RUN_EXPR` as a CST node and an expression form. `run` becomes parseable as a leading-keyword expression that evaluates the inner expression. Outside a sandbox body it's still a valid statement (semantically a no-op marker, but slice 2 doesn't enforce that). Slice 3's type checker will restrict it.

**Parser change in `exprs.rs::primary`:** add `RUN_KW => run_expr(p)`. Implementation:

```rust
fn run_expr(p: &mut Parser) -> bool {
    p.start_node(RUN_EXPR);
    p.bump(RUN_KW);
    p.skip_trivia();
    exprs::expr(p);
    p.finish_node();
    true
}
```

We also need `can_start_expr` to include `RUN_KW`.

**Budget block disambiguation:** `budget { ... } run <expr>` already treats `run` as a separator token, not an expression. The budget parser explicitly consumes `RUN_KW` after the closing `}` — it never reaches expression position. So no collision.

**Sandbox body change in `concurrency.rs::sandbox_block`:** keep the existing `block(p)` call — `run job(input)` inside that block now lowers naturally because `RUN_EXPR` is in `can_start_expr`.

**HIR:** add `HirExpr::Run(ExprId)`.

### 3.7 Top-level `sandbox` items

Spec §16.1 actually defines `sandbox` as a top-level **item**, not an expression. Slice 1 only parses it as expression. For slice 2 we add a thin wrapper: at item position, if we see `SANDBOX_KW`, we parse a `SANDBOX_BLOCK` and wrap it as a top-level item. This keeps the CST shape the same (still `SANDBOX_BLOCK`) but lets it appear at the file root.

Actually, **scope decision:** the example 18 currently wraps the sandbox in a `fn tool_run`. To restore §16.1 fidelity we'd need top-level sandbox parsing **and** HIR Item wrapping. That's broader than the dispatch scope (#7 only says "extend sandbox body grammar so `run job(input)` parses"). **Defer top-level sandbox items to slice 3** — keep example 18 wrapped in `fn tool_run` but use `run job(input)?` inside the body.

### 3.8 Restoring examples 19 and 20

After 3.1–3.6 land:

**Example 19** (`19_backend_service.sd`):

- `cache = Map.new()` → `cache = Map::[Str, Json]{}` (struct literal of generic type). **Note:** struct literals with turbofish need a syntactic decision — `Map::[Str, Json]{}` is unambiguous (the path picks up generics, then `{}` is the struct body). The slice-1 lookahead for struct literals (`lookahead_is_struct_literal`) needs to fire after a generic-args list as well.
- `match cache.get(q) { Some(hit) => ..., None => {} }` → `if let Some(hit) = cache.get(q) { return Ok(hit) }`.
- `effect net, model` → `effect net, model, spawn` is still blocked because `spawn` is a reserved keyword and `effect_clause` calls `paths::name` (which is `IDENT`-only). To allow keyword effect names we'd extend `effect_clause` to accept `IDENT | *_KW`. **In scope** — small parser change, no AST/HIR impact since effect names are stored as text.

**Example 20** (`20_frontend_component.sd`):

- `dom.listen("#inc", "click", c)` → `dom.on("#inc", "click", fn() { c!Click() })`. Requires §3.1 (lambda) + §3.4 (keyword method name).

### 3.9 `mty explain <CODE>`

**Behavior:** `mty explain MT0001` prints a paragraph describing the diagnostic. The data lives in `crates/mty-diagnostics/src/codes.rs` as a `pub fn explain(code: DiagCode) -> Option<&'static str>` lookup table.

**CLI:** add `Cmd::Explain { code: String }` to `main.rs`, handler in `cmd::explain::run`.

Each existing code (UNEXPECTED_TOKEN, UNTERMINATED_STRING, INVALID_ESCAPE, UNKNOWN_DURATION_UNIT, EXPECTED_ITEM, EXPECTED_EXPR, MISMATCHED_DELIMITER, DUPLICATE_ON_HANDLER, PUB_NEEDS_RETURN_TYPE, DEPTH_LIMIT_EXCEEDED, UNRESOLVED_NAME, USE_RESOLVES_TO_NOTHING) gets a 2-4 sentence explanation. Update `docs/reference/diagnostics.md` and `docs/reference/cli/` accordingly.

## 4. Real formatter

The slice-1 formatter is identity-passthrough (it emits `node.text()`). Slice 2 implements **canonical** per-node formatting per spec §11 + §28.1.

### 4.1 Architecture

`crates/mty-fmt/src/fmt/*.rs` becomes the body of the formatter, one module per syntactic category:

- `items.rs` — file, use, mod, package, fn, struct, enum, type alias, impl, trait, const, export, extern
- `agents.rs` — protocol, agent, supervisor
- `concurrency.rs` — arena, task scope, budget, sandbox, run
- `exprs.rs` — all expression nodes
- `patterns.rs` — all pattern nodes
- `types.rs` — all type nodes
- `mod.rs` — dispatch `file(node)` builds a top-level `Doc`

Each module exports `fn <node>(n: &SyntaxNode) -> Doc` returning a Wadler/Lindig `Doc`. The dispatch in `mod.rs` walks the file root's children and joins with blank-line separators between top-level items.

### 4.2 Canonical rules (spec §28.1 + house style)

| Construct | Canonical form |
|---|---|
| Indent | 2 spaces |
| EOF | One trailing `\n` |
| Binary op | `a + b` (space around) |
| Call | `f(a, b)` (no space inside parens, single space after comma) |
| Block | `{\n  ...\n}` always |
| If-else | `if cond { ... } else { ... }` |
| Multi-line list | trailing comma |
| Single-line list | no trailing comma |
| Compact agent | `agent X: Y { ... }` (one-line ctor when no ctor params) |
| Compact protocol | `protocol P { Msg(...) -> R }` (single message inline if short enough) |
| `T!E` | preserved when input wrote `T!E`; emitted as `T!E` when canonical |
| Type union sugar | `T!{A, B, C}` preserved with trailing space inside braces only when wrapping |
| Comments | attached to nearest node, leading position preserved |

### 4.3 Trivia attachment

Slice-1 stub `trivia.rs` is filled in. The strategy:

- For each `SyntaxNode`, we collect **leading trivia** (whitespace + comments immediately preceding the node's first non-trivia token) and **trailing trivia** (line comments on the same line, if any).
- A `LineComment` (`//...`) preceding a top-level item attaches to that item and is emitted before its `Doc`.
- A `LineComment` at end of line stays trailing on the same line.
- `BlockComment` and `DocComment` follow the same attachment rule.
- Blank lines between top-level items are preserved as **at most one** blank line (canonical).

Implementation: a `Trivia` helper that takes a `SyntaxNode` and returns `(Vec<&SyntaxToken>, Vec<&SyntaxToken>)` for (leading, trailing).

### 4.4 Idempotence & round-trip strategy

The slice-1 sweep tests already pass trivially. To keep them green after the real formatter lands:

1. Each per-node formatter is **conservative**: when in doubt, fall back to `Doc::text(n.text().to_string())` for that subtree. This guarantees no information loss.
2. We add a property test (`tests/idempotence.rs` already exists, extend it) that for each example, after `format(parse(src))` we re-parse and re-format, byte-for-byte equal.
3. Round-trip test (`tests/round_trip.rs`) already verifies item-shape equality. We extend to verify **token-text-stream equality** modulo whitespace.

### 4.5 Width

Default 100 columns (already in `Layout::default`). When a group fits, render flat; when not, break. Indent is always 2 spaces.

## 5. Test plan

- **Lexer** (`tests/lexer.rs`): add cases for `1k`, `2m`, `4096k`. Verify they tokenize as `SIZE_LITERAL`.
- **Parser** (`tests/parse_exprs.rs`):
    - lambdas — `fn() { 0 }`, `fn(x: I32) -> I32 { x + 1 }`, `fn(x, y) { x + y }`.
    - `if let` — `if let Some(x) = opt { x } else { 0 }`, `if let Ok(n) = parse(s) { ... }`.
    - turbofish — `Map::[Str, I32].new()`, `Some::[I32](42)`, `Vec::[T]::new()` (chained segments).
    - keyword method names — `dom.on("click")`, `agent.spawn()`, `x.match(...)`.
    - sandbox body `run` — `sandbox X with { ... } { run job(x) }`.
- **HIR** (`tests/lower_items.rs`): new `lower_lambda`, `lower_if_let`, `lower_run`, `lower_path_generic`.
- **CLI** (`tests/explain.rs` new): `mty explain MT0001` prints non-empty body, exits 0; `mty explain MT9999` exits 1 with "unknown code".
- **Formatter** (`tests/fmt/canonical/`): add fixture files exercising each canonical rule. Sweep test (`tests/idempotence.rs`) verifies idempotence on all examples + fixtures.
- **End-to-end** (`tests/conformance/`): `mty check examples/19_backend_service.sd` passes with restored syntax. Same for #20.

## 6. Risks

- **Per-node formatter regression risk:** the slice-1 sweep tests pass because the formatter is identity. Real formatting could break the round-trip test for any node we forget to handle. Mitigation: conservative fallback (4.4 #1) plus per-category insta snapshots so regressions are caught early.
- **Turbofish ambiguity:** `Path::[T]` collides with `Path :: [T]` (two-token path-separator + array-literal). The `[` after `::` is unambiguous only because no other construct uses `::[`. Verified — no other parser path consumes `COLON_COLON` followed by `L_BRACK`.
- **`if let` HIR fan-out:** adding `HirExpr::IfLet` means slice-3 (type checker) needs to handle it. Acceptable — desugaring it to `match` at HIR is also reasonable but loses fidelity for the formatter (which works off CST, so no impact). Decision: keep as a distinct HIR variant.
- **Keyword-as-method-name in formatter:** the formatter must emit the raw keyword text, not the keyword's nominal source form (they're the same — `on` for `ON_KW`). No risk.

## 7. Out-of-scope corner-cases (defer to slice 3)

- Effect names of arbitrary expression-keyword form (`yield`, `await`, etc.). Spec §10 doesn't enumerate. We only need `spawn` for example 19.
- `run <expr>` outside a sandbox/budget context is parseable but type-checker should reject. Slice 3 work.
- Top-level `sandbox` items (spec §16.1). Slice 3 work.
- HIR `tail` inference for `if let` — for slice 2 we treat the body identically to `if`.
- Doc-comment Markdown parsing. Out of scope; we just preserve them as text.

## 8. Acceptance

Repository state at slice end:

- All 9 dispatched features land or are explicitly deferred in `SLICE2.md` with rationale.
- Tag `v0.2.0-phase1-polish` points at the slice tip.
- README roadmap table marks slice 2 shipped.
- `SLICE1.md` updated to remove closed deferrals.
- `SLICE2.md` summarizes what landed and what's still open.
- Tour pages 06 and 12 updated to reflect lambdas and the new examples.
- CLI reference documents `mty explain`.
- `docs/spec/v0.1-amendments.md` documents the `k`/`m` size suffix amendment and the `::[T]` turbofish choice.
