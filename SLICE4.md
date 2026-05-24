# Mighty Slice 4 — Complete

**Tag:** `v0.4.0-borrowck`
**Date:** 2026-05-24

## What landed

- **New crate `mty-borrow`** implementing the ownership / borrow / affine
  / arena checker. Distinct from `mty-types` so the dep graph stays
  honest (the type checker stays usable in isolation).
- **`Copy` predicate** — primitives, shared refs, raw ptrs, fn ptrs,
  `Str`, tuples/arrays of Copy, opaque ADTs. NOT Copy: `&mut T`,
  `String`, `Bytes`, user struct/enum, `Param`/`Var`.
- **`Sendable` predicate** — Copy ∨ owned String/Bytes ∨ owned Adt
  (opaque or all-Sendable-payload) ∨ tuple/array of Sendable. References,
  raw pointers, fn pointers are NOT Sendable; `Param`/`Var` permissive.
- **Linear lexical walker** (`flow.rs::BorrowCx`):
  - Per-local `Ownership` state machine: Owned | Moved | Borrowed{n} |
    BorrowedMut | Uninit
  - `Move` keyword + auto-move on non-Copy call args (when param isn't a
    `Ref { .. }`) + non-Copy return
  - `&` shared borrow / `&mut` exclusive borrow with the canonical
    "many shared XOR one mut" rule
  - `if`/`match`/`if-let` join-by-intersection (definitely-moved wins)
  - Lexical borrow regions — borrows decay at end of innermost enclosing
    block
- **Arena escape detection** (`MT3010`) — arena body's tail expression
  directly naming an arena-local non-Copy binding errors. Indirect flow
  is post-v0.1.
- **Cross-agent Sendable check** (`MT3011`) — every arg to
  `target!Msg(args)` / `target?Msg(args)` validated via `is_sendable`.
- **Drop intent** — at scope exit, `DropPlan` accumulates entries for
  every Owned non-Copy local. Codegen consumes this in a later slice.
- **15 SD3xxx diagnostics** + `mty explain` text for each.
- **Slice-3 hardening**:
  - **Scope-aware tolerance** (A21) — replaces blanket permissive policy;
    `MT2021` fires on top-level fn bodies' unresolved names
  - **Real impl-method dispatch** (A17) — user-ADT methods resolve via
    indexed impl blocks; opaque + primitive receivers stay permissive
  - **Protocol-aware handler param types** (A18) — agent handler params
    bind to protocol-declared types; `MT2026` warns when no match
  - **Match exhaustiveness as error** (A16) — `MT2015` flipped from
    Warning to Error
  - **Integer/float defaulting pass** (A19) — `IntInfer` → `I32`,
    `FloatInfer` → `F64` rewritten in the side-table after each body
- **HIR additions**:
  - `let mut <pat>` mutability flows through `HirStmt::Let.mutable`
  - `extern { fn ... }` block lowering (slice 2 had only parsing)
  - Agent method collection (was previously dropped)
  - `use std.http` binds `http` as a module reference

## All 20 examples type-check + borrow-check clean

```
mty check examples/01_hello.sd            → ok
... (all 20)
mty check examples/20_frontend_component  → ok
```

Examples 06 and 11 were amended (per the slice-3 deferral) with explicit
`extern { fn ... }` declarations for their referenced free functions and
return types of `Unit!WorkErr` / `Unit!RunErr` so the strict `?`
operator rule applies cleanly.

## Spec interpretation calls (recorded as amendments)

- **A13** — Copy derivation set for slice 4 (primitives + refs +
  raw ptrs + opaque ADTs)
- **A14** — Sendable set for cross-agent messages
- **A15** — Arena escape is direct-naming MVP
- **A16** — Non-exhaustive match is an error (was warning)
- **A17** — Method dispatch policy (user ADTs require impl; opaque keep
  built-in table)
- **A18** — Protocol-aware handler params (MT2026 warning on miss)
- **A19** — Integer/float defaulting pass
- **A20** — Lexical borrow regions (Rust-2015 style; no NLL/Polonius)
- **A21** — Scope-aware tolerance for unresolved values

## Stats

- **266 tests pass** (slice 3: 224 → slice 4: +42)
- ~2 500 lines of Rust added (mty-borrow + items.rs/check.rs/resolve.rs
  extensions)
- 15 new SD3xxx + MT2026 diagnostic codes
- 12 negative borrow fixtures (one per SD3xxx-reachable code)
- 20 example-borrowck integration tests + 4 mty-borrow direct tests +
  17 unit tests in copy/sendable/state

## Still deferred (post-v0.1 unless noted)

- Polonius / non-lexical lifetimes — post-v0.1
- Cross-function lifetime inference / explicit lifetime params — post-v0.1
- Field-level borrow tracking (split `&s.f1` vs `&s.f2`) — post-v0.1
- Drop ordering across reordered scopes / drop-flag bookkeeping — slice 5+
- Manual `derive(Copy)` on user ADTs — slice 5
- Real serializable-shape audit for cross-agent messages — slice 6
- Trait coherence + dyn dispatch — slice 5
- Effect closure + capability narrowing enforcement — slice 5
- `move *ref` modelling for MT3009 — slice 5
- Tighter MT3002 vs MT3008 distinction — slice 5
- Real codegen of `drop()` calls + MtyIR consumption of `DropPlan` —
  slice 6+
- Pattern-typed locals (struct field types in patterns) — slice 5

## Files of note

- `crates/mty-borrow/` — new crate
- `crates/mty-types/src/{lib,items,check,resolve}.rs` — typed side
  tables + tolerance + dispatch + defaulting
- `crates/mty-hir/src/{nodes,lower/items,lower/agents,lower/exprs}.rs`
  — `let mut`, extern blocks, agent methods
- `crates/mty-driver/src/pipeline.rs` — `type_and_borrow_check`
- `docs/internals/borrowck.md` — internals reference
- `docs/spec/v0.1-amendments.md` — A13..A21
- `docs/tour/14-ownership.md` — new tour chapter
- `tests/borrow_neg/*.sd` — 12 negative borrow fixtures
- `crates/mty-driver/tests/{borrow_negatives,examples_borrowck}.rs`
  — integration tests
