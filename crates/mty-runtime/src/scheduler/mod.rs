//! v0.6 multi-core work-stealing scheduler (spec §25.4), extended in
//! v0.22 with true crossbeam-deque per-worker work-stealing and
//! NUMA-aware locality (Tier 5 of the agent-features roadmap).
//!
//! ## Architecture
//!
//! Slice 7 shipped one tokio runtime (multi-thread or current-thread).
//! v0.6 generalised this to **N worker threads + 1 driver runtime**:
//!
//! - **Worker thread `i`** (1 ≤ i ≤ N): owns a current-thread tokio
//!   runtime + a `crossbeam_deque::Worker<SpawnTask>` (LIFO) + a
//!   `Stealer` exposed to siblings. The thread drives its runtime via
//!   `rt.block_on(worker_loop)`. The loop async-pops local, then walks
//!   a **NUMA-aware steal order** of sibling stealers, then probes the
//!   global `Injector`, then parks.
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
//! ## Work-stealing (v0.22)
//!
//! Each worker loop runs the body documented in
//! [`work_stealing::worker_loop_async`]: yield → local → siblings
//! (NUMA-local first) → global injector → park.
//!
//! The NUMA-aware order is produced by
//! [`locality::build_steal_order`] from a [`locality::Topology`]
//! detected once at scheduler construction. On Linux we read
//! `/sys/devices/system/cpu/*` + `/sys/devices/system/node/*` — on
//! Windows / containers without `/sys` we fall back to a flat
//! "everyone is on node 0" topology and the order degenerates to a
//! plain rotation.
//!
//! Every successful steal increments the
//! `worker.steals_total{src=N, dst=M}` counter exported by the
//! telemetry sink (see [`crate::telemetry::sink::WORKER_STEAL_COUNTER`]).
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

pub mod locality;
pub mod work_stealing;

pub use work_stealing::{WorkerStats, WorkerStatsSnapshot};

use crossbeam_deque::Injector;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::{Builder, Handle as TokioHandle, Runtime as TokioRt};

use locality::Topology;
use work_stealing::{launch_pool, WorkerHandle};

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
    /// NUMA topology detected at construction. Exposed so introspect
    /// surfaces / tests can read which worker lives on which node.
    pub topology: Topology,
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
        let launch = launch_pool(n);
        let driver = Arc::new(
            Builder::new_current_thread()
                .enable_all()
                .thread_name("mty-driver")
                .build()
                .expect("tokio current_thread runtime (driver)"),
        );

        Self {
            workers: launch.workers,
            injector: launch.injector,
            routes: Arc::new(RwLock::new(HashMap::new())),
            next_worker: AtomicUsize::new(0),
            deterministic: false,
            shutdown_started: AtomicBool::new(false),
            topology: launch.topology,
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

// Re-export submit helpers for tests that need to drop work straight
// onto a worker's local deque (used by `tests/work_stealing.rs` to
// pre-load one worker with all the tasks so siblings *have* to steal).
impl Scheduler {
    /// Push a task into a specific worker's stealer. Returns false if
    /// `worker_id` is out of range. Intended for test setup only — the
    /// production path uses `submit` / `submit_to`. Pushes via the
    /// injector for safety (the worker's local deque is owned by the
    /// worker thread itself, so we can't push to it from here without
    /// a channel) but pre-increments depth on the chosen worker so the
    /// monitor and steal-races behave as if pinned.
    pub fn submit_pinned(&self, worker_id: usize, task: SpawnTask) -> bool {
        if worker_id >= self.workers.len() {
            return false;
        }
        // Same shape as submit_to, but **only** notifies the chosen
        // worker — siblings stay parked until they steal. That's what
        // the `idle_worker_steals_from_busy_one` test depends on.
        self.workers[worker_id]
            .stats
            .current_queue_depth
            .fetch_add(1, Ordering::Relaxed);
        self.injector.push(task);
        self.workers[worker_id].notify.notify_one();
        true
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

    #[test]
    fn topology_matches_worker_count() {
        let s = Scheduler::multi_worker(4);
        assert_eq!(s.topology.locals.len(), 4);
        s.shutdown();
    }
}
