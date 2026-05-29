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
| Task-local arenas            | per-turn arena push/pop in MtyIR interp |
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
  `MTY_RUNTIME_THREADS=N`, or the legacy `STARDUST_RUNTIME_THREADS`).
- `RuntimeBuilder::threads(n)` is a slice-7 alias of `.workers(n)`.
- `Runtime::start_monitor()` spawns the load-monitor OS thread.

## v0.22: per-message work-stealing (Tier 5)

The v0.6 scheduler already used `crossbeam-deque` deques + sibling
stealers. v0.22 closes the Tier 5 item from
`docs/internals/agent-features-roadmap.md` by:

1. **Splitting the scheduler into a module** —
   `crates/mty-runtime/src/scheduler/` now contains:
   - `mod.rs` — `Scheduler`, `Affinity`, `LoadMonitor`, routing table.
   - `work_stealing.rs` — `launch_pool`, the `worker_loop_async` body,
     per-worker stats.
   - `locality.rs` — `Topology`, `WorkerLocality`, `build_steal_order`.
   The public re-exports (`mty_runtime::scheduler::{Scheduler, Affinity,
   LoadMonitor, SpawnTask, WorkerStatsSnapshot}`) are unchanged, so no
   callers need an update.
2. **NUMA-aware steal order** — each worker probes siblings in
   locality order:
   ```text
   Tier 1: same NUMA node + same socket
   Tier 2: same socket,    different node
   Tier 3: anywhere
   ```
   The order is computed once at scheduler construction from a
   `Topology` detected by parsing
   `/sys/devices/system/cpu/cpu*/topology/physical_package_id` and
   `/sys/devices/system/node/node*/cpulist` on Linux. On Windows or
   any system without `/sys` we fall back to a **flat topology**
   (every worker on node 0 / socket 0) and the order degenerates to a
   plain rotation — identical to v0.21 behaviour.
3. **`worker.steals_total{src,dst}` counter** — every successful
   steal increments
   `mty_runtime::telemetry::sink::WORKER_STEAL_COUNTER` for the
   appropriate `(src, dst)` pair. `src = usize::MAX` is the sentinel
   for "stolen from the global injector"; any other value is a
   sibling worker id. Read it via
   [`telemetry::steal_counter_snapshot`] (returns a
   `Vec<(usize, usize, u64)>`) or
   [`telemetry::steal_counter_total`] (sum across all pairs).

### Work-stealing loop (v0.22)

```text
loop {
  if shutdown { break }
  yield_now().await                          // let spawned tasks run
  if let Some(t) = local.pop() { execute(t); continue }   // (1)
  for sibling in steal_order {                              // (2)
    match stealers[sibling].steal_batch_and_pop(&local) {
      Success(t) => { counter[src=sibling,dst=self]++;
                      stats.tasks_stolen++; execute(t);
                      continue }
      Retry      => retry_needed = true                     // (3)
      Empty      => {}
    }
  }
  if got_work    { continue }
  if retry_needed { yield_now().await; continue }
  match injector.steal_batch_and_pop(&local) {              // (4)
    Success(t) => { counter[src=GLOBAL_INJECTOR,dst=self]++;
                    stats.tasks_stolen++; execute(t);
                    continue }
    ...
  }
  stats.parks++; timeout(50ms, notify.notified()).await    // (5)
}
```

The crucial change from v0.21 is that **siblings are probed before
the global injector**. v0.21 went `local → injector → siblings`,
which stranded cache-warm sibling work whenever the injector held a
long stream of incoming tasks. v0.22 reverses the order so
already-pinned work redistributes first.

### Counter semantics

`worker.steals_total{src=N, dst=M}` is a **monotonically-increasing
counter** (typed for an OTel `Counter` export). One increment per
successful steal:

| src                    | dst       | meaning                                      |
|------------------------|-----------|----------------------------------------------|
| 0..n_workers           | worker id | stolen from sibling `src`                    |
| `usize::MAX` (sentinel)| worker id | stolen from the global injector              |

The counter is shared across **all schedulers** in the process
(static `OnceLock<Mutex<HashMap>>`) because the OTel meter provider
in real deployments is process-wide. Tests that need isolation can
read the baseline at start, do their work, and assert on the delta —
see `tests/work_stealing.rs::counter_increments_on_steal`.

### v0.23 follow-ups

- **Tickless / steady-state mode**: when every worker has been
  parked for >100 ms, skip the 50 ms wake timeout and rely purely on
  `notify_one()`. Cuts idle wakeups to ~0 for batch-style workloads
  that quiesce between phases.
- **Adaptive steal batch size**: currently `steal_batch_and_pop`
  uses crossbeam's default (half of `src.len()`). A higher
  cap would reduce steal-event frequency on bursts; a lower cap
  would smooth fairness on small queues.
- **OTel meter integration**: today the counter lives in a static
  map; a v0.23 patch can install an OTel observable counter callback
  that snapshots it on every export interval.

## See also

- `docs/internals/multi-core.md` — deeper notes on the v0.6 layout.
- `docs/internals/runtime.md`
- `docs/internals/agent-features-roadmap.md` — Tier 5.
- `docs/spec/v0.1-amendments.md` — A39, A41, A101-A110
- `dev/history/notes/WORK_STEALING_V0_22_NOTES.md` — design choices.
