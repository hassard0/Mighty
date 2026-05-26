# RFC-006 — Lossless live agent migration

**Status:** **IMPLEMENTED in v0.21** (Tier 4.3, agent-features roadmap).
See [`docs/internals/cluster.md#live-migration`](../../internals/cluster.md#live-migration)
for the as-built architecture and `crates/mty-runtime/src/cluster/migration.rs`
for the implementation. The wire layer ships three new variants —
`MigrateSnapshot`, `MigrateAck`, `MigrateError` — atop the v0.18 cluster
transport.
**Tracks amendment:** A103 (lightweight migration shipped in v0.6;
routing-table-only, lossless live migration deferred).
**Target release:** v0.21 (shipped).
**Owner:** v0.21 Tier 4.3 implementation slice.

## Implementation Status

**SHIPPED in v0.21 (Tier 4.3).** Cross-reference with
[`RFC_DASHBOARD.md`](RFC_DASHBOARD.md). Concrete artefacts:

* `MigrationOrchestrator::migrate_agent(agent, target, deadline)` —
  ships an agent's snapshot + queued mailbox + continuation between
  cluster nodes.
* `SnapshotSource` / `SnapshotSink` / mesh wire hooks — runtime
  abstractions so the orchestrator doesn't bake in a single transport.
* 6 MB hard cap on snapshot payloads.
* `PlacementPolicy` trait + 3 bundled policies
  (`StickyPolicy` / `LeastLoadedPolicy` / `StaticPolicy`) consumed by
  `RestartRequested` placement hints; matches the RFC's "policy
  pluggability" requirement.
* `[cluster.placement]` manifest block — declarative placement-hint
  surface, parsed by `mty-pkg`.
* OTel cluster metrics for migration in-flight / completed / failed.
* New `MT507x` diagnostic band (cluster / migration failures); see
  the `mty explain` catalog.

Foundation work shipped earlier:

* **v0.18** — Cluster transport (`AgentAddr = node:type:pid` +
  framed CBOR-over-TLS) and `SharedRouter`.
* **v0.19** — Cross-node send + ask routing, peer-disconnect fan-out
  to in-flight asks (`MT5032`).
* **v0.20** — mTLS via `ClusterMesh::from_config_mtls`; Tier 4.2
  `ClusterSupervisor` (`OneForOne` / `RestForOne` / `OneForAll`) +
  per-child circuit breaker.

The public comment window opened 2026-05-26 closes the **process**;
the design has shipped. Expected disposition is **accept**, ratifying
the shipped semantics; reviewer-surfaced refinements would land as
point patches.

## Summary

Promote the v0.6 **lightweight migration** model (the monitor updates
the routing table so the next spawn of an agent lands on a lighter
worker; existing loops continue on their original worker) to
**lossless live migration**: an in-flight agent's mailbox + state +
pending replies are moved from one worker runtime to another without
dropping messages, with at-most-once delivery guarantees preserved
across the migration boundary.

## Motivation

The v0.6 stopgap is sufficient for steady-state load balancing but
misses three real workloads:

1. **Drain-for-shutdown.** A worker scheduled to terminate (host
   doing a rolling restart, k8s SIGTERM, etc.) can't drain its
   in-flight agents to siblings — the agents continue on the
   draining worker until they exit, blocking shutdown.
2. **Hot-spot relief.** A single worker hosting a runaway-load agent
   can't shed it mid-execution; the load monitor sees the imbalance
   but can only redirect *new* spawns. The hot agent stays hot.
3. **Live upgrades.** A v1.x → v1.x+1 runtime upgrade that drops a
   worker can't migrate the resident agents over without losing
   in-flight messages.

Lossless migration closes all three.

## Detailed design

> **Note.** This section is a first-draft sketch. The runtime
> primitives required (tokio task waker-set re-binding, mailbox
> snapshot + restore, cancellation-token transfer) are non-trivial;
> the v0.7+ scoping for A103 reflected that. Treat this section as a
> design *direction*, not a final blueprint. A design owner should
> harden it before v1.1-alpha.

### Migration protocol

A migration is initiated by the scheduler's `LoadMonitor` (existing
v0.6 component). Three phases:

1. **Quiesce.** The source worker:
   - Pauses delivery of new mailbox frames to the agent (the mpsc
     receiver yields no new items).
   - Drains the agent's *currently executing* turn to completion (it
     is bounded by the per-turn budget — A70).
   - Captures the agent's `AgentState` snapshot via a new
     `Agent::snapshot(&self) -> Bytes` method that every agent type
     must implement (auto-derived via `#[derive(Migratable)]`).
   - Captures the mailbox's pending-frame queue as a `Vec<MessageFrame>`.
   - Captures the pending-reply oneshot table as a `Vec<(AskId,
     OneshotSender)>`.

2. **Transfer.** The destination worker:
   - Receives the `(snapshot, pending_frames, pending_replies,
     spawn_args)` tuple via a high-priority channel.
   - Reconstructs the agent via a new `Agent::restore(&snapshot, &spawn_args)
     -> Self` method.
   - Re-creates the mailbox and pushes pending frames in original
     order (mpsc preserves FIFO).
   - Re-binds the pending-reply oneshots by wrapping each `OneshotSender`
     in a `MigratedSender` proxy that the source worker holds onto.
     When the destination's restored agent produces the reply, it
     calls into the proxy, which crosses the worker boundary and
     fires the original sender.

3. **Cutover.** The routing table is updated atomically: senders that
   look up the agent's `AgentId` after the cutover route to the
   destination worker. Pre-cutover sends already in flight reach the
   source worker; the source worker forwards them via a one-shot
   "stale routing" path.

