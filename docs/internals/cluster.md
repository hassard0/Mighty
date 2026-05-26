# Cluster — distributed agents (Tier 4.1 + 4.2, v0.18 + v0.19 + v0.20)

> Status: **transport layer landed in v0.18; runtime integration landed
> in v0.19; mTLS + CN-bound identity + cluster supervisor (Tier 4.2)
> landed in v0.20**. Lossless live migration is Tier 4.3, scheduled for
> v0.21+.

Mighty's first three tiers (v0.10–v0.17) deliver an in-process
agent runtime. Tier 4.1 lifts that out of one process:
[agent addresses](#agent-addresses) gain a `node` axis, and
the runtime can hand non-local sends to a [mesh](#mesh) that ships
them as framed CBOR over TLS.

## Architecture

```
           +----------------------------+
           |  process A  (node-a)       |
           |                            |
           |   +---------+              |
           |   | Runtime |              |
           |   +----+----+              |
           |        | route(WireFrame)  |
           |        v                   |
           |   +----+----------------+  |
           |   | ClusterRouter (dyn) |--|--+
           |   +----+----------------+  |  |  TLS + CBOR frames
           |        |                   |  |  (length-prefixed)
           |   +----+----------------+  |  |
           |   | ClusterMesh        |   |  |
           |   |  - DashMap<peer>   |---|--+
           |   |  - inbox mpsc      |   |
           |   |  - listener task   |   |
           |   +--------------------+   |
           +----------------------------+
                                          \
                                           v
           +----------------------------+
           |  process B  (node-b)       |
           |   ClusterMesh listener     |
           |   → reader → inbox         |
           |   → routes to Runtime      |
           +----------------------------+
```

Each node runs one [`ClusterMesh`]. It owns:

- **One listener** (`TcpListener` + `tokio-rustls` acceptor) for inbound
  connections.
- **One `DashMap<NodeId, Arc<Peer>>`** of outbound peers.
- **One central mpsc inbox.** Every peer's reader task pushes
  inbound frames here; the runtime drains it.

## Agent addresses

Pre-v0.18, an agent was identified by `AgentId(u64)` + `AgentHandle.name`.
That's still the in-process truth — Tier 4.1 ADDS a richer address:

```rust
pub struct AgentAddr {
    pub node: NodeId,        // e.g. "node-a"; from MTY_NODE_ID
    pub agent_type: String,  // matches AgentHandle.name
    pub agent_id: u64,       // matches AgentId(u64)
}
```

Construction:

```rust
let local  = AgentAddr::local("Greeter", 7);            // node = current
let remote = AgentAddr::remote("node-b", "Greeter", 7); // node = "node-b"
assert!(local.is_local());
assert!(!remote.is_local());
```

The current node id is read once from `MTY_NODE_ID` (default: `"local"`)
and cached for the process lifetime. Display format: `node:type:pid`.

## Wire protocol

Every frame on the wire is `u32 BE length || CBOR body`. The 4-byte
length prefix bounds reads + allocations. The body is a serde-CBOR
encoding of `WireFrame`:

```rust
pub enum WireFrame {
    Hello { node_id, version },         // post-TLS handshake
    Heartbeat,                          // 5s liveness ping
    Send { from, to, msg, msg_bytes },  // fire-and-forget
    Ask  { from, to, msg, msg_bytes, correlation }, // request-reply
    Reply { correlation, msg_bytes },
    Error { correlation, kind, message },
    Goodbye,                            // voluntary teardown
}
```

CBOR via [`ciborium`] (the maintained successor to `serde_cbor`).
Max body size: 8 MiB.

The user payload travels as opaque `msg_bytes`. The cluster module
does NOT re-serialize the runtime's `Value` graph — the runtime
already has a canonical encoding for replay and reuses it here.

## Mesh

```rust
let cfg = ClusterConfig {
    node_id: NodeId::new("node-a"),
    listen_addr: Some("0.0.0.0:9700".parse()?),
    peers: vec![PeerEntry {
        node_id: NodeId::new("node-b"),
        addr: "10.0.0.7:9700".parse()?,
        server_name: Some("node-b.cluster.local".into()),
    }],
    tls: TlsConfig { connector, acceptor },
};
let mesh = ClusterMesh::from_config(cfg).await?;

// Routing:
mesh.route(WireFrame::Send { from, to, msg, msg_bytes })?;
```

Routing rules:

- `to.node == self_node` → `MeshError::WouldLoopLocal`. The caller
  should have taken the in-process path.
- `to.node` unknown → `MeshError::UnknownNode`.
- Peer present but writer task gone → `MeshError::PeerDisconnected`.
- Otherwise: hand off to the peer's writer task.

### Reconnect

Each configured peer gets a background dialer task. On first start
the dialer connects; if it fails (peer not up yet, network blip), it
sleeps with exponential backoff (100ms → 30s, capped) and retries.
Default cap: 10 attempts; set `RECONNECT_MAX_ATTEMPTS = 0` for
unbounded.

Once connected, the dialer supervises the peer slot — if it notices
the writer task died (`is_connected() == false`), it kicks off the
backoff loop again.

### Heartbeats

The writer task ticks `WireFrame::Heartbeat` every 5 seconds. The
reader task ABSORBS heartbeats locally (does not push them onto the
inbox) — they exist so that the OS-level TCP stack notices a dead
peer faster, not so the application has to.

## Configuration (`mighty.toml`)

v0.19 parses the `[cluster]` block. The shape mirrors the in-memory
`ClusterConfig`: node id, listen address, static peer list, and an
optional `[cluster.tls]` table for cert paths.

```toml
[package]
name = "demo"
version = "0.1.0"
edition = "2026"

[cluster]
node_id    = "node-a"
listen     = "0.0.0.0:9700"

[cluster.tls]
cert_pem   = "certs/node-a.pem"
key_pem    = "certs/node-a.key"
trusted_roots = ["certs/cluster-ca.pem"]

[[cluster.peers]]
node_id     = "node-b"
addr        = "10.0.0.7:9700"
server_name = "node-b.cluster.local"   # optional, defaults to node_id

[[cluster.peers]]
node_id     = "node-c"
addr        = "10.0.0.8:9700"
```

`MTY_NODE_ID` overrides `cluster.node_id` for ad-hoc local runs.

The parser lives in `mty_driver::manifest::ClusterManifest`. It only
records the shape — translating `cert_pem` / `key_pem` paths into a
live `rustls::ServerConfig` is the runtime's job at startup, so the
`mty-driver` and `mty-pkg` crates stay TLS-free.

## Runtime integration

v0.19 wires the mesh into the runtime via two opt-in entry points:

```rust
use mty_runtime::{Runtime, RuntimeBuilder, AgentAddr};
use mty_runtime::cluster::{ClusterMesh, ClusterConfig};

let mesh = ClusterMesh::from_config(cluster_cfg).await?;
let rt = RuntimeBuilder::new()
    .build(prog)
    .with_cluster(mesh);  // takes a SharedRouter = Arc<dyn ClusterRouter>

// Local — no router involvement, zero overhead vs single-node:
rt.send(&handle, "ping", vec![]).await?;

// Addressed — checks the router on every call:
let to = AgentAddr::remote("node-b", "Greeter", 42);
rt.send_addr(AgentAddr::local("Caller", 1), to.clone(), "ping", vec![])
    .await?;
let reply = rt
    .ask_addr(AgentAddr::local("Caller", 1), to, "ask", vec![], Some(deadline))
    .await?;
```

The handle-taking `send` / `ask` keep their v0.17 signatures
unchanged — they're the in-process fast path and never consult the
router. Callers who want distributed routing opt in to `send_addr` /
`ask_addr`.

### Dispatch table

| `to.is_local()` | router installed | result |
| --- | --- | --- |
| yes | any | in-process mailbox path (same as legacy `send`/`ask`) |
| no | yes | `router.route_send(...)` or `router.route_ask(...)` |
| no | no | `Trap { code: "MT5030" }` — clear "no cluster configured" error |

Diag codes:

- `MT5030` — addressed message to a remote node but no cluster router
  is installed on this runtime.
- `MT5031` — cluster send / ask transport failure (peer disconnected
  mid-flight, peer not configured, frame too large, …).
- `MT5032` — remote replier returned a structured `Error` frame
  (e.g. handler panicked on the far side).

### Ask + Reply correlation

`Runtime::ask_addr` reserves a fresh correlation id via
[`CorrelationTable::register`](#correlation-table), sends the
`WireFrame::Ask`, and awaits the matching reply on a `oneshot`. The
mesh's reply-demultiplexer task drains the central inbox, peels
`Reply` / `Error` frames into the table by correlation id, and
forwards everything else (`Send`, `Ask`) to the user-facing inbox.

```text
   Runtime A                                       Runtime B
   ----------                                      ----------
   ask_addr(to, msg, deadline)
       │
       ▼
   router.route_ask
       │ register(id) → oneshot::Receiver
       ▼
   write WireFrame::Ask { correlation: id, … }    ─────► reader task
                                                          │
                                                          ▼
                                                  inbox → handler runs
                                                          │
                                                          ▼
                                                  writes Reply { id, … } ◄─┐
                                                                            │
   read WireFrame::Reply { correlation: id }     ◄───── socket ────────────┘
       │
       ▼
   demux → table.complete(id, Reply)
       │
       ▼
   oneshot resolves → return Value to user
```

If the peer disconnects mid-ask, the dialer task notices the
`is_connected() == false` transition and calls
`CorrelationTable::fail_targeting_node(node)`, which resolves every
pending ask aimed at that node to a synthetic `peer_disconnected`
Error frame. The caller sees `Trap { code: "MT5032" }`.

### Zero-cost when cluster is None

The `Runtime` struct gains exactly one field: `cluster:
Option<SharedRouter>`. The legacy `send` / `ask` methods never read
it. `send_addr` / `ask_addr` are new methods — code that doesn't call
them pays nothing. When `cluster.is_none()` and the caller passes a
remote address, they get an immediate `Trap` with code `MT5030` (no
hidden retry / fallback).

## Correlation table

`crates/mty-runtime/src/cluster/correlation.rs`:

```rust
pub struct CorrelationTable {
    next_id: AtomicU64,
    pending: DashMap<u64, oneshot::Sender<WireFrame>>,
    targets: DashMap<u64, String>, // for peer-disconnect fan-out
}
```

- `register() -> (u64, oneshot::Receiver<WireFrame>)` — hands out the
  next correlation id (monotonic, starts at 1) and the receiver to
  await.
- `register_for_node(node)` — same plus side-records the target so
  `fail_targeting_node` can wake every pending ask aimed at a peer
  that just disconnected.
- `complete(id, frame)` — resolves a pending oneshot. Late /
  duplicate replies are dropped silently.
- `cleanup(id)` — purges a slot without delivering (used by the
  ask future's RAII guard when the caller times out).
- `fail_all_with(frame_for)` — used by `ClusterMesh::shutdown` to
  resolve every pending ask with a synthetic `mesh_shutdown` error
  so callers don't hang.

## Security

- **TLS is mandatory.** The mesh has no plain-TCP fallback; every
  socket goes through `tokio-rustls`. The same rustls 0.23 + `ring`
  provider as `std.tls`.
- **Cert layout.** For self-hosted clusters: one internal CA, one
  cert per node (SAN = the node's public DNS or `node_id`, **CN =
  `node_id`** so mTLS can bind cert identity to advertised id),
  every node trusts the internal CA. For dev: per-node self-signed
  certs with explicit trust roots (the integration tests do this
  with `rcgen`).
- **Mutual TLS (v0.20).** Opt-in via `ClusterMesh::from_config_mtls`
  (and the `[cluster.tls].require_client_cert = true` manifest
  knob, when wired). Every accepted connection MUST present a
  client cert chaining to `ClusterTlsConfig::client_ca`; every
  dialer MUST present `client_cert` (or, by default, the
  `server_cert` reused for both roles).
- **CN-bound identity (v0.20).** After the TLS handshake completes,
  both sides peel the peer's leaf cert and verify the Subject CN
  equals the `Hello.node_id` the peer claims. A cert holder
  pretending to be a different node fails the post-Hello check
  with diag `MT5040`.

### v0.20 mTLS config

```rust
use mty_runtime::cluster::tls::ClusterTlsConfig;

let cfg = ClusterTlsConfig {
    server_cert: node_a_cert,
    server_key:  node_a_key,
    client_ca:   Arc::new(cluster_ca_roots), // who can dial us
    server_ca:   Arc::new(cluster_ca_roots), // who we trust as server
    client_cert: None,         // default: reuse server_cert
    client_key:  None,         // default: reuse server_key
    require_client_cert: true,
};

let acceptor  = mty_runtime::cluster::tls::build_acceptor(&cfg)?;
let connector = mty_runtime::cluster::tls::build_connector(&cfg)?;

let mesh = ClusterMesh::from_config_mtls(ClusterConfig {
    node_id: NodeId::new("node-a"),
    listen_addr: Some("0.0.0.0:9700".parse()?),
    peers: vec![/* … */],
    tls: TlsConfig { acceptor: (*acceptor).clone(), connector: (*connector).clone() },
}).await?;
```

`require_client_cert: false` keeps the v0.18 / v0.19 server-only
TLS shape — `ClusterMesh::from_config` still works unchanged.

### Diag codes added in v0.20

- `MT5040` — peer cert CN does not match the `Hello.node_id` claim,
  OR the peer presented an empty / unparseable cert chain. Mesh
  drops the connection before installing the peer.

## Cluster supervisor (Tier 4.2, v0.20)

The in-process [`supervisor`] tree restarts agents that crash inside
this node. The cluster supervisor lifts that one level up: its
children can live on **remote** nodes, and the events it reacts to
include "peer disconnected → every child on that node is now
`:noproc`".

### Shape

```rust
use mty_runtime::cluster::supervisor::{
    ChildSpec, ClusterSupervisor, RestartPolicy, RestartStrategy,
    SupervisorEvent,
};

let sup = Arc::new(ClusterSupervisor::new(RestartStrategy::OneForOne));
sup.add_child(ChildSpec {
    addr: AgentAddr::remote("node-b", "Worker", 1),
    restart: RestartPolicy::Permanent,
    max_restarts: 5,
    window_ms: 30_000,
});

// Wire mesh disconnect events into the supervisor:
mesh.register_supervisor(sup.clone());

// Drain restart events:
while let Some(ev) = sup.next_event().await {
    match ev {
        // v0.21: `placement_hint` is set when a PlacementPolicy is
        // installed; respect it to drive cross-node fail-over.
        SupervisorEvent::RestartRequested { child, siblings, reason, placement_hint } => {
            // Caller re-spawns child + every sibling the strategy lists.
        }
        SupervisorEvent::CircuitBreakerTripped { child, attempts, window_ms } => {
            // Operator alert — supervisor has stopped trying.
        }
        SupervisorEvent::NodeDisconnect { node, lost_children } => {
            // Diagnostic — emitted alongside the per-child events.
        }
    }
}
```

### Strategies

| Strategy     | Effect when child `B` of `{A, B, C, D}` (insertion order) fails |
| ------------ | --------------------------------------------------------------- |
| `OneForOne`  | Restart only `B`.                                               |
| `OneForAll`  | Restart `B` + `{A, C, D}`.                                      |
| `RestForOne` | Restart `B` + `{C, D}` (siblings inserted AFTER `B`).           |

### Circuit breaker

Each child carries a `(max_restarts, window_ms)` budget. The
supervisor keeps a sliding-window count of recent restart timestamps;
once the count exceeds `max_restarts` within `window_ms`, the
supervisor emits `SupervisorEvent::CircuitBreakerTripped` and moves
the child to `ChildState::Dead(why)`. No further restart events fire
until the operator re-adds the child via `add_child`.

### Node-disconnect cascade

When the mesh's dialer task notices a peer is gone, it calls
`ClusterMesh::notify_node_disconnect(node)`. Every registered
supervisor's `SupervisorHook::on_node_disconnect` fires. The
supervisor:

1. Finds every child whose `addr.node == node` and marks them
   `ChildState::NoProc`. (Idempotent — already-NoProc children
   are skipped.)
2. Emits one `NodeDisconnect { node, lost_children }` event.
3. For each lost child, runs `plan_restart_locked` (which applies
   the strategy + circuit breaker) and emits `RestartRequested`.

The supervisor does NOT re-place children on a different node. That's
**cross-node fail-over** and lives in Tier 4.2.1 / v0.21+ once a
placement-policy abstraction lands. v0.20's restart events let the
caller decide: try the same node again when it reconnects, or pick a
new one from operator-supplied policy.

## Placement policy (v0.21 Tier 4.3)

v0.21 adds a [`PlacementPolicy`] trait the supervisor consults at
restart time. When a policy is installed via
`ClusterSupervisor::set_placement_policy`, every `RestartRequested`
event carries a `placement_hint: Option<NodeId>` set by the policy's
`place()` method. The caller's restart logic uses the hint to decide
which node to re-spawn the child on.

### Bundled policies

| Policy             | Behaviour                                                       |
| ------------------ | --------------------------------------------------------------- |
| `StickyPolicy`     | Keep on current node if reachable, else least-loaded fallback.   |
| `LeastLoadedPolicy`| Always pick the reachable node with the smallest child count.    |
| `StaticPolicy(n)`  | Always pick `n`. Useful for "send everything to the spare".      |

Custom policies are wired via the Rust API only — the manifest's
`[cluster.placement]` block selects one of the three bundled names:

```toml
[cluster.placement]
policy = "sticky"            # or "least-loaded" or "static"
default_node = "node-spare"  # required when policy = "static"
```

The supervisor also exposes `set_available_nodes(...)` so the runtime
can keep the policy's view of the cluster in sync with the mesh's
connection state.

## Live migration (v0.21 Tier 4.3) {#live-migration}

`migrate_agent(agent, target, deadline)` ships a running agent's
mailbox + continuation from the source node to `target`. Implementation
in `crates/mty-runtime/src/cluster/migration.rs`; RFC-006 carries the
design discussion.

### Sequence

```
  source                                         target
    |                                              |
    |  1. drain + snapshot (Resumable)             |
    |     agent transitions to MIGRATING           |
    |     new messages keep enqueueing             |
    |                                              |
    |--- WireFrame::MigrateSnapshot --------------->|
    |                                              |
    |                                  2. verify schema_hash
    |                                     SnapshotSink::restore
    |                                     assign new agent_id
    |                                              |
    |<-- WireFrame::MigrateAck --------------------|
    |                                              |
    |  3. forward queued mailbox frames            |
    |     to new addr as plain Send frames         |
    |  4. install routing rewrite                  |
    |     original_addr -> new_addr                |
    |     (subsequent sends route via mesh)        |
```

### Wire frames

- `WireFrame::MigrateSnapshot { agent_addr, target_node, agent_type, schema_hash, state }`
- `WireFrame::MigrateAck { migrating, new, route_to }`
- `WireFrame::MigrateError { migrating, route_to, kind, message }`

### Error taxonomy

| Diag code | Variant                  | Trigger                                            |
| --------- | ------------------------ | -------------------------------------------------- |
| MT5060    | IncompatibleSchema       | target's `Resumable::SCHEMA_HASH` doesn't match    |
| MT5071    | AgentNotFound            | source can't locate the agent locally               |
| MT5072    | TargetUnreachable        | target node isn't a connected peer                  |
| MT5073    | SameNode                 | target == local node                                |
| MT5074    | Deadline                 | ack didn't arrive within `deadline_ms`              |
| MT5075    | Rejected                 | target sent `MigrateError`                          |
| MT5076    | SnapshotTooLarge         | state > 6 MiB                                       |
| MT5077    | Mesh                     | mesh routing failure (peer disconnect mid-migrate)  |

On any failure the source's `SnapshotSource::rollback` hook is called
and the agent resumes processing locally — no half-migrated state.

### Metrics

`MigrationMetrics` exposes Prometheus-shaped counters (no per-agent
labels):

- `migrations_started` / `migrations_completed` / `migrations_failed`
- `migrations_rolled_back`
- `bytes_shipped_total`
- `messages_forwarded_total`

## Operational notes

### Rolling restart

To restart `node-b` without disrupting traffic:

1. Drain. (Application-level — pause new work targeted at `node-b`.)
2. `SIGTERM` node-b. `ClusterMesh::shutdown` sends `Goodbye` to every
   peer; their reader tasks see the EOF and tear down cleanly.
3. Restart. Other nodes' dialer tasks reconnect on the same addr.

The reconnect-after-disconnect integration test
(`tests/cluster.rs::peer_reconnects_after_disconnect`) exercises
exactly this path in-process.

### Adding a node

Add the new node's entry to every existing node's `[[cluster.peers]]`,
rolling-restart them, then bring up the new node. The new node's
config lists every existing peer; their dialers + its dialer
converge symmetrically.

### Topology

Initial v0.18 topology is static-list mesh — every node knows every
peer up front. A discovery protocol (gossip / consul / etc.) is a
post-v0.20 item.

## Tests

`crates/mty-runtime/tests/cluster.rs` (v0.18 baseline, 7 tests):

- `addr_parse_local_remote_distinguishes` — `AgentAddr` semantics.
- `wire_frame_roundtrip` — every variant survives encode/decode.
- `peer_connect_to_listener` — Peer → mesh listener handshake +
  frame delivery.
- `mesh_routes_remote_frame_to_peer` — two-node A → B routing.
- `mesh_returns_error_on_unknown_node` — clear error on bad node.
- `mesh_returns_error_on_local_loop` — clear error on self-target.
- `peer_reconnects_after_disconnect` — kill peer, dialer reconnects.

`crates/mty-runtime/tests/cluster_mtls.rs` (v0.20, 5 tests):

- `mtls_handshake_with_matching_cert_succeeds` — happy path A→B
  with mTLS + CN binding; B's inbox receives the routed Send.
- `mtls_handshake_with_wrong_cn_rejected` — peer presents a cert
  with CN=`node-attacker` but claims `node-victim-impostor` in Hello
  → `MT5040 IdentityMismatch`.
- `mtls_handshake_with_untrusted_ca_rejected` — A requires client
  certs signed by A's CA; B's cert isn't trusted → rustls rejects
  before the Hello exchange runs.
- `server_only_tls_still_works_when_mtls_disabled` — back-compat
  regression: a v0.18-shape mesh routes A→B unchanged.
- `cert_node_id_pins_subject_cn_against_san_only_cert` — guard
  against rcgen behaviour change in the CN-extraction path.

`crates/mty-runtime/tests/cluster_supervisor.rs` (v0.20, 6 tests):

- `supervisor_marks_children_noproc_on_peer_disconnect` — node-b
  goes away, every child whose addr.node == "node-b" is NoProc.
- `one_for_one_restart_strategy` — only the failing child gets
  a `RestartRequested`.
- `one_for_all_restart_strategy` — failing child + every sibling.
- `rest_for_one_restart_strategy` — failing child + only siblings
  inserted after it.
- `max_restarts_window_circuit_breaker` — fourth crash within
  the window trips the breaker and stops further restarts.
- `mesh_disconnect_propagates_to_registered_supervisor` — covers
  the `ClusterMesh::notify_node_disconnect` → supervisor hook
  wiring path.

`crates/mty-runtime/tests/cluster_routing.rs` (v0.19, 8 tests):

- `runtime_with_cluster_routes_remote_send` — A → B Send through the
  router trait.
- `runtime_with_cluster_routes_remote_ask` — A → B Ask + synthesised
  Reply correlate end-to-end.
- `runtime_without_cluster_documents_trap_code` — pins the `MT5030 /
  MT5031 / MT5032` diag codes so refactors can't quietly change them.
- `manifest_cluster_section_parses` — full `[cluster]` + `[[cluster.peers]]`
  + `[cluster.tls]` round-trip.
- `manifest_without_cluster_section_still_parses` — regression guard
  for manifests that never opt in.
- `correlation_table_completes_replies` — basic register + complete.
- `correlation_table_handles_concurrent_asks` — 100 in-flight asks
  resolve in arbitrary order.
- `runtime_send_addr_local_routes_to_mailbox` — local addresses
  bypass the router entirely.
- `peer_disconnect_fails_pending_asks` — pending asks for a dropped
  peer resolve cleanly instead of hanging.

Self-signed certs minted per-test via `rcgen`; no on-disk fixtures.

## What's deferred

| Item | Slice |
| --- | --- |
| Mutual TLS (client certs) | **shipped v0.20** |
| Cluster-aware supervisor (Tier 4.2) | **shipped v0.20** |
| Placement-policy-driven restart hints | **shipped v0.21** |
| Lossless live migration (Tier 4.3) | **shipped v0.21** |
| Cluster-wide ACID transactions | v0.22+ |
| Per-frame ACK / retransmit | v0.22+ |
| SPIFFE / SAN-URI identity | v0.22+ (post-CN-binding) |
| Discovery / gossip | post-v0.21 |
