# Cluster routing — design notes (v0.19, Tier 4.1 cont.)

Roadmap reference: `docs/internals/agent-features-roadmap.md` Tier 4.1
"single-cluster mesh." Public-facing arch: `docs/internals/cluster.md`.
Predecessor: `dev/history/notes/CLUSTER_V0_18_NOTES.md`.

## What landed in this slice

| Component | File | Status |
| --- | --- | --- |
| `CorrelationTable` (ask/reply demux) | `cluster/correlation.rs` | landed |
| `ClusterRouter::route_send` + `route_ask` | `cluster/mod.rs` | landed |
| Mesh ↔ correlation wiring + reply demultiplexer task | `cluster/mesh.rs` | landed |
| `Runtime::with_cluster` + `send_addr` + `ask_addr` | `runtime.rs` | landed |
| Peer-disconnect fan-out (in-flight asks → clean trap) | `cluster/mesh.rs` + `correlation.rs` | landed |
| `[cluster]` / `[[cluster.peers]]` / `[cluster.tls]` parser | `mty-driver/src/manifest.rs` | landed |
| Cluster routing integration tests | `tests/cluster_routing.rs` | landed (8 tests) |
| Cluster supervisor (Tier 4.2) | n/a | **deferred to v0.20** |
| Lossless live migration (Tier 4.3) | n/a | **deferred to v0.20** |

## Design choices

### Additive surface over a rewrite

The legacy `Runtime::send(&AgentHandle, ...)` keeps its v0.17
signature. We introduced new entry points `send_addr(AgentAddr, ...)`
and `ask_addr(AgentAddr, ...)` rather than rewriting the existing
methods. Three reasons:

1. **Zero-cost guarantee.** Callers that don't pass an `AgentAddr`
   never read the `cluster: Option<SharedRouter>` field. The legacy
   path is bit-identical to v0.18 for every existing test
   (`cargo test -p mty-runtime` baseline preserved).
2. **`AgentHandle` is in-process by construction.** It carries an
   `Arc<Mailbox>`, which only makes sense for an agent that lives in
   this process. Forcing a node-routing decision on the handle path
   would be artificial.
3. **Future ergonomics.** Library code that wants cluster routing has
   to ask for an `AgentAddr` anyway; threading both shapes through
   the same fn would have made the call sites bigger, not smaller.

### Correlation table: per-node, not per-peer

Each peer maintains its own ordered byte stream, so technically a
per-peer correlation counter would suffice. We picked a single
node-wide table because:

- One `Arc<CorrelationTable>` clones cleanly into the reply
  demultiplexer task; otherwise the demux would have to look up the
  peer-id from the inbound envelope and route to a per-peer table.
- The id space (u64) is unboundable in practice (one node would
  need to issue >1e19 asks before wrapping).
- `fail_all_with` / `fail_targeting_node` get simpler — one place to
  walk, one shard layout to think about.

### Oneshot, not broadcast

Exactly one consumer of the reply (the `ask()` caller). If the
caller drops its `Receiver` (e.g. an outer `timeout` fired), the
`AskGuard` RAII helper purges the slot so the map doesn't leak.
A broadcast channel would have wasted memory and forced every reply
to clone the bytes; oneshot is the right shape.

### Reply-demultiplexer task

v0.18 had the mesh's central inbox surface every `WireFrame` to
whoever called `take_inbox()`. v0.19 needs to peel `Reply` / `Error`
frames off into the correlation table before the runtime sees them
— otherwise every `ask_addr` caller would have to scan the inbox
themselves.

The fix: a two-stage channel.

```
   peer reader tasks ─► raw_tx ─► demux task ─► inbox_tx ─► take_inbox()
                                       │
                                       ▼
                              correlations.complete(id, frame)
```

`raw_tx` and `inbox_tx` both have capacity `MESH_INBOX_CAPACITY`. The
v0.18 inbox shape (Send/Ask only) is preserved — every existing test
in `tests/cluster.rs` still passes.

### Peer-disconnect cleanup

If peer B dies mid-ask, the local oneshot would otherwise hang
forever. The dialer task tracks `was_connected: bool` and on the
high→low edge calls
`correlations.fail_targeting_node(node)`, which resolves every
pending ask aimed at B to a synthetic `peer_disconnected` Error
frame. The caller's `route_ask` returns `RouteReply::Err` →
`Runtime::ask_addr` maps that to `Trap { code: "MT5032" }`.

Alternative considered: a watchdog timer per ask. Rejected — it'd
either need a tunable timeout (annoying to surface as a runtime
config) or duplicate the deadline parameter already on `ask_addr`.
The dialer-driven fan-out is exact (every disconnected ask wakes
immediately) and free (the dialer is already polling
`is_connected()`).

### Diag codes

Three new codes, all in the `MT503x` range to keep them visually
distinct from the existing budget/handler codes:

- `MT5030` — addressed message to a remote node but no cluster
  router is installed.
- `MT5031` — cluster transport failure (UnknownNode, PeerDisconnected,
  send queue full, …).
- `MT5032` — remote replier returned a structured `Error` frame.

The contract test `runtime_without_cluster_documents_trap_code`
pins these against literal strings so a refactor can't silently
change them.

### Manifest parser stays TLS-free

`ClusterManifest` records cert / key / roots as `Option<String>`
paths. The runtime — not the parser — translates those into a live
`rustls::ServerConfig`. This keeps `mty-driver` and `mty-pkg` from
gaining a `rustls` dependency just to parse a manifest, which would
have meant every CLI invocation paid the TLS link-time cost.

## What's next (v0.20+)

- **Cluster supervisor (Tier 4.2).** A supervisor that watches
  remote children: it spawns the agent on a peer, listens for
  `Exit` frames, and re-spawns on a fallback node when the primary
  dies.
- **Lossless live migration (Tier 4.3).** Pause a remote agent,
  serialise its `Value` state, ship it across the wire, resume on
  the new node. Requires the v0.17 deterministic-replay state
  snapshotting to be wired through the cluster.
- **Mutual TLS.** v0.18 has server certs only; clients are not
  authenticated. mTLS is mechanical (rustls supports it natively)
  but cert-rotation tooling lands with it.
- **Per-frame ACK + retransmit.** TLS gives us order + integrity
  but not durability across reconnect. v0.20 ACKs in-flight `Ask`
  frames so a peer reconnect doesn't lose pending requests.
- **Discovery / gossip.** The static peer list is a fine starting
  point but doesn't scale to >10 nodes. A SWIM-style gossip layer
  is post-v0.20.
