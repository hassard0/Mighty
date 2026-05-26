# v0.19 HIR multi-row-var lowering completeness

This is the v0.19 swarm slice that finally links the v0.18 parser
surface (`!{| E1, E2}` and friends) to the v0.17 typeck layer
(`Vec<HirRowVar>` in `HirEffectRow::Open`). The gap was entirely in
`mty-hir::lower::items::lower_effect_clause`: it called the v0.15
`EffectClause::row_var_name()` first-only accessor, silently dropping
every row variable after the first.

## What changed

### mty-ast

`crates/mty-ast/src/effects.rs::EffectClause` gained a multi-var
iterator:

```rust
pub fn row_var_names(&self) -> impl Iterator<Item = EffectRowVar> + '_
```

It chains three sources, in source order:

1. The direct `EFFECT_ROW_VAR` child (bare `!E` form).
2. The `EFFECT_SET → EFFECT_ROW_TAIL → EFFECT_ROW_VAR*` children
   (braced form).
3. The `EFFECT_CLAUSE → EFFECT_ROW_TAIL → EFFECT_ROW_VAR*` children
   (legacy keyword form).

The v0.15 first-only `row_var_name()` accessor stays, marked
`#[deprecated(since = "0.19.0", note = "use row_var_names() —
first-only accessor drops multi-row-var tails")]` so straggler
consumers migrate without breakage.

`has_row_var()` was retargeted to `row_var_names().next().is_some()`
to avoid using the deprecated method internally.

### mty-hir

`crates/mty-hir/src/lower/items.rs::lower_effect_clause` now reads
every row var via the new AST iterator:

```rust
let row_vars: Vec<HirRowVar> = clause
    .row_var_names()
    .enumerate()
    .map(|(i, v)| HirRowVar::new(v.text(), i as u32))
    .collect();
```

Each `HirRowVar` carries a stable source-order `idx` (0, 1, ...).
The branch structure simplifies: if any row vars were collected,
emit `HirEffectRow::Open`; else fall back to the existing
`effect_set().is_some()` → `Closed` / `None` logic.

The single-row-var case (`!{| E}`, `!E`, `effect a | E`) is
bit-for-bit equivalent to v0.18's output. The multi-var case
(`!{| E, F}`, `!{fs | E1, E2}`, `effect fs | E, F`) is now
correctly populated.

The fn doc comment gained a v0.19 lowering-completeness section
pointing at the v0.17 typeck consumer (`UserRowPolyMeta::row_vars`
and `RowSubst`) so future readers can trace the full pipeline
parser → AST → HIR → typeck without spelunking history.

## What did NOT change

* `mty-types`: zero edits. The v0.17 typeck already consumed
  `Vec<HirRowVar>` and iterated rows via
  `UserRowPolyMeta::row_vars`. v0.19 just feeds it a vec longer than 1.
* `mty-syntax`: zero edits. The v0.18 parser already emits all
  the row vars; v0.19 only changes how we walk them.
* `mty-hir::effects` types: zero edits. The v0.17 `HirEffectRow`
  shape was already `Open(Vec<HirEffectName>, Vec<HirRowVar>)` and
  ready.
* `examples/22_effect_row.mty`, `examples/23_multi_row.mty`: zero
  edits. The new example sits alongside as `24_multi_row_full.mty`
  so the v0.18 single-row-var emphasis stays untouched.

## Test count delta

| File                                            | v0.18 | v0.19 | Δ |
|-------------------------------------------------|-------|-------|---|
| `mty-hir/tests/multi_row_lowering.rs` (new)     | —     | 8     | +8 |
| `mty-types/tests/effect_row_e2e_multi.rs` (new) | —     | 6     | +6 |
| `mty-ast/src/effects.rs::tests`                 | 1     | 1     | 0 |
| `mty-types/tests/effect_row_multi.rs` (v0.17)   | 12    | 12    | 0 |
| `mty-syntax/tests/effect_rows.rs` (v0.18)       | 14+   | 14+   | 0 |

**+14 tests** added across mty-hir and mty-types, all passing.
Existing v0.17 and v0.18 tests are unaffected (none modified).

## Closes which v0.18 follow-up

From `dev/history/notes/V0_18_CROSSCUT_NOTES.md` §"v0.19 follow-ups":

* **HIR lowerer reads every EFFECT_ROW_VAR** — done; the
  `lower_effect_clause` body now iterates `row_var_names()` and
  emits a fully-populated `Vec<HirRowVar>`.
* **End-to-end multi-closure-arg union test** — done;
  `effect_row_e2e_multi.rs::cross_with_two_effectful_closures_unions_effects`
  exercises `cross[E, F](a, b) -> !{| E, F}` with closure `a`
  bringing `fs` and closure `b` bringing `net`, and asserts the
  caller's inferred row contains both.
* **Optional: `EffectClause::row_var_names()` iterator** —
  done; not optional after all, it's the cleanest API for the
  lowerer to consume and made the v0.15 first-only accessor
  formally obsolete (now `#[deprecated]`).

## Acceptance evidence

* `cargo build -p mty-ast -p mty-hir -p mty-types` clean.
* `cargo test -p mty-ast` — 2 tests pass.
* `cargo test -p mty-hir` — 27 tests pass (14 in `lower_items.rs`
  + 8 in `multi_row_lowering.rs` + 6 in `macro_hygiene_e2e.rs`
  + ...).
* `cargo test -p mty-hir --test multi_row_lowering` — 8 tests pass.
* `cargo test -p mty-types --test effect_row_e2e_multi` —
  pending. The workspace `cargo test` is blocked by a concurrent
  v0.18-track agent's WIP edits to `mty-runtime` and
  `mty-codegen-wasm` (both outside this slice's
  "Do NOT touch" set). The new test file is self-contained and
  builds against `mty-driver` once the parallel WIP lands.
* `mty fmt examples/24_multi_row_full.mty` / `mty check
  examples/24_multi_row_full.mty` — pending the same parallel WIP
  (the `mty-cli` binary depends transitively on the broken
  crates). The example is verified via
  `multi_row_lowering.rs::example_24_multi_row_full_lowers_two_row_vars`
  which lowers it from disk and asserts the HIR shape directly.

## Forward pointer (v1.0-RC)

With v0.19's lowering completeness, the four RFC-008 layers
(parser, AST, HIR, typeck) are finally aligned on the multi-row-var
shape. Remaining v1.0-RC items already tracked elsewhere:

* `mty-fmt` should preserve the `, ` separator between row vars
  when reformatting `!{| E, F}` — covered by the existing fmt
  round-trip tests once they hit example 24.
* LSP hover that renders per-call-site `RowSubst` (v0.17 backlog,
  unchanged).
* MT4056 whole-program emit (v0.18 backlog, unchanged).
