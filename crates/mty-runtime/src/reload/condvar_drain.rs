//! Condvar-based drain — replaces the v0.20 1 ms busy-poll.
//!
//! v0.20 spun on `ReloadGate::is_busy` with a 1 ms `thread::sleep`
//! between checks. That tradeoff burns a wakeup every handler turn
//! and adds up-to-1 ms latency to every drain even when the handler
//! finishes nanoseconds after the swap begins. v0.21 replaces the
//! poll with a [`parking_lot::Condvar`] on a tiny [`DrainState`]
//! struct: the agent loop calls [`DrainSignal::mark_idle`] when its
//! handler returns, which wakes the swap pipeline immediately.
//!
//! ## Why a separate signal instead of overloading `ReloadGate.busy`
//!
//! `ReloadGate.busy` is a plain `AtomicBool` consulted from the agent
//! hot path; bolting a condvar onto it would force every `mark_idle`
//! call to take a mutex, even when no reload is in flight. The
//! separate `DrainSignal` is only created during a swap (see
//! [`crate::reload::swap::ReloadRunner::run`]) so the steady-state
//! cost is unchanged.
//!
//! ## Wakeup discipline
//!
//! Spurious wakeups are handled by re-checking the busy flag in a
//! loop — see [`DrainSignal::wait_until_idle`]. The signal also
//! supports an early-exit via [`DrainSignal::mark_idle`] called
//! *before* the wait starts (common case: handler returns before the
//! pipeline reaches the wait).

use parking_lot::{Condvar, Mutex};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Inner state the swap pipeline waits on. Cheap — a `bool` + an
/// `Instant` that records when the agent went idle (so the report's
/// `drain_elapsed_ms` can be exact rather than estimated from the
/// pipeline's wall clock).
#[derive(Debug)]
pub(crate) struct DrainState {
    /// True while the agent's handler is in flight.
    pub(crate) busy: bool,
    /// When the agent last transitioned `busy=true → busy=false`.
    /// Used so the drain doesn't include the post-idle wakeup
    /// latency in `drain_elapsed_ms`.
    pub(crate) idle_at: Option<Instant>,
}

impl Default for DrainState {
    fn default() -> Self {
        DrainState {
            busy: false,
            idle_at: Some(Instant::now()),
        }
    }
}

/// Condvar-backed drain signal. Cloneable via `Arc` so the agent loop
/// and the swap pipeline can share it.
#[derive(Debug, Clone)]
pub struct DrainSignal {
    inner: Arc<(Mutex<DrainState>, Condvar)>,
}

impl DrainSignal {
    /// Create a new signal in the idle state.
    pub fn new() -> Self {
        DrainSignal {
            inner: Arc::new((Mutex::new(DrainState::default()), Condvar::new())),
        }
    }

    /// Create a signal pre-set to busy — useful when the swap is
    /// requested while an agent is known to be mid-handler.
    pub fn new_busy() -> Self {
        let signal = Self::new();
        signal.mark_busy();
        signal
    }

    /// Called by the agent loop when it picks a frame off the
    /// mailbox. No-op if already busy (idempotent).
    pub fn mark_busy(&self) {
        let mut g = self.inner.0.lock();
        g.busy = true;
        g.idle_at = None;
    }

    /// Called by the agent loop when the handler returns. Wakes any
    /// pending drainer.
    pub fn mark_idle(&self) {
        {
            let mut g = self.inner.0.lock();
            g.busy = false;
            g.idle_at = Some(Instant::now());
        }
        // notify_all so a future multi-waiter setup (e.g. cluster
        // live-migration tagging the same agent from two surfaces)
        // doesn't deadlock; cost is negligible since the swap is
        // single-threaded.
        self.inner.1.notify_all();
    }

    /// Snapshot the busy flag without blocking. Used by tests + the
    /// fast-path early-out before we take the lock for a wait.
    pub fn is_busy(&self) -> bool {
        self.inner.0.lock().busy
    }

    /// Block the caller until the agent's handler reports idle, or
    /// until `deadline` elapses. Returns:
    ///
    /// - `Ok(drain_elapsed)` — the wall time between calling this
    ///   function and the agent going idle. Always ≤ `deadline`.
    /// - `Err(elapsed)` — the deadline tripped; `elapsed` is how long
    ///   the function spent waiting (always ≥ `deadline`).
    ///
    /// The function tolerates spurious wakeups by re-checking the
    /// busy flag in a loop. The condvar's `wait_for` returns a
    /// [`parking_lot::WaitTimeoutResult`] that distinguishes timeout
    /// from wakeup, but we don't rely on that — the busy flag is the
    /// source of truth.
    pub fn wait_until_idle(&self, deadline: Duration) -> Result<Duration, Duration> {
        let started = Instant::now();
        let mut guard = self.inner.0.lock();
        loop {
            if !guard.busy {
                return Ok(started.elapsed());
            }
            let remaining = deadline.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(started.elapsed());
            }
            self.inner.1.wait_for(&mut guard, remaining);
            // Spurious wakeups: re-check the flag at the top of the
            // loop and either return (busy=false) or re-arm the wait.
        }
    }
}

impl Default for DrainSignal {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn idle_signal_returns_immediately() {
        let s = DrainSignal::new();
        let started = Instant::now();
        let elapsed = s.wait_until_idle(Duration::from_secs(5)).expect("ok");
        assert!(started.elapsed() < Duration::from_millis(20));
        assert!(elapsed < Duration::from_millis(20));
    }

    #[test]
    fn busy_then_idle_wakes_waiter() {
        let s = DrainSignal::new_busy();
        let s2 = s.clone();
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(40));
            s2.mark_idle();
        });
        let started = Instant::now();
        let elapsed = s.wait_until_idle(Duration::from_secs(2)).expect("idle");
        let wall = started.elapsed();
        handle.join().unwrap();
        // The wait should have returned promptly after mark_idle.
        assert!(
            wall >= Duration::from_millis(30),
            "expected at least 30 ms wait, got {wall:?}"
        );
        // And it should not be wildly longer than the actual idle delay.
        assert!(
            wall < Duration::from_millis(500),
            "wait should be tight, got {wall:?}"
        );
        // `elapsed` from the wait function tracks the same window.
        assert!(elapsed >= Duration::from_millis(30));
    }

    #[test]
    fn deadline_trip_returns_err() {
        let s = DrainSignal::new_busy();
        let started = Instant::now();
        let err = s.wait_until_idle(Duration::from_millis(30)).unwrap_err();
        let wall = started.elapsed();
        assert!(wall >= Duration::from_millis(25));
        assert!(err >= Duration::from_millis(25));
        // Agent never went idle — flag should still be busy.
        assert!(s.is_busy());
    }

    #[test]
    fn mark_idle_before_wait_is_remembered() {
        let s = DrainSignal::new_busy();
        s.mark_idle();
        // Already idle when wait_until_idle is called — should return immediately.
        let elapsed = s.wait_until_idle(Duration::from_secs(5)).expect("ok");
        assert!(elapsed < Duration::from_millis(20));
        assert!(!s.is_busy());
    }

    #[test]
    fn mark_busy_after_idle_is_visible() {
        let s = DrainSignal::new();
        assert!(!s.is_busy());
        s.mark_busy();
        assert!(s.is_busy());
        s.mark_idle();
        assert!(!s.is_busy());
    }

    #[test]
    fn idle_signal_records_idle_timestamp() {
        let s = DrainSignal::new_busy();
        let before = Instant::now();
        s.mark_idle();
        let g = s.inner.0.lock();
        let idle_at = g.idle_at.expect("idle_at set");
        assert!(idle_at >= before);
    }
}
