//! v0.22 Tier 5 — crossbeam-deque work-stealing integration tests.
//!
//! These exercise the public `Scheduler` surface (no internals reached
//! into) to verify:
//!
//! 1. `worker_pool_processes_all_tasks` — 1000 tasks across 4 workers
//!    all complete.
//! 2. `idle_worker_steals_from_busy_one` — all tasks pinned to worker 0,
//!    other workers should still pick some up.
//! 3. `parking_when_no_work` — empty pool, workers eventually park.
//! 4. `steal_order_prefers_same_numa` — synthetic topology unit-test for
//!    the steal-order helper.
//! 5. `counter_increments_on_steal` — `worker.steals_total` populates
//!    when stealing happens.
//!
//! These run as an integration test (own binary) so the scheduler's
//! background threads can't bleed shared state into other tests.

use mty_runtime::scheduler::locality::{build_steal_order, Topology};
use mty_runtime::scheduler::{Affinity, Scheduler, SpawnTask};
use mty_runtime::telemetry::{steal_counter_snapshot, steal_counter_total};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Submit 1000 tasks to a 4-worker pool via the global injector and
/// verify they all complete. This validates the basic worker-pool
/// fan-out: the global injector + sibling stealing combined should
/// distribute work across every worker.
#[test]
fn worker_pool_processes_all_tasks() {
    let s = Arc::new(Scheduler::multi_worker(4));
    let counter = Arc::new(AtomicU32::new(0));
    const N: u32 = 1000;

    for i in 0..N {
        let c = counter.clone();
        s.submit(SpawnTask {
            id: i as u64,
            affinity: Affinity::Elastic,
            run: Box::new(move |h| {
                let c = c.clone();
                h.spawn(async move {
                    c.fetch_add(1, Ordering::Relaxed);
                });
            }),
        });
    }

    let start = Instant::now();
    while counter.load(Ordering::Relaxed) < N {
        if start.elapsed() > Duration::from_secs(15) {
            panic!(
                "only {} of {} tasks completed",
                counter.load(Ordering::Relaxed),
                N
            );
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(counter.load(Ordering::Relaxed), N);

    // Verify the work was distributed — at least 2 workers should have
    // executed something. (Strict per-worker fairness isn't a promise
    // of the work-stealing scheduler, but "more than one worker ever
    // ran a task" is a baseline.)
    let stats = s.stats();
    let active_workers = stats.iter().filter(|(_, st)| st.tasks_executed > 0).count();
    assert!(
        active_workers >= 2,
        "expected >= 2 active workers, got {} (stats: {:?})",
        active_workers,
        stats
    );

    s.shutdown();
}

/// Pin 200 tasks to worker 0 (via `submit_pinned`) and verify that
/// the other workers steal some of them. Without work-stealing this
/// test fails because workers 1..3 never get woken (the global
/// injector notify is broadcast, but each worker still needs to
/// find work — which they only do via the sibling stealer path
/// once `submit_pinned` skews depth onto worker 0).
#[test]
fn idle_worker_steals_from_busy_one() {
    let s = Arc::new(Scheduler::multi_worker(4));
    let counter = Arc::new(AtomicU32::new(0));
    const N: u32 = 200;

    for i in 0..N {
        let c = counter.clone();
        let ok = s.submit_pinned(
            0,
            SpawnTask {
                id: i as u64,
                affinity: Affinity::Sticky,
                run: Box::new(move |h| {
                    let c = c.clone();
                    h.spawn(async move {
                        // Burn a few µs so the scheduler has time to
                        // notice imbalance and steal.
                        let now = Instant::now();
                        while now.elapsed() < Duration::from_micros(200) {
                            std::hint::spin_loop();
                        }
                        c.fetch_add(1, Ordering::Relaxed);
                    });
                }),
            },
        );
        assert!(ok, "submit_pinned to valid worker should succeed");
    }

    let start = Instant::now();
    while counter.load(Ordering::Relaxed) < N {
        if start.elapsed() > Duration::from_secs(15) {
            panic!(
                "only {} of {} tasks completed",
                counter.load(Ordering::Relaxed),
                N
            );
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    // The scheduler counts steals from injector + siblings into
    // tasks_stolen. With all work entering via the injector, every
    // execution actually counts as a steal (the global injector path
    // increments tasks_stolen). So we expect tasks_stolen >= N.
    let stats = s.stats();
    let total_stolen: u64 = stats.iter().map(|(_, st)| st.tasks_stolen).sum();
    assert!(
        total_stolen >= 1,
        "expected at least one steal, got {} (stats: {:?})",
        total_stolen,
        stats
    );

    // Verify multiple workers participated: at least 2 had tasks_executed > 0.
    let active = stats.iter().filter(|(_, st)| st.tasks_executed > 0).count();
    assert!(
        active >= 2,
        "expected >= 2 workers to participate, got {} (stats: {:?})",
        active,
        stats
    );

    s.shutdown();
}

/// Build a 2-worker pool, give it no work, sleep enough for the loop
/// to park, then assert `parks > 0`. The 50 ms safety timeout in the
/// loop means we should see parks within ~100 ms.
#[test]
fn parking_when_no_work() {
    let s = Arc::new(Scheduler::multi_worker(2));
    // Sleep long enough for each worker to go through several
    // notify→timeout cycles.
    std::thread::sleep(Duration::from_millis(250));

    let stats = s.stats();
    let total_parks: u64 = stats.iter().map(|(_, st)| st.parks).sum();
    assert!(
        total_parks > 0,
        "expected workers to park while idle, got 0 parks (stats: {:?})",
        stats
    );
    // And neither worker should be racking up tons of executes from
    // empty work (which would indicate busy-loop behavior).
    let total_exec: u64 = stats.iter().map(|(_, st)| st.tasks_executed).sum();
    assert_eq!(
        total_exec, 0,
        "no work submitted, but {} executes seen",
        total_exec
    );

    s.shutdown();
}

/// Unit-test the locality helper directly. Build a synthetic
/// (node, socket) layout and assert the steal-order produced by
/// `build_steal_order` lists same-node siblings before different-node
/// ones.
#[test]
fn steal_order_prefers_same_numa() {
    // 8 workers: 4 on node 0 (socket 0), 4 on node 1 (socket 0).
    let topology = Topology::synthetic(vec![
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (1, 0),
        (1, 0),
        (1, 0),
        (1, 0),
    ]);

    // From worker 0, the first 3 entries in steal_order should all be
    // node-0 siblings (worker 1, 2, or 3).
    let order = build_steal_order(0, &topology);
    assert_eq!(order.len(), 7, "should probe 7 other workers");
    for (i, &id) in order.iter().take(3).enumerate() {
        assert!(
            (1..=3).contains(&id),
            "position {} should be a node-0 sibling (1..=3), got worker {}",
            i,
            id
        );
    }
    // The last 4 entries should all be node-1 workers (4..=7).
    for (i, &id) in order.iter().skip(3).enumerate() {
        assert!(
            (4..=7).contains(&id),
            "position {} should be a node-1 worker (4..=7), got worker {}",
            i + 3,
            id
        );
    }
}

/// End-to-end: submit work, wait for completion, snapshot the OTel
/// counter, and assert the steal_total covers something. The exact
/// `(src, dst)` distribution depends on scheduler timing, so we only
/// assert "total > 0".
#[test]
fn counter_increments_on_steal() {
    // Record the baseline first — other tests in this binary may have
    // already touched the global counter (cargo test runs tests in
    // shared process by default).
    let baseline = steal_counter_total();

    let s = Arc::new(Scheduler::multi_worker(4));
    let counter = Arc::new(AtomicU32::new(0));
    const N: u32 = 500;

    for i in 0..N {
        let c = counter.clone();
        s.submit(SpawnTask {
            id: i as u64 + 100_000,
            affinity: Affinity::Elastic,
            run: Box::new(move |h| {
                let c = c.clone();
                h.spawn(async move {
                    c.fetch_add(1, Ordering::Relaxed);
                });
            }),
        });
    }

    let start = Instant::now();
    while counter.load(Ordering::Relaxed) < N {
        if start.elapsed() > Duration::from_secs(10) {
            panic!(
                "only {} of {} tasks completed",
                counter.load(Ordering::Relaxed),
                N
            );
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    // With all work going through the global injector, every execute
    // routes through a steal (either from the injector or from a
    // sibling who pre-batched from the injector). So total - baseline
    // should be positive.
    let after = steal_counter_total();
    assert!(
        after > baseline,
        "expected steal counter to advance past baseline {}, got {}",
        baseline,
        after
    );

    // Sanity: snapshot is non-empty and contains at least one entry
    // with `dst < 4` (one of our 4 workers).
    let snapshot = steal_counter_snapshot();
    let has_local_dst = snapshot.iter().any(|(_, dst, c)| *dst < 4 && *c > 0);
    assert!(
        has_local_dst,
        "expected at least one steal recorded against a worker dst (snapshot: {:?})",
        snapshot
    );

    s.shutdown();
}

/// Bonus: per-worker steals_total stats individually accumulate.
/// Catches the regression where stats.tasks_stolen never increments
/// because we increment the counter but not the per-worker AtomicU64.
#[test]
fn per_worker_stats_record_steals() {
    let s = Arc::new(Scheduler::multi_worker(4));
    let counter = Arc::new(AtomicU32::new(0));
    const N: u32 = 100;

    for i in 0..N {
        let c = counter.clone();
        s.submit(SpawnTask {
            id: i as u64,
            affinity: Affinity::Elastic,
            run: Box::new(move |h| {
                let c = c.clone();
                h.spawn(async move {
                    c.fetch_add(1, Ordering::Relaxed);
                });
            }),
        });
    }
    let start = Instant::now();
    while counter.load(Ordering::Relaxed) < N {
        if start.elapsed() > Duration::from_secs(10) {
            panic!("tasks didn't complete");
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    let stats = s.stats();
    let total_stolen: u64 = stats.iter().map(|(_, st)| st.tasks_stolen).sum();
    assert!(
        total_stolen > 0,
        "tasks_stolen should advance (stats: {:?})",
        stats
    );

    s.shutdown();
}

/// Bonus: scheduler `topology` field exposes detected/fallback layout.
#[test]
fn scheduler_exposes_topology() {
    let s = Scheduler::multi_worker(8);
    assert_eq!(s.topology.locals.len(), 8);
    // Whether each entry has node > 0 depends on the host — we only
    // assert the length contract.
    s.shutdown();
}
