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
//!
//! v0.6 changes:
//!
//! - The single tokio runtime is replaced by an `Arc<Scheduler>` that
//!   owns N **worker threads**, each with its own tokio current-thread
//!   runtime and a crossbeam-deque local LIFO + global injector +
//!   sibling stealing (see [`crate::scheduler`]).
//! - Each spawned agent is pinned to a worker via [`Affinity`] (round-
//!   robin elastic by default; sticky agents pin to worker 0).
//! - A background load monitor migrates elastic agents away from
//!   overloaded workers every ~100 ms (best-effort).
//! - `RuntimeBuilder::workers(n)` controls worker count (default
//!   `num_cpus::get()`-equivalent via `std::thread::available_parallelism`).
//!   `workers(1)` + `deterministic(seed)` reproduces v0.5 behavior
//!   exactly.

use crate::agent::{run_one_turn_async, AgentDescriptor, AgentHandle, AgentRegistry, TurnOutcome};
use crate::budget::{Budget, BudgetTracker};
use crate::cancel::{CancelReason, CancellationToken};
use crate::error::{RuntimeError, RuntimeResult};
use crate::host_std::StdHost;
use crate::mailbox::{Mailbox, MessageFrame, SendPolicy, SmallPayload};
use crate::scheduler::{Affinity, LoadMonitor, Scheduler};
use crate::slab_pool::DEFAULT_POOL_SIZE;
use crate::supervisor::SupervisorRegistry;
use crate::telemetry::{TelemetryEvent, TelemetrySink};
use crate::timer::with_deadline;
use mty_ir::interp::host::Host;
use mty_ir::interp::value::Value;
use mty_ir::ir::{Agent as IrAgent, Program};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;

