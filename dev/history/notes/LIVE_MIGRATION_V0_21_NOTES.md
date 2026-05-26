# Live agent migration — v0.21 (Tier 4.3) — design notes

This is the design log for the v0.21 slice that lifts agent migration
from "lightweight" (the v0.6 routing-table flip) to "lossless live"
(state + mailbox + continuation moves between nodes intact). It pairs
with `docs/internals/cluster.md#live-migration` (the public story) and
`docs/spec/rfcs/RFC-006-lossless-live-agent-migration.md` (the
amendment record).

## What shipped

- New module `crates/mty-runtime/src/cluster/migration.rs` — the
  `MigrationOrchestrator` + two hooks (`SnapshotSource` /
  `SnapshotSink`) the runtime wires for its `Value`-shaped state.
- New module `crates/mty-runtime/src/cluster/placement.rs` — the
  `PlacementPolicy` trait + three bundled policies (`StickyPolicy`,
  `LeastLoadedPolicy`, `StaticPolicy`).
- New wire frames: `WireFrame::MigrateSnapshot`,
  `WireFrame::MigrateAck`, `WireFrame::MigrateError`. The CBOR
  surface is additive — v0.20 peers that don't know these variants
  fail decode and the connection tears down (audible failure beats
  silent skipping; the existing wire-protocol invariant).
- Supervisor extension: `RestartRequested` events now carry a
  `placement_hint: Option<NodeId>`. `None` for legacy
  no-policy-installed deployments; `Some(node)` when a
  `PlacementPolicy` has been wired in.
- Manifest extension: `[cluster.placement]` block in `mighty.toml`
  with `policy` + optional `default_node` for the static variant.
- Tests in `crates/mty-runtime/tests/cluster_migration.rs` cover
  the happy path, queued-message forwarding, schema mismatch,
  unreachable target, same-node rejection, sticky/least-loaded
  placement, supervisor → placement hint, metrics surface.

## Architectural decisions

### Push vs pull

The source pushes the snapshot to the target (`MigrateSnapshot`
followed by `MigrateAck` on success, `MigrateError` on failure). The
alternative — target pulls — would have required:

1. The target to know the source's local addressing scheme (it
   doesn't — `agent_id` is opaque outside the source).
2. Source-side credentials for the target to dial back through
   (an mTLS pin, since cluster connections are bidirectional).
3. A drain-then-pull protocol where the source keeps the agent
   paused while the target races to pull — worst-case latency.

Push lets the source drive timing: it knows when the drain
completes, it knows the snapshot bytes, it can apply backpressure
on its own outbound queue.

### Ack vs no-ack

Every migration is ack'd. The source rolls back on `MigrateError`
or on a deadline. The no-ack variant ("fire and forget, assume
the target restored") was rejected because:

1. The source has no way to install the routing rewrite without
   knowing the target's freshly-assigned `agent_id`. (Local IDs
   are not portable across nodes — the target picks them from
   its own `AgentRegistry::next_id`.)
2. Schema-incompatible snapshots need to surface as a clean
   `MT5060` instead of a silent message-loss bug.
3. Operators want visible "migration completed / failed" counters
   for dashboards.

The ack is single-frame (no two-phase commit), so the failure
domain is limited to the source ↔ target conversation; either
side dying mid-migrate produces a clean rollback rather than a
half-migrated state.

### Mailbox queue location

Queued messages stay on the **source** during the drain → ack
window, then are forwarded as plain `WireFrame::Send` frames after
the target's ack lands. The alternative — ship the entire
mailbox-tail as part of `MigrateSnapshot.state` — was rejected
because:

1. CBOR-encoding the mailbox bumps the snapshot above the
   `MAX_FRAME_BYTES` cap for any non-trivial backlog.
2. Mailbox forwarding fits the existing transport (the messages
   are already framed as `Send`); we get reordering safety + TLS
   integrity for free.
3. The forwarder can hop the new agent's `agent_id` without
   re-running the agent's deserialiser.

Trade-off: messages enqueued *between the ack and the rewrite
install* are forwarded to the new node; messages enqueued *after*
the rewrite install go through the routing-rewrite layer
(`MigrationOrchestrator::lookup_rewrite`). The two paths converge
because the target's `agent_id` is the same.

### Schema-hash strictness

The target rejects with `MT5060` if `snapshot.schema_hash !=
target.expected_hash`. v0.21 does NOT call into the v0.20 hot-reload
`schema_compatible_with` extensibility hook because we want the
strictest possible check at the cluster boundary — if anything
slips, we'd much rather see the migration fail loud than silently
get a partial restore. v0.22 may relax this for known-good
forward-compat shapes once the schema-migration registry is in
production.

### Placement policy as a trait, not an enum

The trait shape lets user code plug in domain-specific routing
(GPU placement, consistent hashing, tenant affinity) without
touching the supervisor. The three bundled policies cover the
common ops cases:

- `Sticky` — preserve agent / node affinity until forced to move.
- `LeastLoaded` — pure load-spread; what most rolling restarts want.
- `Static` — "send everything to the spare" for canary deploys.

The manifest exposes only the three names by string. Custom
policies wire via the Rust API.

## v0.22 follow-ups

- **Cluster-wide ACID-style transactions.** The current ack is
  single-frame; a "migrate cohort" primitive that moves N agents
  atomically (commit all or rollback all) would let operators
  drain a node by topic instead of one-by-one.
- **Partial migration** — ship the state but keep the mailbox on
  the source for a configurable grace period. Useful when the
  target has higher-latency host-side resources (e.g. local
  caches) that need to warm up before the agent dispatches.
- **Sticky-session affinity.** Carry an opaque "session key" on
  the agent and have the placement policy pin all agents with the
  same key to the same node. Lets workloads with cross-agent
  state collocate without manual `StaticPolicy` plumbing.
- **Migration during peer disconnect.** Right now the source's
  `route_async` errors out cleanly if the target peer drops
  mid-snapshot. A retry-with-fresh-snapshot layer that survives
  one disconnect would harden the rolling-restart story.
- **Wasm-module reload + migration co-orchestration.** Tier 1.5
  / Tier 1.6 schema migration registry can apply chained
  upgrades at deserialise time; wiring it through
  `SnapshotSink::restore` lets a migration also be a version
  bump.
- **Frame-level retransmit.** The cluster transport assumes TLS
  ordering + integrity; for migration we currently lean on the
  ack to confirm delivery. A negative-ack retry path would let us
  recover from "ack lost, agent landed" failures without rolling
  back.

## Verification

```
cargo build --workspace
cargo test -p mty-runtime --test cluster_migration       # 9 tests
cargo test -p mty-runtime --test cluster_routing         # baseline still passes
cargo test -p mty-runtime --test cluster_supervisor      # baseline still passes
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

(See the slice's commit message for the full verification
transcript; v0.21 verification ran on a multi-agent shared tree
where other slices were also in flight.)
