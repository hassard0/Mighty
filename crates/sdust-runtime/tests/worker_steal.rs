//! v0.6 work-stealing: when a worker has no local work, it should
//! steal from siblings.

use sdust_runtime::scheduler::{Affinity, Scheduler, SpawnTask};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[test]
fn steal_balances_load() {
    let s = Arc::new(Scheduler::multi_worker(4));
    let counter = Arc::new(AtomicU32::new(0));

    // Submit 64 tasks via the global injector. With 4 workers and
    // batch-stealing, all of them should complete and most should
    // complete on workers other than worker 0.
    for i in 0..64u64 {
        let c = counter.clone();
        s.submit(SpawnTask {
            id: i,
            affinity: Affinity::Elastic,
            run: Box::new(move |h| {
                let c = c.clone();
                h.spawn(async move {
                    // small busy-ish span so the scheduler actually
                    // distributes work
                    let now = std::time::Instant::now();
                    while now.elapsed() < Duration::from_micros(200) {
                        std::hint::spin_loop();
                    }
                    c.fetch_add(1, Ordering::Relaxed);
                });
            }),
        });
    }

    let start = std::time::Instant::now();
    while counter.load(Ordering::Relaxed) < 64 {
        if start.elapsed() > Duration::from_secs(10) {
            panic!(
                "only {} of 64 tasks completed",
                counter.load(Ordering::Relaxed)
            );
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(counter.load(Ordering::Relaxed), 64);

    // Verify some stealing happened. With 4 workers and the global
    // injector pattern, stealing should be > 0.
    let stats = s.stats();
    let total_stolen: u64 = stats.iter().map(|(_, s)| s.tasks_stolen).sum();
    assert!(
        total_stolen > 0,
        "expected some work-stealing to occur, got 0 (stats: {:?})",
        stats
    );

    s.shutdown();
}

#[test]
fn empty_workers_park_then_wake() {
    let s = Arc::new(Scheduler::multi_worker(2));
    // Give them a moment to enter park.
    std::thread::sleep(Duration::from_millis(100));

    let parks_before: u64 = s.stats().iter().map(|(_, s)| s.parks).sum();
    assert!(parks_before > 0, "workers should have parked while idle");

    // Now submit work; they should wake and execute.
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    s.submit(SpawnTask {
        id: 0,
        affinity: Affinity::Elastic,
        run: Box::new(move |h| {
            let c = c.clone();
            h.spawn(async move {
                c.fetch_add(1, Ordering::Relaxed);
            });
        }),
    });
    let start = std::time::Instant::now();
    while counter.load(Ordering::Relaxed) == 0 {
        if start.elapsed() > Duration::from_secs(2) {
            panic!("notified worker never ran the task");
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    s.shutdown();
}
