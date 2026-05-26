# Cluster — distributed agents (Tier 4.1, v0.18)

> Status: **transport layer landed in v0.18**; opt-in via
> `ClusterRouter`. Runtime-side wiring (`Runtime::send` consults the
> router) lands in v0.19. Cluster-aware supervisors + lossless live
> migration are Tier 4.2 / 4.3.

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

> Forward-looking; the config parser lands with the runtime
> integration in v0.19. v0.18 ships the in-memory `ClusterConfig`
> struct that the parser will populate.

```toml
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

`crates/mty-runtime/tests/cluster.rs`:

- `addr_parse_local_remote_distinguishes` — `AgentAddr` semantics.
- `wire_frame_roundtrip` — every variant survives encode/decode.
- `peer_connect_to_listener` — Peer → mesh listener handshake +
  frame delivery.
- `mesh_routes_remote_frame_to_peer` — two-node A → B routing.
- `mesh_returns_error_on_unknown_node` — clear error on bad node.
- `mesh_returns_error_on_local_loop` — clear error on self-target.
- `peer_reconnects_after_disconnect` — kill peer, dialer reconnects.

Self-signed certs minted per-test via `rcgen`; no on-disk fixtures.

## What's deferred

| Item | Slice |
| --- | --- |
| `Runtime::send` consults `ClusterRouter` | v0.19 |
| `[cluster]` parsing in `mighty.toml` | v0.19 |
| Mutual TLS (client certs) | v0.19 |
| Per-frame ACK / retransmit | v0.19+ |
| Cluster-aware supervisors (Tier 4.2) | v0.20+ |
| Lossless live migration (Tier 4.3) | v0.20+ |
| Discovery / gossip | post-v0.20 |
