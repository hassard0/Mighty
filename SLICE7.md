# Stardust Slice 7 — Complete

**Tag:** `v0.7.0-runtime`
**Date:** 2026-05-24

## What landed

### Runtime crate (spec §25 + §31.5)

- New crate `sdust-runtime` (~1 800 lines + tests).
- Tokio-backed executor (multi-thread by default; current-thread for
  deterministic mode).
- Per-agent `AgentDescriptor` with bounded mailbox, atomic budget
  counters, supervisor link, in-memory state.
- Bounded MPSC mailbox slabs with `Block` / `Drop` / `Fail` policies
  (default depth 1024, default `Block`).
- Supervisor strategies (`OneForOne`/`OneForAll`/`RestForOne`/
  `Escalate`) + `restart up_to N in DUR` sliding-window rate limit
  + uniform-jitter backoff with a deterministic seeded LCG.
- `BudgetTracker`: CPU, wall, memory (approximate per A37), mailbox
  depth, spawned-tasks, host allowlist, read/write path allowlist
  (prefix-match).
- Deadline-aware `ask` via `tokio::time::timeout` (`with_deadline`).
- Deterministic mode primitives: seeded `XorShift*` RNG +
  `LogicalClock`.
- JSON-line telemetry emitter (`Discard`/`Stderr`/`File`/`Buffer`),
  controlled by `STARDUST_TRACE` env var. 9 event kinds:
  `turn_start`, `turn_end`, `send`, `ask`, `reply`, `spawn`,
  `restart`, `budget_breach`, `shutdown`.
- Minimal `std.http` server (HTTP/1.1 GET, in-tree parser +
  `serve_in_memory`).
- `StdHost` routes effect calls through the budget tracker so net
  hosts and fs paths get gated.

### SIR per-turn evaluator hooks

- `sdust_sir::interp::run::run_handler_isolated(prog, handler, state,
  args, host)` — single-handler execution with proper self-ref state
  pass-through.
- `sdust_sir::interp::run::run_fn_with_budget(...)` — caller-controlled
  step budget for per-turn CPU translation.
- `Interp::assign_place` gained a slice-7 deref-of-ref write path
  (A44) so `(*self).fN = v` actually mutates state. The slice-6
  `invoke_handler` now delegates to `run_handler_isolated`, so the
  Counter example (08) returns the correct 1, 2, 3 sequence under
  both `sdust run` and the programmatic Runtime API.
- Top-frame locals snapshot on outer return so callers can recover
  synthetic state-holder values.

### Diagnostics SD5011..SD5015

- SD5011 deadline_exceeded
- SD5012 mailbox_full
- SD5013 supervisor_escalated
- SD5014 restart_limit_exceeded
- SD5015 capability_outside_sandbox

All have `sdust explain SD5xxx` entries.

### `sdust run` upgrade

- Default path now runs through the runtime
  (`pipeline::run_file_with_runtime`).
- `--legacy-interp` flag opts back into the slice-6 synchronous
  interpreter for diagnostic comparison (A45).
- Examples 07 (Echoer) and 08 (Counter) run end-to-end including
  state mutation via the new deref-write path.

### Conformance corpus

`tests/conformance/runtime-7/` ships **8** new cases:

1. `echo_main` — spawn + ask Echoer, observe reply
2. `counter_main` — three asks → 3 (state mutates)
3. `supervisor_sample` — supervisor declared, declarations parse + lower
4. `deterministic_pi` — pure arithmetic, deterministic
5. `sandbox_meta` — top-level sandbox + main
6. `deadline_pass` — ask with @500ms deadline against fast handler
7. `deadline_fail` — ask with @1s, succeeds (no slow handler in slice 7)
8. `extern_log` — control flow continues across a no-op

## Spec interpretations (A36..A45)

| Amendment | Topic |
|-----------|-------|
| A36 | `std.http.serve` MVP shape |
| A37 | slice-7 memory budget approximation |
| A38 | telemetry JSON schema (OTLP-flavoured) |
| A39 | deterministic mode = current-thread + seeded RNG + logical clock |
| A40 | mailbox defaults (depth 1024, Block policy) |
| A41 | slice-7 cancellation = at next await |
| A42 | `restart up_to N in DUR` semantics |
| A43 | top-level `sandbox` executes as a child runtime |
| A44 | slice-7 deref-of-ref write path (fixes counter mutation) |
| A45 | `sdust run --legacy-interp` opt-out |

## Stats

- **327 tests pass** (slice 6: 290 → slice 7: +37)
- 5 new SD5xxx diagnostic codes
- 8 runtime-7 conformance cases
- New crate `sdust-runtime`
- `sdust-driver` + `sdust-cli` rewired so `sdust run` defaults to runtime

## Still deferred (slice 8 unless noted)

- LLVM / Cranelift codegen — slice 8 (final v0.1 slice)
- Wasm component-model codegen — slice 8
- Monomorphization of generics — slice 8
- Real arena allocator — slice 8 (slice 7 ships approximate `mem_bytes`)
- Real `extern { fn ... }` calls — slice 8
- Real effect-system syscalls — slice 8 (slice 7 wires host trait + sandbox gates)
- Automatic supervisor restart orchestrator — slice 8 (strategies +
  rate-limit + backoff are in place)
- Cooperative cancellation inside a turn — slice 8 (with codegen)
- Strict OTLP wire format for telemetry — slice 8
- Field-level borrow tracking — slice 8 (slice-4 still local-granular)
- DCE / inlining / escape analysis — post-v0.1
- True NLL / Polonius — post-v0.1
- Effect-row polymorphism — post-v0.1
- Full Drop impl execution at scope exit — post-v0.1
- Distributed cross-machine agents — post-v0.1

## Files of note

- `crates/sdust-runtime/src/runtime.rs` — Runtime + RuntimeBuilder
- `crates/sdust-runtime/src/agent.rs` — AgentDescriptor, run_one_turn
- `crates/sdust-runtime/src/mailbox.rs` — bounded MPSC + MessageFrame
- `crates/sdust-runtime/src/supervisor.rs` — RestartTracker, strategies
- `crates/sdust-runtime/src/budget.rs` — BudgetTracker, allowlists
- `crates/sdust-runtime/src/timer.rs` — deadline helper
- `crates/sdust-runtime/src/telemetry.rs` — JSON emitter
- `crates/sdust-runtime/src/deterministic.rs` — SeededRng, LogicalClock
- `crates/sdust-runtime/src/http.rs` — minimal HTTP/1.1 server
- `crates/sdust-runtime/src/host_std.rs` — real OS host with sandbox gates
- `crates/sdust-runtime/src/error.rs` — RuntimeError taxonomy
- `crates/sdust-sir/src/interp/run.rs` — `run_handler_isolated`,
  `run_fn_with_budget`, deref-of-ref write path, top-frame snapshot
- `crates/sdust-driver/src/pipeline.rs` — `run_file_with_runtime`
- `crates/sdust-cli/src/cmd/run.rs` — `--legacy-interp` flag wiring
- `crates/sdust-diagnostics/src/codes.rs` — SD5011..SD5015 + explain
- `docs/internals/runtime.md`, `scheduler.md`, `mailboxes.md`,
  `supervisors.md`, `budgets.md`, `telemetry.md` — new
- `docs/spec/v0.1-amendments.md` — A36..A45
- `tests/conformance/runtime-7/*` — 8 runtime cases
- `crates/sdust-driver/tests/conformance_runtime_7.rs` — corpus driver
- `crates/sdust-runtime/tests/*.rs` — 8 integration tests
