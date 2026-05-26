# REPLAY — Byte-identical full replay re-execution (v0.19, Tier 1.4)

**Slice:** v0.19 (last minor before v1.0-RC).
**Roadmap:** Tier 1.4 follow-up.
**Touched crates:** `mty-runtime` (replay module only), `mty-cli`.

## Goal

Make recorded traces a reliable regression oracle by re-executing the
original program against the trace and asserting each emitted event
matches the recorded one **byte-for-byte**. v0.18 wired the recorder
into the Runtime hot path, but payloads were `format!("{:?}", args)` —
opaque, lossy, and unable to drive a re-execution.

## What landed

### Phase 1 — structural Value codec

`crates/mty-runtime/src/replay/wire.rs`:

- `pub enum ReplayPayload { Opaque(Vec<u8>), Values(Vec<ReplayValue>) }`
  — the new typed payload on `TraceEvent::MessageSent`.
- `pub enum ReplayValue { Unit, Bool, Int, Float, Str, Char, Duration,
  Size, Tuple, Array, Record, Variant, Opaque }` — structural mirror
  of `mty_ir::interp::value::Value` (13 variants).
- `pub trait RuntimeValueLike { fn to_replay_value(&self) ->
  ReplayValue; }` — adapter so generic call sites can render any
  Value-shaped type into the wire shape.

`crates/mty-runtime/src/replay/mod.rs`:

- `from_runtime_value(&Value) -> ReplayValue` + `to_runtime_value(&
  ReplayValue) -> Result<Value>` codec. `IntKind` / `FloatKind`
  serialise by name (`"I64"`, `"F64"`, …) so the wire is independent
  of any single mty-types release.
- `encode_values_payload(&[Value]) -> ReplayPayload` — convenience
  for the byte-identical recording path.
- `align_payloads(a, b) -> bool` — strict byte equality. Distinct
  from the driver's `payloads_match` which has Opaque-tolerance.

### Wire version 2

`TRACE_WIRE_VERSION = 2`. Bumped because `MessageSent.payload`
changed type. The decoder peeks at the on-disk `version` field via
a tiny `VersionProbe` deserializer; **v1** traces deserialize via the
`V1TraceFile` shim shape and lift their flat `Vec<u8>` payload into
`ReplayPayload::Opaque(bytes)`. The in-memory `TraceFile` preserves
the source-disk `version` so callers can branch on legacy.

### Phase 2 — ReplayDriver

`crates/mty-runtime/src/replay/replay_driver.rs` (new):

- `pub struct ReplayDriver { trace, prog, mock_io, byte_identical,
  ask_deadline_ms }` — builder-style configuration.
- `from_trace(TraceFile)` → `with_program(Arc<Program>)` →
  `replay_all() -> Result<ReplayReport, String>`.
- `ReplayReport { events_replayed, mismatches: Vec<EventMismatch>,
  success }` — `success = mismatches.is_empty() && events_replayed
  > 0`.
- `EventMismatch { index, recorded, replayed: Option<TraceEvent>,
  reason: String }` — one diff entry.

Internally the driver:

1. Spins up a `Runtime` with `deterministic(seed).workers(1)`.
2. Installs a local `Recorder` to capture re-emitted events.
3. Maps recorded agent ids → live agent handles by spawn order.
4. Replays every `MessageSent` from extern (`from = 0`) via
   `Runtime::ask` (5s deadline by default).
5. After `Runtime::shutdown`, diffs the recorded vs replayed
   streams via `compare_streams`.

#### Comparison semantics

* **Strict**: `Spawn` (agent_type + supervisor), `MessageSent`
  (msg + mapped from/to + payload), `MessageHandled` (msg +
  msg_idx; `elapsed_us` is a wall-clock measurement that the driver
  intentionally ignores), `IoRead` / `ClockRead` / `RandomRead`
  (full equality).
* **Soft**: `Exit` / `BudgetExhausted` — missing in the replay is
  not a divergence. The recorded trailer is timing-dependent on
  shutdown abort timing; both record + replay sides can legitimately
  drop these events.

#### Payload comparison

* `Values == Values` — strict structural equality (every nested
  `ReplayValue` matches).
