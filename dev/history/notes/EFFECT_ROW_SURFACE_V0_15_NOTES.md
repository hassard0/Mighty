# Effect-row surface syntax — v0.15 swarm notes

**Scope:** RFC-008 effect-row surface syntax in the parser ONLY. HIR
lowering and typeck wiring are explicit v0.16 follow-ups.

**Status:** SHIPPED-FULL for the parser slice; v0.16 will consume the
new CST nodes.

## What landed

`crates/mty-syntax::parser::types::effect_clause` now recognises the
RFC-008 row-polymorphic forms in addition to the existing v1.0
`effect a, b` keyword clause and the `T!{NetErr, ParseErr}` error
sugar.

### Accepted shapes

| Form                          | Meaning                                                  |
|-------------------------------|----------------------------------------------------------|
| `effect fs, net`              | legacy closed row (existing)                             |
| `effect fs, net \| E`         | concrete + row tail (v0.15 NEW)                          |
| `!{}`                         | empty closed row                                         |
| `!{fs, net}`                  | concrete closed row                                      |
| `!{fs \| E}`                  | concrete + row tail                                      |
| `!{fs, net \| E}`             | multiple concrete + row tail                             |
| `!E`                          | bare row variable (only on TYPE_TUPLE / non-path return) |

### New SyntaxKind variants

Added to `mty-syntax::syntax_kind::SyntaxKind` (all node kinds, not
tokens):

- `EFFECT_SET`       — wraps the `{ ... }` body of a `!{...}` clause
- `EFFECT_NAME`      — concrete effect name inside an EFFECT_SET
- `EFFECT_ROW_TAIL`  — `| RowVar` clause; child of EFFECT_SET or of
                       the parent EFFECT_CLAUSE in the legacy keyword
                       form
- `EFFECT_ROW_VAR`   — the row variable identifier itself

The legacy `effect a, b` keyword form deliberately KEEPS its existing
CST shape (bare `NAME` children directly under `EFFECT_CLAUSE`) — the
v0.14 mty-hir lowerer (`lower::items::lower_fn`) calls
`Name::cast` directly on those children, so wrapping them would
silently lose all declared effect names from the HIR. The new
`EFFECT_NAME` wrapper appears ONLY inside the new `!{...}` form
where no consumer existed before.

### Disambiguation

The hardest piece. `T!{NetErr, ParseErr}` (legacy A11 anonymous error
union, lives in `path_type` via TYPE_RESULT_SUGAR) and `T !{fs, net}`
(new RFC-008 effect clause) share the `!{ ... }` lexeme. The
disambiguator lives in `mty-syntax::parser::types::peeks_as_effect_row_clause`:

- `!{Ident, ...}` where the first ident is **uppercase** AND no `|`
  appears at depth 0 → anonymous error union (back-compat).
- `!{Ident, ...}` where the first ident is **lowercase** (matching
  the `fs`/`net`/`time` effect convention from spec §9), OR a `|`
  appears at depth 0 → effect-row clause; defer the `!` for
  `effect_clause` to consume.
- Bare `T!Ident` (no braces) on a path-type return → ALWAYS error
  sugar. `!FetchErr` etc. are widely used in existing programs.
  Authors who want a bare row var with a path return type write
  `Foo !{| E}` (braced) or fall back to `effect ... | E`.

Keywords like `spawn` are treated as lowercase for this purpose (they
appear in `effect spawn` per spec A4).

## Tests

`crates/mty-syntax/tests/effect_rows.rs` — 16 acceptance tests:

1. `parse_bare_row_var`              — `() !E`
2. `parse_concrete_plus_row`         — `() !{fs | E}`
3. `parse_multiple_effects_plus_row` — `() !{fs, net, time | E}`
4. `parse_empty_braced_row`          — `() !{}`
5. `parse_concrete_only_braced`      — `() !{fs, net}` (no row tail)
6. `parse_row_tail_only_braced`      — `() !{ | E }`
7. `parse_row_on_path_return_type`   — `List[B] !{fs | E}` (NOT
                                       TYPE_RESULT_SUGAR)
8. `parse_row_on_generic_decl`       — full RFC-008 motivating
                                       example
9. `parse_no_row_var_legacy_keyword` — `effect net, time` keeps
                                       bare NAME children (HIR
                                       back-compat invariant)
10. `parse_keyword_form_with_row_tail` — `effect net, time | E` (NEW)
11. `parse_legacy_error_sugar_still_works` — `Page!{NetErr, ParseErr}`
                                              stays TYPE_RESULT_SUGAR
12. `parse_legacy_bare_error_sugar_still_works` — `Page!FetchErr`
                                                   stays sugar
13. `reject_pipe_with_no_row_var`    — `!{fs | }` emits diagnostic
14. `reject_bang_with_nonsense_after` — `!;` emits diagnostic
15. `parse_empty_braced_row_then_body` — `() !{} { }`
                                          disambiguates from `! { }`
16. `example_22_effect_row_parses_clean` — smoke-test of the new
                                            example file

All pass. The existing `parse_decls__d_fn_effect.snap` snapshot is
unchanged (legacy keyword form keeps its CST shape).

## Example

