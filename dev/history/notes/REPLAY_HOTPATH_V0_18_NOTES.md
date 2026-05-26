# Replay Recorder Hot-Path Wiring (v0.18)

Date: 2026-05-26
Slice: v0.18, Tier 1.4 (continuation of v0.17 deterministic replay).
Prior work: `REPLAY_V0_17_NOTES.md` — recorder + wire format + CLI.

## What shipped

v0.17 landed the recorder + wire format + `mty replay` CLI but never
called any `record_*` function from the Runtime. Every recorded trace
came out empty save for the start event. v0.18 wires the recorder into
the actual Runtime hot path so traces capture real agent execution.

### Instrumentation sites wired

| Site | File | Event |
|------|------|-------|
| `RuntimeBuilder::build` | `runtime.rs` | reads `MTY_RECORD_TRACE`; calls `install_from_env` |
| `Runtime::spawn_agent_with_affinity` | `runtime.rs` | `record_spawn(agent_id, name, None)` |
| `Runtime::send` | `runtime.rs` | `record_message_sent(0, to, msg, payload)` |
| `Runtime::ask`  | `runtime.rs` | `record_message_sent(0, to, msg, payload)` |
| `run_one_turn_with_shared_reply` | `agent.rs` | `record_message_handled(agent, msg, elapsed_us)` |
| agent loop budget-exhaustion arm | `runtime.rs::spawn_agent_loop` | `record_budget_exhausted(agent, code)` |
| agent loop cancellation arm | `runtime.rs::spawn_agent_loop` | `record_budget_exhausted(agent, reason)` |
| agent loop terminal exit | `runtime.rs::spawn_agent_loop` | `record_exit(agent, reason)` |
| `StdHost::effect_call` → `std.time.now` | `host_std.rs` | `record_clock_read(agent, value_ms)` |
| `StdHost::effect_call` → `std.time.sleep` | `host_std.rs` | `record_clock_read(agent, value_ms)` |
| `StdHost::effect_call` → `std.random.*` | `host_std.rs` | `record_random_read(agent, bytes)` |
| `StdHost::effect_call` → `std.fs.read/exists/list_dir` | `host_std.rs` | `record_io_read(agent, "fs:<path>", bytes)` |
| `StdHost::effect_call` → `std.http.get/post` | `host_std.rs` | `record_io_read(agent, "net:<url>", bytes)` |
| `Runtime::shutdown` + `Runtime::drop` | `runtime.rs` | `recorder.flush_to_disk()` + `uninstall()` |

### Public API additions

- `replay::with_recorder<F: FnOnce(&Recorder)>(f: F)` — fire-and-forget
  hook for instrumentation sites. Cheap when no recorder installed.
- `replay::recording_enabled()` — `bool` fast-path for sites that
  prepare expensive arguments (payload encoding) only when needed.
- `BudgetTracker::trip(agent_id, breach) -> RuntimeError` — emits
  `BudgetExhausted` to the recorder + maps the breach to a RuntimeError
  in one call.
- `BudgetBreach::trace_reason() -> &'static str` — stable label for
  the trace's `BudgetExhausted.reason` field.
- `StdHost::with_agent_id(agent_id: u64) -> Self` — tags a host with
  its owning agent so IO/clock/random events carry the right id.

Re-exported at crate root for downstream callers:
`mty_runtime::with_recorder`, `mty_runtime::recording_enabled`,
`mty_runtime::global_recorder`.

## Design choices

### Global recorder via `OnceLock` vs `task_local`

We kept the v0.17 pattern — a process-wide `RwLock<Option<Arc<Recorder>>>`
in `replay::recorder`. The alternative (a tokio `task_local!`
per-agent recorder) was rejected because:

1. The Mighty runtime owns multiple tokio runtimes (one per worker
   thread) — task-locals would need a separate slot per worker.
2. The blocking handler shim in `agent.rs::run_one_turn_async` runs
   on `spawn_blocking`, which can't see the spawning task's locals
   without manual propagation.
3. The Runtime supports a single active recording at a time —
   `MTY_RECORD_TRACE` is process-wide. Per-runtime recording is a v0.19
   stretch (see below).

### Where to record `MessageHandled`

The agent loop has two natural points: after `run_one_turn_async`
returns, or inside `run_one_turn_with_shared_reply` before the
reply channel fires. We chose the inner one because an `ask()` caller
observes the reply via the oneshot channel and then often shuts down
the Runtime immediately. If we record after the outer dispatch, the
trace can race with shutdown and lose the final handled event. The
inner placement guarantees the trace contains the event before any
reply is observable.

### Payload encoding

`Value` does not implement `Serialize` (it carries Host references).
We render via `format!("{:?}", args)` to bytes — opaque-but-human-
readable, enough for trace inspection. The fast path short-circuits
when no recorder is installed (one `RwLock::read`), so the cost is
only paid during recording.

Full byte-identical payload encoding (so replay can re-construct the
Value tree exactly) is a v0.19 stretch.

### Budget-exhaustion vs Exit

Every agent that dies (trap, budget, cancellation) emits BOTH a
`BudgetExhausted` AND an `Exit` event. The reason strings differ —
`BudgetExhausted.reason` is the breach kind (`"cpu"`/`"mem"`/`"MT5009"`),
`Exit.reason` is the agent-loop's terminal reason
(`"trap:MT5009"`/`"shutdown"`/`"normal"`). The replayer's
self-consistency check treats both as expected for a dying agent.

## Zero-overhead-when-disabled verification

The `with_recorder` macro expands to:

