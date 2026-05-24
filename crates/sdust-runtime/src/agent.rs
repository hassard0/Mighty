//! Agent descriptor + registry (spec §25.2) + per-turn evaluator.

use crate::budget::BudgetTracker;
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

/// Run a single agent turn. Returns the reply value (or runtime error).
///
/// State is read from `desc.state`, threaded into the handler invocation
/// via `sdust_sir::interp::run::run_handler_isolated`, and any state
/// mutation is written back to `desc.state` before returning.
pub fn run_one_turn(
    prog: &Program,
    desc: &AgentDescriptor,
    frame: MessageFrame,
    host: &mut dyn Host,
    telemetry: &TelemetrySink,
) -> RuntimeResult<()> {
    let agent = prog.agent_by_id(desc.sir_id);
    let handler_fn_id = match agent
        .handlers
        .iter()
        .find(|(m, _)| m == &frame.proto_msg)
        .map(|(_, id)| *id)
    {
        Some(id) => id,
        None => {
            let err = RuntimeError::HandlerNotFound {
                agent: desc.name.clone(),
                msg: frame.proto_msg.clone(),
            };
            if let Some(reply) = frame.reply {
                let _ = reply.send(Err(err.clone()));
            }
            return Err(err);
        }
    };

    telemetry.emit(&TelemetryEvent::TurnStart {
        agent: desc.name.clone(),
        msg: frame.proto_msg.clone(),
    });
    let started = std::time::Instant::now();

    let state_in = desc.state.lock().clone();
    let msg_args = frame.payload.into_vec();

    let (rr, new_state, reply_value) =
        sdust_sir::interp::run::run_handler_isolated(prog, handler_fn_id, state_in, msg_args, host);

    desc.budget.record_cpu(started.elapsed());
    telemetry.emit(&TelemetryEvent::TurnEnd {
        agent: desc.name.clone(),
        msg: frame.proto_msg.clone(),
        duration_us: started.elapsed().as_micros(),
    });

    match rr {
        sdust_sir::interp::run::RunResult::Ok { .. } => {
            *desc.state.lock() = new_state;
            if let Some(reply) = frame.reply {
                telemetry.emit(&TelemetryEvent::Reply {
                    from: desc.name.clone(),
                    msg: frame.proto_msg.clone(),
                    ok: true,
                });
                let _ = reply.send(Ok(reply_value));
            }
            Ok(())
        }
        sdust_sir::interp::run::RunResult::Trap { code, message } => {
            let err = RuntimeError::Trap { code, message };
            if let Some(reply) = frame.reply {
                telemetry.emit(&TelemetryEvent::Reply {
                    from: desc.name.clone(),
                    msg: frame.proto_msg.clone(),
                    ok: false,
                });
                let _ = reply.send(Err(err.clone()));
            }
            Err(err)
        }
        sdust_sir::interp::run::RunResult::BudgetExceeded => {
            let err = RuntimeError::BudgetExceeded("steps".into());
            if let Some(reply) = frame.reply {
                let _ = reply.send(Err(err.clone()));
            }
            Err(err)
        }
        sdust_sir::interp::run::RunResult::NoMain => {
            // Handler somehow missing — fail closed.
            let err = RuntimeError::HandlerNotFound {
                agent: desc.name.clone(),
                msg: frame.proto_msg.clone(),
            };
            if let Some(reply) = frame.reply {
                let _ = reply.send(Err(err.clone()));
            }
            Err(err)
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
