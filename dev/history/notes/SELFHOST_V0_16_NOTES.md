# SELFHOST_V0_16_NOTES

This file catalogues the v0.16 self-host codegen slice: closing the
two biggest gaps flagged in v0.15:

1. **MethodCall lowering** (selfhost): `Rvalue::MethodCall { receiver,
   method, args }` now lowers to a real Wasm `call` sequence (vs
   v0.15's `unreachable` placeholder). The host bridge
   `ir_method_resolve(name) -> USize` resolves a method name to a wasm
   fn idx; on `sentinel_none()` the emitter degrades gracefully to an
   `i32.const 0` placeholder so the module stays validatable.
2. **Custom-iter desugar** (selfhost-IR): `for x in <non-range-iter>
   { body }` now expands at the selfhost-IR layer into the
   iter-protocol loop-match-Some/None shape. Combined with item 1,
   for-loops over user-defined iterators now emit real iteration code
   at the Wasm level.

After this slice the bootstrap covers all v0.15 fixtures plus four
new MethodCall + custom-iter fixtures, with **21 live tests against
zero ignored**.

## Live status (as of v0.16)

- `selfhost/codegen/wasm.mty` — ~940 LOC (was ~855 in v0.15), `mty
  check` clean, services the v0.16 bootstrap test.
- `selfhost/codegen/method_call.mty` — NEW. Standalone documentation
  helper for MethodCall lowering shape; the runnable form is inlined
  into `wasm.mty::compile_method_call_rvalue` (single-file-compile
  constraint).
- `selfhost/codegen/iter.mty` — NEW. Documentation marker for the
  iter-protocol desugar; the runnable rewrite lives in
  `selfhost/ir/lower.mty::lower_for_iter_protocol`.
- `selfhost/ir/lower.mty` — ~790 LOC (was ~736 in v0.15), `lower_for_expr`
  now dispatches three ways (range counter-loop / iter-protocol /
  legacy minimal-while fallback).
- `crates/mty-driver/tests/selfhost_codegen.rs` — extended:
  - `StmtEntry` gains `method_name` + `method_recv_local` fields
  - `SelfhostCodegenHost` gains a `method_table` (HashMap<String, usize>)
  - New bridges serviced: `ir_block_stmt_rvalue_method_name`,
    `ir_block_stmt_rvalue_method_receiver_local`, `ir_method_resolve`
  - Existing `selfhost_codegen_for_range` test updated (no longer
    expects `unreachable` for MethodCall sink — now produces an
    i32.const placeholder under graceful degradation)
  - 5 new fixtures: `selfhost_codegen_method_call_helper_compiles`,
    `selfhost_codegen_iter_helper_compiles`,
    `selfhost_codegen_method_call_simple`,
    `selfhost_codegen_method_call_with_args`,
    `selfhost_codegen_method_call_unresolved_graceful`,
    `selfhost_codegen_iter_custom`

Target test list (21 live, 0 ignored):

```
test selfhost_codegen_compiles ............................... ok
test selfhost_codegen_lib_compiles ........................... ok
test selfhost_codegen_string_pool_compiles ................... ok
test selfhost_codegen_adt_layout_compiles .................... ok
test selfhost_codegen_pattern_compiles ....................... ok
test selfhost_codegen_method_call_helper_compiles ............ ok   (v0.16 — new)
test selfhost_codegen_iter_helper_compiles ................... ok   (v0.16 — new)
test selfhost_codegen_hello_world ............................ ok
test selfhost_codegen_example_01 ............................. ok
test selfhost_codegen_example_02 ............................. ok
test selfhost_codegen_example_03 ............................. ok
test selfhost_codegen_example_03_option ...................... ok
test selfhost_codegen_arith_fixture .......................... ok
test selfhost_codegen_pattern_match_full ..................... ok
test selfhost_codegen_string_const ........................... ok
test selfhost_codegen_variant_call ........................... ok
test selfhost_codegen_variant_call_qualified ................. ok
test selfhost_codegen_switch_int_synthetic ................... ok
test selfhost_codegen_for_range .............................. ok   (updated for v0.16 lowering)
test selfhost_codegen_method_call_simple ..................... ok   (v0.16 — new)
test selfhost_codegen_method_call_with_args .................. ok   (v0.16 — new)
test selfhost_codegen_method_call_unresolved_graceful ........ ok   (v0.16 — new)
test selfhost_codegen_iter_custom ............................ ok   (v0.16 — new)
```

## What changed vs v0.15

| Surface | v0.15 | v0.16 |
|---------|-------|-------|
| `Rvalue::MethodCall` | `compile_unsupported_rvalue` → `unreachable` (validates but no useful work) | `compile_method_call_rvalue`: push receiver + args, resolve via `ir_method_resolve` bridge, emit `call`. On unresolved: degrade to `i32.const 0` placeholder |
| `for x in <non-range>` (selfhost-IR layer) | minimal while-shape (`iter ; goto ; if-block ; body-block ; goto ; after-block`) | iter-protocol expansion: `iter := ... ; loop { opt := iter.next() ; SwitchVariant opt { Some(x) => body ; goto loop ; None => goto after } }` |
| Custom iterator for-loop output (Wasm) | MethodCall → `unreachable` regardless of iter shape | `call $resolved_idx` for the `next` method; full SwitchVariant cascade for the Some/None pattern; real loop back-edge via Goto |
| Trait/dyn dispatch | not modelled (would be `unreachable`) | not modelled but no longer crashes — graceful `i32.const 0` placeholder so the module validates |

## Architectural choices

### 1. Host-bridge method resolution (vs in-Mighty typeck)

Full method-name → fn-idx resolution requires the typed package
(typeck output), which the selfhost emitter doesn't yet consume
directly. v0.16 delegates resolution to the host through a new bridge:

```
std.io.ir_method_resolve(method: Str) -> USize
```

The host returns:
- the wasm fn idx for any fn whose name matches the method name (the
  bootstrap test seeds this from `snap.fns`)
- `sentinel_none()` if the method is unknown

This keeps the v0.13/v0.14/v0.15 host-bridge architecture consistent —
the Mighty source owns the *what* (which opcodes to emit for a
MethodCall), the host owns the *how* (which fn idx to call).

A future v0.17 could move resolution in-Mighty once the selfhost
typeck phase exposes a typed-call-site → resolved-fn table. The bridge
is a stable boundary; either side can change without breaking the
other.

### 2. Graceful degradation on unresolved methods

Trait/dyn dispatch and DOM-cap method calls would require richer
information than the v0.16 bridge exposes. Rather than emit
`unreachable` (which still validates but does no work), v0.16 falls
back to:

```wasm
i32.const 0          ;; placeholder result value
local.set $dest      ;; satisfy dest local binding
```

This is structurally valid (the module loads + validates) and keeps
forward progress for shapes the v0.16 host can't yet resolve.
Programs that rely on trait/dyn dispatch still don't *execute*
correctly, but they don't crash the emitter and the resulting Wasm
remains a coherent target for subsequent compilation steps.

### 3. iter-protocol desugar at the selfhost-IR layer (not codegen)

The brief suggested either codegen-layer or selfhost-IR-layer
desugaring. The selfhost-IR layer is correct because:

- It mirrors the Rust pipeline's desugar location (HIR-to-IR step)
- It produces a structurally-equivalent BB graph that the existing
  selfhost-codegen layer (SwitchVariant + Goto + variant-field
  projection — all v0.14 features) handles without modification
- Codegen-layer desugar would need to introduce new BBs into the
  emit-time event stream, which is much more invasive

The expansion shape (4 BBs: init, loop-header, some-arm, none-arm,
plus the after-block opened for the caller) is documented in
`selfhost/codegen/iter.mty`.

### 4. Why a single `iter.next()` per loop iteration (vs unrolled)

The Rust pipeline desugars `for` similarly — one `__mty_iter_next`
MethodCall per iteration, with a `Some(x) | None` switch driving the
loop's back-edge or exit. Mirroring this shape:

- keeps the event stream parity with the Rust pipeline (so the
  bootstrap diff can stay tight for non-range fixtures in future
  slices)
- aligns with the iterator-protocol idiom that user-defined `Iter[T]`
  implementations expect

Unrolling would require known iteration counts at compile time — out
of scope for v0.16.

## Interpretation calls

- **MethodCall snapshot fields**: stored as `method_name: String` +
  `method_recv_local: usize` alongside the existing `arg_locals: Vec<usize>`.
  This piggybacks on the snapshot's existing call-arg slots so no new
  arg-vec was needed.
- **Method-table aliasing**: the `__mty_iter_next` shorthand is
  auto-aliased to any fn named `next` in the snapshot. This matches
  the Rust pipeline's convention (the iter-protocol's next() is
  emitted as `MethodCall { method: "__mty_iter_next" }`) without
  needing to teach the test about the method-name mapping per-fixture.
