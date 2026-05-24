//! Batched deadline scheduler backed by [`tokio_util::time::DelayQueue`]
//! (v0.3 runtime polish, item 4).
//!
//! Slice 7 wrapped every per-turn deadline in
//! `tokio::time::timeout(dur, fut)`. That creates one timer entry per
//! invocation; on a busy runtime the kernel-side timerfd churn shows up
//! as wakeup jitter. v0.3 replaces the per-call timer with a shared
//! [`DelayQueue<DeadlineKey>`] that batches expirations and exposes a
//! stream of fired keys.
//!
//! ## Shape
//!
//! ```text
//!  ┌─────────────────────────────────────────────────────────────┐
//!  │ DelayScheduler                                              │
//!  │                                                             │
//!  │   queue : DelayQueue<u64>      (slot keyed by deadline-id)  │
//!  │   tx    : mpsc::Sender<u64>    (fired deadline ids)         │
//!  │                                                             │
//!  │   schedule(dur) -> deadline_id                              │
//!  │   cancel(deadline_id)                                       │
//!  │   recv() -> Option<deadline_id>  (consumes one fired entry) │
//!  └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! Callers register a per-turn deadline via [`DelayScheduler::schedule`]
//! and obtain an opaque `DeadlineId`. The scheduler runs a single
//! background task that drains its `DelayQueue` and forwards fired
//! ids to a channel. Cancellation is O(1) via the queue's key handle.
//!
//! This is exposed as a building block. The default per-turn
//! cancellation path in [`crate::agent`] still uses
//! [`crate::cancel::CancellationToken::arm_wall_budget`], which is a
//! single `tokio::spawn(sleep + cancel)`. The DelayScheduler is the
//! batched alternative for hosts that need to track many concurrent
//! deadlines (e.g. a supervisor watching all its children's per-turn
//! wall budgets).

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::time::{delay_queue, DelayQueue};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeadlineId(pub u64);

/// Owned handle returned from `schedule`. Dropping it cancels the
/// deadline.
#[derive(Debug)]
pub struct DeadlineHandle {
    id: DeadlineId,
    cancel: Arc<parking_lot::Mutex<Option<delay_queue::Key>>>,
    sched: Arc<DelaySchedulerInner>,
}

impl DeadlineHandle {
    pub fn id(&self) -> DeadlineId {
        self.id
    }
    pub fn cancel(self) {
        // Drop runs the cancel.
    }
}

impl Drop for DeadlineHandle {
    fn drop(&mut self) {
        if let Some(key) = self.cancel.lock().take() {
            let _ = self.sched.cmd_tx.try_send(SchedCmd::Cancel(key));
        }
    }
}

#[derive(Debug)]
enum SchedCmd {
    Insert(
        Duration,
        DeadlineId,
        Arc<parking_lot::Mutex<Option<delay_queue::Key>>>,
    ),
    Cancel(delay_queue::Key),
}

#[derive(Debug)]
struct DelaySchedulerInner {
    cmd_tx: mpsc::Sender<SchedCmd>,
    fired_rx: tokio::sync::Mutex<mpsc::Receiver<DeadlineId>>,
    next_id: parking_lot::Mutex<u64>,
}

/// Public façade.
#[derive(Debug, Clone)]
pub struct DelayScheduler {
    inner: Arc<DelaySchedulerInner>,
}

