# Stdlib HOF row-polymorphism roll-out — v0.14 swarm notes

**Scope:** RFC-008 §"v0.14 follow-up" — extend the v0.13 row infra
beyond the single wired `List.map` signature to cover the rest of the
stdlib higher-order functions.
**Status:** SHIPPED-SUBSET — 19 new row-polymorphic signatures landed
in `mty-types::effects::row::stdlib_sigs`, all unit-tested at the
signature-instantiation layer. Stdlib runtime crates and call-site
typeck wiring deferred (see "Wiring gap" below).
**Owner:** v0.14 type-system swarm (parallel agent).

## What ships in v0.14

A new `stdlib_sigs` submodule inside the existing
`crates/mty-types/src/effects.rs::row` module, gated and re-exported
at the `effects::row` level. The v0.13 `stdlib_list_map_sig()` is
unchanged and remains the canonical example; the v0.14 functions are
sibling fns following the same `RowPolySig` shape.

### Sigs added (19 fns)

| Container  | Method      | Function name                             | RowSpec shape                                          |
|------------|-------------|-------------------------------------------|--------------------------------------------------------|
| List       | filter      | `stdlib_list_filter_sig`                  | `[Skip, Var(0)] → Var(0)`                              |
| List       | fold        | `stdlib_list_fold_sig`                    | `[Skip, Skip, Var(0)] → Var(0)`                        |
| List       | flat_map    | `stdlib_list_flat_map_sig`                | `[Skip, Var(0)] → Var(0)`                              |
| Iterator   | map         | `stdlib_iter_map_sig`                     | `[Skip, Var(0)] → Var(0)`                              |
| Iterator   | filter      | `stdlib_iter_filter_sig`                  | `[Skip, Var(0)] → Var(0)`                              |
| Iterator   | fold        | `stdlib_iter_fold_sig`                    | `[Skip, Skip, Var(0)] → Var(0)`                        |
| Iterator   | for_each    | `stdlib_iter_for_each_sig`                | `[Skip, Var(0)] → Var(0)`                              |
| Iterator   | find        | `stdlib_iter_find_sig`                    | `[Skip, Var(0)] → Var(0)`                              |
| Iterator   | any         | `stdlib_iter_any_sig`                     | `[Skip, Var(0)] → Var(0)`                              |
| Iterator   | all         | `stdlib_iter_all_sig`                     | `[Skip, Var(0)] → Var(0)`                              |
| Iterator   | flat_map    | `stdlib_iter_flat_map_sig`                | `[Skip, Var(0)] → Var(0)`                              |
| Iterator   | collect     | `stdlib_iter_collect_sig`                 | `[Skip] → VarPlus(0, {alloc})` (see "collect quirk")   |
| Option     | map         | `stdlib_option_map_sig`                   | `[Skip, Var(0)] → Var(0)`                              |
| Option     | and_then    | `stdlib_option_and_then_sig`              | `[Skip, Var(0)] → Var(0)`                              |
| Option     | or_else     | `stdlib_option_or_else_sig`               | `[Skip, Var(0)] → Var(0)`                              |
| Option     | filter      | `stdlib_option_filter_sig`                | `[Skip, Var(0)] → Var(0)`                              |
| Result     | map         | `stdlib_result_map_sig`                   | `[Skip, Var(0)] → Var(0)`                              |
| Result     | map_err     | `stdlib_result_map_err_sig`               | `[Skip, Var(0)] → Var(0)`                              |
| Result     | and_then    | `stdlib_result_and_then_sig`              | `[Skip, Var(0)] → Var(0)`                              |
| Result     | or_else     | `stdlib_result_or_else_sig`               | `[Skip, Var(0)] → Var(0)`                              |

19 sig fns total (plus the v0.13 `stdlib_list_map_sig`, total 20).
17 of them share a `single_row_closure_sig()` internal helper — same
2-param shape (`[Skip, closure-Var(0)]` → `Var(0)`) — kept as
individual `pub fn` entry points so each method is greppable.