### Loss-free guarantees

- **No dropped messages.** Pending frames in the source mailbox are
  transferred; new frames sent after cutover land on the destination;
  in-flight frames during cutover are forwarded.
- **At-most-once delivery.** The mailbox's per-frame `seq` counter is
  preserved; the destination's mailbox replay skips any frame whose
  `seq` is already in the agent's processed-set (a Bloom filter
  derived from `snapshot.processed_seqs`).
- **Replies arrive exactly once.** The `MigratedSender` proxy
  guarantees that the destination's reply reaches the original
  oneshot exactly once — even if a network blip caused the proxy to
  retry, the source-side oneshot is single-fire.

### Snapshot format

The `Agent::snapshot` format is bincode-encoded by default but the
trait allows custom encoders. Snapshot version is `(crate_version,
agent_type_hash)`; mismatch on restore → `MT5022 migration_version_mismatch`,
caller-controlled retry policy (typically: don't migrate; fall back
to v0.6 lightweight migration).

Sendability already constrains agent fields (A65.b), so snapshot
serialisation is well-defined: every Sendable field is bincode-
encodable.

### Auto-derive

```mty
#[derive(Migratable)]
agent Worker(state) {
  state: WorkerState,
  on Job(j) { ... }
}
```

`#[derive(Migratable)]` (a new derive macro in the v0.5 set) emits
`Agent::snapshot` + `Agent::restore` methods plus a `MIGRATABLE: bool
= true` const. Manual implementations are allowed for agents with
non-trivial state (e.g. holding a file descriptor that can't cross
worker boundaries — those agents must opt out via `#[derive(NotMigratable)]`
and the scheduler will fall back to v0.6 routing-table-only).

### Constraints

- **The source worker must not be terminated** until the destination
  acknowledges receipt of the migration tuple. The shutdown path
  serialises this via a `MigrationAck` channel.
- **In-flight extern FFI calls** can't migrate (`libloading` handles
  are worker-local). An agent in the middle of an `extern { fn ... }`
  call defers migration until the call returns. The monitor's
  cancellation-token check (A70) gates this.
- **Capability handles** (FsCap, NetCap) are Copy (RFC-004) and
  cross worker boundaries trivially.

## Drawbacks

- **Snapshot cost.** Snapshotting every agent's state on every
  migration is non-trivial; pathological cases (large `Vec`-of-bytes
  state) could exceed migration time budgets. Mitigation:
  `MIGRATION_MAX_STATE_BYTES = 16 MiB` default cap; oversized agents
  fall back to v0.6 routing-only.
- **Implementation complexity.** Three new runtime subsystems
  (snapshot, proxy senders, atomic routing cutover) and one new
  derive macro. Each individually small; together a significant
  surface.
- **Determinism break.** A migrated agent's execution order may
  differ from a non-migrated baseline (the destination worker's
  scheduling may interleave differently). Deterministic mode (A39)
  forbids migration entirely; otherwise the host accepts the
  determinism break in exchange for the load-balancing win.

## Alternatives considered

1. **Keep v0.6 lightweight only.** Defeats motivation.
2. **Migrate via process restart.** Spawn the agent fresh on the
   destination, lose in-flight state, rely on at-least-once retry.
   Loses exactly-once and breaks supervisor semantics.
3. **Tokio runtime "evac" API.** Upstream tokio has discussed a
   per-task migration primitive; not yet shipped, no timeline.
   Reconsider if upstream lands it; track as a v1.2+ revisit.
4. **Erlang-style hot code load + migrate.** Different model
   (modules, not tasks); too far from Mighty's runtime shape.

## Unresolved questions

> **DRAFT — design owner needed.** The following questions require
> deeper runtime expertise before v1.1-alpha:

- Exact mechanism for proxy sender (Arc / channel / unsafe pointer
  bridge?).
- Atomic routing cutover in the presence of concurrent send-arms
  reading the routing table — RCU-style or per-slot RwLock?
- Snapshot ordering with respect to mailbox draining — should we
  drain first and then snapshot, or snapshot mid-drain with a
  consistent boundary?
- How does this interact with sandboxed proc-macro execution
  (RFC-003)? Proc-macro sub-interpreters don't have agents, so the
  answer is "they don't"; confirm.
- What's the API surface for hosts that want to *trigger* a migration
  (e.g. "drain worker 3 now")? Reservation:
  `Scheduler::drain_worker(usize) -> impl Future<Output = ()>`.

## Adoption plan

1. **v1.1-alpha.1:** `#[derive(Migratable)]` shipped; snapshot/restore
   tested in isolation (no live migration yet); A103 stays OPEN.
2. **v1.1-alpha.2:** proxy sender + routing cutover land behind a
   `--experimental-migration` flag; the load monitor invokes them
   under sustained imbalance.
3. **v1.1-beta:** `Scheduler::drain_worker` API ships; rolling-restart
   integration tests pass.
4. **v1.1.0:** flag flips for the `drain_worker` path; load-monitor
   auto-migration remains opt-in under `mighty.toml [scheduler]
   auto_migration = true`.
5. **v1.2:** auto-migration default-on if no v1.1 regressions are
   reported.

**OR** — if the runtime primitives prove harder than the alpha cycle
can absorb — slip the entire RFC to v1.2 and let v1.1 ship the
`#[derive(Migratable)]` surface (parse + store + snapshot-only) as a
soft-launch. The compiler-side work is decoupled from the runtime-
side work and can land first without forcing the runtime to catch up.

A 60-day public comment window opens with v1.1-alpha.1.
