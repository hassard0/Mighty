# Mighty v0.48 Release Notes

**Tag:** `v0.48.0`
**Date:** 2026-06-03
**Status:** SHIPPED — clears the v0.47 carry-forward Cranelift
struct-codegen item and lands the Vec-of-aggregate fix, native String
constructors, and a permanent Windows-CI fix.

**Headline:** Mighty v0.48 — Cranelift aggregate-codegen realignment:
struct field-assignment, `Vec`-of-aggregate element sizing, and native
`String` constructors all land together (marquee: `Vec[String]` no
longer corrupts the heap, and example `26_string_vec` runs native at
interpreter parity).

## Summary

v0.48 is a Cranelift aggregate-codegen pass. T1 closes the v0.47
carry-forward struct field-assignment defect (sibling corruption +
nested-aggregate boxing). The Vec-of-aggregate work then fixes the
class of `Vec[T]`-where-`T`-is-aggregate bugs: push-only element
inference, `Vec.new()` result-temp element sizing, and the
`Vec[String]` element-push SIGSEGV. Native `String` constructors
(`new`/`with_capacity`/`from_str`) build on that to make
`26_string_vec` JIT-match the interpreter — the first of the
`VecOfAggregate` conformance examples to flip green. Alongside the
codegen work, the Windows CI job was rescued from a 6-hour serial-libtest
hang (now ~9 min via `cargo-nextest`) and a flaky work-stealing test was
de-flaked.

## Shipped

- **PR #36 (T1) — Cranelift struct field-assignment codegen.** Closes
  the v0.47 carry-forward: `let mut p = Point{..}; p.x = 5` no longer
  clobbers a sub-8-byte-offset sibling field, and nested writes
  (`o.inner.x = 7`) project inline instead of dereferencing a boxed
  child. The previously-`#[ignore]`d `projection_store_v047` Cranelift
  cases are un-ignored and pass.

- **PR #37 — Windows CI rescue + conformance-sweep parallelisation.**
  The Windows test job moved to `cargo-nextest` (process-per-test +
  per-test terminate-after), turning a 6-hour serial-libtest deadlock
  into a ~9-minute run. The example-corpus conformance sweep is
  parallelised (`diff_all`, per-example tempdir isolation) and stays
  off the per-PR Windows critical path (it runs in full on Linux +
  macOS; the Windows-specific compile+link path is covered by
  `conformance_native`). De-flaked
  `work_stealing::worker_pool_processes_all_tasks` (distribution is
  best-effort, not a contract).

- **PR #38 (#297 partial) — `Vec`-of-aggregate codegen.** Three layers:
  (1) push-only element inference — `Vec.new()`/`with_capacity()` synth
  `Vec[?E]` and `push(x)` unifies `x` with `E`, so a Vec whose element
  is pinned only by `.push` infers `T`; (2) the `Vec.new()` result-temp
  carries the real `Vec[T]` so `emit_vec_new` records the true element
  size (16 for `String`, not the 8-byte fallback); (3) the `Vec[String]`
  element push routes through `string_pair` instead of
  memcpy-from-operand-address, fixing the heap-corruption SIGSEGV.
  Vulcan-validated (full workspace, 3894 tests, 0 failed).

- **PR #39 (#297) — native `String` constructors → example 26.**
  `String.from_str(s)` (identity on the `(ptr,len)` pair),
  `String.new()` / `String.with_capacity(n)` (empty String) now have
  Cranelift codegen instead of resolving to unhandled extern symbols
  that yielded garbage. Combined with the Vec-of-aggregate fixes,
  **`26_string_vec` JIT-matches the interpreter** and is removed from
  `KNOWN_FAILING`.

## Carry-forward priorities

The remaining two `VecOfAggregate` examples (`42_crypto_url`,
`43_secure_session`) stay `KNOWN_FAILING`. They depend on two distinct
follow-up efforts, both scoped under task #297:

- **Native String value-model + methods.** `String.len()` /
  `push_str` / `push` / `clear` / `pop` are not yet native (JIT routes
  them through the interpreter-hosted extern stub; `len()` returns 0).
  Making them native requires typing `String` bindings, which has broad
  blast radius (the let-rebind/log path and struct/agent `String` fields
  both need coordinated handling) — a dedicated value-model rework. A
  working `len` prototype + the exact failure modes
  (`dynamic_log::log_of_local_str_passes_through_let`,
  `28_agent_with_llm_field`) are recorded in #297.

- **Native stdlib for `42`/`43`.** Their segfault is the
  `std.crypto` / `std.encoding` / `std.url` / `std.uuid` / `std.regex`
  surface returning aggregates that are stubbed to null in JIT. ~27
  functions need a runtime ABI wrapper (calling the existing
  `mty-stdlib` implementations) + a codegen dispatch table modelled on
  `is_native_fs_method` / `emit_fs_call`. Crypto/encoding are mechanical
  (`(bytes) -> slot`); url/uuid/regex return structs the code
  field-accesses (opaque-handle codegen — the hard part).

- Pending tasks #253 (SWE-bench), #262 (BOLT training profile path).

## Acknowledgements

The Vec-of-aggregate work was diagnosed CLIF-first (`MTY_DUMP_CLIF`)
and validated layer-by-layer on the `vulcan` workspace suite. The
native-String thread is recorded as a value-model lesson: a 16-byte
`(ptr,len)` aggregate must be materialised through the *registered*
`agg_slots` slot, never an ad-hoc `eval_const` slot, or a later
`place_addr` clobbers the binding.
