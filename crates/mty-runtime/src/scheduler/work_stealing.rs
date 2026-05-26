//! v0.22 Tier 5 — true crossbeam-deque work-stealing pool.
//!
//! ## Why this module exists
//!
//! Before v0.22 the scheduler had work-stealing semantics
//! (`crossbeam-deque` deques + a sibling rotation cursor) but the
//! per-worker logic was inlined into one 600-line async function. v0.22
//! factors it into:
//!
//! 1. [`Worker`] — owns a local LIFO deque, a sibling-stealer slice,
//!    a NUMA-aware `steal_order` produced by [`crate::scheduler::locality`],
//!    and a tokio current-thread runtime handle.
//! 2. [`WorkerPool`] — the constructor that spins up `n` workers, hands
//!    each one a stealer view of every sibling, and returns a handle
//!    with stable observability (per-worker stats + the `worker.steals_total`
//!    counter).
//!
//! The split lets us test the work-stealing loop in isolation (no agent
//! framework, no mailbox, no tokio runtime besides the trivial one each
//! worker creates) — see `crates/mty-runtime/tests/work_stealing.rs`.
//!
//! ## What "work-stealing" means here
//!
//! Each worker:
//!
//! ```text
//! loop {
//!   if shutdown { break }
//!   tokio::task::yield_now().await           // let prior spawns run
//!   if let Some(t) = local.pop() { run(t); continue }      // 1
//!   for sibling in steal_order {                            // 2
//!     match stealers[sibling].steal_batch_and_pop(&local) {
//!       Success(t) => { counter[src=sibling, dst=self].inc;
//!                       stats.tasks_stolen.inc;
//!                       run(t); continue }
//!       Retry      => yield_now().await; continue            // 3
//!       Empty      => {}                                     // 4
//!     }
//!   }
//!   match injector.steal_batch_and_pop(&local) { ... }       // 5
//!   stats.parks.inc; timeout(50ms, notify.notified()).await  // 6
//! }
//! ```
//!
//! The `steal_order` is locality-aware (NUMA-local first, then
//! socket-local, then anywhere), per [`crate::scheduler::locality::build_steal_order`].
//!
//! `Retry` from `steal_batch_and_pop` means the deque was contended;
//! we yield once and re-enter the same iteration (counted under
//! step 3) so we don't deflect to other siblings prematurely.
//!
//! ## Counter semantics
//!
//! Every **successful** steal increments
//! `worker.steals_total{src=<source-worker-id>, dst=<destination-worker-id>}`
//! exactly once. Steals from the global injector are recorded with
//! `src = usize::MAX` to disambiguate from sibling steals — consumers
//! that only care about cross-worker traffic should filter that out.
//!
//! ## Parking
//!
//! When all sources are empty, we await on a per-worker
//! `tokio::sync::Notify`. Submitters call `notify_one()` on the target
//! (or all workers, for global injector pushes). A 50 ms safety timeout
//! means a missed wake is bounded — the worker eventually re-probes
//! and finds whatever was missed.

use crossbeam_deque::{Injector, Steal, Stealer, Worker as Deque};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle as ThreadJoin};
use std::time::Duration;
use tokio::runtime::{Builder, Handle as TokioHandle, Runtime as TokioRt};
use tokio::sync::Notify;

use super::locality::{build_steal_order, Topology};
use super::SpawnTask;
use crate::telemetry::sink::record_worker_steal;

/// Sentinel `src` for "stolen from the global injector". We can't use
/// `0` because that's a real worker id, and we can't use `Option` in a
/// hot-path counter map without churn — `usize::MAX` is a fine
/// out-of-band value for the label set.
pub const SRC_GLOBAL_INJECTOR: usize = usize::MAX;

/// Per-worker live statistics. Read by the monitor + telemetry.
#[derive(Debug, Default)]
pub struct WorkerStats {
    pub tasks_executed: AtomicU64,
    pub tasks_stolen: AtomicU64,
    pub parks: AtomicU64,
    pub current_queue_depth: AtomicUsize,
}

