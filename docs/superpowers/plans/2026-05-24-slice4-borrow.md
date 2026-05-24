# Slice 4 — Ownership / Borrow / Affine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship Stardust's ownership/borrow/affine checker (spec §7), close the slice-3 hardening backlog, and ensure all 20 canonical examples + the new negative borrow corpus stay green. Tag `v0.4.0-borrowck`.

**Architecture:** New `sdust-borrow` crate that consumes the typed-HIR side tables produced by `sdust-types::check_package_typed`. Linear top-down walk over typed HIR with per-local Ownership state. Lexical borrow regions only (NLL / Polonius post-v0.1). See design doc in `docs/superpowers/specs/2026-05-24-slice4-borrow-design.md`.

**Tech Stack:** Rust 1.82, la-arena (already used). Same workspace as slice 3.

---

## File Structure

**Create:**
- `crates/sdust-borrow/Cargo.toml`
- `crates/sdust-borrow/src/lib.rs` — `check_package` entry; re-exports
- `crates/sdust-borrow/src/state.rs` — `Ownership`, `LocalState`, `ScopeFrame`
- `crates/sdust-borrow/src/copy.rs` — `is_copy(TyId, &TyArena, &DefMap)`
- `crates/sdust-borrow/src/sendable.rs` — `is_sendable(TyId, ...)`
- `crates/sdust-borrow/src/flow.rs` — `BorrowCx`, linear walker, expr/stmt handlers
- `crates/sdust-borrow/src/arena.rs` — arena region tracking
- `crates/sdust-borrow/src/drop.rs` — drop-intent emission
- `crates/sdust-borrow/src/diag.rs` — SD3xxx constructors
- `crates/sdust-borrow/tests/basic.rs`
- `crates/sdust-borrow/tests/borrows.rs`
- `crates/sdust-borrow/tests/arena.rs`
- `crates/sdust-borrow/tests/sendable.rs`
- `crates/sdust-borrow/tests/examples.rs`
- `crates/sdust-borrow/tests/negatives.rs`
- `tests/borrow_neg/*.sd` — 12 negative-test inputs
- `docs/internals/borrowck.md`
- `docs/tour/14-ownership.md`
- `SLICE4.md`

**Modify:**
- `Cargo.toml` — add `crates/sdust-borrow` to members
- `crates/sdust-driver/Cargo.toml` — depend on `sdust-borrow`
- `crates/sdust-driver/src/pipeline.rs` — `borrow_check(pkg) -> Vec<Diagnostic>` stage
- `crates/sdust-cli/src/cmd/check.rs` — run the new stage after type-check
- `crates/sdust-diagnostics/src/codes.rs` — add SD2026 + SD3001..SD3015 + `explain` entries
- `crates/sdust-types/src/lib.rs` — export `TypedPackage`, `check_package_typed`
- `crates/sdust-types/src/items.rs` — track typed-HIR side tables; replace warning severity on SD2015 with Error; defaulting pass
- `crates/sdust-types/src/check.rs` — scope-aware tolerance set; real method dispatch; protocol-message handler param types
- `crates/sdust-types/src/diag.rs` — `non_exhaustive_match` severity Error; new SD2026 `protocol_msg_unknown`
- `crates/sdust-types/src/infer.rs` — `default_inference` walks expr_ty + local_ty and rewrites IntInfer/FloatInfer
- `examples/06_for_while_loop.sd` — add `-> Unit!WorkErr`
- `examples/11_budget_block.sd` — `-> Unit!RunErr` (was `Result!RunErr`)
- `docs/spec/v0.1-amendments.md` — A13..A21
- `docs/reference/diagnostics.md` — SD3xxx section + SD2026
- `docs/tour/02-types.md` — Copy section
- `docs/tour/09-arenas.md` — arena escape rule
- `docs/tour/10-capabilities.md` — cross-agent message rule
- `docs/tour/README.md` — link new chapter 14
- `README.md` — roadmap mark slice 4 shipped
- `SLICE3.md` — strike closed deferrals

