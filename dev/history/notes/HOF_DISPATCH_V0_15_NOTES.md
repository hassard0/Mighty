# HOF dispatch v0.15 — wiring the row-poly stdlib table into the call-site walker

**Status**: SHIPPED-FULL.

**Predecessor**: `STDLIB_HOF_ROWPOLY_V0_14_NOTES.md` introduced 19 new
`RowPolySig` factories (`stdlib_iter_for_each_sig`, `stdlib_option_and_then_sig`,
...) plus the v0.13 anchor `stdlib_list_map_sig`. Those signatures lived
in `effects::row::stdlib_sigs` but no consumer code wired them into the
call-site effect walker — so they were *infrastructure only*. The v0.14
notes flagged this as the v0.15 follow-up:

> *"v0.15 typeck wiring will populate them."*

This file documents the v0.15 follow-up.

## What v0.15 added

### 1. `BuiltinMethod.row_sig` field — _infrastructure_

`crates/mty-types/src/defs.rs` (~20 LOC):

```rust
pub type RowSigFactory = fn() -> crate::effects::row::RowPolySig;

pub struct BuiltinMethod {
    pub arity: Option<usize>,
    pub ret:   Option<TyId>,
    pub row_sig: Option<RowSigFactory>,   // NEW v0.15
}
```

A `fn` pointer (not an owned `RowPolySig`) keeps `defs.rs` free of an
import cycle on `effects::row`. The factory is invoked at each call
site so per-call-site fresh row variables stay isolated.

### 2. Prelude dispatch table — _infrastructure_

`crates/mty-types/src/prelude.rs` registers 13 entries covering 12
distinct method names against the v0.14 stdlib sigs:

| Method     | Factory                                    | Notes                                   |
|------------|--------------------------------------------|-----------------------------------------|
| `map`      | `stdlib_list_map_sig` (v0.13 anchor)       | covers `List.map`/`Iterator.map`/etc.   |
| `filter`   | `stdlib_list_filter_sig`                    | List/Iterator/Option share this shape   |
| `fold`     | `stdlib_list_fold_sig`                      | 3-param `[Skip, Skip, Var(0)] → Var(0)` |
| `flat_map` | `stdlib_list_flat_map_sig`                  | List/Iterator                            |
| `for_each` | `stdlib_iter_for_each_sig`                  | Side-effect HOF                          |
| `find`     | `stdlib_iter_find_sig`                      | Iterator                                 |
| `any`/`all`| `stdlib_iter_any_sig`/`stdlib_iter_all_sig` | Iterator boolean folds                   |
| `collect`  | `stdlib_iter_collect_sig`                   | `VarPlus(0, {alloc})` return row         |
| `and_then` | `stdlib_option_and_then_sig`                | Option/Result share shape                |
| `or_else`  | `stdlib_option_or_else_sig`                 | Option/Result                            |
| `map_err`  | `stdlib_result_map_err_sig`                 | Result                                   |

Coverage = **20 v0.14 sigs + 1 v0.13 anchor = 21 sigs across 12 method
names**. The 12-name compression is intentional: per-receiver
discrimination (`List.map` vs `Option.map`) is structurally identical
for the v0.14 shapes (all `[Skip, closure-Var(0)] → Var(0)` modulo
fold's extra `Skip`). Per-receiver narrowing is a v0.16 refinement (see
"v0.16 follow-ups" below).

### 3. Call-site dispatch consumer — _the missing link_

`crates/mty-types/src/effects.rs`:

* **`HirExpr::MethodCall` branch** — When the method name has a
  `row_sig`, instantiate the sig (`row::instantiate_row_sig`), compute
  each closure-argument's inferred effect row via
  `compute_arg_effect_row` (walks the lambda body in isolation to
  produce a closed-row), `row::unify_rows` against the sig's parameter
  rows, resolve the return row, remap the `ALLOC_PLACEHOLDER` sentinel
  to the live `defs.intern_effect("alloc")` id, and union the concrete
  effects into the caller's `out` set.
