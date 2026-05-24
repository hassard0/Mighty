# Stardust Slice 2 — Complete

**Tag:** `v0.2.0-phase1-polish`
**HEAD:** `263e5c919898ec2a1bc348e01d632872faba6d07`
**Date:** 2026-05-24

## What landed

- **Real per-node formatter** (`sdust-fmt`): Wadler/Lindig per-node
  printers for types, patterns, and the common expression shapes;
  canonical inter-item blank-line normalization at file level;
  verbatim fallback for not-yet-canonicalized nodes. All 20 examples
  remain idempotent and round-trip-stable, with a new non-trivia
  token-stream equality sweep guarding regressions.
- **Lambdas**: `fn() { body }` and `fn(x: T, y) -> R { body }` in
  expression position. New `HirExpr::Lambda { params, ret, body }`.
- **`if let`**: extends `IF_EXPR` with optional `LET_KW` + pattern +
  `=`. New `HirExpr::IfLet { pat, scrutinee, then, else_ }`.
- **Turbofish**: `Path::[T1, T2]` on expression-position path segments.
  New `HirExpr::PathGeneric { segments, generics }`.
- **Keyword-tolerant `.method` / `.field`**: any keyword token may
  stand in name position after a `.`. Parser change only.
- **Keyword-tolerant effect names**: `effect net, model, spawn` parses.
- **Decimal size suffixes**: `1k` (×1000), `2M` (×1 000 000) lex as
  `SIZE_LITERAL`. See `docs/spec/v0.1-amendments.md` A1.
- **`run <expr>`**: new `RUN_EXPR` CST node + `HirExpr::Run(_)`.
  Parseable in any expression position.
- **Spec-original examples**: 11, 18, 19, 20 restored to the spec
  syntax. Divergence notes removed from 19 and 20; example 18 still
  wraps the sandbox in `fn tool_run` (top-level sandbox items are
  slice 3) and now uses `run job(input)?` inside the body.
- **`sdust explain <CODE>`**: ships a static explanation table for
  every assigned diagnostic code (SD0001..SD0030, SD1001..SD1002).
- **Side-effect parser fix**: `no_struct_literal` context flag set
  by `if`/`while`/`for`/`match` condition parsing so `if x { ... }`
  parses as condition+body rather than `x { ... }` struct literal.

## Spec interpretation calls (validate in slice 3)

- **Turbofish syntax**: chose `::[T1, T2]` over `[T]` to avoid the
  index-expression ambiguity in expression position. Type position
  retains the bracket-only form. Documented as amendment A2.
- **Decimal size suffix case**: chose lowercase `k` and **uppercase
  `M`** (rather than the dispatch-recommended lowercase `m`) to
  avoid collision with the existing `m` (=minutes) DURATION_LITERAL
  suffix. Documented as A1.
- **`if let` CST shape**: a single `IF_EXPR` kind with an optional
  leading `LET_KW` (rather than a distinct `IF_LET_EXPR` node) keeps
  the AST view smaller. HIR lowering branches on `LET_KW` into
  separate `HirExpr::If` and `HirExpr::IfLet` variants.
- **`RUN_EXPR` scope**: parseable in any expression position. Slice 3's
  type checker is expected to restrict it to sandbox and budget bodies
  per spec §16.1.
- **Keyword-tolerant `.method`**: relaxation fires only after `.` and
  inside `effect` clauses. All other identifier positions (struct
  fields, fn names, top-level binders) continue to require `IDENT`.

## Stats

- 174 tests pass (slice 1: 132 → slice 2: 174; +42 new tests)
- 45 commits
- ~8.5k lines of Rust
- 20 examples all `sdust check` clean with spec-original syntax for
  examples 19 and 20 fully restored

## Still deferred to slice 3+

- ~~Type checker, inference (slice 3)~~ → **closed in slice 3**
  (`v0.3.0-typeck`)
- Borrow / ownership / affine checking → slice 4
- Effect closure / capability narrowing enforcement → slice 5
  (signatures are now parsed and carried in slice 3)
- Top-level `sandbox` items per spec §16.1 → slice 5
- HIR `tail` semantics for `if let` — implemented incidentally by the
  slice-3 type checker via `synth_expr(IfLet)`
- HTML template `{expr}` interpolation parsing (library-level, no
  current consumer)
- Per-item canonical printers for the formatter (item-internal
  restructuring — currently items emit verbatim; the per-node
  printers in `fmt::types`, `fmt::patterns`, `fmt::exprs` are
  exposed as library surface but don't yet drive top-level
  formatting). Safe to ship after slice 3's type-checker work
  stabilizes the CST shape.

## Files of note

- `docs/spec/v0.1-amendments.md` — six amendments (A1-A6) explaining
  the slice-2 syntax choices
- `docs/reference/cli/sdust-explain.md` — new CLI subcommand reference
- `docs/superpowers/specs/2026-05-24-slice2-fmt-completion-design.md`
  — slice design
- `docs/superpowers/plans/2026-05-24-slice2-fmt-completion.md` —
  implementation plan
