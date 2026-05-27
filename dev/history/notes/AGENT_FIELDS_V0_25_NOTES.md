# Agent Fields v0.25 — Track C notes

**Scope**: two related gaps surfaced by v0.24 Track E's canvas-game
work that block the Notetris board from living on the agent.

1. `agent X { board: [U32; 200] }` — fixed-size-array type rejected
   "downstream" (Track E's words; the symptom they hit was the
   typeck/runtime treating it as a slice).
2. Cross-callback persistence — when `inst.exports.keydown(k)` is
   invoked from a JS host repeatedly, the agent state set in one
   callback isn't visible in the next.

## (1) Arrays in agent fields

### What was actually broken

The parser already accepted `[T; N]` in agent field declarations —
`agents::state_decl` calls `types::type_expr`, which has had `array`
support since at least v0.2. The real break was in **HIR lowering**:
`crates/mty-hir/src/lower/types.rs`'s `TYPE_ARRAY` arm extracted the
element type but unconditionally set `len: None`, dropping the size
expression. Downstream:

* `mty-types/src/resolve.rs::resolve_hir_type` calls
  `const_eval_len(&pkg.exprs[lid])` on the HIR length expression and
  passes the result to `arena.array(elem, n)` — but with `len = None`
  the resolver always built `TyData::Array { elem, len: None }`,
  which is the slice shape (`&[T]`).
* That meant any layout or storage-size computation downstream
  couldn't tell the agent's `board: [U32; 200]` from a slice
  reference. The Notetris demo's board declaration silently
  degraded.

### The fix

One-line shape: capture the first expression-shaped child of
`TYPE_ARRAY` and lower it as an `ExprId`, then pass `len = Some(...)`
into `HirType::Array`. The downstream `const_eval_len` path already
existed and handles integer literals (`HirLiteral::Int(v, _)` →
`Some(v as u64)`), so a `[U32; 200]` round-trips to
`TyData::Array { elem: u32, len: Some(200) }` without further work.

Files touched: `crates/mty-hir/src/lower/types.rs` (12 lines).

### What still doesn't work

* `const N: U32 = 200; agent X { board: [U32; N] }` parses, but
  `const_eval_len` only handles literal ints — a constant reference
  resolves to `len: None` (slice degrade). v0.26 should grow a real
  const-evaluator for array lengths; for v0.25 users pass literals.
* `[Piece; 7]` where `Piece` is a user-defined enum parses and
  typechecks but the SIR runtime hasn't been exercised against
  enum-typed cells. The smoke example uses `[I32; 16]` to stay on
  the well-tested arithmetic path.

### Tests

* `crates/mty-syntax/tests/agent_fields_arrays.rs` — 5 tests, all
  pinning the parser surface (`agent X { board: [U32; 200] }` parses
  without errors; complex elem types work; the named-size form
  parses even if typeck can't const-eval it).
* `crates/mty-types/tests/agent_field_array_typeck.rs` — 4 tests
  including an explicit assertion that `HirType::Array.len ==
  Some(_)` (the regression we just fixed) and that `cells[i] = k`
  typechecks inside a handler body.

## (2) Cross-callback persistence

### State of play at v0.25

Two execution paths exist for agents in Mighty:

* **SIR runtime path** — `mty-runtime` spawns `AgentDescriptor`s into
  an `AgentRegistry`, each owning a `Mutex<Value>` slot for the state
  struct. Every message dispatch goes through
  `run_one_turn_with_shared_reply`, which `lock()`s the state, hands
  it to the interpreter as the `self` value, and writes back the
  mutated state at end-of-turn. **Persistence already works** here —
  it's just never been pinned by a regression test.
* **wasm32-web export-callback path** — `crates/mty-codegen-wasm` has
  no agent lowering at all. `agent` blocks get the IR-side ctor +
  handler shells (`Agent::ctor` / `Agent::handlers`), but those fns
  aren't lifted into the embedded core wasm module and the JS host's
  `inst.exports.keydown(k)` doesn't dispatch through them. The demo
  06 canvas-game source admits this in its top comment: "agent-
  dispatched handlers (`on Left()` etc.) compile but aren't
  reachable from the exported callbacks at the wasm-backend's
  current dead-code elimination".

### What v0.25 ships

* A **regression test** pinning the SIR-runtime persistence
  contract: `crates/mty-runtime/tests/agent_callback_persistence.rs`
  spawns an agent, sends three `Inc()` messages, and asserts the
  replies are `1, 2, 3` (not `1, 1, 1`). A companion test sets via
  callback A and reads via callback B (the exact Track E worry).
  A third test pins that two agent instances have independent
  state.
* The **design** for wasm32-web persistence (the canvas-game's
  "single-agent-instance" target), recorded here so the v0.26 swarm
  has a starting point. **Not implemented in v0.25** — Track C's
  6-hour budget got consumed by the field-array work + cross-track
  build-break recovery (see below).

### Design: single-agent-instance pattern for wasm32-web

The canonical web-game shape: one agent declared at the top level,
spawned once from `main()`, then driven by a small set of host-
invoked callbacks (`frame`, `keydown`, `keyup` per
`web_lower::is_web_callback_export`). Multi-agent persistence (the
cluster mesh case) keeps using the existing SIR-runtime path.

**Storage layout**:
* At module init the wasm emitter reserves a fixed memory region
  per agent declaration, sized to the agent's state struct (sum of
  field sizes derived from typeck-resolved types — with array fields
  now carrying `len = Some(N)` this is computable).