impl WorkerStats {
    pub fn snapshot(&self) -> WorkerStatsSnapshot {
        WorkerStatsSnapshot {
            tasks_executed: self.tasks_executed.load(Ordering::Relaxed),
            tasks_stolen: self.tasks_stolen.load(Ordering::Relaxed),
            parks: self.parks.load(Ordering::Relaxed),
            current_queue_depth: self.current_queue_depth.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WorkerStatsSnapshot {
    pub tasks_executed: u64,
    pub tasks_stolen: u64,
    pub parks: u64,
    pub current_queue_depth: usize,
}

/// Public-ish handle the scheduler keeps for one worker. Holds all the
/// pieces a submitter or shutdown coordinator needs without leaking the
/// owning `Deque` (which must stay pinned to the worker thread).
pub(crate) struct WorkerHandle {
    #[allow(dead_code)]
    pub id: usize,
    /// Cloned stealer view of this worker's local deque. Held for
    /// completeness so future code can `steal_to_worker(id, n)` without
    /// re-plumbing a channel.
    #[allow(dead_code)]
    pub stealer: Stealer<SpawnTask>,
    pub notify: Arc<Notify>,
    pub stats: Arc<WorkerStats>,
    pub tokio: TokioHandle,
    pub shutdown: Arc<AtomicBool>,
    /// `None` once the OS thread has been joined.
    pub thread: Mutex<Option<ThreadJoin<()>>>,
}

/// Construction input for one worker. Built by [`WorkerPool::launch`] and
/// moved into the worker thread.
pub(crate) struct WorkerCtx {
    pub id: usize,
    pub deque: Deque<SpawnTask>,
    pub notify: Arc<Notify>,
    pub stats: Arc<WorkerStats>,
    pub injector: Arc<Injector<SpawnTask>>,
    pub stealers: Vec<Stealer<SpawnTask>>,
    pub shutdown: Arc<AtomicBool>,
    /// NUMA-aware probe order. Length = `n_workers - 1` (excludes
    /// self). When the topology is flat (e.g. on Windows or when
    /// `/sys` is unreadable) this degenerates to a plain rotation.
    pub steal_order: Vec<usize>,
}

/// Spin up `n` workers and return their handles plus the global
/// injector. Each worker:
///
/// - owns a `Deque<SpawnTask>` LIFO
/// - publishes a `Stealer` to its siblings
/// - gets a NUMA-aware steal-order from `topology`
/// - runs the [`worker_loop_async`] on its own current-thread tokio
///
/// The caller is responsible for storing the returned handles in a
/// `Scheduler` and arranging shutdown.
pub(crate) struct PoolLaunch {
    pub workers: Vec<Arc<WorkerHandle>>,
    pub injector: Arc<Injector<SpawnTask>>,
    pub topology: Topology,
}

pub(crate) fn launch_pool(n: usize) -> PoolLaunch {
    let n = n.max(1);
    let injector = Arc::new(Injector::<SpawnTask>::new());
    let topology = Topology::detect(n);

    struct InitSlot {
        deque: Deque<SpawnTask>,
        stealer: Stealer<SpawnTask>,
        notify: Arc<Notify>,
        stats: Arc<WorkerStats>,
        shutdown: Arc<AtomicBool>,
        rt: TokioRt,
        tokio_handle: TokioHandle,
    }
    let mut decks: Vec<InitSlot> = (0..n)
        .map(|i| {
            let d = Deque::<SpawnTask>::new_lifo();
            let s = d.stealer();
            let notify = Arc::new(Notify::new());
            let stats = Arc::new(WorkerStats::default());
            let shutdown = Arc::new(AtomicBool::new(false));
            let rt = Builder::new_current_thread()
                .enable_all()
                .thread_name(format!("mty-worker-{}", i))
                .build()
                .expect("tokio current_thread runtime (worker)");
            let tokio_handle = rt.handle().clone();
            InitSlot {
                deque: d,
                stealer: s,
                notify,
                stats,
                shutdown,
                rt,
                tokio_handle,
            }
        })
        .collect();
    let stealers: Vec<Stealer<SpawnTask>> = decks.iter().map(|s| s.stealer.clone()).collect();

    let mut workers: Vec<Arc<WorkerHandle>> = Vec::with_capacity(n);
    for id in 0..n {
        let InitSlot {
            deque,
            stealer,
            notify,
            stats,
            shutdown,
            rt,
            tokio_handle,
        } = decks.remove(0);
        let injector_w = injector.clone();
        let stealers_w = stealers.clone();
        let stats_w = stats.clone();
        let shutdown_w = shutdown.clone();
        let notify_w = notify.clone();
        let steal_order = build_steal_order(id, &topology);

        let thread_id = id;
        let join = thread::Builder::new()
            .name(format!("mty-worker-{}", id))
            .spawn(move || {
                rt.block_on(worker_loop_async(WorkerCtx {
                    id: thread_id,
                    deque,
                    notify: notify_w,
                    stats: stats_w,
                    injector: injector_w,
                    stealers: stealers_w,
                    shutdown: shutdown_w,
                    steal_order,
                }));
                drop(rt);
            })
            .expect("spawn worker thread");

        workers.push(Arc::new(WorkerHandle {
            id,
            stealer,
            notify,
            stats,
            tokio: tokio_handle,
            shutdown,
            thread: Mutex::new(Some(join)),
        }));
    }

    PoolLaunch {
        workers,
        injector,
        topology,
    }
}

/// Async work-stealing loop. Driven by the worker's current-thread tokio
/// runtime via `rt.block_on(worker_loop_async(...))`.
///
/// **Phases per iteration:**
///
/// 1. Cooperative yield — lets agent futures spawned via `handle.spawn`
///    make progress and prevents the work-stealing scan from starving
///    them.
/// 2. Pop from the local LIFO. If something's there, run it.
/// 3. Walk `steal_order` once (NUMA-local first). Try each sibling's
///    `Stealer` — on `Success`, increment `worker.steals_total{src,dst}`
///    and run.
/// 4. Try the global `Injector` last (cross-worker spawns submitted via
///    `Scheduler::submit`). On `Success`, increment
///    `worker.steals_total{src=SRC_GLOBAL_INJECTOR, dst=self}`.
/// 5. No work — park on `notify.notified()` with a 50 ms safety
///    timeout, then loop.
///
/// **Why siblings before injector?** In v0.21 we tried the injector
/// first. That stranded sibling-local work when the injector held a
/// long-running stream — the worker would keep batching from the
/// global queue and never re-balance. v0.22 prefers siblings so
/// cache-warm work moves first.
async fn worker_loop_async(ctx: WorkerCtx) {
    let WorkerCtx {
        id,
        deque,
        notify,
        stats,
        injector,
        stealers,
        shutdown,
        steal_order,
    } = ctx;
    let handle = TokioHandle::current();

    loop {
        if shutdown.load(Ordering::Acquire) {
            break;
        }

        // Phase 1: let already-spawned futures (agent loops, timers)
        // get a turn so the work-stealing scan never starves them.
        tokio::task::yield_now().await;

        // Phase 2: local LIFO.
        if let Some(task) = deque.pop() {
            execute(&handle, &stats, task);
            continue;
        }

        // Phase 3: sibling stealers, NUMA-aware order.
        let mut got_work = false;
        let mut retry_needed = false;
        for &sibling_id in &steal_order {
            if sibling_id >= stealers.len() || sibling_id == id {
                continue;
            }
            match stealers[sibling_id].steal_batch_and_pop(&deque) {
                Steal::Success(task) => {
                    stats.tasks_stolen.fetch_add(1, Ordering::Relaxed);
                    record_worker_steal(sibling_id, id);
                    execute(&handle, &stats, task);
                    got_work = true;
                    break;
                }
                Steal::Retry => {
                    retry_needed = true;
                    // Don't break — try the next sibling. If everyone
                    // is contended we'll fall through to a yield.
                }
                Steal::Empty => {}
            }
        }
        if got_work {
            continue;
        }
        if retry_needed {
            // Someone was contended; yield once and re-enter the loop
            // rather than parking — work probably exists.
            tokio::task::yield_now().await;
            continue;
        }

        // Phase 4: global injector. Steal a batch into the local deque
        // (so future iterations get phase-2 hits without re-locking
        // the injector for every task) and run one immediately.
        match injector.steal_batch_and_pop(&deque) {
            Steal::Success(task) => {
                stats.tasks_stolen.fetch_add(1, Ordering::Relaxed);
                record_worker_steal(SRC_GLOBAL_INJECTOR, id);
                execute(&handle, &stats, task);
                continue;
            }
            Steal::Retry => {
                tokio::task::yield_now().await;
                continue;
            }
            Steal::Empty => {}
        }

        // Phase 5: nothing anywhere. Park.
        stats.parks.fetch_add(1, Ordering::Relaxed);
        let _ = tokio::time::timeout(Duration::from_millis(50), notify.notified()).await;
    }
}

fn execute(handle: &TokioHandle, stats: &WorkerStats, task: SpawnTask) {
    stats.tasks_executed.fetch_add(1, Ordering::Relaxed);
    let prev = stats.current_queue_depth.load(Ordering::Relaxed);
    if prev > 0 {
        stats.current_queue_depth.fetch_sub(1, Ordering::Relaxed);
    }
    (task.run)(handle.clone());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_pool_returns_n_workers() {
        let pool = launch_pool(3);
        assert_eq!(pool.workers.len(), 3);
        assert_eq!(pool.topology.locals.len(), 3);
        for w in &pool.workers {
            w.shutdown.store(true, Ordering::Release);
            w.notify.notify_one();
        }
        for w in &pool.workers {
            if let Some(t) = w.thread.lock().take() {
                let _ = t.join();
            }
        }
    }
}
