//! v0.3 #4 (batched timers): DelayScheduler tests.

use sdust_runtime::delay_timers::DelayScheduler;
use std::time::Duration;

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn multiple_deadlines_fire_in_order() {
    let s = DelayScheduler::new();
    let h1 = s.schedule(Duration::from_millis(50)).await;
    let h2 = s.schedule(Duration::from_millis(75)).await;
    let h3 = s.schedule(Duration::from_millis(100)).await;
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_millis(60)).await;
    let first = tokio::time::timeout(Duration::from_millis(50), s.next_fired())
        .await
        .ok()
        .flatten()
        .expect("first deadline");
    assert_eq!(first, h1.id(), "first scheduled fires first");
    tokio::time::advance(Duration::from_millis(20)).await;
    let second = tokio::time::timeout(Duration::from_millis(50), s.next_fired())
        .await
        .ok()
        .flatten()
        .expect("second deadline");
    assert_eq!(second, h2.id());
    tokio::time::advance(Duration::from_millis(30)).await;
    let third = tokio::time::timeout(Duration::from_millis(50), s.next_fired())
        .await
        .ok()
        .flatten()
        .expect("third deadline");
    assert_eq!(third, h3.id());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn cancelling_handle_drops_deadline() {
    let s = DelayScheduler::new();
    let _h = s.schedule(Duration::from_millis(20)).await;
    tokio::task::yield_now().await;
    drop(_h);
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_millis(100)).await;
    let res = tokio::time::timeout(Duration::from_millis(50), s.next_fired()).await;
    assert!(res.is_err(), "cancelled deadline must not fire");
}
