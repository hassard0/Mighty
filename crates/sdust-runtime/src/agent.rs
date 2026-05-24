//! Agent descriptor + registry (spec §25.2) + per-turn evaluator.
//!
//! ## v0.3 cooperative cancellation
//!
//! The per-turn evaluator is wrapped in
//! `tokio::task::spawn_blocking`. The async parent races the blocking
//! join handle against a [`crate::cancel::CancellationToken`]; when
//! the token fires (wall budget elapsed, ask deadline expired,
//! runtime shutdown), the parent returns immediately and the
//! interpreter thread is detached. The handler's step budget caps
//! the worst-case wall time of the detached thread.
//!
//! ### Reply race
//!
//! The frame's `reply` oneshot is moved into a shared
//! `Mutex<Option<Sender>>` slot before scheduling. Both the blocking
//! shim and the async cancellation arm take it via `.take()`; the
//! first one wins. This guarantees the caller of `ask` sees exactly
//! one outcome — success/trap from the handler **or** the cancellation
//! error — never a hang and never a double-send.

use crate::budget::BudgetTracker;
use crate::cancel::{CancelReason, CancellationToken};
use crate::error::{RuntimeError, RuntimeResult};
use crate::mailbox::{Mailbox, MessageFrame};
use crate::telemetry::{TelemetryEvent, TelemetrySink};
use dashmap::DashMap;
use parking_lot::Mutex;
use sdust_sir::interp::host::Host;
use sdust_sir::interp::value::Value;
use sdust_sir::sir::{AgentSirId, Program};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AgentId(pub u64);

#[derive(Debug)]
pub struct AgentDescriptor {
    pub id: AgentId,
    pub name: String,
    pub sir_id: AgentSirId,
    pub state: Mutex<Value>,
    pub mailbox: Arc<Mailbox>,
    pub budget: Arc<BudgetTracker>,
    pub supervisor: Option<AgentId>,
    pub mailbox_depth: AtomicU64,
}

#[derive(Debug, Clone)]
pub struct AgentHandle {
    pub id: AgentId,
    pub name: String,
    pub mailbox: Arc<Mailbox>,
}

#[derive(Debug, Default)]
pub struct AgentRegistry {
    next_id: AtomicU64,
    by_id: DashMap<AgentId, Arc<AgentDescriptor>>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn next_id(&self) -> AgentId {
        AgentId(self.next_id.fetch_add(1, Ordering::Relaxed))
    }
    pub fn insert(&self, desc: Arc<AgentDescriptor>) {
        self.by_id.insert(desc.id, desc);
    }
    pub fn get(&self, id: AgentId) -> Option<Arc<AgentDescriptor>> {
        self.by_id.get(&id).map(|r| r.clone())
    }
    pub fn remove(&self, id: AgentId) {
        self.by_id.remove(&id);
    }
    pub fn len(&self) -> usize {
        self.by_id.len()
    }
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

/// Result of a per-turn execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnOutcome {
    Completed,
    Cancelled(CancelReason),
}

/// Shared optional reply sender — both the blocking shim and the
/// async cancellation arm race to `.take()` it.
pub type SharedReply = Arc<Mutex<Option<oneshot::Sender<RuntimeResult<Value>>>>>;