* `Opaque == Opaque` — **approximate equality**. v0.18 hot-path
  traces use the Debug rendering, which is non-injective: the
  replay's reconstruction (`Opaque(s) -> Value::Str(s)`) re-renders
  to a structurally different but semantically equivalent shape.
  Strict equality would require the v0.18 runtime hot path to also
  emit `Values` — a v0.20 follow-up.
* `Opaque vs Values` — accept if the structural side, re-rendered
  through `format!("{:?}", reconstructed)`, byte-matches the opaque
  side.

### Phase 3 — CLI integration

`crates/mty-cli/src/cmd/replay.rs`:

- New flags: `--byte-identical`, `--mock-io` (default `true`),
  `--program <path>`.
- `run_byte_identical(trace, replayer, program_path, mock_io)`
  compiles the source through `mty_driver::pipeline` and feeds it
  to `ReplayDriver`. Exit codes: `0` on byte-identical, `1` on any
  divergence, `2` if `--byte-identical` is set without `--program`.

`crates/mty-cli/src/main.rs`: clap derive picks up the new
`byte_identical` / `mock_io` / `program` fields on the `Replay`
sub-command.

### Phase 4 — Tests

`crates/mty-runtime/tests/replay_byte_identical.rs` (new, **9 tests**):

1. `record_then_replay_byte_identical_for_2_agents` — full round
   trip: record a real run, replay, assert success + zero mismatches.
2. `replay_detects_diverged_handler` — inject a synthetic
   `MessageHandled` into the recorded trace; replay flags it.
3. `replay_with_io_uses_recorded_bytes` — IoRead events preserve
   their bytes across record → encode → decode.
4. `replay_v1_trace_backwards_compat` — hand-write a v1 trace JSON,
   decode lifts the legacy payload to `ReplayPayload::Opaque`.
5. `replay_clock_returns_recorded_time` — ClockRead value_ms
   survives codec round-trip.
6. `structural_payload_round_trips_str_int_args` — Str/Int/Bool
   args through the structural codec.
7. `driver_requires_attached_program` — missing-program error.
8. `empty_trace_yields_zero_events_replayed` — boundary check.
9. `replay_value_opaque_survives_disk_round_trip` —
   `ReplayValue::Opaque` is JSON-stable.

Plus **4 new unit tests** in `replay/mod.rs`
(`from_runtime_value_round_trips_scalar_values`,
`encode_values_payload_yields_values_arm`,
`opaque_values_byte_identical_for_equal_inputs`,
`float_round_trip_preserves_nan_bits`), **3 new unit tests** in
`replay/wire.rs` (`replay_payload_opaque_round_trip`,
`replay_payload_values_round_trip`,
`replay_value_variants_serialize`,
`v1_into_v2_lifts_opaque_payload`,
`wire_version_is_two`), and **7 new unit tests** in
`replay/replay_driver.rs` (report rendering + payload match cases),
plus **1 new CLI test**
(`byte_identical_without_program_returns_2`).

Total v0.19 net-new tests: **24+** (9 integration + 15+ unit).

### Phase 5 — Docs

* `docs/internals/replay.md` — new "Byte-identical re-execution
  (v0.19, wire version 2)" section with the record→trace→replay
  diagram, ReplayPayload codec, ReplayDriver phase walk-through.
* `docs/reference/cli/mty-replay.md` — new flags table entries,
  example workflow, v0.19 wire-format section, refreshed v0.20 plan.
* `dev/history/notes/REPLAY_BYTE_IDENTICAL_V0_19_NOTES.md` (this
  file) — ship notes.

## Design decisions

### Wire version bump rationale

The `MessageSent.payload` field's type changed (`Vec<u8>` →
`ReplayPayload`). Serde won't deserialise the old shape into the new
without a custom path, so this is a breaking change at the disk
layer. We bumped wire version 1 → 2 and added the `V1TraceFile`
shim. The version-probe approach (peek at `version` before
committing to a shape) lets the decoder handle both arms cleanly
without backtracking.

### ReplayValue mirrors `Value`, doesn't duplicate