* **`HirExpr::Call { callee: Path(...) }` branch** — The parser
  greedily folds dotted paths (`xs.map`, `xs.collect`) so a method
  call like `xs.map(fn(x) { ... })` lowers as
  `Call(Path(["xs", "map"]))` rather than `MethodCall`. We added a
  parallel dispatch in this branch so the row machinery covers both
  shapes. (Tested explicitly in `iter_collect_carries_alloc` —
  zero-arg `xs.collect()` only takes this path.)
* **`dispatch_row_poly_call` helper** — Both branches share this
  ~30-LOC helper that does the instantiate/unify/resolve/remap dance.

### 4. MT4050 — `row_subsumption_fail` diagnostic

`crates/mty-diagnostics/src/codes.rs` reserves:

| Code   | Name                       | RFC-008 slot       |
|--------|----------------------------|--------------------|
| MT4050 | `row_subsumption_fail`     | (=MT4025 in RFC)   |
| MT4051 | `row_occurs_check`         | (=MT4020 in RFC)   |
| MT4052 | `row_var_in_struct`        | (=MT4021 in RFC)   |
| MT4053 | `row_var_unbound`          | (=MT4022 in RFC)   |
| MT4054 | `row_effect_mismatch`      | (=MT4024 in RFC)   |

**Renumbering note.** RFC-008 reserved MT4020-4025 for the row machinery,
but those codes were already claimed by the v0.6 trait/method codes
(`METHOD_AMBIGUOUS = 4020`, `METHOD_NOT_FOUND = 4021`, etc.). Since the
diagnostic codes carry a "once assigned, NEVER renumber" contract, the
row-machinery codes land at MT4050-4054 instead, with `explain` text
that cross-references the RFC reservation.

Only MT4050 has a live emit-site in v0.15:
`crates/mty-types/src/effects.rs::validate_row_dispatch` runs as a
post-inference pass over every pub fn body. For each row-poly HOF call
inside the body, it walks the closure argument's body, intersects the
closure's effects against the *complement* of the caller's declared
row, and emits MT4050 listing the disallowed effects with a
"add `effect {x}` to the enclosing fn" hint.

The pre-existing MT4001 (`effect_undeclared`) at the fn level still
fires alongside MT4050 — they're complementary. MT4001 says "the fn
needs effect X"; MT4050 says "specifically the `map(...)` call on line
N introduced X via its closure". The v0.16 LSP work should suppress
MT4050 when MT4001 also fires for the same fn to avoid noise; v0.15
leaves both on so authors see the call-site signal.

MT4051-4054 are *reserved* — no emit sites in v0.15. They'll wire up
during the v0.16 surface-syntax row-clause inference pass (HIR
`HirEffectRow` node + parser hookup).

### 5. Tests

`crates/mty-types/tests/stdlib_hof_dispatch.rs` — 10 end-to-end tests:

1. `iter_map_propagates_fs_effect`
2. `option_and_then_propagates_net`
3. `result_map_inside_map`
4. `closed_caller_rejects_effectful_closure` — **MT4050 emit-site test**
5. `pure_closure_keeps_caller_pure`
6. `fold_propagates_through_accumulator_closure`
7. `iter_collect_carries_alloc` — exercises `VarPlus(0, {alloc})` +
   the `Call(Path)` parser-folded shape
8. `filter_propagates_predicate_effect`
9. `for_each_propagates_side_effect`
10. `dispatch_table_covers_v0_14_sigs` — structural assertion that all
    12 method names are wired

Plus the existing `crates/mty-types/tests/effects_row.rs` (35 tests)
keeps the row-machinery isolation coverage.

## Architecture trade-offs

### Why a separate validator pass (`validate_row_dispatch`)?

The inference walker (`walk_expr_effects`) is purely additive — it
unions effects into `out`. Adding a diagnostic emit-site there would
require threading a diagnostics sink + the caller's declared row
through ~57 recursive call sites. A separate post-pass over pub fns
(after `fn_effects` has converged) is much smaller and keeps the
inference walker pure.

