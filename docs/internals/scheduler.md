# Scheduler

**Module:** `sdust_runtime::scheduler`
**Spec:** §25.4 Scheduling

## v0.6: multi-worker work-stealing

v0.6 replaces the slice-7 single-tokio-runtime model with **N worker
threads + 1 driver runtime**. Each worker owns:

- A tokio `current_thread` runtime.
- A `crossbeam_deque::Worker<SpawnTask>` (LIFO local queue) +
  `Stealer` exposed to siblings.
- A `tokio::sync::Notify` for cross-worker async wake.
- Atomic counters for tasks_executed / tasks_stolen / parks /
  current_queue_depth.

A separate `Scheduler::rt` (also `current_thread`) is the **driver
runtime** — used only by `Runtime::block_on(user_main)`. This keeps
slice-7 callers (`runtime.scheduler.rt.block_on(...)`) working
unchanged without sharing a runtime instance between the embedder and
the worker pool (which would deadlock).

### Worker loop

```text
loop {
  if shutdown { break }
  yield_now().await                 // let spawned tasks run
  if let Some(t) = local.pop() { execute(t); continue }
  if let Steal::Success(t) = injector.steal_batch_and_pop(&local) {
    stats.stolen += 1; execute(t); continue
  }
  for stealer in siblings (random rotation) {
    if let Steal::Success(t) = stealer.steal_batch_and_pop(&local) {
      stats.stolen += 1; execute(t); continue
    }
  }
  stats.parks += 1
  timeout(50ms, notify.notified()).await    // park (async)
}
```

`execute(task)` calls the task's `run(handle)` closure with the
worker's tokio handle so the closure can `handle.spawn(...)` its
future. The future then lives on the worker's runtime alongside the
work-stealing loop.

### Affinity

`Affinity` is parsed (best-effort, runtime-only for v0.6) as either:

- `Sticky` — pin to worker 0 at spawn, never migrate. Use for
  agents that own non-portable host resources (sockets, file handles).
- `Elastic` (default) — round-robin assignment at spawn; may be
  migrated by the load monitor when imbalance crosses the threshold.

Front-end syntax (`agent X(...): Y with affinity = sticky`) is
reserved for a later slice; v0.6 only exposes the runtime API
(`RuntimeBuilder::spawn_agent_with_affinity`).

### Load monitor

`LoadMonitor` runs on a dedicated OS thread. Every `interval`
(default 100 ms) it samples per-worker `current_queue_depth` and
emits a migration suggestion when:

```
busiest.depth > threshold * max(lightest.depth, 1)
  AND busiest.depth >= threshold
```

(default `threshold = 4`). The suggestion picks the first elastic
agent routed to `busiest` and asks `Scheduler::update_route_worker`
to retarget future spawns of that agent to `lightest`. The
in-flight mailbox loop is **not** killed mid-recv — that would lose
buffered messages — so v0.6's migration is "future-spawn retargeting"
rather than live thread-of-control hand-off. Lossless live migration
is v0.7+ scope.

### Deterministic mode

`RuntimeBuilder::deterministic(seed)` builds a single-worker
scheduler with no load monitor. This is byte-identical to v0.5
behavior. `.workers(1)` without `.deterministic(_)` is also valid
but does not disable the (single-worker, hence dormant) monitor
slot.

## Spec §25.4 alignment

| Spec property                | v0.6 form |
|------------------------------|-----------|
| Work-stealing per core       | crossbeam-deque per-worker LIFO + sibling stealers (batch 16) |
| Cooperative cancellation     | `tokio::select!` over a per-turn `CancellationToken` |
| Deadline-aware polling       | `with_deadline(d, fut)` wraps reply oneshots |
| Backpressure on mailboxes    | bounded MPSC + `SendPolicy::Block` (default) |
| Task-local arenas            | per-turn arena push/pop in SIR interp |
| Agent turn fairness          | each agent is one tokio task; yields between turns |
| Multi-core throughput        | **N worker threads, default `available_parallelism()`** |

## Slice-7 compat

The `Scheduler::rt: Arc<TokioRt>` field is preserved. Slice-7 callers
who do `runtime.scheduler.rt.clone().block_on(future)` still work —
the field now points at a dedicated driver runtime rather than the
single worker runtime. Same external behavior; new internal layout.

## Where this fits

- `RuntimeBuilder::workers(n)` controls worker count
  (default = `std::thread::available_parallelism()` or
  `STARDUST_RUNTIME_THREADS=N`).
- `RuntimeBuilder::threads(n)` is a slice-7 alias of `.workers(n)`.
- `Runtime::start_monitor()` spawns the load-monitor OS thread.

## See also

- `docs/internals/multi-core.md` — deeper notes on the v0.6 layout.
- `docs/internals/runtime.md`
- `docs/spec/v0.1-amendments.md` — A39, A41, A101-A110
