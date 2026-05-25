//! v0.6 multi-core work-stealing scheduler (spec §25.4).
//!
//! ## Architecture
//!
//! Slice 7 shipped one tokio runtime (multi-thread or current-thread).
//! v0.6 generalises this to **N worker threads + 1 driver runtime**:
//!
//! - **Worker thread `i`** (1 ≤ i ≤ N): owns a current-thread tokio
//!   runtime + a `crossbeam_deque::Worker<SpawnTask>` (LIFO) + a
//!   `Stealer` exposed to siblings. The thread drives its runtime via
//!   `rt.block_on(worker_loop)`. The loop async-pops local, then the
//!   global `Injector`, then steals from siblings, then parks.
//!
//! - **Driver runtime** (`rt`): a separate tokio current-thread runtime
//!   that the embedding application uses with `rt.block_on(user_main)`.
//!   It does no work-stealing — it just provides an async context for
//!   the user's program to call `spawn_agent`, `send`, `ask`. Agent
//!   loops are spawned onto worker runtimes via their `Handle`.
//!
//! Why two layers? Because tokio's `current_thread` runtime panics if
//! you try to call `block_on` from within itself, but the driver and
//! the workers each need their own `block_on` driver. A separate
//! driver runtime keeps the pre-v0.6 pipeline API (`scheduler.rt
//! .block_on(...)`) working unchanged.
//!
//! ## Work-stealing
//!
//! Each worker loop:
//!
//! 1. Pop from the local LIFO deque.
//! 2. Steal a batch from the global `Injector` into the local deque
//!    and pop one.
//! 3. Steal a batch from a random sibling's `Stealer` into the local
//!    deque and pop one.
//! 4. Park until an `Unparker` wake (with a 50 ms timeout so monitors
//!    still tick).
//!
//! The unit of work is an [`SpawnTask`]: a `FnOnce(TokioHandle)`
//! closure that the worker invokes — the closure is responsible for
//! using the handle to `spawn` a future onto the worker's runtime.
//! The handle is the worker's runtime handle, so any future spawned
//! lives on that worker's tokio runtime.
//!
//! ## Affinity
//!
//! - `Affinity::Sticky` — pin at spawn, never migrate.
//! - `Affinity::Elastic` (default) — may migrate.
//!
//! ## Migration
//!
//! A [`LoadMonitor`] periodically samples per-worker queue depths and
//! suggests migrations when the busiest worker exceeds the lightest by
//! the configurable threshold (default 4×). Migration in v0.6 is
//! **lightweight** — it updates the routing table so subsequent
//! spawns of the same agent land on the lighter worker. We don't
//! tear down an in-flight `mailbox.recv()` loop mid-receive because
//! that would lose buffered messages.
//!
//! ## Deterministic mode
//!
//! `RuntimeBuilder::deterministic(seed)` forces a single worker +
//! single driver — byte identical to v0.5.

use crossbeam_deque::{Injector, Steal, Stealer, Worker as Deque};
use crossbeam_utils::sync::{Parker, Unparker};
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle as ThreadJoin};
use std::time::Duration;
use tokio::runtime::{Builder, Handle as TokioHandle, Runtime as TokioRt};
use tokio::sync::Notify;

/// Per-agent affinity hint. v0.6 parses the syntax in the front-end
/// (best-effort) but only the two coarse modes below influence
/// migration today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Affinity {
    /// Pin to first worker, never migrate. For IO-bound agents that
    /// own host resources (sockets, files) that don't move cheaply.
    Sticky,
    /// Default. May be migrated by the load monitor.
    #[default]
    Elastic,
}

/// A unit of work that needs to run on some worker. The closure is
/// invoked on the worker thread with the worker's tokio handle so the
/// body can `handle.spawn(async {...})` directly.
pub struct SpawnTask {
    /// Stable identifier so monitor/migration can correlate stats.
    pub id: u64,
    pub affinity: Affinity,
    /// Closure that receives the worker's tokio handle and is expected
    /// to spawn the underlying future onto it.
    pub run: Box<dyn FnOnce(TokioHandle) + Send + 'static>,
}