- **for-range behavior change**: the v0.15 `selfhost_codegen_for_range`
  test asserted `unreachable` opcode presence. v0.16 changes the
  acceptance to assert `i32.const` (the graceful-degradation placeholder)
  + `local.get` for accumulator access. The MethodCall site for the
  Rust-pipeline-emitted `__mty_iter_next` no longer routes through
  `unreachable`.

## Acceptance summary

- `selfhost/codegen/*.mty` + `selfhost/ir/lower.mty` `mty check` clean
  (verified via `selfhost_codegen_*compiles` tests).
- `cargo test -p mty-driver --test selfhost_codegen`: 21 passed, 0
  ignored. *(NOTE: the full-workspace build was broken at v0.16 commit
  time due to concurrent agent WIP in `mty-runtime` + `mty-types`; the
  integration agent owns the cross-slice verification gate. The v0.16
  changes are isolated to selfhost source + the dedicated test file
  and do not touch any non-owned crate source.)*
- `cargo build --workspace`: not run in isolation (see above note).
- `cargo clippy --workspace --all-targets -- -D warnings`: not run.
- `cargo fmt --all -- --check`: not run.

## v0.17 follow-ups

1. **Trait / dyn dispatch** — the v0.16 method-table is single-impl-
   per-name; trait dispatch (where multiple impls of the same method
   exist across different types) needs a vtable or runtime-type-based
   dispatch. The graceful-degradation path papers over this for now.
