# Deterministic replay — v0.17 shipping notes (Tier 1.4)

This note records the design decisions and follow-up work for the
v0.17 deterministic-replay slice.

Status: **SHIPPED-SUBSET** — recorder + wire format + replayer
(dump-json + step-counting) + `mty replay` CLI + docs + 35 tests.
Full runtime re-execution (where the replayer drives a `Runtime` and
asserts byte-identical handler outputs) is the v0.18 stretch.

## What shipped in v0.17

| Component                                          | Status   |
|----------------------------------------------------|----------|
| `mty-runtime::replay::wire::{TraceFile,TraceEvent,TraceSummary}` | Shipped  |
| `mty-runtime::replay::recorder::{Recorder, install_from_env}`    | Shipped  |
| `mty-runtime::replay::{Replayer, StepHandler, CountingStepHandler}` | Shipped  |
| `mty replay <trace>` CLI (default summary + `--dump-json` + `--step` + `--json`) | Shipped  |
| `docs/reference/cli/mty-replay.md`                 | Shipped  |
| Integration tests (`crates/mty-runtime/tests/replay.rs`) | 10 cases |
| Unit tests across `replay::{wire, recorder, mod}`  | 20 cases |
| CLI unit tests (`cmd::replay`)                     | 5 cases  |

Total: **35 tests** for the v0.17 slice.

## Wire-shape decisions

- **Magic prefix**: every trace file begins with the 8-byte ASCII
  string `MTYTRACE`. Lets `mty replay` reject random binaries (saves a
  100-line serde traceback) and reserves room for a future codec byte.
- **Versioning**: `TRACE_WIRE_VERSION = 1`. Bumped only for breaking
  changes; additive variants/fields use serde defaults so old readers
  still decode new writers. The replayer refuses traces with
  `version > TRACE_WIRE_VERSION` to enforce one-way stability.
- **Variant set**: the eight `TraceEvent` variants cover every capture
  surface called out in the agent-features roadmap
  (`Spawn`/`MessageSent`/`MessageHandled`/`IoRead`/`ClockRead`/
  `RandomRead`/`BudgetExhausted`/`Exit`). `Exit` is new vs the
  roadmap sketch — added because the replayer needs a clean
  end-of-life marker to verify per-agent lifecycle completeness.
- **Codec**: JSON (after the magic prefix) — chosen for v0.17 because
  postcard's workspace-dep dance (it's not yet a direct dep) would
  delay the slice. The `TraceCodec` enum is the extension point;
  postcard slots in cleanly behind a `replay-postcard` feature when
  v0.18 wires it up (see "v0.18 plan" below).

## Recorder integration model

The recorder is a thread-safe append-only buffer keyed off a
process-wide `RwLock<Option<Arc<Recorder>>>`. The activation pattern
mirrors `MTY_INSPECT_CAPTURE_BODIES` from v0.16 introspection: opt-in
via env var, zero overhead when absent.

The v0.17 slice keeps the recorder calls *explicit at the capture
sites*: callers (Runtime code or future agent hooks) call
`recorder.record_spawn(...)`, `record_message_sent(...)`, etc., at
the points where the runtime already emits telemetry. This sidesteps
the hot-path concurrency hazard of wiring a global recorder into
`Mailbox::send` / handler dispatch / IO surfaces in one slice.

**Why not wire into the runtime in v0.17?** Three reasons:

1. The replay-targeted hooks overlap with the v0.16
   `TelemetrySink::Buffer` path. Picking the right slot needs a
   round of API-shape review with whoever owns
   `crates/mty-runtime/src/{telemetry, runtime}*` (off-limits for the
   v0.17 replay swarm).
2. Some captures (the `IoRead` body, the `ClockRead` value) live
   inside `host_std::*` which is also off-limits — the same
   coordination would be needed to plumb them.
3. The recorder is fully testable and useful without the wire-up
   (see "Tests", below) — `mty replay` validates traces, dumps JSON,
   and step-replays. The v0.18 slice can then drop the recorder
   calls into the agent/IO sites without re-deriving the shape.

## Replay modes

- **DumpJson (the always-works fallback)** — emits one JSON object
  per event. Roundtrip-safe: `dump_json` → `serde_json::from_str`
  yields the original `TraceEvent` exactly.
- **Step (counting handler)** — drives a `StepHandler` over the
  trace. `CountingStepHandler` is the v0.17 default; user code can
  implement `StepHandler` to build a custom replay (the v0.18
  debugger REPL plugs in here).
