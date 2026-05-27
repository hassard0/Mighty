# Agent feature roadmap

The five v1.0 agent primitives — agents, mailboxes, supervisors,
protocols, budgets — are sufficient for the kinds of programs the
canonical examples and demos exercise. The features below are the
next layer: instruments and integrations that make a Mighty agent
system production-grade.

This document is a forward-looking plan, not a spec. Each item is
sized roughly so the v0.16+ integrator can pull from it without
re-doing the design work.

## Tier 1 — production observability + reload (v0.16 candidates)

### `agent.introspect()` + `mty inspect <pid>` CLI

Runtime exposes a snapshot view of any live agent:

- agent name, type, supervisor parent
- mailbox depth (current + high-water)
- in-flight handler (if any) + elapsed wall time
- budget state (memory used, ticks used, deadline)
- last N messages handled (ring buffer; opt-in to capture bodies)

Wire shape: a `__inspect` system message every agent answers
without going through the handler dispatch. CLI: `mty inspect
<sock-or-pid>` connects to the runtime's local control socket (new
opt-in `RUNTIME_CONTROL_SOCK` env var) and pretty-prints the
snapshot.

Stretch: a `mty top` mode that polls every 1s and shows the
worker-by-worker schedule.

### OpenTelemetry agent spans

`opentelemetry-otlp` is already a workspace dep. Auto-instrument:

- `spawn` — span name `agent.spawn`, attribute `agent.type`
- `send` — link-only span (one-way); attribute `protocol.msg`
- `ask` — full span; child of caller; closed when reply arrives
- `supervise.restart` — event on the supervisor's span
- `budget.exhausted` — event with reason

Off by default. Enable via `MTY_OTLP_ENDPOINT=...` env or
`[telemetry]` block in `mighty.toml`. The handler-span context is
threaded through the runtime's task-local store so user code can
add child spans.

### Live diagnostic logs

Every agent already has access to `log()`. Add structured
counterpart: `agent.event(name: &str, fields: &[(&str, &str)])`
that emits a typed event into the OTEL pipeline (or stdout when no
OTLP). Tested against `wasi:logging@0.2.x` shape so it round-trips
through the Component Model unchanged.

## Tier 2 — debugger-grade recording (v0.17 candidates)

### Deterministic replay

Mighty already supports a deterministic-execution mode. Extend it
to record:

- every external IO event (file reads, network reads, clock reads,
  random reads — anything from the `wasi:*` import surface)
- every message exchanged on every mailbox

Output: a `mty-trace-<timestamp>.bin` file (compact wire format,
likely cbor or postcard). Re-run with `mty replay
trace-<timestamp>.bin` produces byte-identical output. Backed
agent-by-agent so a sub-graph can be re-played in isolation for
debugging.

### Step debugger over the trace

`mty debug trace.bin` enters a REPL:

- `step` advances one message dispatch
- `peek <agent>` shows current state
- `print msg` shows the in-flight message
- `bt` shows the call chain (supervisor → spawn → handler)

This piggybacks on the existing DWARF source maps so the user sees
Mighty-source-level positions, not MtyIR.

## Tier 3 — hot reload (v0.17 / v0.18 candidates)

### State-preserving code swap

`mty reload <agent-type> --from new.wasm`:

1. Drain the agent's current handler to completion (deadline
   enforced)
2. Serialise the agent state via a derived `Resumable` trait (new
   in stdlib)
3. Stop the agent
4. Load the new wasm module, spawn an agent of the same type
5. Deserialise the state into the new agent
6. Reattach the mailbox; resume

Constraint: the `Resumable` trait derive checks that the state
shape is *forward-compatible* — fields can be added or removed but
not renamed in conflicting ways. The runtime refuses the swap if
the new version's `Resumable::SCHEMA_HASH` is incompatible with
the recorded snapshot's hash.

This unblocks long-running services (a backend server can update
its handlers without dropping connections, because connection
sockets live in the supervisor's external state, not the agent's).

## Tier 4 — distributed agents (v0.19+)

### Single-cluster mesh

- Agent addresses become `node:type:pid` instead of `type:pid`
- `send` / `ask` cross node boundaries transparently
- Wire protocol: framed cbor over TLS (rustls already in tree)
- Initial topology: static peer list in `mighty.toml`
  `[cluster.peers]`