### Tests added (24 new integration tests)

`crates/mty-types/tests/effects_row.rs` grows from 11 to 35 tests:

| New test                                              | What it verifies                                                  |
|-------------------------------------------------------|-------------------------------------------------------------------|
| `list_filter_propagates_predicate_effects`            | `{fs}` predicate flows through `List.filter` row var              |
| `list_fold_propagates_folder_effects`                 | `{fs, net}` folder flows through `List.fold` (3-param shape)      |
| `list_flat_map_propagates_closure_effects`            | `{time}` closure flows through `List.flat_map`                    |
| `iterator_map_propagates_effects`                     | `{fs}` closure flows through `Iterator.map`                       |
| `iterator_filter_propagates_effects`                  | `{net}` predicate flows through `Iterator.filter`                 |
| `iterator_fold_propagates_effects`                    | `{fs}` folder flows through `Iterator.fold` (3-param shape)       |
| `iterator_for_each_propagates_effects`                | `{fs,net,time}` flows through `Iterator.for_each`                 |
| `iterator_find_propagates_effects`                    | `{fs}` predicate flows through `Iterator.find`                    |
| `iterator_any_and_all_propagate_effects`              | `Iterator.any`/`.all` both row-thread the predicate row           |
| `iterator_flat_map_propagates_effects`                | `{fs,net}` closure flows through `Iterator.flat_map`              |
| `iterator_collect_carries_alloc_with_unbound_tail`    | `Iterator.collect`'s VarPlus return carries alloc + open tail     |
| `iterator_collect_unifies_upstream_row_into_return`   | A typeck pass can synthesize a param row + unify it into collect  |
| `option_map_propagates_effects`                       | `{fs}` flows through `Option.map`                                 |
| `option_and_then_propagates_effects`                  | `{net}` flows through `Option.and_then`                           |
| `option_or_else_propagates_effects`                   | `{fs,net}` flows through `Option.or_else`                         |
| `option_filter_propagates_effects`                    | `{time}` flows through `Option.filter`                            |
| `result_map_propagates_effects`                       | `{fs}` flows through `Result.map`                                 |
| `result_map_err_propagates_effects`                   | `{net}` flows through `Result.map_err`                            |
| `result_and_then_propagates_effects`                  | `{fs,net}` flows through `Result.and_then`                        |
| `result_or_else_propagates_effects`                   | `{time}` flows through `Result.or_else`                           |
| `pure_closure_through_each_new_sig_yields_empty_row`  | All 17 single-row sigs stay pure for an empty-row closure         |
| `all_new_sigs_match_v0_13_list_map_shape_invariants`  | Invariant cross-check: row_var_count=1, last-param=Var(0), etc.   |
| `nested_iter_chain_unions_three_effects`              | Realistic `filter.map.collect` chain accumulates fs+net+alloc     |
| `closure_row_open_unifies_through_each_new_sig`       | Open-into-open unification (closure with its own row var)         |

## Verification

- `cargo build -p mty-types` — clean.
- `cargo test -p mty-types` — **35** effects_row tests pass (was 11),
  + 37 lib tests + 12 sendable tests + 0 doc-tests = 84 total, no
  regressions.
- `cargo clippy -p mty-types --lib --tests --no-deps -- -D warnings`
  — clean (the two `mty-hir` deprecated-function warnings are
  pre-existing, unrelated to this work).
- `cargo fmt -p mty-types -- --check` — clean.

Workspace-wide `cargo build --workspace` is currently RED due to
**unrelated** in-flight changes in `mty-hir`/`mty-macros`/
`mty-codegen-wasm` by other concurrent swarm agents. The
row-polymorphism work touches **only** `crates/mty-types/` and is
independently green.

## Wiring gap (v0.15 follow-up)