impl std::fmt::Debug for SpawnTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpawnTask")
            .field("id", &self.id)
            .field("affinity", &self.affinity)
            .finish_non_exhaustive()
    }
}

/// Per-worker live statistics. Read by the monitor + telemetry exposers.
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

/// Handle the [`Scheduler`] keeps per worker. Holds the joinable OS
/// thread plus the cheap-to-clone bits (stealer, notifier, stats,
/// tokio handle).
struct WorkerHandle {
    #[allow(dead_code)]
    id: usize,
    /// Cloned out of the worker's `Deque` at construction. Held by the
    /// scheduler so future expansion (e.g. injecting work directly into
    /// a worker's queue from the monitor) doesn't need to plumb a new
    /// channel. Currently unread on the hot path because `submit_to`
    /// routes through the global injector.
    #[allow(dead_code)]
    stealer: Stealer<SpawnTask>,
    /// Async notifier used to wake the worker's `Notify::notified()`
    /// await from any thread. Replaces the older OS-thread `Unparker`
    /// so the wake doesn't bypass the tokio reactor.
    notify: Arc<Notify>,
    /// Legacy OS-thread unparker. Retained for direct-from-thread
    /// shutdown signalling; we never park on this in the hot path.
    unparker: Unparker,
    stats: Arc<WorkerStats>,
    /// `TokioHandle` is `Clone + Send`, so we can hand it out from
    /// any thread without holding an `Arc<Runtime>` here. The actual
    /// `Runtime` is owned by the worker thread itself (moved into the
    /// thread closure) — when that thread exits, the runtime drops on
    /// the worker thread, not on the embedder's async stack. This
    /// sidesteps tokio's "Cannot drop a runtime in a context where
    /// blocking is not allowed" panic when the embedder drops the
    /// `Runtime` from inside its own `block_on`.
    tokio: TokioHandle,
    shutdown: Arc<AtomicBool>,
    /// `None` after the worker thread has been joined.
    thread: Mutex<Option<ThreadJoin<()>>>,
}

/// Per-agent routing entry.
#[derive(Debug, Clone, Copy)]
pub struct AgentRoute {
    pub worker: usize,
    pub affinity: Affinity,
}

/// The multi-worker scheduler.
///
/// Constructing one fires up `n` worker threads + 1 standalone driver
/// runtime. Drop joins everything.
pub struct Scheduler {
    workers: Vec<Arc<WorkerHandle>>,
    /// Global FIFO injector for tasks not yet pinned to any worker.
    injector: Arc<Injector<SpawnTask>>,
    /// Routing table: `agent_id -> AgentRoute`.
    pub(crate) routes: Arc<RwLock<HashMap<u64, AgentRoute>>>,
    /// Round-robin counter for elastic spawns.
    next_worker: AtomicUsize,
    /// True when we picked the deterministic single-worker mode.
    pub deterministic: bool,
    shutdown_started: AtomicBool,
    /// Driver runtime. Separate from any worker runtime so embedders
    /// can call `rt.block_on(...)` safely (the worker runtimes are
    /// already inside their own `block_on`). Slice-7 callers used
    /// `scheduler.rt.block_on(...)` to drive user code — that pattern
    /// still works unchanged.
    pub rt: Arc<TokioRt>,
}

impl std::fmt::Debug for Scheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scheduler")
            .field("workers", &self.workers.len())
            .field("deterministic", &self.deterministic)
            .finish_non_exhaustive()
    }
}