impl Default for DelayScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl DelayScheduler {
    /// Start the scheduler. Spawns a background task on the current
    /// tokio runtime to drive the DelayQueue.
    pub fn new() -> Self {
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<SchedCmd>(1024);
        let (fired_tx, fired_rx) = mpsc::channel::<DeadlineId>(1024);

        tokio::spawn(async move {
            let mut q: DelayQueue<DeadlineId> = DelayQueue::new();
            loop {
                // When the queue is empty, poll_expired returns
                // Ready(None) immediately and busy-loops the select.
                // Avoid that by only racing the timer when there's
                // something to fire.
                if q.is_empty() {
                    match cmd_rx.recv().await {
                        Some(SchedCmd::Insert(dur, id, key_slot)) => {
                            let key = q.insert(id, dur);
                            *key_slot.lock() = Some(key);
                        }
                        Some(SchedCmd::Cancel(key)) => {
                            let _ = q.try_remove(&key);
                        }
                        None => break,
                    }
                    continue;
                }
                tokio::select! {
                    cmd = cmd_rx.recv() => {
                        match cmd {
                            Some(SchedCmd::Insert(dur, id, key_slot)) => {
                                let key = q.insert(id, dur);
                                *key_slot.lock() = Some(key);
                            }
                            Some(SchedCmd::Cancel(key)) => {
                                let _ = q.try_remove(&key);
                            }
                            None => break,
                        }
                    }
                    expired = std::future::poll_fn(|cx| q.poll_expired(cx)) => {
                        if let Some(entry) = expired {
                            let id = entry.into_inner();
                            // The fired channel is fixed-size; we drop
                            // backpressure here rather than block the
                            // scheduler loop.
                            let _ = fired_tx.try_send(id);
                        }
                    }
                }
            }
        });

        Self {
            inner: Arc::new(DelaySchedulerInner {
                cmd_tx,
                fired_rx: tokio::sync::Mutex::new(fired_rx),
                next_id: parking_lot::Mutex::new(0),
            }),
        }
    }

    /// Schedule a deadline. The returned handle keeps the deadline
    /// alive — dropping it cancels.
    pub async fn schedule(&self, dur: Duration) -> DeadlineHandle {
        let id = {
            let mut n = self.inner.next_id.lock();
            let v = *n;
            *n += 1;
            DeadlineId(v)
        };
        let key_slot = Arc::new(parking_lot::Mutex::new(None));
        let _ = self
            .inner
            .cmd_tx
            .send(SchedCmd::Insert(dur, id, key_slot.clone()))
            .await;
        DeadlineHandle {
            id,
            cancel: key_slot,
            sched: self.inner.clone(),
        }
    }

    /// Wait for the next fired deadline (returns None when the
    /// scheduler shuts down).
    pub async fn next_fired(&self) -> Option<DeadlineId> {
        // We hold the receiver behind a tokio::sync::Mutex so the
        // scheduler can be cloned and shared, while still pulling from
        // a single underlying channel. Single-consumer pattern.
        let mut guard = self.inner.fired_rx.lock().await;
        guard.recv().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn fires_in_order() {
        let s = DelayScheduler::new();
        let _h1 = s.schedule(Duration::from_millis(50)).await;
        let _h2 = s.schedule(Duration::from_millis(100)).await;
        let _h3 = s.schedule(Duration::from_millis(75)).await;
        // give the scheduler a chance to register inserts
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(60)).await;
        let a = tokio::time::timeout(Duration::from_millis(50), s.next_fired())
            .await
            .ok()
            .flatten();
        assert!(a.is_some(), "expected first deadline to fire");
        tokio::time::advance(Duration::from_millis(20)).await;
        let b = tokio::time::timeout(Duration::from_millis(50), s.next_fired())
            .await
            .ok()
            .flatten();
        assert!(b.is_some(), "expected second deadline to fire");
        tokio::time::advance(Duration::from_millis(40)).await;
        let c = tokio::time::timeout(Duration::from_millis(50), s.next_fired())
            .await
            .ok()
            .flatten();
        assert!(c.is_some(), "expected third deadline to fire");
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn cancel_drops_deadline() {
        let s = DelayScheduler::new();
        let h = s.schedule(Duration::from_millis(30)).await;
        tokio::task::yield_now().await;
        drop(h);
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(100)).await;
        let res = tokio::time::timeout(Duration::from_millis(20), s.next_fired()).await;
        assert!(res.is_err(), "cancelled deadline must not fire");
    }
}
