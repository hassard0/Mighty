# RFC-008 multi-row-var + broadened diagnostics — v0.17 notes

This slice extends v0.16's single-row-var, single-MT4057-emit user-fn
row-poly wiring into the multi-row-var representation, and activates
four additional reserved-but-dormant diagnostic codes (MT4055,
MT4056, MT4058, MT4059). The v0.16 SHIPPED path is preserved
bit-for-bit; v0.17 widens the representation and adds new emit sites
without changing existing behaviour.

## What ships

### Representation

- `HirEffectRow::Open` now carries `Vec<HirRowVar>` instead of a
  single `HirRowVar`. `row_vars()`, `row_var_count()`, and the
  retained `row_var()` convenience accessor cover both the
  length-1 (v0.16) and length-N (v0.18) shapes.
- `UserRowPolyIndex` gains a `meta: HashMap<FnId, UserRowPolyMeta>`
  side table. Each entry records the row-var names, fn-typed param
  count, fn span/name, and concrete-effect names so the call-site
  walker can emit MT4058/MT4059 without re-walking the HIR.

### Active diagnostics (delta vs v0.16)

| Code   | v0.16 status            | v0.17 status                     |
|--------|-------------------------|----------------------------------|
| MT4055 | Reserved (codes-only)   | **Active** — declaration-time    |
| MT4056 | Reserved                | **Active heuristic** — concrete + row var, no fn-typed param |
| MT4057 | Active (return-position)| Unchanged                        |
| MT4058 | Reserved                | **Active** — call-site arity mismatch |
| MT4059 | Reserved                | **Active** — call-site subsumption fail |

Four new active codes; MT4057 retains its v0.16 semantics. The
declaration-time pass disambiguates the "no fn-typed param" case
across MT4055/MT4056/MT4057 by the surrounding shape:

  * **MT4055** when `params.len() >= 2 && fn_typed_params == 0`
    AND no concrete effects — the author plausibly meant to make
    one of the params a closure.
  * **MT4056** when `concrete.is_empty() == false` AND
    `fn_typed_params == 0` — the row var is structurally inert
    next to the concrete effects.
  * **MT4057** otherwise — bare row var on a parameterless or
    single-non-fn-param fn, classic return-position case.

### Call-site validation (`validate_user_row_dispatch`)

Runs after the inference fixpoint, walks every pub fn body, and
fires MT4058/MT4059 against calls to user-row-poly fns. Mirrors
v0.15's `validate_row_dispatch` (stdlib HOFs); the two are
intentionally complementary. Recurses into composite expression
shapes (Block, If, Match, Lambda body, ...) to catch nested calls.

## Test count delta

- **mty-hir unit tests**: +2 (`open_multi_row_vars_round_trip`,
  `closed_row_has_empty_row_vars`) → 17 total in the crate.
- **mty-types `effect_row_e2e.rs`**: existing 8 preserved + 1
  shape-assertion update for the `Vec<HirRowVar>` change → 8.
- **mty-types `effect_row_multi.rs`** (new): 9 tests covering the
  five new emit codes, the HIR multi-var round-trip, the
  unification two-open path, and three counter-tests for false
  positives.
- **Example**: `examples/23_multi_row.mty` exercises the v0.17
  single-row-var surface (multi-var parser surface is a v0.18
  follow-up).

## What's NOT in scope (v0.18 follow-ups)

- **Parser surface for `!{a | E1, E2}`**: the v0.15 parser only
  emits one `EFFECT_ROW_VAR` per fn. The HIR/typeck layers are
  already multi-var-ready; v0.18 only needs to extend
  `mty-syntax::parser::types::effect_clause_bang` to comma-loop
  through additional row vars.
- **MT4056 whole-program emit**: the v0.17 heuristic catches the
  obvious "no fn-typed param" case at declaration time. A full
  pass that confirms NO caller's closure ever binds the row var
  (and only then promotes the heuristic to an error) is deferred.
- **LSP per-call-site `RowSubst` hover**: surface the actual
  bindings at each call site so authors can see "this `each`
  call binds E ↦ {fs, net}".
- **Multi-row-var closure-typed parameter binding**: today every
  fn-typed param carries the SAME first row var (matches the
  v0.13 stdlib HOF shape). Per-param row-var assignment so that
  `cross[E1, E2](a: fn() !E1, b: fn() !E2)` binds E1 to `a`'s
  effects and E2 to `b`'s independently is a v0.18 typeck
  follow-up.

## Files touched

- `crates/mty-hir/src/effects.rs` — `HirEffectRow::Open` carries
  `Vec<HirRowVar>`; new `row_vars()` and `row_var_count()`
  accessors; 2 new unit tests.
- `crates/mty-hir/src/lower/items.rs` — `lower_effect_clause`
  wraps the single parser-emitted row var in a `Vec<HirRowVar>`.
- `crates/mty-types/src/effects.rs` — extended `UserRowPolyIndex`
  with `meta` side table; rewrote `build_user_row_poly_index` to
  emit MT4055/MT4056 alongside MT4057; added
  `validate_user_row_dispatch` + the supporting `walk_block_for_…`
  / `walk_expr_for_user_row_violations` walkers.
- `crates/mty-types/src/diag.rs` — new active constructor
  `row_var_in_concrete_only`; rewrote `row_var_arity_mismatch`
  and `row_var_subsumption_fail` with v0.17-active messages.
- `crates/mty-types/tests/effect_row_e2e.rs` — shape-assertion
  update for the `Vec<HirRowVar>` change.
- `crates/mty-types/tests/effect_row_multi.rs` (new) — 9 tests.
- `examples/23_multi_row.mty` (new) — single-row-var multi-call
  demo + multi-var parser-gap note.
- `docs/spec/rfcs/RFC-008-effect-rows.md` — new "v0.17 — multi
  row-variable extension" section.
