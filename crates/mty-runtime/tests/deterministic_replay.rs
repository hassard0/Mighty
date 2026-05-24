use mty_runtime::deterministic::{LogicalClock, SeededRng};
use std::time::Duration;

#[test]
fn replay_byte_identical() {
    let mut a = SeededRng::new(7);
    let mut b = SeededRng::new(7);
    let xs: Vec<u64> = (0..16).map(|_| a.next_u64()).collect();
    let ys: Vec<u64> = (0..16).map(|_| b.next_u64()).collect();
    assert_eq!(xs, ys);
}

#[test]
fn different_seeds_diverge() {
    let mut a = SeededRng::new(1);
    let mut b = SeededRng::new(2);
    assert_ne!(a.next_u64(), b.next_u64());
}

#[test]
fn logical_clock_advances() {
    let mut c = LogicalClock::default();
    assert_eq!(c.now_ns, 0);
    c.advance(Duration::from_millis(5));
    assert_eq!(c.now_ns, 5_000_000);
    c.advance(Duration::from_micros(3));
    assert_eq!(c.now_ns, 5_003_000);
}