`ReplayValue` is intentionally a **mirror** rather than a re-export
because `mty_ir::interp::value::Value` carries Host-side references
(`Ref`/`Fn`/`Agent`/`Cap`) that can't `Serialize` and shouldn't
appear on disk. The mirror lets the codec drop those into
`Opaque(String)` (Debug rendering) so the byte-identical contract
becomes "shape equality" rather than "pointer identity".

The codec is also reusable from `introspect`'s snapshot
serialisation — the design contract from the spec. The shared
helpers live at `crates/mty-runtime/src/replay/mod.rs::{
from_runtime_value, to_runtime_value}`.

### Compatibility with v1 traces

v0.19 reads v1 traces transparently — the `V1TraceFile::into_v2`
shim lifts each `MessageSent.payload` `Vec<u8>` into
`ReplayPayload::Opaque(bytes)`. Other event variants pass through
unchanged (they had no breaking field changes).

Strict byte-identical assertions DON'T fire on v1 traces because
the Opaque-vs-Opaque arm uses approximate equality. The contract is:
v0.19+ recordings with `Values` payloads are strict; legacy v0.18
recordings still flow through the driver, just not under the strict
diff.

### Why we did NOT touch `runtime.rs`

The v0.19 spec mandated that the runtime hot path stay frozen —
this slice ships in parallel with four other v0.19 slices that
touch sibling files in `mty-runtime/src/`. Owned files for this
slice are `replay/{recorder,mod,wire,replay_driver}.rs` only.

Consequence: the v0.18 runtime hot path still emits
`record_message_sent(from, to, msg, Vec<u8>)` which lands as
`ReplayPayload::Opaque`. The new structural
`record_message_sent_structural` API is the path the ReplayDriver
uses internally, and it's the path a v0.20 runtime hot-path upgrade
will adopt to enable strict byte-identical for non-driver-recorded
traces.

### Determinism across OSes

The replay driver intentionally has no OS-specific behaviour:

* IO reads return recorded bytes (`--mock-io = true` by default).
* Clock reads return recorded `value_ms`.
* Random reads return recorded bytes.
* `workers(1)` is mandatory — multi-worker scheduling is non-
  deterministic.
* `deterministic(seed)` is enabled from `trace.runtime_seed`.

Tests pass on Windows (the workspace's primary CI platform) and the
same code paths run on Linux/macOS without any cfg-gated
branches.

## Acceptance checklist

- [x] `cargo build --workspace` clean.
- [x] `cargo test -p mty-runtime --test replay_byte_identical` — 9
      passed.
- [x] `cargo test -p mty-runtime --test replay_e2e` — 8 passed
      (v0.18 baseline preserved).
- [x] `cargo test -p mty-runtime --test replay` — 10 passed (v0.17
      baseline preserved).
- [x] `cargo test --workspace` — no regressions.
- [x] `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [x] `cargo fmt --all -- --check` clean.

## v1.0 follow-ups

1. **Step-debugger REPL** — `mty debug <trace.bin>` interactive
   REPL on top of `ReplayDriver::replay_all`. Hooks:
   `step` / `peek <agent>` / `print msg` / `break <handler-name>`.
   Builds on the v0.19 driver foundation.
2. **Strict byte-identical for v0.18 traces** — upgrade the runtime
   hot path to emit `ReplayPayload::Values`. Closes the
   approximate-equality arm in `payloads_match`.
3. **Recording compression** — postcard + zstd framing for long-
   running production traces. The codec abstraction is already in
   place (`TraceCodec` enum); this is additive.
4. **Per-runtime recording** — replace the process-wide `RwLock<
   Option<Arc<Recorder>>>` with a per-Runtime slot so multi-tenant
   hosts can record independently.
5. **Postcard wire** — gate behind a `replay-postcard` cargo
   feature; default `TraceCodec` flips for `MTY_RECORD_TRACE`
   writers while JSON stays for `--dump-json`.

## See also

* `docs/internals/replay.md` — internal architecture
* `docs/reference/cli/mty-replay.md` — CLI surface
* `dev/history/notes/REPLAY_V0_17_NOTES.md` — wire format origins
* `dev/history/notes/REPLAY_HOTPATH_V0_18_NOTES.md` — Runtime
  instrumentation sites