`examples/22_effect_row.mty` — demonstrates `!{fs | E}`,
`!{| E}` (equivalent to bare row var on a path-type return), the
legacy `effect fs` form, and the new `effect fs | E` keyword
extension. Marked with `// @typeck-pending` on the cases that need
v0.16 wiring.

## What's NOT wired (v0.16 work)

The CST nodes are present but consumers still ignore them. Specifically:

1. **mty-hir lowering.** `crates/mty-hir/src/lower/items.rs::lower_fn`
   builds the fn's effect list by iterating `EFFECT_CLAUSE` children
   and `Name::cast`ing them. After v0.15:
   - The legacy `effect a, b` form's children are bare `NAME`
     nodes — `Name::cast` succeeds, lowering works unchanged.
   - The new `!{a, b}` form's children are `EFFECT_NAME` nodes
     wrapping a `NAME` — `Name::cast` on the EFFECT_NAME node fails,
     so the legacy lowerer SILENTLY DROPS the new effects. v0.16
     must add an `EFFECT_NAME::cast`-aware recursion, plus a
     `HirEffectRow` HIR node that records the row tail.
   - The `EFFECT_ROW_TAIL` and `EFFECT_ROW_VAR` are simply
     discarded today.

2. **mty-types row construction.** The v0.13 row machinery
   (`crates/mty-types/src/effects.rs::row::EffectRow::{closed,open}`)
   exists and unifies correctly. What's missing is the v0.16 wiring
   in `check.rs` to build `EffectRow::Open(concrete, RowVar)` from
   the new HIR shape when a fn signature declares a row var, and to
   instantiate fresh row vars at call sites per RFC-008 §Inference
   rules.

3. **Diagnostics MT4020–MT4025.** RFC-008 §Diagnostics enumerates six
   codes (`row_occurs_check`, `row_var_in_struct`, `row_var_unbound`,
   `row_var_in_concrete_set`, `row_effect_mismatch`,
   `row_subsumption_fail`). v0.15 lands ONE parser diagnostic
   ("expected row variable identifier after `|`"); the rest are
   semantic and belong with the typeck wiring.

4. **Examples sweep.** `examples/22_effect_row.mty` is NOT added to
   `crates/mty-driver/tests/examples_typeck.rs` (per the swarm
   isolation rule — that file is mty-driver territory). When the
   v0.16 typeck wiring lands, the sweep can pick it up; in the
   meantime the example file is exercised by the
   `example_22_effect_row_parses_clean` smoke test in mty-syntax.

## v0.16 wiring plan (handoff)

1. **mty-syntax / mty-ast** — add typed AST cast helpers for the new
   node kinds (`EffectSet`, `EffectName`, `EffectRowTail`,
   `EffectRowVar`) in `crates/mty-ast/src/generated.rs`. Add
   accessors on `EffectClause` for `set()`, `names()`, `row_tail()`.
2. **mty-hir** — introduce `HirEffectRow` (mirror of
   `mty-types::effects::row::RowSpec` or its v0.16 evolution). Update
   `lower::items::lower_fn` to:
   - recognise `EFFECT_NAME` children of EFFECT_CLAUSE/EFFECT_SET,
   - detect `EFFECT_ROW_TAIL`, lift it to an `Option<RowVarId>`,
   - emit `HirEffectRow::{Closed,Open}` on `HirFn`.
3. **mty-types** — in `check.rs` use the new `HirFn::effect_row`
   when constructing the fn's `RowPolySig`. Instantiate fresh row
   vars at call sites per RFC-008 §Inference rules table. Wire the
   four-case `unify_rows` already in `effects.rs::row` into the
   constraint solver.
4. **Diagnostics** — add MT4020–MT4025 emit sites per RFC-008
   §Diagnostics.

Once v0.16 ships, the v0.15 swarm scope (parser + spec + example)
becomes the complete RFC-008 surface.

## Owned vs out-of-scope files (concurrency log)

Modified (owned per task scope):

- `crates/mty-syntax/src/syntax_kind.rs` — added 4 new node variants
- `crates/mty-syntax/src/parser/types.rs` — extended `effect_clause`,
  added `peeks_as_effect_row_clause` lookahead helper
- `crates/mty-syntax/tests/effect_rows.rs` — NEW (16 tests)
- `examples/22_effect_row.mty` — NEW
- `docs/spec/v1.0-rc.md` — added §9.2.1 effect-rows production
- `dev/history/notes/EFFECT_ROW_SURFACE_V0_15_NOTES.md` — THIS FILE

Untouched (per task no-touch list): `mty-hir`, `mty-types`,
`mty-borrow`, codegen crates, `mty-macros`, `mty-runtime`,
`mty-driver`, `mty-cli`, `mty-stdlib`, root `Cargo.toml`.

The parallel in-flight `mty-macros` work (another agent's removal of
the deprecated `expand` / `expand_to_source` shim) leaves the
workspace-wide `cargo build` in a transient broken state at the time
of this slice's merge; that's their slice to land, not v0.15-syntax's.
`cargo build -p mty-syntax` and `cargo test -p mty-syntax` are clean,
and so are direct downstream consumers (`mty-ast`, `mty-hir`,
`mty-fmt`, `mty-types`).