/// Run a single agent turn synchronously, dispatching the reply (if
/// any) through `shared_reply`. The shared slot lets the async parent
/// pre-empt the reply on cancellation; the blocking shim only sends
/// when the slot is still occupied.
pub fn run_one_turn_with_shared_reply(
    prog: &Program,
    desc: &AgentDescriptor,
    proto_msg: String,
    payload_args: Vec<Value>,
    shared_reply: &SharedReply,
    host: &mut dyn Host,
    telemetry: &TelemetrySink,
) -> RuntimeResult<()> {
    let agent = prog.agent_by_id(desc.sir_id);
    let handler_fn_id = match agent
        .handlers
        .iter()
        .find(|(m, _)| m == &proto_msg)
        .map(|(_, id)| *id)
    {
        Some(id) => id,
        None => {
            let err = RuntimeError::HandlerNotFound {
                agent: desc.name.clone(),
                msg: proto_msg.clone(),
            };
            if let Some(tx) = shared_reply.lock().take() {
                let _ = tx.send(Err(err.clone()));
            }
            return Err(err);
        }
    };

    telemetry.emit(&TelemetryEvent::TurnStart {
        agent: desc.name.clone(),
        msg: proto_msg.clone(),
    });
    let started = std::time::Instant::now();

    let state_in = desc.state.lock().clone();
    let (rr, new_state, reply_value) = sdust_sir::interp::run::run_handler_isolated(
        prog,
        handler_fn_id,
        state_in,
        payload_args,
        host,
    );

    desc.budget.record_cpu(started.elapsed());
    telemetry.emit(&TelemetryEvent::TurnEnd {
        agent: desc.name.clone(),
        msg: proto_msg.clone(),
        duration_us: started.elapsed().as_micros(),
    });

    match rr {
        sdust_sir::interp::run::RunResult::Ok { .. } => {
            *desc.state.lock() = new_state;
            if let Some(tx) = shared_reply.lock().take() {
                telemetry.emit(&TelemetryEvent::Reply {
                    from: desc.name.clone(),
                    msg: proto_msg.clone(),
                    ok: true,
                });
                let _ = tx.send(Ok(reply_value));
            }
            Ok(())
        }
        sdust_sir::interp::run::RunResult::Trap { code, message } => {
            let err = RuntimeError::Trap { code, message };
            if let Some(tx) = shared_reply.lock().take() {
                telemetry.emit(&TelemetryEvent::Reply {
                    from: desc.name.clone(),
                    msg: proto_msg.clone(),
                    ok: false,
                });
                let _ = tx.send(Err(err.clone()));
            }
            Err(err)
        }
        sdust_sir::interp::run::RunResult::BudgetExceeded => {
            let err = RuntimeError::BudgetExceeded("steps".into());
            if let Some(tx) = shared_reply.lock().take() {
                let _ = tx.send(Err(err.clone()));
            }
            Err(err)
        }
        sdust_sir::interp::run::RunResult::MemBudgetExceeded { used, limit } => {
            let err = RuntimeError::BudgetExceeded(format!("mem {} B > {} B", used, limit));
            if let Some(tx) = shared_reply.lock().take() {
                let _ = tx.send(Err(err.clone()));
            }
            Err(err)
        }
        sdust_sir::interp::run::RunResult::NoMain => {
            let err = RuntimeError::HandlerNotFound {
                agent: desc.name.clone(),
                msg: proto_msg.clone(),
            };
            if let Some(tx) = shared_reply.lock().take() {
                let _ = tx.send(Err(err.clone()));
            }
            Err(err)
        }
    }
}

/// Back-compat: slice-7 synchronous evaluator. Used by tests + the
/// `--legacy-interp` style code path.
pub fn run_one_turn(
    prog: &Program,
    desc: &AgentDescriptor,
    frame: MessageFrame,
    host: &mut dyn Host,
    telemetry: &TelemetrySink,
) -> RuntimeResult<()> {
    let MessageFrame {
        proto_msg,
        payload,
        reply,
        ..
    } = frame;
    let shared: SharedReply = Arc::new(Mutex::new(reply));
    run_one_turn_with_shared_reply(
        prog,
        desc,
        proto_msg,
        payload.into_vec(),
        &shared,
        host,
        telemetry,
    )
}

