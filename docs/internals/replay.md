# Replay — deterministic agent re-execution (internals, v0.17–v0.18)

**Module:** `mty_runtime::replay` (submodules `wire`, `recorder`, `mod`)
**Roadmap:** Tier 1.4 — `docs/internals/agent-features-roadmap.md`
**CLI reference:** `docs/reference/cli/mty-replay.md`

This document explains the internal architecture of the deterministic-
replay subsystem. v0.17 landed the recorder + wire format + step
replayer + CLI; **v0.18 wired the recorder into the Runtime hot path**
so traces capture real agent execution from spawn through exit.

## Module shape

```
crates/mty-runtime/src/replay/
  ├── mod.rs       — Replayer + StepHandler trait + CountingStepHandler
  │                  + with_recorder / recording_enabled re-exports
  ├── wire.rs      — TraceFile, TraceEvent, TraceSummary, codec, magic
  │                  prefix `MTYTRACE`, wire-format version 1
  └── recorder.rs  — global Recorder slot + install_from_env +
                     record_* helpers + flush_to_disk
```

The recorder ships as a **process-wide** `RwLock<Option<Arc<Recorder>>>`.
With no recorder installed the runtime hot path pays one atomic load +
one branch per instrumentation site (~3 ns on modern x86). Opt-in is
controlled by `MTY_RECORD_TRACE=/path/to/trace.bin` at runtime start.

## Wire format

Every trace file begins with the 8-byte ASCII magic `MTYTRACE`,
followed by JSON-encoded events on the v0.18 codec path. The
`TRACE_WIRE_VERSION` constant is `1`; the replayer refuses files with
`version > TRACE_WIRE_VERSION` and decodes additive variant additions
as `_unknown` for forward-compatible reading.

### Variants (`TraceEvent`)

| Variant            | Carries                                              | Emitted by                                  |
|--------------------|------------------------------------------------------|---------------------------------------------|
| `Spawn`            | `agent_id`, `agent_type`, `supervisor_parent`        | `Runtime::spawn_agent_with_affinity`        |
| `MessageSent`      | `from`, `to`, `msg_name`, `payload_bytes`            | `Runtime::send` / `Runtime::ask`            |
| `MessageHandled`   | `agent_id`, `msg_name`, `msg_idx`, `elapsed_us`      | `run_one_turn_with_shared_reply`            |
| `IoRead`           | `agent_id`, `kind` (`fs:<path>` / `net:<url>`), bytes | `host_std.rs::effect_call` for fs/http      |
| `ClockRead`        | `agent_id`, `value_ms`                               | `host_std.rs::effect_call` for std.time.*   |
| `RandomRead`       | `agent_id`, `bytes`                                  | `host_std.rs::effect_call` for std.random.* |
| `BudgetExhausted`  | `agent_id`, `reason` (`cpu` / `mem` / `MT5009`)      | `BudgetTracker::trip` + agent loop          |
| `Exit`             | `agent_id`, `reason`                                 | agent-loop terminal arm + `record_exit`     |

The magic + version + variant policy mirrors `docs/internals/cluster.md`'s
wire-format rules: additive evolution stays at version 1; renaming or
removing fields bumps the version.

## Recorder integration model

The recorder is installed once via `install_from_env` at
`RuntimeBuilder::build`:

```text
RuntimeBuilder::build()
 └── if let Ok(path) = env::var("MTY_RECORD_TRACE")
       if !path.is_empty():
         recorder = Recorder::new(path)
         replay::install(Arc::new(recorder))
```

Empty-string env (`MTY_RECORD_TRACE=""`) is treated as unset to
match the v0.16 `MTY_RUNTIME_CONTROL_SOCK` convention.

### Instrumentation sites wired in v0.18

