# Multi-core scheduler (v0.6)

**Module:** `sdust_runtime::scheduler`
**Spec:** §25.4 (work-stealing per core)
**Amendments:** A101-A110

## Why

Slice-7 shipped a tokio multi-thread runtime as the scheduler. That
was correct but un-instrumented and made no spec-aligned guarantees
about per-core work-stealing or affinity. v0.6 builds a hand-rolled
N-worker pool on top of tokio's `current_thread` runtime + the
`crossbeam-deque` work-stealing primitives, with:

1. **One tokio runtime per worker**, sized to one OS thread.
2. **Per-worker LIFO deque** for best local cache locality.
3. **Cross-worker stealing in batches** to amortize sync overhead.
4. **Affinity hints** (sticky / elastic) for IO-bound vs CPU-bound.
5. **Periodic load monitor** that retargets elastic agents away from
   overloaded workers.
6. **Per-worker telemetry** (executed / stolen / parks / queue depth).

## Component layout

```text
                +-------------------- Scheduler ----------------------+
                |                                                     |
                |   driver runtime: Arc<tokio::Runtime (current_thread)>
                |   |                                                  |
                |   used by sdust-driver's `runtime.scheduler.rt       |
                |       .block_on(user_main_async)`                    |
                |                                                     |
                |   workers: Vec<Arc<WorkerHandle>>                   |
                |     [0] WorkerHandle { tokio: H, stealer, notify,   |
                |                        stats, shutdown, thread }    |
                |     [1] ...                                          |
                |     ...                                              |
                |                                                     |
                |   injector: Arc<Injector<SpawnTask>>                |
                |     global FIFO of tasks not yet pinned             |
                |                                                     |
                |   routes: RwLock<HashMap<agent_id, AgentRoute>>     |
                |     { worker: usize, affinity: Sticky|Elastic }     |
                +-----------------------------------------------------+

  Worker thread `i`:
    rt.block_on(worker_loop_async {
        loop {
            yield_now().await       // let spawned tasks run
            pop local / inj / steal-from-sibling
            on hit -> execute(task) which calls task.run(handle)
            on miss -> notify.notified() (50 ms timeout)
        }
    })
```

## Task lifecycle

1. `Runtime::spawn_agent_with_affinity(name, args, affinity)` builds
   the `AgentDescriptor` + mailbox.
2. `scheduler.assign_worker(affinity)` returns a worker index
   (round-robin elastic, fixed 0 sticky).
3. `scheduler.register_route(id, worker, affinity)` writes the
   routing entry.
4. `spawn_agent_loop(&rt, desc, worker)` calls
   `scheduler.handle_for(worker).spawn(agent_loop_future)` — the
   agent loop now lives on worker `i`'s current-thread tokio runtime.
5. The agent loop awaits `mailbox.recv().await`. mpsc wakers are
   tokio-aware and re-enter the runtime when a sender pushes.

## Cross-worker messaging

