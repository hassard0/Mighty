# Introspect — live agent introspection (internals, v0.16)

**Module:** `mty_runtime::{introspect, control_socket}`
**Roadmap:** Tier 1.1 — `docs/internals/agent-features-roadmap.md`
**CLI reference:** `docs/reference/cli/mty-inspect.md`

This document explains the internal architecture of the v0.16 live
agent-introspection surface. For configuration and user-visible
behaviour see `docs/reference/cli/mty-inspect.md`.

## Module shape

```
crates/mty-runtime/src/
  introspect.rs       — AgentIntrospectState + RuntimeSnapshot + AgentSnapshot
  control_socket.rs   — Unix-domain socket server (POSIX only)
  runtime.rs          — spawns introspect state alongside each agent
  agent.rs            — AgentRegistry::iter snapshot helper
```

The control socket is **off by default**. Two env vars gate it:

| Env var                              | Effect                                                                                   |
|--------------------------------------|------------------------------------------------------------------------------------------|
| `MTY_RUNTIME_CONTROL_SOCK=<path>`    | Boots the Unix-domain control-socket server at `<path>` on `RuntimeBuilder::build`.      |
| `MTY_INSPECT_CAPTURE_BODIES=1`       | Captures message names in each agent's last-N ring so `mty inspect` can show activity.   |

Telemetry that doesn't require user permission (mailbox depth,
handler timing, budget usage) is always available once the socket is
enabled; message-name capture is gated by the second env var because
message bodies can carry sensitive data.

## Wire version

The snapshot payload carries `version: 1` (constant
`SNAPSHOT_WIRE_VERSION` in `introspect.rs`). Policy:

* **Adding fields** keeps the version at `1`. Clients deserialize
  with `#[serde(deny_unknown_fields)]` *disabled* (the default) so
  newer servers stay compatible with older CLIs.
* **Renaming or removing fields** bumps the version. The CLI rejects
  any payload with `version < 1` today; a future bump (`v2`) gates
  similarly.

## Control-socket protocol

Newline-delimited JSON over a Unix-domain socket. Each request is
one JSON object per line; each response is one JSON object per line.
The server keeps the connection alive so `mty inspect --watch` can
re-use it without reconnecting on every poll.

| Request                                | Response                                          |
|----------------------------------------|---------------------------------------------------|
| `{"op": "snapshot"}`                   | `RuntimeSnapshot` JSON                            |
| `{"op": "snapshot_agent", "id": <u64>}`| `AgentSnapshot` JSON or `{"error":"not_found"}`   |
| `{"op": "list"}`                       | `{"agents":[{"agent_id":..,"agent_type":..},...]}`|
| unknown / bad JSON                     | `{"error":"unknown_op"}` or `{"error":"bad_json"}`|

Bad JSON does NOT close the connection — the server returns the
error line and keeps reading.

## Snapshot shape

```rust
struct AgentSnapshot {
    version: u32,                  // 1
    agent_id: u64,                 // runtime-internal pid
    agent_type: String,            // e.g. "search::Worker"
    supervisor_parent: Option<u64>,
    mailbox_depth: usize,          // live, read from the channel
    mailbox_high_water: usize,     // CAS-tracked maximum since spawn
    in_flight_handler: Option<String>,
    in_flight_elapsed_ms: Option<u64>,
    budget: BudgetSnapshot,
    last_messages: Vec<String>,    // empty unless MTY_INSPECT_CAPTURE_BODIES=1
}

struct BudgetSnapshot {
    mem_used_bytes: u64,
    mem_limit_bytes: Option<u64>,
    ticks_used: u64,
    ticks_limit: Option<u64>,
    deadline_ms: Option<u64>,      // ms remaining in wall budget
}

struct RuntimeSnapshot {
    version: u32,
    agents: Vec<AgentSnapshot>,
    worker_count: usize,
    timestamp_ms: u64,             // unix ms
}
```