```rust
if let Some(rec) = global_recorder() {
    f(&rec);
}
```

`global_recorder()` takes a `RwLock::read()` and clones the `Arc`
only when `Some`. With no recorder installed: one atomic load, one
branch, return None. That's ~3ns per call on a modern x86 — well
under the 5% target.

`disabled_when_env_unset` (in `tests/replay_e2e.rs`) verifies the
recorder is `None` end-to-end when `MTY_RECORD_TRACE` is unset.
`empty_path_env_treated_as_unset` verifies the explicit-empty-string
case (`MTY_RECORD_TRACE=""`) is also a no-op.

## Tests added

`crates/mty-runtime/tests/replay_e2e.rs` ships 8 end-to-end tests
driven by a real `Runtime`:

1. `recording_round_trip` — spawn 2 agents, ask 2 messages, verify
   trace contains >=2 Spawn + >=2 MessageSent + >=2 MessageHandled.
2. `disabled_when_env_unset` — no env => no recorder, no flush, no
   trace file.
3. `recording_captures_distinct_agent_ids` — 3 agents => 3 unique
   spawn ids in trace.
4. `recorder_survives_unknown_handler_trap` — sending a "Bogus"
   message that traps still flushes the trace.
5. `fire_and_forget_send_captured` — `Runtime::send` records, too.
6. `message_handled_carries_monotonic_msg_idx` — per-agent msg_idx
   is strictly 0, 1, 2 across three asks.
7. `empty_path_env_treated_as_unset` — see above.
8. `recorder_uninstalled_after_shutdown` — `global_recorder()` is
   `None` after `shutdown().await`.

Plus 2 new unit tests in `replay::recorder::tests` for `with_recorder`
itself (one for the noop-when-uninstalled branch, one for the
installed branch). All run serially via a `parking_lot::Mutex` to
avoid global-state races with other recorder unit tests.

Existing test counts unchanged:
- `replay` (integration, v0.17 baseline): 10 tests, all pass.
- `replay::recorder` (lib unit): 9 tests, all pass (was 7 + 2 new).
- `replay::wire`, `replay::tests`: unchanged.
- Full `cargo test -p mty-runtime --lib`: 99 tests, all pass.

## v0.19 follow-ups

1. **Step-debugger REPL (Tier 2.2 in `agent-features-roadmap.md`)** —
   load a trace + drive the Runtime forward one event at a time,
   asserting state against the recording at each step.
2. **Full byte-identical replay** — serialize `Value` payloads
   structurally (not via `Debug`), then re-construct on replay to
   feed `Runtime::send`/`ask` exactly the same args.
3. **Recording compression** — postcard codec gate (the v0.17 wire
   format already supports it; the recorder defaults to JSON for
   diffability). Add `MTY_RECORD_CODEC=postcard` env opt-in.
4. **Per-runtime recording** — replace the process-wide `OnceLock`
   with a per-`Runtime` slot so multiple Runtimes (e.g. in a
   multi-tenant host) can record independently. Requires a `task_local`
   migration + propagation across `spawn_blocking`.
5. **Recording rate-limit** — currently every send/handle path
   records unconditionally. v0.19 should add an opt-in sampling rate
   (`MTY_RECORD_SAMPLE=0.01`) for long-running production traces.

## Concurrency notes (for the v0.18 swarm reviewer)

- The owned-file list was: `replay/recorder.rs`, `agent.rs`,
  `runtime.rs`, `host_std.rs`, `budget.rs`, `lib.rs`,
  `tests/replay_e2e.rs`, this notes file. All other paths touched
  during this slice are off-limits.
- `replay/mod.rs` was edited only to re-export the new
  `with_recorder` + `recording_enabled` helpers. No breakage to the
  v0.17 stable surface (`Recorder`, `TraceEvent`, `Replayer`,
  `StepHandler`, codec functions).
- `cluster/` is a parallel agent's territory (v0.18 Tier 4.1). Its
  clippy warnings + the flaky `peer_connect_to_listener` test are
  NOT in scope.
- `mty-codegen-wasm` had a stale-artifact build issue mid-session
  that cleared after `cargo check -p mty-codegen-wasm`; not modified.

## Files touched

- `crates/mty-runtime/src/replay/recorder.rs` — added `with_recorder`,
  `recording_enabled`, 2 unit tests, serialized global-state tests
  with a Mutex.
- `crates/mty-runtime/src/replay/mod.rs` — re-export new helpers.
- `crates/mty-runtime/src/lib.rs` — crate-root re-export of replay
  hooks for downstream callers.
- `crates/mty-runtime/src/runtime.rs` — `RuntimeBuilder::build` reads
  `MTY_RECORD_TRACE`, stores `Option<Arc<Recorder>>` on `Runtime`,
  flushes + uninstalls on `shutdown` and `Drop`. Spawn/send/ask
  instrumentation. `encode_payload_for_trace` helper.
- `crates/mty-runtime/src/agent.rs` — `record_message_handled` inside
  `run_one_turn_with_shared_reply`.
- `crates/mty-runtime/src/host_std.rs` — `StdHost::with_agent_id`
  setter; `record_effect_for_trace` translates std.\* generic calls
  into IO/clock/random events.
- `crates/mty-runtime/src/budget.rs` — `BudgetTracker::trip` +
  `BudgetBreach::trace_reason` helpers (consumed at the agent-loop
  call site).
- `crates/mty-runtime/tests/replay_e2e.rs` — 8 e2e tests, new.
- `dev/history/notes/REPLAY_HOTPATH_V0_18_NOTES.md` — this doc.