* The region is anchored at a stable linear-memory offset
  (immediately after the string-intern pool, before the stack base).
* A small `__agent_<Name>__inst_ptr` global stores the offset; if
  zero, the agent hasn't been spawned yet (a trap on access).

**Dispatch**:
* `main()` lowering recognises `spawn AgentName(args)`, writes the
  ctor's output bytes into the reserved region, and sets
  `__agent_<Name>__inst_ptr` to the region offset.
* Each exported callback (`keydown`, `frame`, ...) checks for an
  associated agent handler and, if present, loads the agent's state
  pointer, calls the handler fn with the state pointer as an
  implicit first arg, and the handler reads/writes through the
  pointer. Linear memory persists across exports so there's no
  marshaling per call.

**Why "single-agent-instance"**:
* Multi-agent in a single web module would need a registry table
  inside linear memory (id → offset), which is doable but pushes
  this out of the 6-hour Track C scope.
* The canvas-game and Notetris cases need exactly one agent —
  matching the v0.25 surface to the realistic v0.26 demo lands the
  cleanest user-visible delta without scope creep.

**Files to extend for v0.26**:
* `crates/mty-codegen-wasm/src/emit.rs` — agent state region
  allocation in the data section; `__agent_<Name>__inst_ptr` global;
  callback-export dispatch.
* `crates/mty-ir/src/lower/items.rs::register_agents` — populate
  the state ADT's variant fields from typeck-resolved state-field
  types (currently `fields: vec![]` per the existing comment
  "populated lazily as state fields appear in handlers" — the wasm
  emitter can't be lazy, it needs the layout up front).
* `crates/mty-codegen-wasm/src/web_lower.rs` — agent-callback
  routing table (name → handler IrFnId).

## Cross-track build-break note

The v0.25 swarm (4 sibling agents in the same working tree) hit
the "shared-tree agent concurrency gotcha" pattern from memory:

1. Track D's in-flight format-spec work added `BadWidth` /
   `BadPrecision` variants to `mty_macros::FormatExpandError` before
   Track D had a chance to update `mty-hir/src/lower/macros.rs`'s
   exhaustive match on that enum.
2. The workspace build broke for everyone else until Track D
   pushed the matching hir-side patch.
3. The recovery pattern (used here): targeted `git stash push --` of
   the sibling tracks' WIP, verify locally, `git stash pop`. The
   alternative (`git add -A`) would sweep sibling WIP into this
   commit.

This is the third documented occurrence of the gotcha; the memory
entry's "single-writer" recommendation is starting to look like the
right long-term answer, especially as the in-tree swarm count
grows.

## Test count

* 5 syntax tests (`agent_fields_arrays.rs`)
* 4 typeck tests (`agent_field_array_typeck.rs`)
* 3 runtime persistence tests (`agent_callback_persistence.rs`)
* 1 example (`examples/25_agent_array.mty`) — verified by the
  CI example sweep through `mty check`

Total: 12 new tests + 1 example.
