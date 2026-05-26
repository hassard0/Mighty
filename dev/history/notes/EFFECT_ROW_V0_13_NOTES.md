# Effect-row polymorphism — v0.13 swarm notes

**Scope:** RFC-008 row-polymorphism infrastructure landed in
`mty-types`, plus one wired stdlib HOF signature (`List.map`).
**Status:** SHIPPED-SUBSET — full row infra including unification +
subsumption + chain resolution is in place; stdlib roll-out is scoped
to `List.map` only.
**Owner:** v0.13 type-system swarm (parallel agent).

## What ships in v0.13

1. **RFC-008** published at `docs/spec/rfcs/RFC-008-effect-rows.md`.
   Covers motivation, syntax (`!E`, `!{a | E}`), four-case unification
   rules, subsumption, anti-patterns, diagnostics, open questions
   (effect handlers deferred to RFC-009).
2. **Row representation** in `crates/mty-types/src/effects.rs`,
   inside the `pub mod row` block:
   - `EffectRow` enum (`Closed(BTreeSet<EffectId>)` /
     `Open(BTreeSet, RowVar)`).
   - `RowVar(u32)` newtype.
   - `RowSubst` substitution table: `fresh()`, `bind()` (with
     occurs-check), `lookup()`, `resolve()` (recursive walk of
     binding chains).
   - `RowError` enum: `ClosedMismatch`, `SubsumptionFail`, `Occurs`.
   - `unify_rows()` — full 4-case unification (closed/closed,
     closed/open, open/closed, open/open with shared fresh tail).
   - `subsume_closed()` — closed-into-closed sub-row check.
   - `RowPolySig` + `RowSpec` for representing row-polymorphic fn
     signatures (de-Bruijn-style row-var indices).
   - `instantiate_row_sig()` — call-site freshening.
   - `pretty_row()` — diagnostic-quality rendering.
   - `stdlib_list_map_sig()` — the v0.13-wired `List.map`
     signature: `fn[A, B, E](xs: List[A], f: fn(A)->B!E) -> List[B]!E`.
3. **Test coverage** — **23 new tests** (12 unit + 11 integration),
   up from 44 → 67 in `mty-types`:
   - `effects::row_tests::row_arith_01..12` (unit tests of the row
     arithmetic in `effects.rs`).
   - `tests/effects_row.rs::*` (11 integration scenarios — HOF
     call-site simulation, anti-pattern detection, signature
     round-trip, chain-resolve).
4. **`EffectId` Ord/PartialOrd derive** in `crates/mty-types/src/ty.rs`
   — required by `BTreeSet<EffectId>`. Tiny, safe, additive derive;
   no behavioral impact on existing code.
5. **Re-exports** from `effects::row::*` at the `effects` module level
   so consumers can write `use mty_types::effects::{EffectRow,
   unify_rows, ...}`.

## What does NOT ship in v0.13

This is a **SHIPPED-SUBSET** because the wider integration into the
surface syntax and the existing call-graph fixpoint is intentionally
out of scope (would touch parser, HIR, and downstream crates owned
by other swarm agents):

1. **Surface-syntax parser support.** The `!E` and `!{a | E}` forms
   are NOT yet recognised by `mty-syntax`. User code cannot declare
   row-polymorphic fns until v0.14.
2. **Wiring into `infer_and_validate`.** The existing call-graph
   fixpoint (which produces the v0.12 closed effect set) is
   UNCHANGED. The row infrastructure is parallel and stand-alone;
   no existing program's effect inference is affected.
3. **Remaining stdlib HOFs.** Only `List.map` has a wired
   row-polymorphic signature. The full v0.14 list:
   - `List.filter` — `fn[A, E](p: fn(A)->Bool!E) -> List[A]!E`
   - `List.fold` — `fn[A, B, E](f: fn(B, A)->B!E) -> B!E`
   - `Iterator.map`, `Iterator.filter`, `Iterator.collect`
     (`Iterator.collect` will use `RowSpec::VarPlus(0, {alloc})`
     — already supported by the infra, tested by
     `iterator_collect_style_var_plus_concrete`).
   - `Result.map`, `Result.and_then`
   - `Option.map`, `Option.and_then`
4. **Diagnostics MT4020..MT4025.** The RFC reserves codes for
   `row_occurs_check`, `row_var_in_struct`, `row_var_unbound`,
   `row_var_in_concrete_set`, `row_effect_mismatch`,
   `row_subsumption_fail`. None are emitted in v0.13 because the
   parser doesn't yet feed the row machinery. Codes are reserved in
   the diag table when v0.14 wires the parser.
5. **LSP hover.** Inferred-type display in the language server does
   not yet render row variables.

## Anti-pattern detection

The infrastructure supports detection of the three RFC-008
anti-patterns. They surface as follows in the v0.13 API:

