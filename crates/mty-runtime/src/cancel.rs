//! Cooperative per-turn cancellation (v0.3, closes A41).
//!
//! Slice 7's runtime could only fire `task scope @D` / `?Msg @D`
//! deadlines *between* turns because the SIR interpreter ran
//! synchronously on a worker thread. A handler that genuinely looped
//! could ignore the deadline indefinitely (bounded only by the
//! interpreter's per-turn step budget).
//!
//! v0.3 introduces a [`CancellationToken`] passed into every
//! [`crate::agent::run_one_turn`] call. The token is observed in two
//! places:
//!
//! 1. **`tokio::select!` race** — the synchronous interpreter is
//!    spawned via `spawn_blocking` and the runtime races its
//!    `JoinHandle` against a `cancel.cancelled()` future. When the
//!    wall-budget timer fires the token is `cancel()`-ed, the select
//!    aborts the wait, and the runtime emits `SD5xxx` telemetry.
//! 2. **Spawn-blocking thread cooperation** — once the select aborts
//!    the runtime *does not* try to join the blocking thread (that
//!    could block waiting on a runaway loop forever). Instead the
//!    thread is detached and the agent is removed from the registry;
//!    its handler is treated as a hard trap with SD5011 (deadline) or
//!    SD5009 (CPU budget). Subsequent turns simply never run because
//!    the agent loop has exited.
//!
//! This delivers the spec-required *interruption mid-turn* without
//! teaching the SIR interpreter about cancellation (which would
//! violate the v0.3 swarm scope rule that this crate must not modify
//! `mty-ir`).
//!
//! ## Wrapping the interpreter
//!
//! The interpreter is single-threaded synchronous Rust. The runtime
//! cannot literally pre-empt it without a separate thread. v0.3 uses
//! `tokio::task::spawn_blocking` to run the turn on tokio's blocking
//! thread pool. The async parent task races the blocking task's join
//! handle against `cancel.cancelled()`. On cancel we:
//!
//! - emit `BudgetBreach { kind: SD5011|SD5009 }` telemetry,
//! - send the failure on the reply oneshot (so `ask` callers get a
//!   timely error instead of waiting for the underlying thread),
//! - drop the JoinHandle (detaches the blocking thread).
//!
//! The detached thread eventually returns (its step budget caps the
//! worst-case wall time at ~1 M steps × a few µs per step ≈ <10 s on
//! the slowest hosts). It cannot affect any agent state because the
//! turn's input was already cloned out before `spawn_blocking` and
//! the agent registry entry has been removed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken as TokioCancel;

/// Why a turn was cancelled. Mapped to SD5xxx in telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelReason {
    /// `wall` budget on the agent fired.
    WallBudget,
    /// `cpu` budget on the agent fired.
    CpuBudget,
    /// Caller's `?Msg @D` deadline fired before the handler completed.
    AskDeadline,
    /// Runtime shutdown.
    Shutdown,
}

impl CancelReason {
    /// SD5xxx code emitted in telemetry when the turn is killed.
    pub fn diag_code(self) -> &'static str {
        match self {
            CancelReason::WallBudget => "SD5009",
            CancelReason::CpuBudget => "SD5009",
            CancelReason::AskDeadline => "SD5011",
            CancelReason::Shutdown => "SD5020",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CancelReason::WallBudget => "wall_budget",
            CancelReason::CpuBudget => "cpu_budget",
            CancelReason::AskDeadline => "ask_deadline",
            CancelReason::Shutdown => "shutdown",
        }
    }
}

/// Cheap-to-clone cancellation handle. Wraps `tokio_util`'s
/// `CancellationToken` plus a `CancelReason` field set by whichever
/// task fires the cancel.
#[derive(Debug, Clone)]
pub struct CancellationToken {
    inner: TokioCancel,
    reason: Arc<parking_lot::Mutex<Option<CancelReason>>>,
    fired: Arc<AtomicBool>,
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            inner: TokioCancel::new(),
            reason: Arc::new(parking_lot::Mutex::new(None)),
            fired: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Construct from an existing tokio token (used when wiring into
    /// a parent scope's cancellation tree).
    pub fn from_tokio(inner: TokioCancel) -> Self {
        Self {
            inner,
            reason: Arc::new(parking_lot::Mutex::new(None)),
            fired: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get the inner tokio token for use with `tokio::select!`.
    pub fn inner(&self) -> &TokioCancel {
        &self.inner
    }

    /// Fire the cancellation. Idempotent — the first reason wins.
    pub fn cancel(&self, reason: CancelReason) {
        if !self.fired.swap(true, Ordering::AcqRel) {
            *self.reason.lock() = Some(reason);
            self.inner.cancel();
        }
    }

    /// True if the token has been fired.
    pub fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }

    /// Reason this token was fired, if it has been.
    pub fn reason(&self) -> Option<CancelReason> {
        *self.reason.lock()
    }

    /// Future that resolves once this token is cancelled. Wraps
    /// `tokio_util::sync::CancellationToken::cancelled`.
    pub fn cancelled(&self) -> impl std::future::Future<Output = ()> + Send + 'static {
        let inner = self.inner.clone();
        async move {
            inner.cancelled().await;
        }
    }

    /// Construct a child token that fires when either this token or
    /// the parent does. Used to nest per-turn cancellation under a
    /// runtime-wide shutdown token.
    pub fn child(&self) -> CancellationToken {
        let child = self.inner.child_token();
        Self {
            inner: child,
            reason: Arc::new(parking_lot::Mutex::new(None)),
            fired: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Spawn a tokio task that fires the cancel after `dur` elapses.
    /// Returns the JoinHandle so callers can abort the timer when the
    /// turn completes early.
    pub fn arm_wall_budget(&self, dur: Duration) -> tokio::task::JoinHandle<()> {
        let tok = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(dur).await;
            tok.cancel(CancelReason::WallBudget);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fire_and_observe() {
        let t = CancellationToken::new();
        let f = t.cancelled();
        let t2 = t.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            t2.cancel(CancelReason::WallBudget);
        });
        f.await;
        assert!(t.is_cancelled());
        assert_eq!(t.reason(), Some(CancelReason::WallBudget));
    }

    #[tokio::test]
    async fn child_inherits_cancel() {
        let parent = CancellationToken::new();
        let child = parent.child();
        let f = child.cancelled();
        parent.cancel(CancelReason::Shutdown);
        f.await;
        assert!(child.is_cancelled());
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn arm_wall_budget_fires() {
        let t = CancellationToken::new();
        let _h = t.arm_wall_budget(Duration::from_millis(50));
        // give the spawned timer task a chance to register its sleep
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(60)).await;
        tokio::task::yield_now().await;
        assert!(t.is_cancelled());
        assert_eq!(t.reason(), Some(CancelReason::WallBudget));
    }

    #[test]
    fn diag_codes_map() {
        assert_eq!(CancelReason::WallBudget.diag_code(), "SD5009");
        assert_eq!(CancelReason::CpuBudget.diag_code(), "SD5009");
        assert_eq!(CancelReason::AskDeadline.diag_code(), "SD5011");
        assert_eq!(CancelReason::Shutdown.diag_code(), "SD5020");
    }
}