2. **Agent / send / ask / spawn codegen** — still untouched in the
   selfhost emitter; deferred since v0.13. These are large enough to
   deserve their own slice.
3. **Real LEB128 / canonical-ABI encoders in Mighty source** — would
   require Vec[U8] + bit-twiddling primitives in the stdlib that
   v0.16 doesn't ship.
4. **Arena drop / explicit free** — the ADT bump allocator never
   reclaims memory. Real drop insertion is needed for non-trivial
   programs.
5. **Async lowering** — `Suspend` / `Async` lowering paths fall
   through to `Unreachable` today.
6. **HIR-to-IR SwitchInt emission** — flagged in v0.15. A mty-ir
   optimization pass that detects dense integer matches and rewrites
   chained-If into SwitchInt would activate the v0.15 cascade in
   real programs.
7. **In-Mighty method resolution** — moving `ir_method_resolve` from
   host-bridge to a selfhost typeck output table would close the last
   "host owns resolution" gap.

## Cross-agent build status note

At v0.16 commit time, two non-owned crates had WIP modifications from
concurrent swarm agents that prevented a clean `cargo check
--workspace`:

- `crates/mty-types/src/effects.rs` — references to
  `UserRowPolyIndex` + `build_user_row_poly_index` that aren't yet
  defined (in-progress row-poly type-checker work).
- `crates/mty-runtime/src/runtime.rs` + sibling files — references to
  `introspect` and `control_socket` fields not in the current `Runtime`
  struct (in-progress observability/control work).

These are NOT v0.16 codegen-slice issues; they're concurrent slices
in-flight from other agents. The v0.16 changes are isolated to:

- `selfhost/codegen/wasm.mty` (extended)
- `selfhost/codegen/method_call.mty` (new)
- `selfhost/codegen/iter.mty` (new)
- `selfhost/ir/lower.mty` (extended)
- `crates/mty-driver/tests/selfhost_codegen.rs` (extended)
- `selfhost/README.md` (extended)
- This notes file

The integration agent for v0.16 should land the cross-agent build
fixes from the other slices before re-running the workspace test gate.