---

## Task 1: Type-check side-tables in `sdust-types`

**Files:** `crates/sdust-types/src/lib.rs`, `crates/sdust-types/src/items.rs`, `crates/sdust-types/src/check.rs`

Extract the typed-HIR side tables produced during checking so the borrow checker can consume them.

Add to `lib.rs`:
```rust
pub struct TypedPackage<'a> {
    pub pkg: &'a Package,
    pub def_map: DefMap,
    pub ty_arena: TyArena,
    pub expr_ty: HashMap<ExprId, TyId>,
    pub local_ty: HashMap<LocalId, TyId>,
    pub fn_params: HashMap<FnId, Vec<(String, TyId)>>,
    pub fn_ret: HashMap<FnId, TyId>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn check_package_typed(pkg: &Package) -> TypedPackage<'_>;
pub fn check_package(pkg: &Package) -> Vec<Diagnostic> {
    check_package_typed(pkg).diagnostics
}
```

In `Cx`, add `pub expr_ty: &'a mut HashMap<ExprId, TyId>;` etc. Every place `synth_expr` returns a TyId, also record `(expr_id, ty)` into the side table.

- [ ] Add `TypedPackage` to `lib.rs`
- [ ] Wire side-table storage through `Cx`
- [ ] Record `expr_ty` on each synth_expr/check_expr return
- [ ] Record `local_ty` on each `cx.locals.bind`
- [ ] Record `fn_params` + `fn_ret` per fn
- [ ] All slice-3 tests still green

## Task 2: `sdust-borrow` crate scaffolding

**Files:** `crates/sdust-borrow/Cargo.toml`, `crates/sdust-borrow/src/lib.rs`, root `Cargo.toml`

```toml
[package]
name = "sdust-borrow"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
sdust-hir.workspace = true
sdust-types.workspace = true
sdust-diagnostics.workspace = true
la-arena.workspace = true
```

```rust
// src/lib.rs
pub mod copy;
pub mod diag;
pub mod drop;
pub mod flow;
pub mod sendable;
pub mod state;

use sdust_diagnostics::Diagnostic;
use sdust_types::TypedPackage;

pub fn check_package(typed: &TypedPackage) -> Vec<Diagnostic> {
    flow::run(typed)
}
```

- [ ] Add the crate; `cargo build -p sdust-borrow` succeeds
- [ ] Add to workspace members
- [ ] Module skeleton compiles

## Task 3: `Copy` predicate

**Files:** `crates/sdust-borrow/src/copy.rs`

`pub fn is_copy(ty: TyId, arena: &TyArena, defs: &DefMap) -> bool` per the design doc §3.6.

- [ ] Implement `is_copy` (primitives, refs, raw_ptr, str, tuple/array, fn, adt-opaque-true, adt-user-false)
- [ ] Unit tests covering each branch (10+ cases)

## Task 4: `Sendable` predicate

**Files:** `crates/sdust-borrow/src/sendable.rs`

`pub fn is_sendable(ty, arena, defs) -> bool` per §3.7. Slice 4: Copy ∨ owned-String/Bytes ∨ owned-Adt (opaque or all-Sendable-payload) ∨ tuple/array of Sendable ∨ Param/Var (conservative permissive). Refs / raw ptrs / fns are not Sendable.

- [ ] Implement `is_sendable`
- [ ] Unit tests (8+ cases incl. recursive struct payloads)

## Task 5: State types

**Files:** `crates/sdust-borrow/src/state.rs`

```rust
#[derive(Clone, Debug)]
pub enum Ownership { Owned, Moved { at: SourceSpan }, Borrowed { count: u32 }, BorrowedMut, Uninit }

#[derive(Clone, Debug)]
pub struct LocalState {
    pub name: String,
    pub ty: TyId,
    pub state: Ownership,
    pub declared_at: SourceSpan,
    pub mutable: bool,
    pub arena_region: Option<ArenaRegionId>,
    pub is_copy: bool,
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, Hash)]
pub struct LocalKey(pub u32);

#[derive(Clone, Debug, Copy, PartialEq, Eq, Hash)]
pub struct ArenaRegionId(pub u32);

#[derive(Default, Clone, Debug)]
pub struct ScopeFrame {
    pub locals: Vec<LocalKey>,           // new locals introduced this scope
    pub arena_region: Option<ArenaRegionId>,
}
```