`Mailbox` wraps `tokio::sync::mpsc`, which is fully thread-safe and
cross-runtime safe. A `send` on worker A pushes onto a channel whose
receiver runs on worker B; B's runtime wakes the recv future and
processes the message. **No special routing logic required** — the
mpsc primitive handles inter-runtime wakeups via its own waker
infrastructure (which calls into tokio's Park/Unpark indirection).

## Why a separate driver runtime?

Because `current_thread` runtimes panic if a `block_on` is invoked
from a thread that's already driving the same runtime, and they
don't permit two threads to call `block_on` concurrently. Putting
the embedder's `block_on` (the slice-7 `runtime.scheduler.rt
.block_on(...)` pattern) on a *distinct* runtime keeps the API
unchanged while preserving worker isolation.

Cost: +1 OS thread for the driver runtime when the driver invokes
`block_on`. For long-running services this is essentially free.

## Why `current_thread` rather than `multi_thread` per worker?

A `multi_thread` runtime per worker would give us nested
work-stealing (tokio's + ours) on the same thread pool, which is
confusing and slow. `current_thread` per worker = one runtime = one
thread = predictable scheduling. The work-stealing happens at our
layer (across workers), and tokio's cooperative scheduling happens
within each worker (across spawned tasks).

## Affinity in v0.6

- **Sticky** — pinned to worker 0; never selected by the monitor for
  migration. The monitor's "find elastic agent on busiest" predicate
  filters them out. Useful for agents with non-portable host state.
- **Elastic** — round-robin assignment at spawn; subject to monitor
  retargeting. This is the default for `spawn_agent()`.

Front-end syntax is reserved for a later slice; v0.6 surfaces
affinity only through the `RuntimeBuilder::spawn_agent_with_affinity`
runtime API.

## Load monitor — what migration actually does

Lossless live migration (move an agent's tokio task from runtime A to
runtime B while preserving buffered mailbox state and in-flight turn
state) is non-trivial — it requires either:

- Pausing the agent loop, copying its waker registrations to the new
  runtime (tokio doesn't expose this), or
- Killing+respawning the loop after draining the in-flight turn.

v0.6 ships **lightweight migration**: the monitor only updates the
routing table. The *next* spawn of that agent (e.g. after a
supervisor restart) lands on the lighter worker. Existing in-flight
loops continue where they were. This is enough to lay the foundation
for honest perf claims because:

- New agent spawns *are* balanced across workers.
- Agents that crash + restart automatically get redistributed.
- Long-lived agents stay on their first worker, which is acceptable
  for v0.6 (and avoids the live-migration correctness pitfalls).

Lossless live migration is tracked as v0.7+.

## Telemetry

Every `Scheduler::stats()` returns `Vec<(usize, WorkerStatsSnapshot)>`:

```rust
struct WorkerStatsSnapshot {
    pub tasks_executed:      u64,
    pub tasks_stolen:        u64,
    pub parks:               u64,
    pub current_queue_depth: usize,
}
```

These are wired into the OTLP exporter (slice-7 infrastructure) as
gauges: `sdust.scheduler.worker.<id>.<metric>`. The exporter side is
a follow-on PR in the OTLP swarm; the snapshot API is already
available for embedders.

## What v0.6 does NOT do

- **No lossless live migration** — see "Load monitor" above.
- **No front-end affinity syntax** — parsing the `with affinity =
  sticky` clause requires `sdust-syntax`/`sdust-ast` changes that are
  out of scope for the scheduler agent.
- **No NUMA awareness** — workers are scheduled by the OS.
- **No per-core CPU pinning** — workers run as ordinary OS threads.
- **No perf benchmarks** — the `sdust-bench` crate is owned by the
  bench swarm; v0.6 just provides the scheduler the benches will
  measure.

## Test surface

- `crates/sdust-runtime/src/scheduler.rs` — unit tests for the
  primitive (`mod tests` at bottom).
- `crates/sdust-runtime/tests/worker_steal.rs` — stealing balances
  load across 4 workers.
- `crates/sdust-runtime/tests/cross_worker_send.rs` — agents pinned
  to different workers exchange messages correctly.
- `crates/sdust-runtime/tests/affinity_sticky.rs` — sticky agent
  pins to worker 0 and is filtered from migration suggestions.
- `crates/sdust-runtime/tests/load_balance.rs` — synthetic skew
  triggers a migration suggestion.
- `crates/sdust-runtime/tests/deterministic_mode.rs` — `.workers(1)
  .deterministic(seed)` reproduces v0.5 behavior.
- `crates/sdust-runtime/tests/multicore_fifo.rs` — companion to
  conformance case 06.
- `crates/sdust-runtime/tests/multicore_throughput_smoke.rs` —
  companion to conformance case 07.

## See also

- `docs/internals/scheduler.md`
- `docs/spec/v0.1-amendments.md` — A101 onwards
- `SCHEDULER_V0_6_NOTES.md` — interpretation calls + open work
