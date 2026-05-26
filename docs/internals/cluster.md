# Cluster — distributed agents (Tier 4.1, v0.18 + v0.19)

> Status: **transport layer landed in v0.18; runtime integration landed
> in v0.19** (`Runtime::send_addr` / `Runtime::ask_addr` consult an
> optional [`ClusterRouter`](#runtime-integration), `[cluster]`
> manifest section parses). Cluster-aware supervisors + lossless live
> migration are Tier 4.2 / 4.3, scheduled for v0.20+.

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
  cert per node (SAN = the node's public DNS or `node_id`),
  every node trusts the internal CA. For dev: per-node self-signed
  certs with explicit trust roots (the integration tests do this
  with `rcgen`).
- **No client certs (yet).** v0.18 trusts the server-cert + node id
  to identify peers. Mutual TLS lands with the v0.19 hardening
  pass.

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
| Mutual TLS (client certs) | v0.20 |
| Per-frame ACK / retransmit | v0.20+ |
| Cluster-aware supervisors (Tier 4.2) | v0.20+ |
| Lossless live migration (Tier 4.3) | v0.20+ |
| Discovery / gossip | post-v0.20 |