impl Scheduler {
    /// Build a multi-worker scheduler with `n` worker threads + a
    /// dedicated driver runtime. `n` is clamped to `>= 1`.
    pub fn multi_worker(n: usize) -> Self {
        let n = n.max(1);
        let injector = Arc::new(Injector::<SpawnTask>::new());

        // Two-phase init: allocate every worker's deque + notifier first
        // so each worker thread can be handed the full stealer list.
        struct InitSlot {
            deque: Deque<SpawnTask>,
            stealer: Stealer<SpawnTask>,
            parker: Parker,
            unparker: Unparker,
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
                let p = Parker::new();
                let u = p.unparker().clone();
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
                    parker: p,
                    unparker: u,
                    notify,
                    stats,
                    shutdown,
                    rt,
                    tokio_handle,
                }
            })
            .collect();
        let stealers: Vec<Stealer<SpawnTask>> = decks.iter().map(|s| s.stealer.clone()).collect();
        let notifies: Vec<Arc<Notify>> = decks.iter().map(|s| s.notify.clone()).collect();

        let mut workers: Vec<Arc<WorkerHandle>> = Vec::with_capacity(n);
        for id in 0..n {
            let InitSlot {
                deque,
                stealer,
                parker,
                unparker,
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

            let thread_id = id;
            let join = thread::Builder::new()
                .name(format!("mty-worker-{}", id))
                .spawn(move || {
                    // Move the runtime *into* the worker thread so its
                    // Drop happens on this thread when the loop exits
                    // — not on the embedder's async stack. The
                    // closure-owned `rt` is the only handle to the
                    // runtime; everyone else uses the cloned
                    // `TokioHandle`.
                    rt.block_on(worker_loop_async(WorkerCtx {
                        id: thread_id,
                        deque,
                        notify: notify_w,
                        stats: stats_w,
                        injector: injector_w,
                        stealers: stealers_w,
                        shutdown: shutdown_w,
                    }));
                    // Defensive: keep the parker alive for the lifetime
                    // of the worker thread (its Unparker is held by the
                    // scheduler for shutdown wakes).
                    drop(parker);
                    // `rt` drops here, on the worker thread, *after*
                    // the runtime has finished its block_on — safe.
                    drop(rt);
                })
                .expect("spawn worker thread");

            workers.push(Arc::new(WorkerHandle {
                id,
                stealer,
                notify,
                unparker,
                stats,
                tokio: tokio_handle,
                shutdown,
                thread: Mutex::new(Some(join)),
            }));
        }
        let _ = notifies;

        // Driver runtime — completely separate from worker runtimes.
        // current_thread so single-threaded embedders aren't surprised
        // by extra threads beyond the workers.
        let driver = Arc::new(
            Builder::new_current_thread()
                .enable_all()
                .thread_name("mty-driver")
                .build()
                .expect("tokio current_thread runtime (driver)"),
        );

        Self {
            workers,
            injector,
            routes: Arc::new(RwLock::new(HashMap::new())),
            next_worker: AtomicUsize::new(0),
            deterministic: false,
            shutdown_started: AtomicBool::new(false),
            rt: driver,
        }
    }

    /// Deterministic mode: 1 worker + 1 driver. Byte-identical to v0.5.
    pub fn deterministic_single() -> Self {
        let mut s = Self::multi_worker(1);
        s.deterministic = true;
        s
    }

    /// Slice-7 compat alias.
    pub fn current_thread() -> Self {
        Self::deterministic_single()
    }

    /// Slice-7 compat alias. Semantics changed: now builds N dedicated
    /// worker threads (each with its own tokio runtime) rather than
    /// tokio's internal multi-thread pool.
    pub fn multi_thread(threads: usize) -> Self {
        Self::multi_worker(threads)
    }

    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }

    pub fn stats(&self) -> Vec<(usize, WorkerStatsSnapshot)> {
        self.workers
            .iter()
            .map(|w| (w.id, w.stats.snapshot()))
            .collect()
    }

    /// Tokio handle for worker N (mod number of workers).
    pub fn handle_for(&self, worker: usize) -> TokioHandle {
        self.workers[worker % self.workers.len()].tokio.clone()
    }

    pub fn any_handle(&self) -> TokioHandle {
        let n = self.workers.len();
        let idx = self.next_worker.fetch_add(1, Ordering::Relaxed) % n;
        self.workers[idx].tokio.clone()
    }

    pub fn assign_worker(&self, affinity: Affinity) -> usize {
        let n = self.workers.len();
        match affinity {
            Affinity::Sticky => 0,
            Affinity::Elastic => self.next_worker.fetch_add(1, Ordering::Relaxed) % n,
        }
    }

    pub fn register_route(&self, agent_id: u64, worker: usize, affinity: Affinity) {
        self.routes
            .write()
            .insert(agent_id, AgentRoute { worker, affinity });
    }

    pub fn route(&self, agent_id: u64) -> Option<AgentRoute> {
        self.routes.read().get(&agent_id).copied()
    }

    pub fn update_route_worker(&self, agent_id: u64, new_worker: usize) {
        if let Some(r) = self.routes.write().get_mut(&agent_id) {
            r.worker = new_worker;
        }
    }

    /// Push a `SpawnTask` onto the global injector. Wakes all idle
    /// workers (asynchronously, via tokio Notify); the first idle one
    /// grabs the task.
    pub fn submit(&self, task: SpawnTask) {
        self.injector.push(task);
        for w in &self.workers {
            w.notify.notify_one();
        }
    }

    /// Submit a task with a preferred worker. We push to the global
    /// injector and notify *that* worker first so it gets the steal
    /// race. The depth counter for the target worker is incremented so
    /// the monitor sees the load even before the worker pops the task.
    pub fn submit_to(&self, worker: usize, task: SpawnTask) {
        let n = self.workers.len();
        let idx = worker % n;
        self.workers[idx]
            .stats
            .current_queue_depth
            .fetch_add(1, Ordering::Relaxed);
        self.injector.push(task);
        self.workers[idx].notify.notify_one();
    }

    pub fn worker_stats(&self, worker: usize) -> WorkerStatsSnapshot {
        if worker < self.workers.len() {
            self.workers[worker].stats.snapshot()
        } else {
            WorkerStatsSnapshot::default()
        }
    }

    /// Stop all worker threads. Idempotent.
    pub fn shutdown(&self) {
        if self.shutdown_started.swap(true, Ordering::AcqRel) {
            return;
        }
        for w in &self.workers {
            w.shutdown.store(true, Ordering::Release);
            // Wake the async loop (it's awaiting on `notify.notified()`).
            w.notify.notify_one();
            // Defensive: also fire the OS-thread unparker in case the
            // worker thread is wedged in a non-tokio code path.
            w.unparker.unpark();
        }
        for w in &self.workers {
            if let Some(t) = w.thread.lock().take() {
                let _ = t.join();
            }
        }
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Per-worker context handed to the async work-stealing loop.
struct WorkerCtx {
    id: usize,
    deque: Deque<SpawnTask>,
    notify: Arc<Notify>,
    stats: Arc<WorkerStats>,
    injector: Arc<Injector<SpawnTask>>,
    stealers: Vec<Stealer<SpawnTask>>,
    shutdown: Arc<AtomicBool>,
}

/// Async work-stealing loop. Driven by the worker's tokio runtime via
/// `rt.block_on(worker_loop_async(...))`. Each iteration:
///
/// 1. Try local LIFO.
/// 2. Try global injector (steal a batch).
/// 3. Try sibling stealers (steal a batch from one).
/// 4. Park (blocking on the worker thread, but the runtime has no
///    other in-flight tasks to wake at that point — pending tasks
///    spawned earlier continue to live on the runtime's pollset and
///    will be polled on the next runtime tick).
///
/// We sprinkle `tokio::task::yield_now()` between iterations so any
/// tasks the worker previously spawned via `Handle::spawn` get to make
/// progress without being starved by a busy spawn-task flood.
async fn worker_loop_async(ctx: WorkerCtx) {
    let WorkerCtx {
        id,
        deque,
        notify,
        stats,
        injector,
        stealers,
        shutdown,
    } = ctx;
    let n = stealers.len();
    let mut steal_cursor = id;
    let handle = TokioHandle::current();

    loop {
        if shutdown.load(Ordering::Acquire) {
            break;
        }

        // Always give in-flight spawned tasks a turn before we look
        // for new work; this prevents the work-stealing loop from
        // starving previously-spawned agent loops.
        tokio::task::yield_now().await;

        // 1. Local LIFO.
        if let Some(task) = deque.pop() {
            execute(&handle, &stats, task);
            continue;
        }

        // 2. Global injector (steal a batch into the local deque).
        match injector.steal_batch_and_pop(&deque) {
            Steal::Success(task) => {
                stats.tasks_stolen.fetch_add(1, Ordering::Relaxed);
                execute(&handle, &stats, task);
                continue;
            }
            Steal::Retry => continue,
            Steal::Empty => {}
        }

        // 3. Sibling stealers.
        let mut got_work = false;
        for offset in 1..n.max(1) {
            let idx = (steal_cursor + offset) % n.max(1);
            if idx == id || idx >= n {
                continue;
            }
            match stealers[idx].steal_batch_and_pop(&deque) {
                Steal::Success(task) => {
                    stats.tasks_stolen.fetch_add(1, Ordering::Relaxed);
                    execute(&handle, &stats, task);
                    got_work = true;
                    break;
                }
                Steal::Retry => {
                    tokio::task::yield_now().await;
                    got_work = true;
                    break;
                }
                Steal::Empty => {}
            }
        }
        steal_cursor = steal_cursor.wrapping_add(1);
        if got_work {
            continue;
        }

        // 4. No work to do. Await on `Notify` — fully async, so the
        // tokio runtime keeps polling any tasks we've spawned earlier
        // (agent loops awaiting on mailbox.recv() can still wake and
        // run). A 50 ms safety timeout means even if a notify is
        // missed the worker doesn't sleep forever.
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

/// Load-balancing monitor. Sampling is best-effort and lock-free on the
/// hot path; only acquires `routes` briefly to read.
pub struct LoadMonitor {
    pub scheduler: Arc<Scheduler>,
    pub threshold: u64,
    pub interval: Duration,
    pub running: Arc<AtomicBool>,
}

impl LoadMonitor {
    pub fn new(scheduler: Arc<Scheduler>) -> Self {
        Self {
            scheduler,
            threshold: 4,
            interval: Duration::from_millis(100),
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
    }

    /// One sampling pass. Returns `Some((from, to, agent_id))` if a
    /// migration suggestion was emitted.
    pub fn sample_once(&self) -> Option<(usize, usize, u64)> {
        let snapshots: Vec<(usize, WorkerStatsSnapshot)> = self.scheduler.stats();
        if snapshots.len() < 2 {
            return None;
        }
        let busiest = snapshots
            .iter()
            .max_by_key(|(_, s)| s.current_queue_depth)?;
        let lightest = snapshots
            .iter()
            .min_by_key(|(_, s)| s.current_queue_depth)?;
        if busiest.0 == lightest.0 {
            return None;
        }
        let b = busiest.1.current_queue_depth as u64;
        let l = lightest.1.current_queue_depth as u64;
        let trigger = b > self.threshold * l.max(1) && b >= self.threshold;
        if !trigger {
            return None;
        }
        let routes = self.scheduler.routes.read();
        routes
            .iter()
            .find(|(_, r)| r.worker == busiest.0 && r.affinity == Affinity::Elastic)
            .map(|(id, _)| (busiest.0, lightest.0, *id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    #[test]
    fn build_and_shutdown_single() {
        let s = Scheduler::deterministic_single();
        assert_eq!(s.worker_count(), 1);
        s.shutdown();
    }

    #[test]
    fn build_and_shutdown_multi() {
        let s = Scheduler::multi_worker(4);
        assert_eq!(s.worker_count(), 4);
        s.shutdown();
    }

    #[test]
    fn round_robin_assign() {
        let s = Scheduler::multi_worker(3);
        let a = s.assign_worker(Affinity::Elastic);
        let b = s.assign_worker(Affinity::Elastic);
        let c = s.assign_worker(Affinity::Elastic);
        let d = s.assign_worker(Affinity::Elastic);
        // round-robin cycles
        assert_eq!(a, d % 3);
        let _ = (b, c);
        s.shutdown();
    }

    #[test]
    fn sticky_always_zero() {
        let s = Scheduler::multi_worker(4);
        for _ in 0..10 {
            assert_eq!(s.assign_worker(Affinity::Sticky), 0);
        }
        s.shutdown();
    }

    #[test]
    fn submit_runs_on_some_worker() {
        let s = Arc::new(Scheduler::multi_worker(2));
        let counter = Arc::new(AtomicU32::new(0));
        let c2 = counter.clone();
        s.submit(SpawnTask {
            id: 1,
            affinity: Affinity::Elastic,
            run: Box::new(move |h| {
                let c = c2.clone();
                h.spawn(async move {
                    c.fetch_add(1, Ordering::Relaxed);
                });
            }),
        });
        let started = std::time::Instant::now();
        while counter.load(Ordering::Relaxed) == 0 {
            if started.elapsed() > Duration::from_secs(2) {
                panic!("submit task never ran");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(counter.load(Ordering::Relaxed), 1);
        s.shutdown();
    }

    #[test]
    fn monitor_detects_imbalance() {
        let s = Arc::new(Scheduler::multi_worker(2));
        // Skew per-worker queue depths and register an elastic agent.
        s.workers[0]
            .stats
            .current_queue_depth
            .store(20, Ordering::Relaxed);
        s.workers[1]
            .stats
            .current_queue_depth
            .store(1, Ordering::Relaxed);
        s.register_route(42, 0, Affinity::Elastic);
        let m = LoadMonitor::new(s.clone());
        assert_eq!(m.sample_once(), Some((0, 1, 42)));
        s.shutdown();
    }

    #[test]
    fn monitor_skips_sticky_agents() {
        let s = Arc::new(Scheduler::multi_worker(2));
        s.workers[0]
            .stats
            .current_queue_depth
            .store(20, Ordering::Relaxed);
        s.workers[1]
            .stats
            .current_queue_depth
            .store(1, Ordering::Relaxed);
        s.register_route(7, 0, Affinity::Sticky);
        let m = LoadMonitor::new(s.clone());
        assert_eq!(m.sample_once(), None);
        s.shutdown();
    }

    #[test]
    fn driver_runtime_block_on_works() {
        let s = Scheduler::multi_worker(2);
        let v: u32 = s.rt.block_on(async {
            tokio::task::yield_now().await;
            7
        });
        assert_eq!(v, 7);
        s.shutdown();
    }

    #[test]
    fn many_submits_complete() {
        let s = Arc::new(Scheduler::multi_worker(4));
        let counter = Arc::new(AtomicU32::new(0));
        for i in 0..32u64 {
            let c = counter.clone();
            s.submit(SpawnTask {
                id: i,
                affinity: Affinity::Elastic,
                run: Box::new(move |h| {
                    let c = c.clone();
                    h.spawn(async move {
                        c.fetch_add(1, Ordering::Relaxed);
                    });
                }),
            });
        }
        let started = std::time::Instant::now();
        while counter.load(Ordering::Relaxed) < 32 {
            if started.elapsed() > Duration::from_secs(5) {
                panic!(
                    "only {} of 32 tasks completed",
                    counter.load(Ordering::Relaxed)
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(counter.load(Ordering::Relaxed), 32);
        s.shutdown();
    }
}