Same caveat as v0.13's `EFFECT_ROW_V0_13_NOTES.md` §"What does NOT
ship in v0.13": the row infrastructure lives in `mty-types`, but
neither (a) the surface-syntax parser nor (b) the call-site checker
in the typeck pipeline consults these sigs yet. User code still goes
through the existing v0.12 closed-set inference + "permissive
methods" allowlist in `prelude.rs` for HOF method calls.

### Why no `mty-stdlib` HOF declarations were modified

The task brief expected to "wire stdlib declarations" in
`crates/mty-stdlib/src/iter.rs`. That file does not exist. The
`mty-stdlib` crate ships only **runtime implementations** of `std.*`
modules (`fs`, `http`, `json`, `tls`, `time`, `test`, `host`,
`http_server`) — none of them declare HOF signatures.

HOF method calls (`xs.map(|x| ...)`, `result.and_then(|x| ...)` etc.)
are currently routed through `crates/mty-types/src/prelude.rs`'s
`permissive_methods` allowlist, which **bypasses type checking
entirely** — `map`/`filter`/`fold`/`collect` appear as
`BuiltinMethod { arity: None, ret: None }` entries (lines ~425-428
and surrounding). The arity-`None` / ret-`None` declaration means the
typeck layer doesn't even know what closure effects each method
should accept.

This is the v0.15 wiring blocker: before any of the new
`stdlib_sigs::*` fns become reachable from user code, the prelude
needs a richer `BuiltinMethod` variant that carries a `RowPolySig`,
and the typeck call-site code needs to dispatch on that variant to
run `instantiate_row_sig` + `unify_rows` + reify the resolved return
row's concrete effects into the caller's inferred effect set.

That richer wiring is intentionally NOT shipped here because:

1. **Avoids cross-agent contention.** The brief explicitly fences
   `mty-types/src/check.rs` off as another agent's territory, and
   the call-site dispatch belongs there.
2. **Keeps v0.14 scope tight.** Same SHIPPED-SUBSET pattern as v0.13:
   ship the signature library + tests in `mty-types`, defer the
   call-site dispatch to the next slice.
3. **Builds future-proof tests.** All 24 new tests exercise
   `instantiate_row_sig` + `unify_rows` directly, so they will
   continue to pass once the call-site dispatch lands.

## The `collect` quirk

`Iterator.collect` does NOT take a closure parameter — its row var
(`E` in the RFC's `!{alloc | E}` notation) is meant to come from the
**upstream iterator chain's accumulated effect row**, not from a
parameter to `collect` itself. v0.14's representation of iterators
does not yet carry per-iterator row vars (iterators are still
modeled as plain `Iterator[A]`, not `Iterator[A, !E]`).

The signature shipped — `RowPolySig { row_var_count: 1, param_rows:
[Skip], return_row: VarPlus(0, {alloc_placeholder}) }` — is
structurally sound but its row var has no parameter binding site in
v0.14. When the typeck call-site dispatch runs `instantiate_row_sig`
and finds no parameter to unify the fresh var against, it must
either:

1. Pull the row from the receiver's iterator type (requires v0.15
   row-carrying iterator types), or
2. Treat the unbound row var as the diagnostic signal MT4022
   (`row_var_unbound`) — same as the v0.13 anti-pattern test.

`stdlib_sigs::ALLOC_PLACEHOLDER` is a sentinel `EffectId(u32::MAX - 7)`
inside the signature; call-site code must remap this to the live
`DefMap::intern_effect("alloc")` id before unification. The remapping
helper is a v0.15 follow-up (a tiny `remap_effects` walker over a
`RowPolySig` would suffice).

The two `collect`-specific tests
(`iterator_collect_carries_alloc_with_unbound_tail` and
`iterator_collect_unifies_upstream_row_into_return`) demonstrate
both halves of the design: the unbound-tail diagnostic case AND the
case where typeck synthesizes a parameter row from the upstream.