1. **Row var only in return.** After `instantiate_row_sig`, if no
   parameter row mentions the fresh row var and the return row does,
   `subst.is_bound(fresh[i])` will remain false. The v0.14 validator
   should walk parameter rows looking for each fresh var; absence
   triggers MT4022. Tested by `row_var_in_return_only_is_rejected`.
2. **Row var in struct field.** Detected at *signature construction*
   time — a `RowPolySig` is only constructed for fn signatures, so
   accidentally pointing a struct field's effect row at a `RowVar`
   would have to go through a separate code path that v0.14 will
   gate with MT4021.
3. **`!{ E }` form.** This is a *parser*-level ambiguity (is `E` a
   concrete effect name or a row var?). The RFC mandates rejecting
   the bare-row-var-in-set form; v0.14 parser will emit MT4023
   pointing the user to either `!E` or `!{a | E}`.

## Migration

**Existing programs are unaffected.** No v0.12 code path changes:

- Functions declared with `!{}` keep closed-empty rows.
- Functions declared without an explicit `!` clause keep
  call-graph-inferred closed effect sets.
- Row variables are *opt-in* — only programs that explicitly write a
  row variable in the generic clause get row polymorphism.
- The `EffectId` Ord derive is purely additive; no existing trait
  resolution changes.

**v0.14 migration plan:**

1. Add parser productions for `!E` (alone), `!{a | E}` (mixed),
   and reserved capital-letter generics in `mty-syntax`.
2. Lower row clauses to a new HIR `HirEffectClause::Row { fixed:
   Vec<String>, tail: Option<String> }`.
3. Extend `FnDef` to carry a `RowPolySig` alongside the existing
   `effects: Vec<EffectId>` field (one or the other, never both).
4. At call sites with a row-poly callee, instantiate the sig
   (`instantiate_row_sig`), unify against actual closure rows
   (`unify_rows`), and add the resolved return row's concrete
   effects to the caller's inferred effect set.
5. Relax stdlib HOFs in `crates/mty-types/src/prelude.rs` (see list
   above).
6. Wire MT4020..MT4025 diagnostics.

## Interpretation calls

These are choices the spec left open that the v0.13 work commits:

1. **Row vars are explicit, not implicit.** RFC-008 considered an
   implicit form (any fn with a fn-typed param silently gains a row
   var). Rejected: too surprising, hurts diagnostic locality.
   Authors write `[E]` explicitly.
2. **Single capital letter convention.** Row var names share the
   generic-clause namespace with type vars. Distinguished by
   *usage position*: appearing in an effect clause means row var.
   Convention (not enforced): capitals `E`, `R`, `F`.
3. **Substitution-scope.** A row var is local to one fn signature.
   Two crates with `fn map[A,B,E]` and `fn map[A,B,R]` are
   structurally identical — the row-var name is purely internal.
4. **Open/open unification uses a single fresh tail.** Standard
   Koka algorithm. The alternative ("two separate fresh vars then
   unify them") works but produces longer substitution chains;
   single-fresh keeps `resolve()` shallower.
5. **Subsumption is one-direction.** A closed row narrows into an
   open row (closure brings less than the open row admits). The
   reverse — an open row flowing into a closed parameter — is
   rejected unless the open row's tail unifies to empty. This is
   the entire point of row polymorphism: the open-row side
   *announces* "I will inherit your effects", and the closed-row
   side announces "I am fixed".
6. **`RowSpec::VarPlus(i, eff)`** is supported in addition to
   `Var(i)` and `Concrete(_)`. Needed for `Iterator.collect`
   (`!{alloc | E}`). Validated by
   `iterator_collect_style_var_plus_concrete`.

## Verification

- `cargo build -p mty-types` — clean.
- `cargo test -p mty-types` — 67 tests pass (was 44; +23 new).
- `cargo fmt -p mty-types -- --check` — clean.
- `cargo clippy -p mty-types --lib --tests --no-deps -- -D warnings`
  — clean.
- Workspace-wide build is currently affected by **unrelated** swarm
  agent work in `mty-cli`, `mty-codegen-wasm`, `mty-macros`,
  `mty-driver`. The row-polymorphism change does not touch those
  crates and is independently green.

## Files touched / created

Created:

- `crates/mty-types/tests/effects_row.rs` (~270 LOC, 11 tests)
- `docs/spec/rfcs/RFC-008-effect-rows.md` (~230 LOC spec)
- `dev/history/notes/EFFECT_ROW_V0_13_NOTES.md` (this file)

Modified:

- `crates/mty-types/src/effects.rs` (+450 LOC: `mod row` block + 12
  unit tests). Existing `infer_and_validate` etc. untouched.
- `crates/mty-types/src/ty.rs` (+1 char: `PartialOrd, Ord` on
  `EffectId`).