/// Run a single agent turn under cooperative cancellation. Returns
/// `(result, outcome)`. See module-level doc.
pub async fn run_one_turn_async(
    prog: Arc<Program>,
    desc: Arc<AgentDescriptor>,
    frame: MessageFrame,
    host: Arc<Mutex<Box<dyn Host + Send>>>,
    telemetry: Arc<TelemetrySink>,
    cancel: CancellationToken,
    wall_budget: Option<Duration>,
) -> (RuntimeResult<()>, TurnOutcome) {
    let MessageFrame {
        proto_msg,
        payload,
        reply,
        deadline: _,
        seq: _,
        _slab,
    } = frame;
    let shared_reply: SharedReply = Arc::new(Mutex::new(reply));
    let payload_args = payload.into_vec();

    // Arm the wall-budget timer (per-turn timeout precision; v0.3 #4).
    let timer_handle = wall_budget.map(|d| cancel.arm_wall_budget(d));

    let prog_b = prog.clone();
    let desc_b = desc.clone();
    let host_b = host.clone();
    let tele_b = telemetry.clone();
    let reply_b = shared_reply.clone();
    let proto_b = proto_msg.clone();
    // Keep `_slab` alive across the spawn_blocking — capture it.
    let _slab_keep = _slab;
    let blocking = tokio::task::spawn_blocking(move || {
        let mut host_guard = host_b.lock();
        let res = run_one_turn_with_shared_reply(
            &prog_b,
            &desc_b,
            proto_b,
            payload_args,
            &reply_b,
            host_guard.as_mut(),
            &tele_b,
        );
        drop(_slab_keep);
        res
    });

    let cancelled_fut = cancel.cancelled();
    tokio::pin!(cancelled_fut);
    let mut blocking_fused = blocking;

    tokio::select! {
        biased;
        _ = &mut cancelled_fut => {
            // Cancellation won. Notify the caller with an error.
            if let Some(h) = &timer_handle { h.abort(); }
            let reason = cancel.reason().unwrap_or(CancelReason::Shutdown);
            let code = reason.diag_code();
            telemetry.emit(&TelemetryEvent::BudgetBreach {
                agent: desc.name.clone(),
                kind: code.into(),
            });
            let err: RuntimeError = match reason {
                CancelReason::AskDeadline => RuntimeError::DeadlineExceeded(
                    wall_budget.unwrap_or(Duration::from_millis(0)),
                ),
                _ => RuntimeError::BudgetExceeded(format!(
                    "{} ({})",
                    reason.as_str(),
                    code
                )),
            };
            // Race the shared reply: if the blocking shim already
            // replied, leave it; otherwise we own the reply now.
            if let Some(tx) = shared_reply.lock().take() {
                let _ = tx.send(Err(err.clone()));
            }
            // Detach: do not join the blocking task. Step budget caps
            // its worst-case wall time.
            drop(blocking_fused);
            (Err(err), TurnOutcome::Cancelled(reason))
        }
        joined = &mut blocking_fused => {
            if let Some(h) = timer_handle { h.abort(); }
            match joined {
                Ok(result) => (result, TurnOutcome::Completed),
                Err(join_err) => {
                    let err = RuntimeError::AgentPanic { msg: format!("{join_err}") };
                    if let Some(tx) = shared_reply.lock().take() {
                        let _ = tx.send(Err(err.clone()));
                    }
                    (Err(err), TurnOutcome::Completed)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::Budget;
    use crate::mailbox::SendPolicy;

    #[test]
    fn registry_round_trip() {
        let reg = AgentRegistry::new();
        let id = reg.next_id();
        let desc = Arc::new(AgentDescriptor {
            id,
            name: "X".into(),
            sir_id: AgentSirId(0),
            state: Mutex::new(Value::Unit),
            mailbox: Arc::new(Mailbox::new(8, SendPolicy::Block)),
            budget: Arc::new(BudgetTracker::new(Budget::default())),
            supervisor: None,
            mailbox_depth: AtomicU64::new(0),
        });
        reg.insert(desc.clone());
        let got = reg.get(id).unwrap();
        assert_eq!(got.name, "X");
        assert_eq!(reg.len(), 1);
        reg.remove(id);
        assert!(reg.is_empty());
    }
}
