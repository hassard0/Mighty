# Stardust Slice 5 — Complete

**Tag:** `v0.5.0-effects`
**Date:** 2026-05-24

## What landed

### Effects (spec §9)

- New module `sdust-types::effects` runs after type-checking and infers
  per-fn effect sets bottom-up.
- Fixpoint over the call graph (bounded at 32 iterations) propagates
  callee effects.
- Public-fn `effect ...` clauses are validated as supersets of the
  inferred set. Missing effects → `SD4001 effect_undeclared`.
- Strict profile (`profile = "core"` in `star.toml`) bans heap
  allocation: `SD4002 alloc_in_core`.
- Inferred effect names exposed today: `alloc`, `net`, `fs`, `time`,
  `dom`, `model`, `spawn`, `unsafe`.

### Capabilities (spec §8)

- `TyData::Cap { family, constraint }` joins the type arena, with five
  core families (`Net`/`Fs`/`Clock`/`Dom`/`Model`) plus `Custom(name)`.
- Constraint algebra: `Any | ReadOnly | Path(p) | Host(xs) | And(xs)`
  with `is_narrower_or_eq` subsumption.
- Built-in narrowing methods: `cap.ro(path)`, `cap.path(path)`,
  `cap.host(host)` produce a narrower `Cap` type.
- Call-site subsumption check emits `SD4010 capability_too_broad` when
  an arg's constraint is wider than the param's.

### Traits + coherence + dispatch (spec §19)

