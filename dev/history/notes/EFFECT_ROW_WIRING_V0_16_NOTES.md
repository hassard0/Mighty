# Effect-Row Wiring — v0.16 notes

## What ships

User-authored row-polymorphic effect annotations from RFC-008 now flow
end-to-end through the compiler:

```
parser (v0.15) → typed AST accessors (v0.16) → HIR (v0.16) → typeck (v0.16)
```

The v0.15 RFC-008 surface syntax (`!E`, `!{a | E}`, `!{| E}`,
`effect a, b | E`) was previously parser-only — the four new
`SyntaxKind` variants (`EFFECT_SET`, `EFFECT_NAME`, `EFFECT_ROW_TAIL`,
`EFFECT_ROW_VAR`) had no consumers. v0.16 wires them through:

- **Typed AST accessors** (`crates/mty-ast/src/effects.rs`): `EffectSet`,
  `EffectName`, `EffectRowTail`, `EffectRowVar` typed views following the
  existing `ast_node!` macro pattern. `EffectClause` gains
  `row_var_name()`, `has_row_var()`, `braced_concrete_names()` accessors
  that the lowerer consumes.
- **HIR shape** (`crates/mty-hir/src/effects.rs`): `HirEffectRow` enum
  (`Closed(Vec<HirEffectName>)` / `Open(Vec<HirEffectName>, HirRowVar)`)
  attached to `HirFn::effect_row` as an additive `Option<_>` field.
  Pure closed-set fns leave it `None` so existing closed-row consumers
  (legacy `HirFn::effects: Vec<String>`) keep working unchanged.
- **HIR lowering** (`crates/mty-hir/src/lower/items.rs`):
  `lower_effect_clause` detects all three v0.15 surface shapes and
  populates BOTH the legacy `effects` view and the new `effect_row`.
- **Typeck wiring** (`crates/mty-types/src/effects.rs`):
  `UserRowPolyIndex` built once per package by
  `build_user_row_poly_index` from `HirFn::effect_row`; threaded
  through `walk_block_effects` / `walk_expr_effects` so the call-site
  walker can propagate closure effects through user fn row variables.

## Diagnostics

Five new diagnostic codes registered in
`crates/mty-diagnostics/src/codes.rs` with `mty explain` text:

| Code   | Name                          | Status            |
|--------|-------------------------------|-------------------|
| MT4055 | `row_var_unused`              | reserved          |
| MT4056 | `row_var_in_concrete_only`    | reserved          |
| MT4057 | `row_var_returned_but_unbound`| **active**        |
| MT4058 | `row_var_arity_mismatch`      | reserved (v0.17)  |
| MT4059 | `row_var_subsumption_fail`    | reserved          |

MT4057 is the active emit-site in v0.16: it fires when a fn declares a
row variable in its effect clause but has no fn-typed parameter from
which the row variable could ever be bound. The other codes are
declared with their `explain` text so v0.17 / future PRs don't
renumber.

RFC-008 originally reserved MT4020..MT4025 for the row-machinery
diagnostics, but those codes were already claimed by the v0.6 trait
codes (`METHOD_AMBIGUOUS`, `METHOD_NOT_FOUND`, etc.). The v0.15
SHIPPED-FULL effort moved the row codes to the MT4050 block (MT4050 +
the four reserved MT4051..MT4054 slots for the originally-RFC-numbered
emit-sites); v0.16 extends the block at MT4055..MT4059.

## Tests

`crates/mty-types/tests/effect_row_e2e.rs` — 8 end-to-end tests:

1. `user_authored_open_row_lowers_to_effect_row_open` — HIR structural
   check + caller effect propagation.
2. `user_authored_row_var_propagates` — minimal vertical slice.
3. `bare_row_var_compatible_with_pure_closure` — counter-test: pure
   closure adds no effects.
4. `concrete_plus_open_row_carries_callee_declared_concrete` — fn
   declared `!{net | E}` propagates closure `fs` correctly.
5. `row_var_in_return_only_emits_mt4057` — MT4057 emit-site test.
6. `legacy_keyword_row_tail_form_propagates` — `effect a, b | E`
   keyword form is recognised + propagated.
7. `pub_fn_closed_caller_rejects_propagated_effect` — MT4001 fires
   when a pub fn's declared closed row doesn't allow the propagated
   effect.
8. `multiple_callsites_propagate_independently` — separate call sites
   of the same row-poly fn carry distinct closure effects.

All 8 pass. The existing `crates/mty-types/tests/stdlib_hof_dispatch.rs`
v0.15 test suite (10 tests) keeps passing — no regression on the
stdlib HOF row-poly dispatch.

## Test count delta

| Suite                                            | v0.15 | v0.16 |
|--------------------------------------------------|-------|-------|
| `mty-ast` (lib)                                  | 0     | 1     |
| `mty-hir::effects` (lib)                         | 0     | 3     |
| `mty-types::tests::effect_row_e2e` (integration) | 0     | 8     |
| **Total new tests**                              | —     | **12**|

