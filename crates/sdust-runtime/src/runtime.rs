//! Runtime + RuntimeBuilder.
//!
//! v0.3 changes (vs slice 7):
//!
//! - per-agent shutdown [`CancellationToken`] (children of a
//!   runtime-wide root) so `Runtime::shutdown` co-operatively
//!   interrupts in-flight turns.
//! - per-turn wall-budget timer wired through
//!   [`crate::agent::run_one_turn_async`].
//! - mailbox capacity defaults to the slab-pool default
//!   ([`crate::slab_pool::DEFAULT_POOL_SIZE`]).

use crate::agent::{run_one_turn_async, AgentDescriptor, AgentHandle, AgentRegistry, TurnOutcome};
use crate::budget::{Budget, BudgetTracker};
use crate::cancel::{CancelReason, CancellationToken};
use crate::error::{RuntimeError, RuntimeResult};
use crate::host_std::StdHost;
use crate::mailbox::{Mailbox, MessageFrame, SendPolicy, SmallPayload};
use crate::scheduler::Scheduler;
use crate::slab_pool::DEFAULT_POOL_SIZE;
use crate::supervisor::SupervisorRegistry;
use crate::telemetry::{TelemetryEvent, TelemetrySink};
use crate::timer::with_deadline;
use parking_lot::Mutex;
use sdust_sir::interp::host::Host;
use sdust_sir::interp::value::Value;
use sdust_sir::sir::{Agent as SirAgent, Program};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;

#[derive(Debug, Clone)]
pub enum RunOutcome {
    Ok,
    Trap { code: &'static str, message: String },
    Timeout,
}

#[derive(Debug)]
pub struct RuntimeBuilder {
    deterministic_seed: Option<u64>,
    telemetry: TelemetrySink,
    default_budget: Budget,
    threads: usize,
}

impl Default for RuntimeBuilder {
    fn default() -> Self {
        Self {
            deterministic_seed: None,
            telemetry: TelemetrySink::from_env(),
            default_budget: Budget::default(),
            threads: std::env::var("STARDUST_RUNTIME_THREADS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1),
        }
    }
}

impl RuntimeBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn deterministic(mut self, seed: u64) -> Self {
        self.deterministic_seed = Some(seed);
        self
    }
    pub fn telemetry(mut self, sink: TelemetrySink) -> Self {
        self.telemetry = sink;
        self
    }
    pub fn default_budget(mut self, b: Budget) -> Self {
        self.default_budget = b;
        self
    }
    pub fn threads(mut self, n: usize) -> Self {
        self.threads = n;
        self
    }
    pub fn build(self, prog: Arc<Program>) -> Runtime {
        let scheduler = if self.deterministic_seed.is_some() {
            Scheduler::current_thread()
        } else {
            Scheduler::multi_thread(self.threads)
        };
        Runtime {
            prog,
            scheduler: Arc::new(scheduler),
            registry: Arc::new(AgentRegistry::new()),
            supervisors: Arc::new(SupervisorRegistry::new()),
            telemetry: Arc::new(self.telemetry),
            default_budget: self.default_budget,
            tasks: Mutex::new(Vec::new()),
            shutdown_token: CancellationToken::new(),
        }
    }
}

pub struct Runtime {
    pub prog: Arc<Program>,
    pub scheduler: Arc<Scheduler>,
    pub registry: Arc<AgentRegistry>,
    pub supervisors: Arc<SupervisorRegistry>,
    pub telemetry: Arc<TelemetrySink>,
    pub default_budget: Budget,
    pub tasks: Mutex<Vec<JoinHandle<()>>>,
    /// Runtime-wide root cancellation token. Per-agent loops listen
    /// for this; `shutdown` fires it with `CancelReason::Shutdown`.
    pub shutdown_token: CancellationToken,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("agents", &self.registry.len())
            .finish()
    }
}