## Interpretation calls

1. **`single_row_closure_sig()` helper is internal.** The 17
   identically-shaped HOFs share one constructor, but each
   user-facing function is a one-line `pub fn` returning the
   constructor's value. Rationale: future per-method tweaks (e.g.
   `result.and_then` someday gaining a `VarPlus` to model error-row
   composition) can edit one fn body without touching the others;
   external code can `grep stdlib_result_and_then_sig` and find a
   single hit.
2. **`fold` is 3-param, not 2.** `List.fold(xs, init, f)` and
   `Iterator.fold(it, init, f)` both have an accumulator-init slot
   between the container and the closure. The init contributes no
   effect row (`RowSpec::Skip`).
3. **No `Option.unwrap_or_else` / `Result.unwrap_or_else`.** These
   are HOFs too (closure-takes-Unit-returns-T) but were judged
   non-essential for the v0.14 cut. Same shape as
   `option_or_else_sig` if added later.
4. **`flat_map` modeled as single-row, NOT two-row.** A purist might
   give `flat_map` two row vars (`E` for the closure, `F` for the
   returned `Iterator[B]`'s lazy chain) but the v0.14 cut conflates
   them. Future work could split if needed for finer-grained
   propagation through nested iterator chains.
5. **`ALLOC_PLACEHOLDER` lives in `stdlib_sigs`, not in `effects::row`'s
   top level.** It's a stdlib-sig-only concern; the row arithmetic
   doesn't need it. Kept namespaced.

## Files touched / created

Created:

- `dev/history/notes/STDLIB_HOF_ROWPOLY_V0_14_NOTES.md` (this file)

Modified:

- `crates/mty-types/src/effects.rs` (+207 LOC — pure additions:
  one re-export of `row::stdlib_sigs`, one new `pub mod stdlib_sigs`
  with 19 sig fns + a helper + the alloc placeholder constant).
  The v0.13 `stdlib_list_map_sig` and all other v0.13 row
  infrastructure (`EffectRow`, `RowSubst`, `unify_rows`, etc.) are
  untouched.
- `crates/mty-types/tests/effects_row.rs` (+355 LOC — 24 new tests
  + 2 helper fns for 2-param / 3-param call simulation).

**NOT modified:** `mty-syntax`, `mty-hir`, `mty-borrow`,
`mty-codegen-*`, `mty-macros`, `mty-driver`, `mty-cli`, `mty-runtime`,
`mty-stdlib`, root `Cargo.toml`. None of the v0.13 row infra in
`effects::row` (only additions to the `effects.rs` re-export list
and a brand-new `stdlib_sigs` submodule).

## v0.15 follow-ups

1. **Wire `RowPolySig` into `prelude::BuiltinMethod`.** Add a third
   variant or a new field carrying `Option<RowPolySig>`. When a
   call-site checker (`mty-types/src/check.rs`, owned by a different
   agent) encounters a method-call to one of the listed HOFs,
   dispatch via the sig.
2. **Surface-syntax `!E` / `!{a | E}` parser.** Add productions in
   `mty-syntax` so user-authored `fn` declarations can carry row
   vars. Lower to a new HIR effect-clause node.
3. **`Iterator[A, !E]`** carrier types. So `collect`'s row var can
   be bound from the receiver's static type rather than synthesized.
4. **`remap_effects(&mut RowPolySig, &dyn Fn(EffectId)->EffectId)`**
   helper — needed by call sites to swap `ALLOC_PLACEHOLDER` for
   the live-`DefMap` alloc id at unification time.
5. **MT4020..MT4025 diagnostics.** Reserved by RFC-008 but not yet
   emitted. v0.15 typeck wiring will populate them.
6. **LSP hover.** Render row variables in inferred-type display
   (e.g. `List.map: fn[A, B, E](xs: List[A], f: fn(A)->B!E) -> List[B]!E`).