| Site                                           | Event                                  |
|------------------------------------------------|----------------------------------------|
| `RuntimeBuilder::build`                        | `install_from_env`                     |
| `Runtime::spawn_agent_with_affinity`           | `record_spawn(agent_id, name, parent)` |
| `Runtime::send`                                | `record_message_sent(0, to, msg, ...)` |
| `Runtime::ask`                                 | `record_message_sent(0, to, msg, ...)` |
| `agent.rs::run_one_turn_with_shared_reply`     | `record_message_handled(...)`          |
| agent loop budget-exhaust arm                  | `record_budget_exhausted(agent, code)` |
| agent loop cancellation arm                    | `record_budget_exhausted(agent, ...)`  |
| agent loop terminal exit                       | `record_exit(agent, reason)`           |
| `StdHost` → `std.time.{now,sleep}`             | `record_clock_read(agent, value_ms)`   |
| `StdHost` → `std.random.*`                     | `record_random_read(agent, bytes)`     |
| `StdHost` → `std.fs.{read,exists,list_dir}`    | `record_io_read(agent, "fs:<path>")`   |
| `StdHost` → `std.http.{get,post}`              | `record_io_read(agent, "net:<url>")`   |
| `Runtime::shutdown` + `Runtime::drop`          | `flush_to_disk()` + `uninstall()`      |

All recorder calls funnel through `with_recorder(|rec| ...)` which is
the cheap fast-path helper:

```rust
pub fn with_recorder<F: FnOnce(&Recorder)>(f: F) {
    if let Some(rec) = global_recorder() {
        f(&rec);
    }
}
```

Sites that need to skip expensive payload encoding when recording is
off use `recording_enabled() -> bool` before the encode.

## Why `MessageHandled` records inside the inner turn loop

There are two natural recording points: after `run_one_turn_async`
returns, or inside `run_one_turn_with_shared_reply` before the reply
oneshot fires. v0.18 chose the **inner** one. An `ask()` caller often
shuts down the Runtime immediately after observing the reply; the
outer placement can race shutdown and lose the final handled event.
The inner placement guarantees the trace contains the event before
any reply becomes observable.

## Replay modes

* **DumpJson (always-works fallback).** `mty replay <trace> --dump-json`
  emits one JSON object per event. Round-trip safe.
* **Step (counting handler).** `mty replay <trace> --step` drives a
  `StepHandler` over the trace. `CountingStepHandler` is the default;
  user code can implement `StepHandler` to build a custom replay
  (the v0.19 debugger REPL plugs in here).
* **Summary (default).** Prints a `TraceSummary` — event counts +
  total bytes + wall time spanned.
* **Self-consistency check.** `Replayer::verify_self_consistent`
  rejects per-agent `msg_idx` sequences that aren't monotonic-from-zero
  and messages/IO targeting unspawned agents.

## Payload encoding

`Value` does not implement `Serialize` (it carries Host references).
v0.18 renders payloads via `format!("{:?}", args)` to bytes — opaque
but human-readable, enough for trace inspection. The fast path
short-circuits when no recorder is installed (one `RwLock::read`), so
the encode cost is only paid during recording.

### Byte-identical re-execution (v0.19, wire version 2)

v0.19 promotes the payload field on `TraceEvent::MessageSent` from a
flat `Vec<u8>` to a structured `ReplayPayload` enum:

```rust
pub enum ReplayPayload {
    Opaque(Vec<u8>),        // v0.18 hot path — Debug-formatted bytes
    Values(Vec<ReplayValue>) // v0.19 structural — byte-identical
}
```

`ReplayValue` is a structural mirror of the SIR interpreter's `Value`
enum (`Unit`/`Bool`/`Int`/`Float`/`Str`/`Char`/`Duration`/`Size`/
`Tuple`/`Array`/`Record`/`Variant`/`Opaque`). Variants holding live
host references (`Ref`/`Fn`/`Agent`/`Cap`) fold into
`ReplayValue::Opaque(String)` carrying the Debug rendering — they
can't round-trip across processes, but their *shape* survives.

The `from_runtime_value` / `to_runtime_value` codec lives at
`crates/mty-runtime/src/replay/mod.rs`; pure-data variants are
lossless. `mty_types::IntKind` / `FloatKind` are serialised by name
(`"I64"`, `"F64"`, …) so the wire stays independent of any single
type-system version.

#### Wire version 2 — backwards-compat read

`TRACE_WIRE_VERSION = 2`. The `decode()` helper peeks at the
on-disk `version` field via a tiny `VersionProbe` deserializer:

* **v1** — the legacy `MessageSent.payload: Vec<u8>` shape. The
  decoder deserializes into `V1TraceFile` and lifts each payload
  into `ReplayPayload::Opaque(bytes)`. The in-memory `TraceFile`
  preserves the source-disk `version = 1` so tools can branch on
  "this was a legacy trace".
* **v2** — direct decode. New traces always write version 2.
* **`version > 2`** — `RecorderError::UnsupportedVersion`. The
  one-way-stable contract means upgrades land on the latest
  consumer first.

#### `ReplayDriver` — full re-execution

`crates/mty-runtime/src/replay/replay_driver.rs` ships the v0.19 full
re-execution surface:

```text
record-time trace
       │
       ▼
  ReplayDriver::from_trace(trace)
   .with_program(prog)            ◀── caller supplies the SIR program
   .mock_io(true)                 ◀── default; IO is replayed from trace
   .replay_all()                  ◀── spins fresh Runtime, drives events
       │
       ▼
  ReplayReport { events_replayed, mismatches, success }
```

The driver:

1. Builds a fresh `Runtime` seeded from `trace.runtime_seed`,
   `workers(1)` (mandatory for determinism).
2. Installs a local `Recorder` so re-emitted events accumulate into a
   parallel stream.
3. For each `Spawn` in the recorded trace, calls
   `Runtime::spawn_agent(agent_type, vec![])` and remembers the
   recorded-id → live-handle mapping.
4. For each `MessageSent` from the synthetic `extern` (id=0),
   reconstructs the args from the payload and calls `Runtime::ask`.
5. After `Runtime::shutdown`, compares the recorded vs replayed
   stream event-by-event via `compare_streams`. `MessageHandled`'s
   `elapsed_us` is intentionally **not** compared (it's a wall-clock
   measurement). `Exit` / `BudgetExhausted` are **soft** events —
   their absence in the replay is not a divergence (the recorded
   trailer is timing-dependent on shutdown).

The byte-identical contract:

* `Values == Values` — strict, every nested `ReplayValue` matches.
* `Opaque == Opaque` — accepted as **approximate** equality. The
  Debug rendering is non-injective, so v0.18 hot-path traces can't
  be strictly byte-identical without upgrading the recording side
  too. Approximate equality keeps v0.18 traces flowing through the
  driver while v0.19+ structural recordings unlock strict equality.
* `Opaque vs Values` — accept if the structural side, re-rendered
  through `format!("{:?}", reconstructed_runtime_values)`, byte-
  matches the opaque side.

```text
            record                                replay
              │                                     │
              ▼                                     ▼
       MTYTRACE | JSON                       fresh Runtime
       (Spawn, MessageSent, …) ────────▶    spawns + asks + IO
                                                     │
                                                     ▼
                                            in-memory Recorder
                                                     │
                                                     ▼
                                       compare_streams (byte-diff)
                                                     │
                                                     ▼
                                              ReplayReport
```

## Budget-exhaust vs Exit duplication

Every agent that dies (trap, budget, cancellation) emits BOTH a
`BudgetExhausted` AND an `Exit` event. The reason strings differ:

* `BudgetExhausted.reason` is the breach kind (`"cpu"` / `"mem"` /
  `"MT5009"`).
* `Exit.reason` is the agent-loop's terminal reason
  (`"trap:MT5009"` / `"shutdown"` / `"normal"`).

The replayer's self-consistency check treats both as expected for a
dying agent.

## Why `OnceLock` over `task_local`

`task_local!` per-agent recorders were rejected because:

1. The Mighty runtime owns multiple tokio runtimes (one per worker
   thread) — task-locals would need a separate slot per worker.
2. The blocking handler shim in `agent.rs::run_one_turn_async` runs
   on `spawn_blocking`, which cannot see the spawning task's locals
   without manual propagation.
3. The runtime supports a single active recording at a time —
   `MTY_RECORD_TRACE` is process-wide. Per-runtime recording is a
   v0.19 stretch.

## Public API surface