### Cluster supervisor

A supervisor can have children on remote nodes. The supervisor
strategy (one-for-one, etc.) applies across the cluster. Node
failure marks all of that node's children as `:noproc`, triggering
the supervisor's restart policy.

### Lossless live migration

(Already on the README's Post-v1.0 roadmap.) Building on Tier 3
(state-preserving reload) + Tier 4 (cluster), an agent can move
between nodes while preserving its mailbox and continuation. A
`migrate(node)` call drains, snapshots, ships the snapshot over
the cluster wire, restores on the target.

## Tier 5 — agent composition primitives (any v0.16+)

### `pipe!` macro for handler chains

`pipe!(A, B, C)` wires `A → B → C` so each agent's response is
fed to the next. Backpressure built in: if `B`'s mailbox is full,
`A` blocks (or drops, configurable). Simpler than hand-wiring
supervisors for ETL-style workloads.

### `broadcast!` macro for fan-out

`broadcast!(msg, [a1, a2, a3])` sends `msg` to all listed
mailboxes in parallel. Returns a `JoinHandle[Vec[Reply]]` for
gather.

### `circuit_breaker` agent

Supervisor variant that opens a breaker after N failures in a
sliding window. While open, all `send` to its children fail-fast
with `:circuit_open`. After a cooldown, half-open probes one
message; on success closes the breaker.

---

## What ships when

| Tier | Items | Status |
|------|-------|--------|
| 1    | introspect, OTel spans, agent events | **SHIPPED v0.16** |
| 2    | record + replay + step debugger | **SHIPPED v0.17 → v0.19** (byte-identical replay end-to-end) |
| 3    | hot reload + Resumable trait | **SHIPPED v0.17 → v0.21** (wasm-bytes swap + schema migrations + condvar drain + control-socket `op=reload` all live) |
| 4    | single-cluster mesh | **SHIPPED v0.18 → v0.19** (cross-node send/ask, framed CBOR-over-TLS) |
| 4    | cluster supervisor | **SHIPPED v0.20** (OneForOne/RestForOne/OneForAll + per-child circuit breaker) |
| 4    | lossless live migration | **SHIPPED v0.21** (RFC-006: MigrationOrchestrator + 3 placement policies + OTel cluster metrics) |
| 5    | per-message work-stealing | **SHIPPED v0.22** (crossbeam-deque per-worker queues + NUMA-locality steal ordering + `worker.steals_total{src,dst}` OTel counter) |
| 5    | pipe/broadcast/circuit-breaker | drop-in helpers — not roadmap blockers |
| post-v1.0 | `std.eval` replay-driven LLM eval harness | **SHIPPED v0.28 Track G** (`Suite`/`Case`/`Member`/`Compare` builder on top of the v0.21 replay machinery; three comparators — `Equal`, `SemanticSimilarity`, `ToolCallSetEqual` — with per-cell verdicts + divergence reporting. See `docs/internals/std-eval.md`.) |

**Every Tier in the roadmap has now landed pre-v1.0.** What
remains for v1.0 GA is RFC comment-window disposition collection
(blocker #2 in the README Roadmap section), the polish items
tracked in the repo-level `CHANGELOG.md`'s `[Unreleased]` block
(BOLT post-link optimisation, multi-socket NUMA benchmark,
`mty conform` CLI shim, v1.0-RC validation sweep, MT3012 closure
pending HIR `CONST_DECL` lowering), and Tier-5 pipe/broadcast/
circuit-breaker helpers which are drop-in additions rather than
roadmap blockers.

## Open questions

- **Wire stability.** Once the introspection wire format ships,
  the `mty inspect` client / server contract becomes a
  compatibility surface. Version it from day one (a `version: u32`
  field on every introspection reply).

- **Privacy.** The introspection snapshot can carry mailbox bodies
  (per opt-in). When OTel is also enabled, the spans can include
  message attributes. Document the capability boundary: a process
  with the `introspect` capability sees everything; without it,
  only public counters.

- **Wasm Component Model implications.** Components run in
  isolation by default. Cross-component agent communication
  through `ask` should lower to a typed interface in the component
  world. This is a follow-up to the v0.13/v0.14 WASI Preview 2
  work — likely needs an RFC of its own.