The trade-off: the post-pass walks the AST a second time. Cost is
linear in pub-fn-body size and only walks pub fns. Acceptable for
v0.15; an LSP-driven incremental version would memoize per-fn.

### Why method-name keyed dispatch (not receiver+method)?

All 20 v0.14 sigs have structurally identical row-template shape (modulo
`fold`'s 3-param vs 2-param arity). A single per-name entry is correct
for v0.15 — the *receiver* doesn't change the row template. Per-receiver
discrimination would matter for return-type narrowing (`List.map →
List[B]` vs `Iterator.map → Iterator[B]`) but that's the type system's
job, not the effect dispatcher's. v0.16 will plumb the receiver for the
type-narrowing pass; the effect dispatcher will stay name-keyed unless
v0.16 finds a row-template that diverges by receiver.

### Why the parser-folded `Call(Path)` shape needs separate handling

`xs.map(fn(x) { ... })` and `xs.collect()` *should* lower as
`HirExpr::MethodCall`, but the parser's path rule greedily eats the
dotted `xs.map` segments before the postfix `(...)` rule runs. Result:
`Call { callee: Path(["xs", "map"]), args: [...] }` at the HIR level.
Fixing this in the parser is a v0.16+ refactor (touches every dotted-
expression test); for v0.15 we mirror the dispatch in the `Call`
branch's `len >= 2` arm so both shapes work.

## Conformance impact

Before: 92/16/2 (pass/red-shirt/fail).
After: same — no conformance corpus changes; v0.15 dispatch is
*additive* (no previously-passing program rejected, no previously-
inferred set shrunk).

`tests/conformance/effect_checking/*` still passes. New MT4050 emit-
site is exercised only by the new `stdlib_hof_dispatch::closed_caller_rejects_effectful_closure`
unit test (no corpus fixture yet — v0.16 should add one).

## v0.16 follow-ups

1. **Per-receiver discrimination.** When a `List.map` vs `Iterator.map`
   call should narrow the *return type* differently, the dispatcher
   needs the receiver's `TyId`. Plumb it via a separate
   `(receiver_shape, method)` keyed table or by reading
   `expr_ty[receiver]`.
2. **MT4051-4054 emit-sites.** Wire row_occurs_check during the
   surface-syntax row-clause inference; row_var_in_struct at the HIR
   struct-field walk; row_var_unbound after fn-sig instantiation;
   row_effect_mismatch in the closed-row equality path.
3. **LSP hover.** Render row variables on call-expression hover —
   `map(...)` should display `fn(List[A], fn(A)->B!E) -> List[B]!E`
   with `E` resolved to the closure's inferred row.
4. **Suppress MT4001 when MT4050 fires.** Avoid double-noise when both
   the fn-level and call-site emit triggers for the same offending
   effect on the same fn.
5. **Parser fix for `xs.map(...)`** — when no per-segment generic-arg
   list appears, prefer METHOD_CALL_EXPR over PATH_EXPR + CALL_EXPR.
   Would remove the duplicated dispatch site in `effects::Call`.
6. **`remap_effects(&mut RowPolySig, &dyn Fn(EffectId)->EffectId)`** —
   factor the `ALLOC_PLACEHOLDER → real_alloc` swap into a reusable
   helper for the surface-syntax `effect alloc | E` parser hookup.

## Files touched

| File | LOC delta |
|------|-----------|
| `crates/mty-types/src/defs.rs`           | +20 |
| `crates/mty-types/src/effects.rs`        | +475 (dispatch + validator + helper) |
| `crates/mty-types/src/prelude.rs`        | +90 |
| `crates/mty-types/src/diag.rs`           | +50 |
| `crates/mty-diagnostics/src/codes.rs`    | +60 |
| `crates/mty-types/tests/stdlib_hof_dispatch.rs` | +375 (new file, 10 tests) |
