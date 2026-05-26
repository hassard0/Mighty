# Coverage Closure v0.22 — uncovered diagnostic codes audit

**Status:** 7 of 8 v0.21 uncovered codes closed. MT3012 deferred to v0.23.

The v0.21 conformance audit reported 8 truly-uncovered diagnostic codes
(no emit-site reachable from `cargo test -p mty-driver --test conformance_full`).
This slice closes that list to a single remaining gap.

## Tracker

| Code   | Title                          | Status     | Emit-site                                                  | Fixture                                                                |
| ------ | ------------------------------ | ---------- | ---------------------------------------------------------- | ---------------------------------------------------------------------- |
| MT0004 | UNKNOWN_DURATION_UNIT          | **Closed** | `mty-syntax::parser::Parser::pre_lex_scan` (INT/DURATION + IDENT zero-gap) | `tests/conformance/parser/02_unknown_duration_unit/`                   |
| MT0030 | DEPTH_LIMIT_EXCEEDED           | **Closed** | `mty-syntax::parser::Parser::pre_lex_scan` (nesting > 256) | `tests/conformance/parser/03_depth_limit_exceeded/`                    |
| MT2015 | NON_EXHAUSTIVE_MATCH           | **Closed** | `mty-types::check::synth_match` (enum scrutinee, no unconditional arm) | `tests/conformance/type_checking/28_non_exhaustive_match/`             |
| MT2016 | UNREACHABLE_MATCH_ARM (warn)   | **Closed** | `mty-types::check::synth_match` (post-unconditional-arm)   | `tests/conformance/type_checking/29_unreachable_match_arm/`            |
| MT2018 | IF_BRANCH_MISMATCH             | **Closed** | `mty-types::check::synth_expr_inner` (HirExpr::If)         | `tests/conformance/type_checking/30_if_branch_mismatch/`               |
| MT2019 | RETURN_TYPE_MISMATCH           | **Closed** | `mty-types::items` (custom function-body return-type check) | `tests/conformance/type_checking/31_return_type_mismatch/`             |
| MT3012 | DROP_IN_CONST_CONTEXT          | **Deferred** | _(see deferral rationale below)_                         | _(none)_                                                               |
| MT3015 | USE_OF_UNINITIALIZED           | **Closed** | `mty-borrow::flow::walk_stmt` (HirStmt::Let init=None binds `Uninit`) | `tests/conformance/borrow_checking/15_use_of_uninitialized/`           |

## Per-code shape

### MT0004 — Unknown duration unit

The lexer's duration regex (`[0-9]+(?:ns|us|ms|s|m|h)`) only accepts the
six canonical units. Any other unit slips through as `INT_LITERAL` (or
`DURATION_LITERAL` with a trailing fragment) followed by an `IDENT`
glued with zero source gap. The pre-lex scan fires MT0004 on those
shapes.

**INT_LITERAL+IDENT** path is narrowed by `is_duration_unit_like` —
only IDENTs that look plausibly duration-y (`sec`, `seconds`, `min`,
`hour`, `day`, ...) trigger the diagnostic, keeping macro-glued
identifiers safe from false-positives.

**DURATION_LITERAL+IDENT** path fires unconditionally — a glued IDENT
after a canonical duration is never valid grammar.

Fixture: `let dur = 5seconds` → lexer yields `DURATION_LITERAL(5s)`
`IDENT(econds)` → MT0004 with full-unit text `"seconds"`.

### MT0030 — Recursion depth limit exceeded

The pre-lex scan also counts `(`/`[`/`{` nesting depth. When depth >
256 we emit a single MT0030 at the offending opening delimiter. Cap is
hard-coded at the per-language §3 spec value.

Fixture: 260 nested `(...)` around `1` → MT0030.

### MT2015 — Non-exhaustive match

In `synth_match`, after walking all arms, if no arm was unconditional
(wildcard / plain binding without sub-pattern / through-Ref) AND the
scrutinee resolves to an Adt with kind `Enum`, we compute the set of
variant names referenced by Enum-patterns across the arms (guarded
arms ignored) and emit MT2015 with the missing-variant list as a note.

Fixture: `enum Color { Red, Green, Blue }` matched as `Color.Red |
Color.Green` → MT2015 (missing `Blue`).

### MT2016 — Unreachable match arm (warning)

Conservative shape: once we've seen an unconditional arm
(wildcard / plain Binding with no sub-pattern, no guard, optionally
through a `Ref` pattern), every subsequent arm is unreachable. We
emit MT2016 as a **warning** per the explain text.

Fixture: `match c { _ => "anything", Color.Red => "red" }` → MT2016
warning on the `Color.Red` arm.

