# Cluster mesh — design notes (v0.18, Tier 4.1)

Roadmap reference: `docs/internals/agent-features-roadmap.md` Tier 4.1
"single-cluster mesh." Public-facing arch: `docs/internals/cluster.md`.

## What landed in this slice

| Component | File | Status |
| --- | --- | --- |
| `AgentAddr = node:type:pid` | `cluster/address.rs` | landed |
| Framed CBOR wire (`WireFrame`) | `cluster/wire.rs` | landed |
| `Peer` (TLS, reconnect, heartbeat) | `cluster/peer.rs` | landed |
| `ClusterMesh` (multi-peer, listener + dialers) | `cluster/mesh.rs` | landed |
| `ClusterRouter` trait | `cluster/mod.rs` | landed |
| Runtime integration (`Runtime::send` consults router) | `runtime.rs` | **deferred to v0.19** |
| `mighty.toml [cluster.peers]` parser | `mty-pkg` | **deferred to v0.19** |
| Multi-node integration tests | `tests/cluster.rs` | landed (7 tests) |

The transport layer is feature-complete and additive — `Runtime`
isn't modified, but it CAN be at any time via a one-line hook:
the runtime gains an optional `Arc<dyn ClusterRouter>` field, and
`send`/`ask` consult it before falling back to the in-process
mailbox. Defaulting that field to `None` keeps every existing
single-node test path bit-identical.

## Design choices

### Why not edit `runtime.rs` in this slice?

The agent-instructions OFF-LIMITS list named `runtime.rs`
explicitly. Wiring the router into `Runtime::send` is a tiny edit
(maybe 6 lines), but doing so out-of-band while a parallel agent
might be touching the same file is precisely the kind of merge
hazard the off-limits rule guards against.

The shape is designed so the wiring is mechanical and additive:

```rust
// in Runtime:
cluster: Option<SharedRouter>,  // None for legacy single-node

// in send():
if let Some(router) = &self.cluster {
    if !router.is_local(&target_addr.node) {
        return router.route(WireFrame::Send { ... });
    }
}
// existing local-send path unchanged
```

v0.19 is purely "wire it up + add the toml parser + integration
tests that bring up two `Runtime`s and round-trip a message."

### Why ciborium, not serde_cbor?

`serde_cbor` is unmaintained (last release 2021). `ciborium` is the
modern serde-cbor implementation, already pulled in transitively
via webpki-style chains. Promoting it to a direct dep added 0
bytes to the dep graph.

### Why not `smol_str` for `NodeId`?

The brief suggested `SmolStr`, but `smol_str` isn't in the
workspace, and node ids are typically 8–24 bytes — well below the
`String` heap-vs-inline threshold's payoff. Skipping the new dep
keeps the cluster module dep-light. Future-proofing: `NodeId` is
opaque, so we can swap the backing type without breaking callers.

### Why a `DashMap<NodeId, Arc<Peer>>` not a `HashMap<NodeId, PeerSlot>`?

Both the listener task (inbound peer accepted) and the dialer task
(outbound peer connected) need to install peers concurrently with
`route()` lookups. `DashMap` gives us shard-level locking; the
read path stays uncontended in the common case. The `PeerSlot`
helper in `peer.rs` is provided for callers that want to mediate
multiple writers per slot, but the current mesh doesn't need it.

### Frame addressing: why no Reply routing?

`Reply` and `Error` frames don't carry a destination address.
They're answers to an earlier `Ask` and travel on the *same socket*
the `Ask` came in on. The mesh's `route()` rejects them with a
clear decode-error message; the runtime layer handles them via
correlation-id bookkeeping (v0.19 work).

### Heartbeats absorbed at reader

The reader task drops `Heartbeat` frames silently instead of
pushing them onto the inbox. The inbox is bounded, and a spammy
peer shouldn't be able to back-pressure the application. Heartbeats
exist for TCP keepalive purposes, not for the app.

### Self-signed certs in tests via `rcgen`

`rcgen` is already a transitive dep (via various test fixtures in
the workspace). The cluster test file mints fresh certs per-test
so there's no shared mutable state between parallel test runs.

## Public API surface

| Item | Path |
| --- | --- |
| `AgentAddr`, `NodeId`, `current_node_id()` | `cluster::address::*` |
| `WireFrame`, `encode_frame`, `decode_frame`, `read_frame_async`, `write_frame_async` | `cluster::wire::*` |
| `Peer`, `PeerError`, `InboundFrame`, `reconnect_backoff` | `cluster::peer::*` |
| `ClusterMesh`, `ClusterConfig`, `PeerEntry`, `TlsConfig`, `MeshError` | `cluster::mesh::*` |
| `ClusterRouter` trait, `SharedRouter` typedef | `cluster::*` |

Re-exported from the crate root via `mty_runtime::AgentAddr` etc.

## v0.19 follow-ups

1. **Runtime hook.** Add `Runtime::cluster: Option<SharedRouter>`
   and consult it in `send` / `ask`. Add `RuntimeBuilder::cluster()`.
2. **`mighty.toml` parser.** New `[cluster]` section in `mty-pkg`'s
   config. Translate to `ClusterConfig`.
3. **Correlation table for `Ask`.** Track pending asks per peer,
   match incoming `Reply` / `Error` by correlation id. Already-
   timed-out asks: silently drop the reply.
4. **Mutual TLS.** Client-cert verification by node id.
5. **Per-peer metrics.** Hook into the `telemetry::TelemetrySink`
   so cluster sends show up in the existing telemetry stream.
6. **Integration test for `Runtime::send` cross-node.** Bring up
   two `Runtime` instances each with a `ClusterMesh`, spawn an
   agent on B, send to its `AgentAddr` from A, assert it ran on B.

## Tier 4.2 (cluster supervisor) — deferred

A `Supervisor` whose child set spans nodes. Strategies (one-for-one,
all-for-one, rest-for-one) apply across the cluster. Node failure
marks every child on that node as `:noproc`; the strategy fires.

Requires:

- A `Node` health view inside the mesh (live / down / unknown).
- An RPC for `supervisor.notify_child_failed(addr)`.
- Strategy code that already exists in
  `supervisor_orchestrator.rs` runs unchanged once the children
  are addressed by `AgentAddr` rather than `AgentId(u64)` only.

## Tier 4.3 (lossless live migration) — deferred

`migrate(addr, target_node)` drains an agent's mailbox, takes a
state snapshot, ships it as a single (large) `WireFrame::Migrate`
frame, restores on the target, and updates routing tables. Builds
on Tier 3 (state-preserving reload, also not yet shipped) + Tier
4.1 (this slice).

## Acceptance trace

- `cargo build -p mty-runtime` clean.
- `cargo build -p mty-runtime --tests` clean (the broken
  `mty-codegen-wasm` mid-flight from a parallel agent is the only
  workspace-wide blocker; my crate builds + checks fine).
- 7 integration tests in `tests/cluster.rs`.
- No edits to off-limits files (`runtime.rs`, `agent.rs`,
  `host_std.rs`, etc.).