impl Runtime {
    pub async fn spawn_agent(&self, name: &str, _args: Vec<Value>) -> RuntimeResult<AgentHandle> {
        let agent = self
            .prog
            .agent_by_name(name)
            .ok_or_else(|| RuntimeError::AgentNotFound(name.into()))?
            .clone();
        let id = self.registry.next_id();
        let mailbox_capacity = self
            .default_budget
            .mailbox
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_POOL_SIZE);
        let mailbox = Arc::new(Mailbox::new(mailbox_capacity, SendPolicy::Block));
        let budget = Arc::new(BudgetTracker::new(self.default_budget.clone()));
        let state = build_initial_state(&self.prog, &agent);
        let desc = Arc::new(AgentDescriptor {
            id,
            name: name.into(),
            sir_id: agent.id,
            state: Mutex::new(state),
            mailbox: mailbox.clone(),
            budget: budget.clone(),
            supervisor: None,
            mailbox_depth: AtomicU64::new(0),
        });
        self.registry.insert(desc.clone());
        self.telemetry.emit(&TelemetryEvent::Spawn {
            name: name.into(),
            agent_id: id.0,
        });
        let task = spawn_agent_loop(self, desc.clone());
        self.tasks.lock().push(task);
        Ok(AgentHandle {
            id,
            name: name.into(),
            mailbox,
        })
    }

    pub async fn send(
        &self,
        target: &AgentHandle,
        msg: &str,
        args: Vec<Value>,
    ) -> RuntimeResult<()> {
        self.telemetry.emit(&TelemetryEvent::Send {
            from: "(extern)".into(),
            to: target.name.clone(),
            msg: msg.into(),
        });
        let frame = MessageFrame::fire_and_forget(msg, SmallPayload::inline(args));
        target.mailbox.send(frame).await
    }

    pub async fn ask(
        &self,
        target: &AgentHandle,
        msg: &str,
        args: Vec<Value>,
        deadline: Option<Duration>,
    ) -> RuntimeResult<Value> {
        self.telemetry.emit(&TelemetryEvent::Ask {
            from: "(extern)".into(),
            to: target.name.clone(),
            msg: msg.into(),
            deadline_ms: deadline.map(|d| d.as_millis() as u64),
        });
        let (frame, rx) = MessageFrame::ask(msg, SmallPayload::inline(args), deadline);
        target.mailbox.send(frame).await?;
        let reply = with_deadline(deadline, rx).await?;
        match reply {
            Ok(inner) => inner,
            Err(_) => Err(RuntimeError::Trap {
                code: "SD5020",
                message: "reply channel closed".into(),
            }),
        }
    }

    pub async fn shutdown(self) -> RunOutcome {
        // Co-operative shutdown: cancel the root token first so any
        // in-flight turn sees its child token fire. Then abort any
        // remaining JoinHandles.
        self.shutdown_token.cancel(CancelReason::Shutdown);
        for t in self.tasks.lock().drain(..) {
            t.abort();
        }
        self.telemetry.emit(&TelemetryEvent::Shutdown);
        self.telemetry.flush();
        RunOutcome::Ok
    }
}

fn build_initial_state(prog: &Program, agent: &SirAgent) -> Value {
    use sdust_sir::interp::host::BufferHost;
    use sdust_sir::interp::run::run_fn_with_budget;
    let ctor = prog.fn_by_id(agent.ctor);
    let mut host = BufferHost::default();
    match run_fn_with_budget(prog, &ctor.name, vec![], &mut host, 1_000_000) {
        Ok(v) => v,
        Err(_) => Value::Unit,
    }
}

fn spawn_agent_loop(rt: &Runtime, desc: Arc<AgentDescriptor>) -> JoinHandle<()> {
    let prog = rt.prog.clone();
    let telemetry = rt.telemetry.clone();
    let registry = rt.registry.clone();
    let shutdown = rt.shutdown_token.clone();
    let wall_budget = rt.default_budget.wall;
    let mut rx = desc
        .mailbox
        .take_receiver()
        .expect("mailbox receiver already taken");
    rt.scheduler.rt.spawn(async move {
        let host: Arc<Mutex<Box<dyn Host + Send>>> =
            Arc::new(Mutex::new(Box::new(StdHost::new(desc.budget.clone()))));
        while let Some(frame) = rx.recv().await {
            // Per-turn cancellation token is a child of the runtime
            // shutdown token. If shutdown fires, every per-turn token
            // is automatically cancelled.
            let per_turn = shutdown.child();
            let (res, outcome) = run_one_turn_async(
                prog.clone(),
                desc.clone(),
                frame,
                host.clone(),
                telemetry.clone(),
                per_turn,
                wall_budget,
            )
            .await;
            match (res, outcome) {
                (Ok(()), TurnOutcome::Completed) => {
                    // happy path
                }
                (Err(e), _outcome) => {
                    telemetry.emit(&TelemetryEvent::BudgetBreach {
                        agent: desc.name.clone(),
                        kind: e.diag_code().into(),
                    });
                    registry.remove(desc.id);
                    break;
                }
                (Ok(()), TurnOutcome::Cancelled(reason)) => {
                    // Cancellation without an error (shouldn't happen
                    // under current code paths, but record for safety).
                    telemetry.emit(&TelemetryEvent::BudgetBreach {
                        agent: desc.name.clone(),
                        kind: reason.diag_code().into(),
                    });
                    registry.remove(desc.id);
                    break;
                }
            }
            if shutdown.is_cancelled() {
                break;
            }
        }
    })
}