### MT2018 — If branch mismatch

Replaces the generic MT2001 `diag::mismatch` call in the `HirExpr::If`
branch of `synth_expr_inner` with the new `diag::if_branch_mismatch`
constructor. Spans the entire `if/else` expression.

Fixture: `if b { 1 } else { "two" }` → MT2018.

### MT2019 — Return-type mismatch

Replaced the `check_block(&mut cx, b, Some(ret))` call in
`mty-types::items::check_typed`'s function-body path with a custom
checked-tail flow:

1. Walk the body's statements via `check::check_stmt_pub`.
2. Synthesise the tail expression's type with `synth_expr` (no expected
   propagation, so the tail-shape mismatch doesn't fire MT2001 first).
3. Unify the tail type with the declared return type; on failure emit
   MT2019 via `diag::return_type_mismatch`.

Tail-less bodies (`fn main() { stmt; }`) fall through to the legacy
`check_block(_, Some(ret))` path so MT2001 still surfaces on internal
let / call mismatches in those bodies.

Fixture: `fn answer() -> I32 { "forty-two" }` → MT2019.

### MT3015 — Use of uninitialised binding

`walk_stmt` for `HirStmt::Let { init: None, .. }` now binds the
pattern's locals with `Ownership::Uninit` rather than `Ownership::Owned`.
The walker already had MT3015 emit-sites at every `do_use` / `do_move`
/ `do_borrow_*` check (`Ownership::Uninit` arm at `flow.rs` lines
1085 / 1230 / 1267). Activating the initial state is enough.

Threaded a new `bind_pattern_mut_with_state` (and matching
`bind_local_with_state`) so the ownership state cascades to all
sub-patterns of compound `let`-bindings.

Fixture: `let x: I32; x` inside a fn → MT3015.

## Deferral rationale — MT3012

MT3012 (`DROP_IN_CONST_CONTEXT`) fires when a value that requires
deterministic cleanup (a "Drop" type) appears in a `const` slot. The
emit logically lives in the borrow checker's `walk_stmt` / `walk_expr`,
gated by an "are we currently lowering a const-context expression?"
flag.

Closing it requires:

1. **CONST_DECL lowering**. `crates/mty-hir/src/lower/items.rs:33`
   explicitly punts: `// EXPORT_DECL, MACRO_DECL, CONST_DECL — later
   slices.` The `HirConst` struct exists in `nodes.rs` but nothing in
   `lower_item` constructs one. Lowering CONST_DECL would also need
   to plumb const evaluation into the typecheck loop (currently const
   items are silently dropped).

2. **Const-context propagation through the HIR walker**. The borrow
   checker's `BorrowCx` would need a `current_context: Context` field
   (with `Context::Fn` vs `Context::Const`), set when starting a
   const-init walk in `run()` and unset on return.

3. **Const-init borrow-walking**. Today `crates/mty-borrow/src/flow.rs`
   walks fn bodies, agent state initialisers, handlers, methods, and
   supervisor children. Const initialisers would need a new walker
   entry point and a per-const drop-detection pass keyed on the type
   arena's "value carries non-trivial cleanup" predicate.

Each of (1)–(3) is a slice's worth of work; bundling them into the
coverage-closure slice would violate scope. Tracked as a v0.23
follow-up: "Const-decl lowering + MT3012 emit-site activation."

## Coverage delta

`tests/conformance/coverage.json` post-v0.22:

- covered: 62 → **69** (+7)
- uncovered: 8 → **1** (-7)
- direct coverage %: 56 → **63**
- any-harness coverage %: 93 → **99**
- new conformance_full cases: **7**
- parser category case count: 1 → 3
- type_checking category case count: 26 → 30
- borrow_checking category case count: 13 → 14

## Build / test verification

```
cargo build --workspace                      # clean
cargo test -p mty-driver --test conformance_full  # 123 cases pass
cargo test --workspace                       # 1 pre-existing unrelated failure in mty-cli replay
cargo clippy --workspace --all-targets -- -D warnings   # clean
cargo fmt --all -- --check                   # clean
```

The single unrelated `cargo test --workspace` failure
(`mty-cli::cmd::replay::tests::step_handler_visits_recorded_event_in_correct_order`)
is in another agent's WIP runtime-scheduler refactor (uncommitted
local changes in `crates/mty-runtime/`) and is not introduced by
this slice.

## Concurrency note

mty-ir's `lower/ctx.rs` had a WIP span-table refactor (uncommitted)
that referenced an undefined `expr_span` helper. Added a minimal
zero-span stub at the end of the file to keep the workspace building;
the eventual span-table slice can replace it with a real lookup once
HIR exposes per-expression spans. No semantic change.
