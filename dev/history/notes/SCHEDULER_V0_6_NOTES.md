# SCHEDULER_V0_6_NOTES.md

Interpretation calls + open work for the v0.6 multi-core scheduler.
Agent: scheduler-swarm. Branch: main. Owner files:

- `crates/mty-runtime/src/scheduler.rs` (rewritten)
- `crates/mty-runtime/src/runtime.rs` (updated for worker-aware spawn)
- `crates/mty-runtime/src/lib.rs` (exports)
- `crates/mty-runtime/Cargo.toml` (added crossbeam deps)
- `crates/mty-runtime/tests/{worker_steal,cross_worker_send,affinity_sticky,load_balance,deterministic_mode,multicore_fifo,multicore_throughput_smoke}.rs`
- `tests/conformance/mailbox_ordering/0{6,7}_multicore_*`
- `docs/internals/scheduler.md` (rewritten)
- `docs/internals/multi-core.md` (new)
- `docs/spec/v0.1-amendments.md` (A101-A106)

## Interpretation calls

### IC-1: Driver runtime vs worker runtimes

The slice-7 API was `runtime.scheduler.rt.clone().block_on(user_main)`
where `rt: Arc<TokioRt>` was the single worker runtime. With N
workers each driving their own current-thread tokio runtime, having
the embedder share one of those runtimes for `block_on` would either:

a) Panic ("Cannot start a runtime from within a runtime") if the
   worker is already in `block_on(worker_loop)`, or
b) Race for the runtime's driver lock.

**Call**: I added a separate driver runtime (`Scheduler::rt`) that's
distinct from all worker runtimes. The driver runtime exists only to
host the embedder's `block_on(user_main)` async block — calls inside
that block to `runtime.spawn_agent` route work onto worker runtimes
via `Handle::spawn`. This adds +1 OS thread when the driver runtime
is actively block_on'd, which is negligible vs the worker overhead.

### IC-2: Crossbeam-deque task granularity

The crossbeam-deque API expects "tasks" — units of work the worker
pops + executes. The natural unit in Mighty is "the per-turn
execution of one mailbox message". But the existing slice-7 design
has each agent as a long-running tokio task that loops on
`mailbox.recv()`. Refactoring to push per-turn work units onto a
crossbeam deque per worker would require gutting the existing async
infrastructure.

**Call**: For v0.6, the `SpawnTask` is an "agent activation" — the
initial spawn of an agent's loop, or a migration re-spawn. The
work-stealing applies to *spawn distribution* + *migration re-spawn*,
not per-message execution. Per-message execution still happens inside
each worker's tokio runtime via the existing agent loop. This is a
pragmatic stepping stone: it lays the crossbeam-deque + per-worker
runtime + telemetry foundation, while leaving per-message
work-stealing as a v0.7+ enhancement.

This matches the "If true work-stealing turns out to need invasive
tokio-runtime surgery, ship a simpler N-workers + round-robin assign
model with a clear v0.7 follow-on note" fallback in the scope.

### IC-3: Migration is non-lossless in v0.6

Live lossless migration would require either:

- Pausing the agent loop, copying tokio waker registrations from
  runtime A to runtime B (no public tokio API for this), or
- Draining the in-flight turn, killing the loop, respawning on B —
  but messages buffered in the mailbox between drain and respawn
  would be lost.

**Call**: The monitor's "migration" is **routing-table retargeting**:
the next time the agent is spawned (e.g. supervisor restart), it
lands on the lighter worker. Existing in-flight loops are not
disturbed. This is documented in A103 and in `multi-core.md`. The
v0.7 follow-on is "lossless live migration".

### IC-4: Telemetry exporter wiring

The slice-7 OTLP exporter (`otlp.rs`) handles `TelemetryEvent`s as
discrete events. Per-worker queue-depth / executed / stolen / parks
are *gauges*, not events. Wiring them into the OTLP exporter requires
either:

- A periodic scrape from the OTLP exporter side (would have to import
  the scheduler), or
- Pushing gauge updates from the scheduler into the telemetry sink.

**Call**: v0.6 exposes `Scheduler::stats()` as the read API. Wiring
it into the OTLP exporter is a follow-on PR; the data is available
for embedders that want to plumb it themselves today. Adding a new
event variant would be a slice-7 telemetry crate change (outside
scheduler agent's owned files).

### IC-5: Front-end affinity syntax

Parsing `agent X(...): Y with affinity = sticky` requires changes in
`mty-syntax`, `mty-ast`, `mty-hir`. Those are outside the
scheduler agent's owned files.

**Call**: Expose only the runtime API
(`Runtime::spawn_agent_with_affinity`). Embedders + future codegen
slices can route the parsed syntax there once the front-end lands.
Documented in A102.

### IC-6: Workers > 1 is the new default

A39 set `threads = 1` by default for slice-7 determinism. v0.6 ships
multi-worker as default (`available_parallelism()`); deterministic
mode still forces single-worker. Existing examples + CLI scripts
that didn't set `STARDUST_RUNTIME_THREADS` will now use all cores by
default. This is a **behavior change** but matches the v0.6 spec
intent (honest perf claims requires honest multi-core use).

Documented in A106. Any test that depends on deterministic ordering
should switch to `.deterministic(seed)` or `.workers(1)`.

## Open work (v0.7+)

1. **Lossless live migration** — move an agent's tokio task between
   runtimes without losing buffered messages. Likely needs tokio
   internals changes or a per-agent intermediate buffer.
2. **Per-message work-stealing** — push individual mailbox messages
   onto the crossbeam deques instead of just spawn-activations.
   Would convert the agent loop from "one tokio task" to "one
   work-stealing job per turn", giving real per-message parallelism
   across cores.
3. **Front-end affinity syntax parse + lower** — mty-syntax/ast
   changes to recognise `with affinity = sticky|elastic` and pass
   it through HIR → MtyIR → runtime.
4. **OTLP gauge wiring for WorkerStats** — wire `Scheduler::stats()`
   into the telemetry sink so the gauges flow to OTLP collectors
   automatically.
5. **NUMA awareness** — pin worker threads to cores or NUMA nodes
   for predictable cache behavior.
6. **Smarter steal strategy** — replace random rotation with a
   nearest-neighbor topology when worker count >= 16.
7. **Conformance harness for runtime-level cases** — the existing
   `conformance_full.rs` runs through the MtyIR interp; add a parallel
   harness that drives `mailbox_ordering/06*`/`07*` through the
   multi-worker runtime + diffs stdout the same way.

## Perf note

The scheduler agent does not benchmark — that's the bench swarm's
scope. v0.6 should give the benches enough surface to measure
honest multi-core throughput. Expect:

- `workers=1` ≈ v0.5 baseline (single tokio current-thread).
- `workers=4` should show ~3x throughput on a 4-core box for
  cross-agent fan-out workloads.
- `workers=4` with high-affinity send-to-same-agent traffic should
  match `workers=1` (because the agent is pinned to one worker).
  This is the v0.7 "per-message work-stealing" follow-on opportunity.

## Build / test caveat

At time of writing the `mty-bench` workspace member listed in
`Cargo.toml` (commit `ef031d2`) had not yet been created on disk by
the bench swarm agent. `cargo build -p mty-runtime` therefore
fails until the bench-swarm creates `crates/mty-bench/Cargo.toml`.
This is a sync-point coordination issue between swarm agents, not a
defect in the scheduler implementation itself. Once `mty-bench`
exists on disk, all the new tests + the existing 76 baseline should
pass.