| Item                                          | Path                                      |
|-----------------------------------------------|-------------------------------------------|
| `Recorder`, `install`, `uninstall`            | `replay::recorder::*`                     |
| `with_recorder`, `recording_enabled`          | `replay::*` (re-exported at crate root)   |
| `global_recorder` (returns `Option<Arc<...>>`)| `replay::recorder::*`                     |
| `TraceFile`, `TraceEvent`, `TraceSummary`     | `replay::wire::*`                         |
| `Replayer`, `StepHandler`, `CountingStepHandler` | `replay::*`                            |
| `ReplayDriver`, `ReplayReport`, `EventMismatch` | `replay::replay_driver::*` (v0.19)      |
| `ReplayPayload`, `ReplayValue`, `RuntimeValueLike` | `replay::wire::*` (v0.19)            |
| `from_runtime_value`, `to_runtime_value`      | `replay::*` (v0.19 codec)                 |
| `MTYTRACE` magic + `TRACE_WIRE_VERSION` (2)   | `replay::wire::*`                         |

Re-exported at crate root for downstream callers:
`mty_runtime::{with_recorder, recording_enabled, global_recorder}`.

## Test coverage

* `crates/mty-runtime/src/replay/{wire, recorder, mod}.rs` unit tests
  (20+ cases): wire-version invariants, magic header rejection,
  per-agent counters, install/uninstall, decode rejection paths,
  serialised global-state coordination via `parking_lot::Mutex`.
* `crates/mty-runtime/tests/replay.rs` (10 integration tests, v0.17
  baseline): recorder round-trip, dump-json equivalence, step-handler
  counts, self-consistency, end-to-end `Replayer::from_path`.
* `crates/mty-runtime/tests/replay_e2e.rs` (8 v0.18 end-to-end tests
  driven by a real `Runtime`): recording round-trip, env-unset noop,
  distinct-agent-id capture, trap recovery, fire-and-forget capture,
  monotonic `msg_idx`, empty-path-env-treated-as-unset,
  recorder-uninstalled-after-shutdown.
* `crates/mty-runtime/tests/replay_byte_identical.rs` (9 v0.19 end-
  to-end byte-identical tests): record-then-replay-2-agents,
  diverged-handler detection, IO-uses-recorded-bytes, v1-backwards-
  compat-read, clock-returns-recorded-time, structural-payload-
  round-trip, driver-requires-program, empty-trace, opaque-survives-
  disk.
* `crates/mty-cli/src/cmd/replay.rs` (6 CLI unit tests): summary
  rendering, invalid-path failure, step-summary formatting, ordered
  event visitation, byte-identical-without-program error path.

## v0.20 / v1.0 follow-ups

1. **Step-debugger REPL** — `mty debug <trace.bin>` REPL on top of
   `ReplayDriver::replay_all`: `step` / `peek <agent>` / `print msg`
   / `break <handler>`. Stop point per-event with structured payload
   inspection.
2. **Strict byte-identical for Opaque traces** — upgrade the v0.18
   hot path to emit structural `ReplayPayload::Values` (with the same
   "skip when no recorder" fast path). Lets pre-v0.19 traces re-
   record losslessly so the `Opaque == Opaque` approximate-equality
   arm disappears.
3. **Postcard codec** — add a `replay-postcard` cargo feature; switch
   `TraceCodec::default` to postcard for `MTY_RECORD_TRACE` writers.
   JSON stays as the `--dump-json` emitter for human-readable
   inspection.
4. **Per-runtime recording** — replace the process-wide `OnceLock`
   with a per-`Runtime` slot so multiple Runtimes (e.g. in a
   multi-tenant host) can record independently.
5. **Recording compression** — postcard + zstd framing for long-
   running production traces. The codec layer already abstracts the
   payload, so this is additive.
6. **Sampling** — `MTY_RECORD_SAMPLE=0.01` env opt-in for long-running
   production traces.

## See also

* `docs/reference/cli/mty-replay.md` — user-facing CLI reference
* `docs/internals/agent-features-roadmap.md` — Tier 1 plan
* `docs/internals/cluster.md` — sibling wire-format design
* `dev/history/notes/REPLAY_V0_17_NOTES.md` — v0.17 ship notes
* `dev/history/notes/REPLAY_HOTPATH_V0_18_NOTES.md` — v0.18 wire-up
* `dev/history/notes/REPLAY_BYTE_IDENTICAL_V0_19_NOTES.md` — v0.19 driver + wire v2