- `HirItem::Trait` and `HirItem::Impl` now lower from CST (slice 4
  parsed but didn't construct them).
- `DefMap.traits: TraitTable` stores `impls`, `by_method`, `impl_keys`,
  `trait_methods`.
- Method dispatch: inherent impl > trait impl. Multiple matches
  → `SD4020 method_ambiguous`; no match → `SD4021 method_not_found`.
  Duplicate trait impls → `SD4022 trait_coherence_violation`
  (name-only — generic overlap detection is post-v0.1).

### `dyn Trait`

- New `dyn` keyword. Parser produces TYPE_DYN; lowerer builds
  `HirType::Dyn`; resolver builds `TyData::Dyn`.
- Slice-5 conservative object-safety bans traits whose methods
  mention `Self` or have method-level generics → `SD4023
  dyn_requires_object_safe`.

### `#[derive(Copy/Hash/Eq)]`

- New `#` attribute token already existed; slice 5 wires the parser to
  consume `#[derive(...)]` (and a `derive Copy` shorthand) before items.
- `Copy` validation walks every field's type; non-Copy field →
  `SD4040 derive_copy_field_not_copy`. On success, the ADT joins
  `DefMap.user_copy` (consulted by `is_copy` / `is_field_copy`).
- `Hash` / `Eq` register synthetic `TraitImpl` entries so `dyn Hash =
  value` resolves; no method bodies (codegen post-v0.1).
- Unknown derive name → `SD4041 derive_unknown`.

### Top-level `sandbox` items (spec §16.1)

- `sandbox Name with { entries } { body }` now parses + lowers at top
  level into `HirItem::Sandbox(HirTopSandbox)`. Type-checking treats
  the body as Unit-typed under sandbox tolerance.

### Strict protocol message types (spec §13)

- `SD4030 protocol_arity_mismatch` — handler param count ≠ message arity.
- `SD4032 protocol_missing_handler` — implemented protocol declares a
  message no handler covers.
- `SD4033 protocol_extra_handler` — handler refers to a message no
  implemented protocol declares.
- Strict checks are skipped when ANY declared protocol is unknown
  (e.g. `http.Handler` defined in an external module) — the slice-4
  `SD2026` warning still applies in that case.

### Slice-4 polish

- `is_copy` consults `DefMap.user_copy` (the new `#[derive(Copy)]` set)
  in addition to the slice-4 rules. New `Cap` and `Dyn` types are
  affine.
- Trait dispatch table built per-pass; `TraitMethodSig` registered
  *before* fn-signature resolution so `dyn Trait` inside fn params
  resolves.

## All 20 examples still type-check + borrow-check clean

```
sdust check examples/01_hello.sd            → ok
... (all 20)
```

Examples 13 and 19 still emit `SD2026` warnings for handlers on
unknown protocols (`Fetch`, `http.Handler`). Pre-existing; no change.

## Spec interpretation calls (recorded as amendments)

- **A22** — Effect inference algorithm (bottom-up + fixpoint)
- **A23** — Capability narrowing constraint algebra (`Any|RO|Path|Host|And`)
- **A24** — Trait coherence name-only; inherent > trait dispatch
- **A25** — `dyn Trait` object safety (no `Self`, no method generics)
- **A26** — Derive set (Copy, Hash, Eq) + shorthand
- **A27** — Top-level `sandbox` items
- **A28** — Strict protocol-handler checks (with unknown-protocol skip)
- **A29** — `move *ref` is SD3009
- **A30** — Strict-profile `alloc` ban

## Stats

- **274 tests pass** (slice 4: 266 → slice 5: +8)
- 13 new SD4xxx diagnostic codes
- 8 negative slice-5 fixtures (effect/protocol/derive/trait/dyn)
- ~1 100 lines of Rust added (effects.rs, trait dispatch, cap typing,
  derive handling, impl/trait/sandbox lowering)

## Still deferred (post-v0.1 unless noted)

- Polonius / non-lexical lifetimes — post-v0.1
- Effect-row polymorphism — post-v0.1
- Constraint-overlap trait coherence (generic args) — post-v0.1
- True object-safe `Self` dispatch — post-v0.1
- Full derive macro system — post-v0.1
- Cross-function lifetime inference / explicit lifetime params — post-v0.1
- Field-level borrow tracking — slice 6
- Capability narrowing via type-arg syntax (`Fs[Path("/x")]`) — post-v0.1
- Per-receiver typed cap dispatch (effects via dispatch rather than
  receiver-path heuristic) — post-v0.1
- Tighter SD3002 vs SD3008 spans — slice 6
- SIR / interpreter — slice 6
- Runtime — slice 7
- Codegen — slice 8

## Files of note

- `crates/sdust-types/src/effects.rs` — new module
- `crates/sdust-types/src/ty.rs` — `TyData::Cap` + `TyData::Dyn` +
  constraint algebra
- `crates/sdust-types/src/defs.rs` — `TraitTable`, `user_copy`,
  `protocol_msg_names`
- `crates/sdust-types/src/items.rs` — `infer_and_validate`,
  `check_protocols_strict`, profile load
- `crates/sdust-types/src/resolve.rs` — derive apply, trait registration,
  trait coherence, cap-family resolution
- `crates/sdust-types/src/check.rs` — cap narrowing methods, cap
  subsumption, trait-aware method dispatch
- `crates/sdust-hir/src/nodes.rs` — `HirItem::Sandbox`, `HirType::Dyn`,
  `derives: Vec<String>` on Struct/Enum
- `crates/sdust-hir/src/lower/items.rs` — `lower_impl_block`,
  `lower_trait_decl`, `collect_derives`
- `crates/sdust-hir/src/lower/types.rs` — TYPE_DYN lowering
- `crates/sdust-hir/src/lower/exprs.rs` — `lower_top_sandbox`
- `crates/sdust-syntax/src/syntax_kind.rs` — `DYN_KW`, `DERIVE_KW`
- `crates/sdust-syntax/src/parser/types.rs` — `dyn_type`
- `crates/sdust-syntax/src/parser/items.rs` — attribute + sandbox_decl
- `crates/sdust-diagnostics/src/codes.rs` — SD4001..SD4041 + explain
- `tests/slice5_neg/*.sd` — 8 negative fixtures
- `crates/sdust-driver/tests/slice5_negatives.rs` — slice-5 driver tests
- `docs/internals/effects.md`, `capabilities.md`, `traits.md` — new
- `docs/tour/15-traits.md` — new chapter
- `docs/spec/v0.1-amendments.md` — A22..A30
