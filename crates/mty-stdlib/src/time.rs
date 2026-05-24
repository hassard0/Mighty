//! `std.time` — monotonic clock + duration arithmetic.
//!
//! Wraps `std::time::Instant` (monotonic on all supported platforms) and
//! `std::time::Duration`. A `Clock` capability is required to call
//! `now` / `sleep`; the runtime synthesizes one per agent based on the
//! manifest's `time` grant.

use std::time::{Duration, Instant as StdInstant};

#[derive(Debug, Clone, Copy)]
pub struct Clock;

#[derive(Debug, Clone, Copy)]
pub struct Instant(pub StdInstant);

impl Instant {
    /// Time elapsed between `other` and `self`. Returns `Duration::ZERO`
    /// when `self < other` (matches std's `checked_duration_since`).
    pub fn elapsed_since(self, other: Instant) -> Duration {
        self.0.checked_duration_since(other.0).unwrap_or_default()
    }
}

pub fn now(_cap: Clock) -> Instant {
    Instant(StdInstant::now())
}

pub async fn sleep(_cap: Clock, dur: Duration) {
    tokio::time::sleep(dur).await;
}

/// Synchronous (blocking) sleep — used by the JIT/interpreter when no
/// tokio context is available.
pub fn sleep_blocking(_cap: Clock, dur: Duration) {
    std::thread::sleep(dur);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_is_monotonic() {
        let a = now(Clock);
        std::thread::sleep(Duration::from_millis(1));
        let b = now(Clock);
        assert!(b.elapsed_since(a) >= Duration::from_millis(1));
    }

    #[test]
    fn elapsed_handles_reversed() {
        let a = now(Clock);
        std::thread::sleep(Duration::from_millis(1));
        let b = now(Clock);
        // a < b → a.elapsed_since(b) saturates to zero.
        assert_eq!(a.elapsed_since(b), Duration::ZERO);
    }
}
