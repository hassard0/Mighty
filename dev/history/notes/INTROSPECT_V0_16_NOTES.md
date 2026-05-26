# `mty inspect` / runtime introspection (v0.16 Tier 1.1)

Implementation notes for the live agent-introspection surface shipped
in v0.16. Tracks `docs/internals/agent-features-roadmap.md` Tier 1.1.

## What landed

| Artifact                                         | Role                                                                      |
|--------------------------------------------------|---------------------------------------------------------------------------|
| `crates/mty-runtime/src/introspect.rs`           | `AgentSnapshot` + `RuntimeSnapshot` + per-agent ring-buffer state         |
| `crates/mty-runtime/src/control_socket.rs`       | Local control-socket server (Unix-domain on POSIX)                        |
| `crates/mty-runtime/src/runtime.rs`              | Spawns introspect-state alongside each agent + boots control socket       |
| `crates/mty-runtime/src/budget.rs`               | New `cpu_ns_used` / `mem_used` / `elapsed_ms` accessors for snapshot read |
| `crates/mty-runtime/src/agent.rs`                | New `AgentRegistry::iter` snapshot helper                                 |
| `crates/mty-cli/src/cmd/inspect.rs`              | `mty inspect` subcommand (pretty + JSON + watch modes)                    |
| `crates/mty-cli/src/main.rs`                     | Wires the `Inspect` variant into `Cmd`                                    |
| `crates/mty-runtime/tests/introspect.rs`         | 7 integration tests covering snapshot/socket/list/lookup                  |
| `docs/reference/cli/mty-inspect.md`              | User-facing CLI docs                                                      |

## Wire version

The snapshot payload carries `version: 1` (constant
`SNAPSHOT_WIRE_VERSION` in `introspect.rs`). The policy is:

- **Adding fields** keeps the version at `1`. Clients deserialize
  with `#[serde(deny_unknown_fields)]` *disabled* (the default) so
  newer servers stay compatible with older CLIs.
- **Renaming or removing fields** bumps the version. The CLI rejects
  any payload with `version < 1` today; a future v0.18 CLI that needs
  v2 wire will gate similarly.

## Off-by-default policy

The control socket is **opt-in**. The runtime calls
`spawn_control_socket()` during `Runtime::build`, but that function
returns `None` when the env var is unset:

- `MTY_RUNTIME_CONTROL_SOCK=<path>` — enables the socket at `<path>`.
  Unset by default.
- `MTY_INSPECT_CAPTURE_BODIES=1` — *also* off by default. Captures
  message names in each agent's last-N ring so `mty inspect` can
  show recent activity. Off by default because message bodies can
  carry sensitive data.

Telemetry that doesn't require user permission (mailbox depth,
handler timing, budget usage) is always available once the socket
is enabled; message-name capture is gated by the second env var.

## Control-socket protocol

Newline-delimited JSON. Each request is one JSON object per line;
each response is one JSON object per line. The server keeps the
connection alive so `mty inspect --watch` can re-use it without
reconnecting on every poll.

Ops:

| Request                                | Response                                          |
|----------------------------------------|---------------------------------------------------|
| `{"op": "snapshot"}`                   | `RuntimeSnapshot` JSON                            |
| `{"op": "snapshot_agent", "id": <u64>}`| `AgentSnapshot` JSON or `{"error":"not_found"}`   |
| `{"op": "list"}`                       | `{"agents":[{"agent_id":..,"agent_type":..},...]}`|
| unknown / bad JSON                     | `{"error":"unknown_op"}` or `{"error":"bad_json"}`|

Bad JSON does NOT close the connection — the server returns the
error line and keeps reading.

## Snapshot fields

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

- **Mailbox depth** is read from `Mailbox::introspect().channel_used`
  (the bounded tokio channel's `capacity - tx.capacity()`). No new
  per-message accounting in the hot path — snapshot reads pay the
  cost on demand. The high-water field uses a CAS loop on a u64
  atomic that the snapshot itself bumps; we don't track high-water
  on every enqueue.
- **In-flight handler** + elapsed are stored under a
  `parking_lot::Mutex<Option<InFlight>>` set by the agent loop
  before each `run_one_turn_async` and cleared after. The mutex is
  uncontended in the steady state (only the snapshot reader competes
  with the agent's own loop).
- **Last-N ring** is `VecDeque<String>` under the same mutex, capped
  at 8 entries. Bodies are stored only when
  `MTY_INSPECT_CAPTURE_BODIES=1` is set; the value is read once per
  agent loop and cached.
- **Snapshot creation never blocks an agent**: registry iteration
  takes a `DashMap` snapshot to `Vec<Arc<_>>` and releases the lock
  before computing the per-agent payload.
- **Internal types are not exposed**: `AgentSnapshot` is a plain
  serializable struct; the runtime computes it from
  `AgentDescriptor` + `AgentIntrospectState` rather than handing out
  references to either.

## Windows status

Windows uses named pipes instead of Unix-domain sockets, and tokio's
named-pipe API has different ergonomics. v0.16 ships Unix-only;
`spawn_control_socket_at` returns `None` and logs a warning on
Windows, and `mty inspect` returns a stub error. The `cfg(unix)`
control-socket integration test is skipped on Windows.

Tracking: see "Tier-1 followups" below.

## Tests

7 integration tests in `crates/mty-runtime/tests/introspect.rs`:

- `snapshot_includes_live_agent_with_correct_type`
- `snapshot_disabled_without_env`
- `agent_id_lookup_works`
- `list_op_enumerates_live_agents`
- `introspect_state_high_water_tracks_max`
- `snapshot_serializes_to_json`
- `control_socket_responds_to_snapshot_op` (`cfg(unix)`)
- `map_insert_get_remove_round_trip`

Plus 4 unit tests in `introspect.rs` and 4 in `control_socket.rs`.

## Tier-1 followups

- **`mty top`**: polling-mode subcommand with a worker-by-worker
  schedule view. `--watch` already polls but doesn't render the
  scheduler's worker stats.
- **OpenTelemetry integration**: the sibling-agent v0.16 work added
  agent spans (`span_spawn`, `span_handler`, etc.). Future
  `mty inspect` revisions could carry trace IDs in the snapshot so
  operators can pivot from a snapshot row to the matching trace.
- **Windows named-pipe backend**: requires figuring out tokio's
  `NamedPipeServer` accept loop and matching path conventions
  (e.g. `\\.\pipe\mty\<service>`).
- **Recording / replay** (Tier 2): the `last_messages` ring is
  intentionally tiny. A debugger-grade recording surface (Tier 2.1
  of the roadmap) would replace it with a sized circular log.
- **Per-handler latency histograms**: the current snapshot only
  shows the *current* handler's elapsed time. A small reservoir
  sampler per (agent, handler) would give p50/p99 without growing
  the wire shape significantly.