Plus `examples/22_effect_row.mty` now participates in the
`conformance_codegen` sweep (22 cases pass — example 22 was previously
skipped via the `@typeck-pending` marker).

## Architectural notes

### Row-propagation as a walk

The v0.16 design models user-fn row instantiation as: "when calling a
user fn whose `HirEffectRow` is `Open(...)`, walk each closure-typed
arg's body and union its effects into the caller's inferred set." This
is OBSERVATIONALLY equivalent to:

1. Instantiating the fn's hypothetical `RowPolySig` (analogous to the
   v0.13 `stdlib_list_map_sig` shape) at the call site.
2. Building a fresh `RowSubst`.
3. Unifying each closure arg's `EffectRow::Closed(...)` against the
   sig's parameter `Var(0)`.
4. Resolving the return row.

The full unification path is exercised by the `effects::row::tests`
unit tests in `crates/mty-types/src/effects.rs` (12 tests covering
all four unification cases). The v0.16 walk-and-union is the
runtime-cheap equivalent for the fixpoint.

A future v0.17 follow-up may switch to the full unification path if /
when row signatures need to encode parameter-specific row constraints
(e.g. `f: fn(A) -> B !{log | E}` where the parameter row carries
fixed concrete effects).

### Closed `!{...}` forms

A `!{}` or `!{fs, net}` (no row var) lowers to
`HirEffectRow::Closed(...)` — exposed on `HirFn::effect_row` so the
v0.16 closed-row paths can flag e.g. `!{a, a}` duplicates in a future
pass. The legacy `effect a, b` keyword form (no row tail) keeps
`effect_row = None` and goes through the existing closed-set
inference unchanged.

### Why not allocate `RowVar` IDs at HIR time

`mty_types::effects::row::RowVar` is densely allocated by a single
`RowSubst`; two `RowVar(0)`s from different substitutions are
unrelated. The HIR is built once per package and consumed by many
separate substitution contexts (one per fn body, one per call site,
plus the package-level effect fixpoint). Allocating concrete `RowVar`
IDs at HIR build time would either pin them across all substitutions
(breaking the "scoped to one table" invariant) or duplicate
bookkeeping in every consumer. Keeping the row variable as a textual
name + per-fn index at the HIR layer and deferring substitution-scoped
ID allocation to typeck matches the v0.13/v0.14 row-machinery design.

## v0.17 follow-ups

1. **Multi-row-var support**: `fn observed[E, F](f: fn()->()!E, g:
   fn()->()!F)` currently lowers both vars to `idx=0` (collapsed to
   one); MT4058 reserves the diagnostic slot for the proper
   multi-row-var error. v0.17 should index by `HashMap<String, u32>`
   in the lowerer.
2. **LSP hover for inferred row vars**: the typed package side table
   (`TypedPackage`) does not yet record per-call-site row
   substitutions. Adding `expr_effect_row: HashMap<ExprId,
   EffectRow>` would let the LSP show "E = {fs}" on hover.
3. **Iterator[A, !E] row-carrying types**: `collect()` currently
   resolves to `{alloc | ?fresh}` (open) because the v0.14 stdlib sig
   has no upstream-iterator row source. v0.17 should model iterator
   chains as row-carrying values so `xs.map(f).collect()`'s row var
   resolves to `f`'s effect row.
4. **MT4059 user-fn subsumption emit-site**: the call-site walker
   currently relies on MT4001 (pub-fn missing effect) to catch row
   propagation that violates the caller's declared closed row. v0.17
   should add a per-call-site MT4059 emit so the diagnostic message
   points at the exact call (analogous to MT4050 for stdlib HOFs).
5. **Bare `!E` after path-typed return**: per the v0.15 parser
   disambiguation (`peeks_as_effect_row_clause`), `Unit !E` always
   parses as legacy `Result[Unit, E]` error sugar. Users wanting the
   bare row var on a path-typed return must use `!{| E}`. v0.17 could
   add an MT0031 parser warning when the form is ambiguous and the
   user appears to want the row-var reading.

## Files changed

```
crates/mty-ast/src/effects.rs              (new, 161 lines)
crates/mty-ast/src/lib.rs                  (+3)
crates/mty-hir/src/effects.rs              (new, 168 lines)
crates/mty-hir/src/lib.rs                  (+2)
crates/mty-hir/src/nodes.rs                (+14)
crates/mty-hir/src/lower/items.rs          (+~72)
crates/mty-types/src/effects.rs            (+~120 in typeck wiring)
crates/mty-types/src/diag.rs               (+~110 in diagnostics)
crates/mty-diagnostics/src/codes.rs        (+~75 with explain text)
crates/mty-types/tests/effect_row_e2e.rs   (new, 8 tests)
examples/22_effect_row.mty                 (rewritten — no `@typeck-pending`)
dev/history/notes/EFFECT_ROW_WIRING_V0_16_NOTES.md  (this file)
```

All under-budget; no sibling-agent crate boundaries crossed.