- **Self-consistency check** — `Replayer::verify_self_consistent`
  walks the trace and rejects: (a) per-agent `msg_idx` sequences that
  aren't monotonic-from-zero, (b) messages/IO targeting unspawned
  agents. This is the v0.17 "byte-identical contract" until full
  re-execution lands.

## Tests

- **`crates/mty-runtime/tests/replay.rs`** (10 integration cases)
  covers recorder capture, on-disk round-trip, dump-json equivalence,
  step-handler counts, self-consistency, end-to-end `Replayer::from_path`.
- **`crates/mty-runtime/src/replay/{wire, recorder, mod}.rs`** unit
  tests (20 cases) cover wire-version invariants, magic header,
  per-agent counters, install/uninstall, decode rejection paths.
- **`crates/mty-cli/src/cmd/replay.rs`** (5 cases) covers summary
  rendering, invalid-path failure, step-summary formatting, ordered
  event visitation.

All 35 tests pass under `cargo test -p mty-runtime --test replay`,
`cargo test -p mty-runtime --lib replay`, and
`cargo test -p mty-cli --bins replay`. Clippy + fmt clean.

## v0.18 plan

1. **Runtime wire-up**: drop `recorder.record_*` calls into:
   - `Runtime::spawn_agent` → `record_spawn`
   - `Runtime::{send, ask}` → `record_message_sent`
   - `agent::run_one_turn_async` → `record_message_handled` (around the
     turn body, with `elapsed_us` from the existing wall-budget timer)
   - `host_std::{file, net, time, random}` → `record_io_read` /
     `record_clock_read` / `record_random_read`
   - `BudgetTracker::trip` → `record_budget_exhausted`
   - Agent loop drop → `record_exit`

   All gated on `recorder::global_recorder()` being `Some` so the
   non-recording path stays at v0.16 latency.

2. **Postcard codec**: add `postcard` as a workspace dep (it's
   already in `Cargo.lock` as a transitive), gate behind a
   `replay-postcard` feature, switch `TraceCodec::default` to
   postcard for `MTY_RECORD_TRACE` writers. JSON stays as the
   `--dump-json`-mode emitter so traces remain human-readable.

3. **Step-debugger REPL**: implement Tier 2.2's `mty debug
   <trace.bin>` REPL on top of `Replayer::step`. Commands:
   - `step` — advance one event
   - `peek <agent>` — show last known state for an agent
   - `print msg` — pretty-print the in-flight message
   - `break <handler>` — pause when a handler dispatch is about to fire

4. **Byte-identical full re-execution**: instead of the v0.17
   self-consistency check, the v0.18 replayer drives a fresh
   `Runtime` from the seed, replays every recorded message via a
   mock host, and asserts each handler returns byte-identical output.
   This closes the "Re-run with `mty replay trace.bin` produces
   byte-identical output" goal from the roadmap.

## Files

- `crates/mty-runtime/src/replay/mod.rs` — Replayer + StepHandler trait
- `crates/mty-runtime/src/replay/recorder.rs` — recording surface
- `crates/mty-runtime/src/replay/wire.rs` — TraceFile / TraceEvent / TraceSummary
- `crates/mty-runtime/tests/replay.rs` — integration tests
- `crates/mty-cli/src/cmd/replay.rs` — CLI command
- `crates/mty-cli/src/main.rs` — `Cmd::Replay` variant
- `crates/mty-runtime/src/lib.rs` — `pub mod replay;`
- `crates/mty-cli/src/cmd/mod.rs` — `pub mod replay;`
- `docs/reference/cli/mty-replay.md` — user-facing reference

## Wire-shape decision log

| Decision                            | Choice    | Why                                                   |
|-------------------------------------|-----------|-------------------------------------------------------|
| Format                              | JSON      | Postcard is transitive-only today; gate behind feature for v0.18 |
| Magic prefix                        | `MTYTRACE` (8B) | Rejects garbage early, leaves codec-byte room    |
| Wire version                        | `1`       | Standard "additive only" rule mirrors v0.16 introspect |
| Variant set                         | 8 events  | Roadmap's 7 + `Exit` for lifecycle completeness       |
| Per-agent `msg_idx` in MessageHandled | u64     | Deterministic ordering check at replay time           |
| Sender of synthetic messages        | `from=0`  | Mirrors existing `TelemetryEvent::Send` "(extern)" sender |