## Implementation notes

* **Mailbox depth** is read from `Mailbox::introspect().channel_used`
  (the bounded tokio channel's `capacity - tx.capacity()`). No new
  per-message accounting in the hot path — snapshot reads pay the
  cost on demand. The high-water field uses a CAS loop on a u64
  atomic that the snapshot itself bumps; we don't track high-water
  on every enqueue.
* **In-flight handler** + elapsed are stored under a
  `parking_lot::Mutex<Option<InFlight>>` set by the agent loop
  before each `run_one_turn_async` and cleared after. The mutex is
  uncontended in the steady state (only the snapshot reader competes
  with the agent's own loop).
* **Last-N ring** is `VecDeque<String>` under the same mutex, capped
  at 8 entries. Bodies are stored only when
  `MTY_INSPECT_CAPTURE_BODIES=1` is set; the value is read once per
  agent loop and cached.
* **Snapshot creation never blocks an agent**: registry iteration
  takes a `DashMap` snapshot to `Vec<Arc<_>>` and releases the lock
  before computing the per-agent payload.
* **Internal types are not exposed**: `AgentSnapshot` is a plain
  serializable struct; the runtime computes it from
  `AgentDescriptor` + `AgentIntrospectState` rather than handing out
  references to either.

## Windows status

Windows uses named pipes instead of Unix-domain sockets, and tokio's
named-pipe API has different ergonomics. v0.16 ships **Unix-only**;
`spawn_control_socket_at` returns `None` and logs a warning on
Windows, and `mty inspect` returns a stub error. The `cfg(unix)`
control-socket integration test is skipped on Windows.

Windows named-pipe backend is tracked as a v0.19 (or later) follow-up;
the wire shape is identical, so the CLI doesn't change.

## Test coverage

* 7 integration tests in `crates/mty-runtime/tests/introspect.rs`:
  * `snapshot_includes_live_agent_with_correct_type`
  * `snapshot_disabled_without_env`
  * `agent_id_lookup_works`
  * `list_op_enumerates_live_agents`
  * `introspect_state_high_water_tracks_max`
  * `snapshot_serializes_to_json`
  * `control_socket_responds_to_snapshot_op` (`cfg(unix)`)
* 4 unit tests each in `introspect.rs` and `control_socket.rs`.

## Cross-references with replay + telemetry

`mty inspect` and `mty replay` are siblings in the v0.16/v0.17/v0.18
observability tier:

* **Inspect** answers "what's running right now?" — live snapshot
  of mailbox depth, in-flight handler, budget headroom.
* **Replay** answers "what ran when X happened?" — recorded trace
  of every send / handle / IO event.
* **OpenTelemetry spans** answer "what does the trace look like in
  Jaeger / Tempo?" — span tree across agent boundaries.

All three respect the same off-by-default contract: zero hot-path
overhead when their env var is absent.

## v0.19+ follow-ups

* **`mty top`** — polling-mode subcommand with a worker-by-worker
  schedule view. `--watch` already polls but doesn't render the
  scheduler's worker stats.
* **OpenTelemetry trace-id pivot** — carry trace IDs in the snapshot
  so operators can pivot from a snapshot row to the matching trace.
* **Windows named-pipe backend** — see Windows status above.
* **Per-handler latency histograms** — a small reservoir sampler per
  (agent, handler) for p50/p99 without growing the wire shape
  significantly.

## See also

* `docs/reference/cli/mty-inspect.md` — user-facing CLI reference
* `docs/internals/agent-features-roadmap.md` — Tier 1 plan
* `docs/internals/telemetry-spans.md` — OTel agent spans (Tier 1.2+1.3)
* `docs/internals/replay.md` — deterministic replay (Tier 1.4)
* `docs/internals/agents.md` — cross-cutting agent overview
* `dev/history/notes/INTROSPECT_V0_16_NOTES.md` — v0.16 ship notes
