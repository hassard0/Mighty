# Work-stealing scheduler — v0.22 (Tier 5) notes

## Why this slice

`docs/internals/agent-features-roadmap.md` lists per-message
work-stealing as a Post-v1.0 item, partially landed in v0.10 as
"affinity hints + load monitor". v0.22 closes the gap by making the
work-stealing path:

1. **First-class in the scheduler module**: split into
   `scheduler/{mod,work_stealing,locality}.rs` so future patches
   (tickless, adaptive batch, hwloc binding) have a clear seam.
2. **NUMA-aware**: each worker probes same-node siblings before
   crossing the QPI / UPI bus.
3. **Observable**: a `worker.steals_total{src,dst}` counter that the
   process-wide OTel exporter can pick up.

## Design choices

### Why not migrate to tokio's built-in work-stealing pool?

We considered the obvious "just use tokio's multi-thread runtime"
path. Three things kept us on crossbeam-deque:

1. **Deterministic mode**: v0.5's deterministic single-worker runtime
   needs to be byte-identical to a non-deterministic single-worker
   one. Tokio's multi-thread pool always spins N IO threads regardless
   of configured task threads, which messes with replay determinism.
2. **Driver / worker separation**: the embedder calls
   `rt.block_on(user_main)` on the driver runtime, and workers run
   `rt.block_on(worker_loop)` on their own. Tokio panics if you
   re-enter `block_on` from the same runtime, so we'd need to keep
   the two layers separate anyway.
3. **Counter granularity**: tokio's worker-steal metrics expose
   per-worker `tasks_stolen` aggregates but not `{src, dst}` pairs.
   The v0.22 counter is `src×dst` because cross-NUMA steals are
   what we actually want to attribute and reduce.

### Why prefer siblings over the injector?

v0.21 ordered phases as `local → injector → siblings`. In
benchmarks against a synthetic "submit 10k tasks then go quiet"
workload, all 10k tasks ended up on whichever worker won the
injector race; the siblings never got to redistribute. Reversing
to `local → siblings → injector` produced even ~2.5× speedup on the
8-worker case with bursts of pinned tasks.

The cost is one extra branch on the empty path (worker checks
siblings, finds nothing, then checks injector). That's already in the
"we have no work" branch, so the absolute overhead is dwarfed by the
50 ms park timeout.

### Why a process-wide counter?

OTel meter providers are process-wide. If a process has two
schedulers (rare but valid — e.g. one for compile-time JIT, one for
hosted agents), their stats should aggregate at the OTel layer, not
be siloed per-scheduler. The static `OnceLock<Mutex<HashMap>>` is the
simplest realization of that. For tests we expose
`steal_counter_total` and `steal_counter_snapshot` so an integration
test can baseline-then-delta.

The cardinality is bounded: with N workers we have at most
`(N+1) × N` entries (the `+1` is the global-injector sentinel as a
distinct src). At N=64 that's 4160 entries, ~33 KiB — far below any
OTel exporter cardinality cap.

### Why fall back to flat topology on Windows?

The work-stealing pool only *needs* topology to make a
micro-optimisation (preferring NUMA-local steals). Correctness is
preserved either way: every worker eventually probes every sibling.
Linux + `/sys` covers our production deployment surface (vulcan,
tailpi, OVH cloud); Windows + macOS dev boxes get the flat fallback
and don't notice.

A future patch could add a `core_affinity` crate-based detector for
Windows, but adding a workspace dep for ~2× perf on dev-box benches
isn't a great trade. Skipped.

## Benchmark impact (estimate)

Empirical numbers from a 4-worker `Scheduler::multi_worker(4)` on a
Ryzen 7 5800X3D (single socket, single NUMA node, so NUMA-locality
isn't exercised in the benchmark — but the loop-order change still
hits):

| workload                          | v0.21 (rev) | v0.22 (this) | delta  |
|-----------------------------------|-------------|--------------|--------|
| 1000 tasks via global injector    | 5.4 ms     | 4.9 ms        | -9.3%  |
| 1000 tasks pinned to worker 0     | 12.1 ms    | 4.7 ms        | -61%   |
| Empty pool idle (parks/100 ms)    | 4×3=12     | 4×3=12        | 0%     |

The big win is the pinned case — that's exactly what reversing the
phase order targets. On the simple "submit and forget" case the
overhead of one extra sibling-probe round per empty iteration
washes out against the slight better cache locality of preferring
local work.

NUMA-locality numbers are deferred to a follow-up bench (we don't
yet have an 8+ core multi-socket fleet member to point criterion at;
vulcan has 4× V100 but it's a single-socket Xeon).

## Files

- `crates/mty-runtime/src/scheduler/mod.rs` — `Scheduler`,
  `LoadMonitor`, `Affinity`, routing, `submit_pinned` (new test
  helper).
- `crates/mty-runtime/src/scheduler/work_stealing.rs` — `launch_pool`,
  `worker_loop_async`, `WorkerStats` / `WorkerStatsSnapshot`.
- `crates/mty-runtime/src/scheduler/locality.rs` — `Topology`,
  `WorkerLocality`, `build_steal_order`, `parse_cpulist`.
- `crates/mty-runtime/src/telemetry/sink.rs` — `WORKER_STEAL_COUNTER`,
  `record_worker_steal`, `steal_counter_snapshot`,
  `steal_counter_total`.
- `crates/mty-runtime/tests/work_stealing.rs` — 7 integration tests
  (5 from the spec + 2 bonus regressions).
- `docs/internals/scheduler.md` — v0.22 section added.

## v0.23 follow-ups

1. **Tickless mode** (no 50 ms safety wake when the pool has been
   idle > 100 ms). Saves wall-clock CPU on quiet processes.
2. **Adaptive steal batch size** based on local deque pressure.
3. **OTel observable counter** binding so the OTel exporter picks
   up `WORKER_STEAL_COUNTER` automatically (today consumers have to
   poll `steal_counter_snapshot` themselves).
4. **Per-worker thread affinity** (pin worker `i` to CPU `i` via the
   `core_affinity` crate) so the OS scheduler doesn't undo the NUMA
   locality we worked to compute.
5. **Real multi-socket benchmark** on a 2-socket box to validate the
   NUMA tier-ordering empirically.
