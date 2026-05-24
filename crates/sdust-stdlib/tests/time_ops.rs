//! `std.time` monotonic clock + sleep.

use sdust_stdlib::time::{now, sleep_blocking, Clock};
use std::time::Duration;

#[test]
fn now_sleep_now_is_increasing() {
    let a = now(Clock);
    sleep_blocking(Clock, Duration::from_millis(5));
    let b = now(Clock);
    assert!(b.elapsed_since(a) >= Duration::from_millis(5));
}

#[test]
fn elapsed_zero_when_same_instant() {
    let a = now(Clock);
    assert_eq!(a.elapsed_since(a), Duration::ZERO);
}

#[tokio::test]
async fn async_sleep_advances() {
    use sdust_stdlib::time::sleep;
    let a = now(Clock);
    sleep(Clock, Duration::from_millis(10)).await;
    let b = now(Clock);
    assert!(b.elapsed_since(a) >= Duration::from_millis(10));
}