#[derive(Debug, Clone)]
pub enum RunOutcome {
    Ok,
    Trap { code: &'static str, message: String },
    Timeout,
}

/// How many worker threads to spin up by default. We prefer
/// `std::thread::available_parallelism` over `num_cpus` to avoid a new
/// workspace dep, and fall back to 1 when the platform refuses to tell us.
fn default_worker_count() -> usize {
    // Deterministic-mode callers always override via .workers(1), so a
    // multi-core default is safe.
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

#[derive(Debug)]
pub struct RuntimeBuilder {
    deterministic_seed: Option<u64>,
    telemetry: TelemetrySink,
    default_budget: Budget,
    workers: usize,
}

impl Default for RuntimeBuilder {
    fn default() -> Self {
        let env_workers = std::env::var("STARDUST_RUNTIME_THREADS")
            .ok()
            .and_then(|s| s.parse().ok());
        let workers = env_workers.unwrap_or_else(default_worker_count);
        Self {
            deterministic_seed: None,
            telemetry: TelemetrySink::from_env(),
            default_budget: Budget::default(),
            workers,
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
    /// Set worker thread count (v0.6). The slice-7 `.threads(n)` alias
    /// is kept for backwards-compat.
    pub fn workers(mut self, n: usize) -> Self {
        self.workers = n.max(1);
        self
    }
    /// Slice-7 alias; same semantics as [`Self::workers`].
    pub fn threads(self, n: usize) -> Self {
        self.workers(n)
    }
    pub fn build(self, prog: Arc<Program>) -> Runtime {
        let scheduler = if self.deterministic_seed.is_some() {
            Scheduler::deterministic_single()
        } else {
            Scheduler::multi_worker(self.workers)
        };
        let scheduler = Arc::new(scheduler);
        // Spin up the load monitor unless we're in deterministic mode.
        let monitor = if scheduler.deterministic || scheduler.worker_count() < 2 {
            None
        } else {
            Some(Arc::new(LoadMonitor::new(scheduler.clone())))
        };
        Runtime {
            prog,
            scheduler,
            registry: Arc::new(AgentRegistry::new()),
            supervisors: Arc::new(SupervisorRegistry::new()),
            telemetry: Arc::new(self.telemetry),
            default_budget: self.default_budget,
            tasks: Mutex::new(Vec::new()),
            shutdown_token: CancellationToken::new(),
            monitor,
            monitor_thread: Mutex::new(None),
            monitor_stop: Arc::new(AtomicBool::new(false)),
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
    /// Best-effort load monitor that migrates elastic agents between
    /// workers when imbalance crosses the threshold. `None` in
    /// deterministic mode or single-worker.
    pub monitor: Option<Arc<LoadMonitor>>,
    monitor_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    monitor_stop: Arc<AtomicBool>,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("agents", &self.registry.len())
            .field("workers", &self.scheduler.worker_count())
            .finish()
    }
}

impl Runtime {
    /// Start any background services (load monitor). Idempotent.
    pub fn start_monitor(&self) {
        if self.monitor.is_none() {
            return;
        }
        let mut slot = self.monitor_thread.lock();
        if slot.is_some() {
            return;
        }
        let monitor = self.monitor.as_ref().unwrap().clone();
        let stop = self.monitor_stop.clone();
        let interval = monitor.interval;
        let scheduler = self.scheduler.clone();
        let telemetry = self.telemetry.clone();
        let thread = std::thread::Builder::new()
            .name("mty-monitor".into())
            .spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    if let Some((from, to, agent_id)) = monitor.sample_once() {
                        // Migrate. We can't kill the agent's current loop
                        // mid-recv without losing buffered messages, so the
                        // migration just retargets *future* spawns and
                        // updates the routing table. The next time the
                        // agent loop is respawned (e.g. supervisor restart)
                        // it will land on `to`.
                        scheduler.update_route_worker(agent_id, to);
                        telemetry.emit(&TelemetryEvent::Spawn {
                            name: format!("(migrate agent #{} {}->{})", agent_id, from, to),
                            agent_id,
                        });
                    }
                    std::thread::sleep(interval);
                }
            })
            .expect("spawn monitor thread");
        *slot = Some(thread);
    }

    pub async fn spawn_agent(&self, name: &str, args: Vec<Value>) -> RuntimeResult<AgentHandle> {
        self.spawn_agent_with_affinity(name, args, Affinity::Elastic)
            .await
    }

    /// v0.6: spawn with an explicit affinity hint.
    pub async fn spawn_agent_with_affinity(
        &self,
        name: &str,
        _args: Vec<Value>,
        affinity: Affinity,
    ) -> RuntimeResult<AgentHandle> {
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
        // Pick worker + register route.
        let worker = self.scheduler.assign_worker(affinity);
        self.scheduler.register_route(id.0, worker, affinity);
        self.telemetry.emit(&TelemetryEvent::Spawn {
            name: name.into(),
            agent_id: id.0,
        });
        let task = spawn_agent_loop(self, desc.clone(), worker);
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
                code: "MT5020",
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
        self.monitor_stop.store(true, Ordering::Release);
        if let Some(t) = self.monitor_thread.lock().take() {
            let _ = t.join();
        }
        self.telemetry.emit(&TelemetryEvent::Shutdown);
        self.telemetry.flush();
        // Scheduler `Drop` joins worker threads.
        RunOutcome::Ok
    }
}

fn build_initial_state(prog: &Program, agent: &IrAgent) -> Value {
    use mty_ir::interp::host::BufferHost;
    use mty_ir::interp::run::run_fn_with_budget;
    let ctor = prog.fn_by_id(agent.ctor);
    let mut host = BufferHost::default();
    match run_fn_with_budget(prog, &ctor.name, vec![], &mut host, 1_000_000) {
        Ok(v) => v,
        Err(_) => Value::Unit,
    }
}

fn spawn_agent_loop(rt: &Runtime, desc: Arc<AgentDescriptor>, worker: usize) -> JoinHandle<()> {
    let prog = rt.prog.clone();
    let telemetry = rt.telemetry.clone();
    let registry = rt.registry.clone();
    let shutdown = rt.shutdown_token.clone();
    let wall_budget = rt.default_budget.wall;
    let mut rx = desc
        .mailbox
        .take_receiver()
        .expect("mailbox receiver already taken");
    let handle = rt.scheduler.handle_for(worker);
    handle.spawn(async move {
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