- [ ] All structs/enums + Display impl for Ownership
- [ ] Tests on `join_states` (intersection of two state maps)

## Task 6: Per-fn linear walker

**Files:** `crates/sdust-borrow/src/flow.rs`

Define `BorrowCx<'a>` holding the typed package, locals table (`HashMap<String, LocalState>`), scope stack, diagnostic buffer, arena counter. Implement `walk_block`, `walk_stmt`, `walk_expr`. The expr walker classifies usage:

- **`HirExpr::Path([name])`** in non-`&` non-`move` context: a *use* of the local. If non-Copy and currently Owned, mark Moved. If Moved, error SD3001. If Borrowed (any kind), accept (immutable read of a borrowed value is fine — the borrow held it; we don't double-borrow on plain reads).
- **`HirExpr::Move(inner)`**: force move semantics: must be Owned (not Borrowed, not Moved). Mark Moved.
- **`HirExpr::Borrow { mutable, inner }`**: if `inner` resolves to a local path, transition: shared → Borrowed.count+=1; mut requires Owned + mutable=true binding; emit SD3004/3005/3006/3013 on conflict.
- **`HirExpr::Call { callee, args }`**: walk callee (usually fn name), then each arg. Each arg's parameter type tells whether it's a move or a ref. Slice 4 simplification: look up callee's `Fn { params, .. }` type from `expr_ty`; for each arg position, if param type is `Ref { .. }`, the arg is treated as a temporary borrow (held until call returns; lexical = just that expression); else the arg is moved if non-Copy.
- **`HirExpr::MethodCall { receiver, method, args }`**: similar; receiver is borrowed (shared by default, mut if method takes `&mut self`). Slice 4 keeps it permissive: receiver borrowed shared for the duration.
- **`HirExpr::Field { receiver, name }`**: read-only field access; if receiver is a local, no state change.
- **`HirExpr::Send { target, msg, args }` / `Ask { target, msg, args }`**: check each arg's resolved type via `expr_ty[arg.value]` is Sendable; else SD3011. Args of non-Copy Sendable types are moved.
- **`HirExpr::Return(opt)`**: walk inner; tail-value moved out (or copied if Copy).
- **`HirExpr::Block(b)`**: enter scope frame, walk, leave (running drop intent + dropping borrows for locals introduced in this frame).
- **`HirExpr::If/IfLet/Match`**: snapshot state, walk each arm with a clone, join via intersection at the end (a local is "definitely moved" iff moved on every arm).
- **`HirExpr::Arena { name, body }`**: push arena region, walk body; on body end, check tail-expression for direct local references that are arena-local and emit SD3010.
- **`HirExpr::Unsafe(b)`**: walk block normally (borrow rules still apply even in unsafe; only the *value namespace* tolerance relaxes — handled in slice-3-hardening task).
- Other variants (`Spawn`, `Lambda`, `Cast`, `Run`, `TaskScope`, `Budget`, `Sandbox`, etc.): walk children; lambdas open a fresh local-state map.

`fn run(typed: &TypedPackage) -> Vec<Diagnostic>` iterates every fn body, every agent state-init / handler / method, every supervisor child expression, and runs the walker.

- [ ] `BorrowCx`
- [ ] `walk_block`/`walk_stmt`/`walk_expr` skeleton
- [ ] Path-use → move detection
- [ ] `move expr`
- [ ] `&expr` / `&mut expr`
- [ ] Call argument move-or-borrow
- [ ] Field access read-only
- [ ] Send/Ask sendable check
- [ ] Return as move
- [ ] If/Match join (intersection)
- [ ] Lambda fresh scope
- [ ] Hit every HirExpr variant (assignment, index, deadline, ...)

## Task 7: Arena escape

**Files:** `crates/sdust-borrow/src/arena.rs`, integrated into `flow.rs`

Push `ArenaRegionId` on entry to `HirExpr::Arena`, pop on exit. Locals introduced inside an arena body carry the region id. On arena exit, inspect the *tail expression* of the body: if it's a `Path([name])` and `name` resolves to a local with the active region, emit `SD3010 arena_escape`. For block-bodied arenas, the tail is the block's `tail` field. For the short form `arena name: expr`, the tail is the expression directly.

- [ ] Implement region push/pop
- [ ] Tag locals on declaration with active region (if any)
- [ ] Check tail-expression at arena exit
- [ ] Negative test: `arena turn { let x = String("hi"); x }` errors SD3010
- [ ] Positive test: `arena turn { let x = String("hi"); compute(x) }` accepted (call result is fresh)

## Task 8: Drop intent

**Files:** `crates/sdust-borrow/src/drop.rs`

At each scope exit, walk locals introduced in that scope; for any whose state is `Owned` and `!is_copy`, record `DropEntry { local: LocalKey, span: SourceSpan }` in a `DropPlan`. The plan is returned alongside diagnostics from `check_package` as a side artifact (we don't expose it through `pipeline.rs` for slice 4, but we do unit-test it). Locals in `Moved` state at scope exit produce no drop.

- [ ] `DropPlan { entries: Vec<DropEntry> }` struct
- [ ] Emit at scope exit
- [ ] Unit test: simple fn that binds a `String` drops at fn-end
- [ ] Unit test: explicit `move x` removes the drop entry

## Task 9: SD3xxx + SD2026 diagnostic codes

**Files:** `crates/sdust-diagnostics/src/codes.rs`, `crates/sdust-borrow/src/diag.rs`

Add SD3001..SD3015 plus SD2026; wire each into `explain()` with 2-4 sentence text per slice-3 style. Constructors in `sdust-borrow::diag` mirror `sdust-types::diag` shape.

- [ ] All codes declared
- [ ] All explain entries
- [ ] Constructor per code
- [ ] Round-trip test: every code is reachable from some negative .sd file

## Task 10: Scope-aware tolerance set (slice-3 hardening)

**Files:** `crates/sdust-types/src/check.rs`, `crates/sdust-types/src/items.rs`

Build a `ToleranceSet { names: HashSet<String>, allow_any: bool }` per body. The set is populated by the body's enclosing scope kind (agent / supervisor / sandbox / budget / unsafe / extern) plus the agent's state/method names. `allow_any = true` for extern bodies, macro bodies, and the inside of `unsafe` blocks beyond the first level.

In `synth_path` for a single-segment unresolved value: if the name is in the tolerance set OR `allow_any`, return a fresh inference variable (slice-3 behaviour). Otherwise emit `SD2021 unresolved_value` and return Error type.

- [ ] `ToleranceSet` type
- [ ] Builder per body kind
- [ ] `synth_path` consults the set
- [ ] All 20 examples still pass
- [ ] Slice-3 typeck_neg corpus still passes (some may need amendment to bring back to a tolerated-name shape)

## Task 11: Real method dispatch on user ADTs

**Files:** `crates/sdust-types/src/resolve.rs`, `crates/sdust-types/src/check.rs`, `crates/sdust-types/src/defs.rs`

Index `HirImpl` blocks: for each impl, map `(self_adt_id, method_name) → FnDef`. Method-call resolution:

1. If receiver type is `Adt(id, _)` and `id` is a user-declared (Struct/Enum) ADT → look up in the impl index; on miss emit SD2007.
2. Otherwise (opaque, primitive) → slice-3 built-in table; on miss return fresh Var (permissive).

- [ ] Index impl blocks at def-map build time
- [ ] `synth_method_call` consults the index for user ADTs
- [ ] User-struct .foo() with no impl: error SD2007
- [ ] User-struct .foo() with impl: succeeds, returns impl's ret type
- [ ] Opaque receiver still permissive

## Task 12: Protocol-aware handler param types

**Files:** `crates/sdust-types/src/items.rs`, `crates/sdust-types/src/resolve.rs`

Build `protocol_msg_index: HashMap<String, HashMap<String, Vec<TyId>>>` mapping `(protocol_name, msg_name) → param_types`. When checking an agent body, gather every protocol the agent implements (via the agent's HIR `protocols: Vec<TypeId>` list), then for each `on Msg(p1,...)` handler:

1. Find `Msg` in any implemented protocol (first-win).
2. If found, bind each handler param to the corresponding declared type.
3. Else emit `SD2026 protocol_msg_unknown` (warning) and fall back to fresh vars.

- [ ] `protocol_msg_index` built
- [ ] Handler-param binding consults the index
- [ ] Unknown-message warning
- [ ] Example 07 (Echo / Ping(msg: Str)) handler now sees msg: Str
- [ ] Example 08 (Count / Inc()) handler with no params: clean

## Task 13: Match exhaustiveness as error

**Files:** `crates/sdust-types/src/diag.rs`

Flip `non_exhaustive_match` severity from Warning to Error. Update the diagnostic message ("non-exhaustive match") and the `sdust explain` entry for SD2015.

- [ ] Severity flipped
- [ ] Updated explain text
- [ ] Negative test: match with a missing arm now errors instead of warning
- [ ] All 20 examples still pass (none have non-exhaustive matches)

## Task 14: Defaulting pass

**Files:** `crates/sdust-types/src/infer.rs`, `crates/sdust-types/src/items.rs`

Implement `default_typed_package(typed: &mut TypedPackage)`:
- Walk `expr_ty` map; for each entry, fully resolve via subst, and if the resolved kind is `IntInfer` rebind to `I32`; if `FloatInfer` rebind to `F64`.
- Same for `local_ty`.

Call after each fn-body check completes.

- [ ] Implementation
- [ ] Test: `let x = 1; x` → `x: I32` post-pass
- [ ] Test: `let x = 1.5; x` → `x: F64` post-pass

## Task 15: Driver/CLI wiring

**Files:** `crates/sdust-driver/Cargo.toml`, `crates/sdust-driver/src/pipeline.rs`, `crates/sdust-cli/src/cmd/check.rs`

Add `borrow_check(pkg: &Package) -> Vec<Diagnostic>` to the pipeline:
1. Run `check_package_typed`
2. If type-check has no *errors*, run `sdust_borrow::check_package(typed)`
3. Return concatenated diagnostics

In the CLI `check.rs`, run lex+parse+lower+type-check+borrow-check; exit code non-zero only on errors.

- [ ] `borrow_check` stage added
- [ ] CLI invokes it
- [ ] All 20 examples still `sdust check` clean

## Task 16: Source edits to examples 06 + 11

**Files:** `examples/06_for_while_loop.sd`, `examples/11_budget_block.sd`

Per slice-3 deferral note, amend the return types so the `?` operator's strict rule applies:

```sdust
fn process(items: &[I32]) -> Unit!WorkErr {
  ...
}
```

```sdust
fn run_job(input: Bytes) -> Unit!RunErr {
  ...
}
```

- [ ] Edit the two files
- [ ] `sdust check` clean for both

## Task 17: Negative borrow corpus

**Files:** `tests/borrow_neg/*.sd`, `crates/sdust-driver/tests/borrow_negatives.rs`

Twelve small inputs, one per SD3xxx + a couple combos:

- `use_after_move.sd` — `let a = String("x"); let b = move a; log_str(a)` → SD3001
- `move_out_of_borrow.sd` — `let a = String("x"); let r = &a; let b = move a` → SD3002
- `borrow_after_move.sd` — `let a = String("x"); let b = move a; let r = &a` → SD3003
- `mut_borrow_while_shared.sd` — SD3004
- `shared_borrow_while_mut.sd` — SD3005
- `two_mut_borrows.sd` — SD3006
- `borrow_outlives_owner.sd` — SD3007
- `cannot_move_borrowed.sd` — SD3008
- `move_out_of_ref.sd` — SD3009
- `arena_escape.sd` — `arena turn { let x = String("hi"); x }` → SD3010
- `non_sendable_message_arg.sd` — `agent!Msg(&buf)` → SD3011
- `assign_to_immut_local.sd` — SD3014

Driver test enumerates the folder and asserts each file emits its target code.

- [ ] Twelve fixtures
- [ ] Driver enumeration test
- [ ] Each fixture's expected code visible in output

## Task 18: Borrow-checker unit tests (per-crate)

**Files:** `crates/sdust-borrow/tests/*.rs`

- `basic.rs` — pure copy fn body (no diagnostics), pure no-op
- `borrows.rs` — shared borrow + mut borrow rules
- `arena.rs` — escape positive + negative
- `sendable.rs` — Copy ∨ owned ∨ ref-rejected
- `examples.rs` — drives every canonical `examples/*.sd` end-to-end; expects 0 errors
- `negatives.rs` — drives `tests/borrow_neg/*.sd`; expects ≥1 error each

- [ ] Five test files; counts roughly 25+ tests

## Task 19: SD2015 → error update + tolerance check

**Files:** `crates/sdust-types/tests/`, `tests/typeck_neg/`

Update any slice-3 typeck-neg fixture that was expecting Warning on SD2015 to expect Error.

- [ ] Audit + update
- [ ] All slice-3 fixtures still emit their expected codes (severity may change)

## Task 20: Docs — `borrowck.md`

**Files:** `docs/internals/borrowck.md`

Mirror `docs/internals/typeck.md`. Sections: overview, architecture, Ownership state machine, Copy rule, Sendable rule, arena regions, drop intent, lexical regions, scope-aware tolerance, dispatch policy, diagnostics table, future work (NLL).

- [ ] ~300 lines covering all sections

## Task 21: Tour chapter 14 — Ownership

**Files:** `docs/tour/14-ownership.md`

A new chapter walking through: ownership basics, `move`, Copy types, `&T`/`&mut T`, borrow rules, scope-end drop, arenas vs heap, cross-agent messages must be Sendable. With worked code examples.

- [ ] ~150 lines
- [ ] Linked from `docs/tour/README.md`

## Task 22: Tour updates

**Files:** `docs/tour/02-types.md`, `docs/tour/09-arenas.md`, `docs/tour/10-capabilities.md`

- 02-types: add "Copy types" subsection
- 09-arenas: add "Arena escape" callout with SD3010
- 10-capabilities: add "Cross-agent values must be Sendable" callout with SD3011

- [ ] Three small additions

## Task 23: Amendments A13..A21

**Files:** `docs/spec/v0.1-amendments.md`

Append the eight new amendments per the design doc §6.

- [ ] Eight entries
- [ ] Brief, follow A7-A12 style

## Task 24: `docs/reference/diagnostics.md`

Add the SD3xxx table + SD2026 row + flip SD2015 severity.

- [ ] Updated

## Task 25: README + SLICE files

**Files:** `README.md`, `SLICE3.md`, `SLICE4.md`

- README: roadmap table mark slice 4 done
- SLICE3.md: strike "Ownership / move / affine / borrow checking", "Match exhaustiveness as an error", "Explicit defaulting pass for IntInfer/FloatInfer", "Real protocol message-type checking for agent handlers" (those closed)
- SLICE4.md: new summary (mirror SLICE3.md template)

- [ ] Three files updated

## Task 26: Gate sweep

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
for f in examples/*.sd; do cargo run -q -p sdust-cli -- check $f || exit 1; done
```

- [ ] fmt clean
- [ ] clippy clean
- [ ] tests green (~270+)
- [ ] every example clean

## Task 27: Tag

```bash
git tag -a v0.4.0-borrowck -m "Slice 4: ownership + borrow checker"
git push origin main --tags
```

- [ ] Tag created
- [ ] Tag pushed
