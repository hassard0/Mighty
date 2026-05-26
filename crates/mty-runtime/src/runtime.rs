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
use crate::cluster::{AgentAddr, RouteReply, SharedRouter};
use crate::control_socket::{spawn_control_socket, ControlContext, ControlSocketHandle};
use crate::error::{RuntimeError, RuntimeResult};
use crate::host_std::StdHost;
use crate::introspect::{AgentIntrospectState, IntrospectMap};
use crate::mailbox::{Mailbox, MessageFrame, SendPolicy, SmallPayload};
use crate::replay::{install_from_env, with_recorder, Recorder};
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
        // v0.18: opt-in deterministic-replay recording. If
        // `MTY_RECORD_TRACE=<path>` is set, install a process-wide
        // `Recorder`; otherwise the with_recorder() hooks are no-ops.
        // We hand back the `Arc` so `Runtime::shutdown` can flush.
        let runtime_seed = self.deterministic_seed.unwrap_or(0);
        let recorder = install_from_env(runtime_seed, scheduler.worker_count() as u32);
        // Spin up the load monitor unless we're in deterministic mode.
        let monitor = if scheduler.deterministic || scheduler.worker_count() < 2 {
            None
        } else {
            Some(Arc::new(LoadMonitor::new(scheduler.clone())))
        };
        let registry = Arc::new(AgentRegistry::new());
        let introspect = Arc::new(IntrospectMap::new());

        // v0.16: optional control socket. Spawns onto the driver
        // runtime if `MTY_RUNTIME_CONTROL_SOCK` is set. Failures are
        // logged, never fatal.
        let control_socket = {
            let ctx = ControlContext {
                registry: registry.clone(),
                introspect: introspect.clone(),
                worker_count: scheduler.worker_count(),
            };
            spawn_control_socket(ctx, scheduler.rt.handle().clone())
        };

        Runtime {
            prog,
            scheduler,
            registry,
            introspect,
            supervisors: Arc::new(SupervisorRegistry::new()),
            telemetry: Arc::new(self.telemetry),
            default_budget: self.default_budget,
            tasks: Mutex::new(Vec::new()),
            shutdown_token: CancellationToken::new(),
            monitor,
            monitor_thread: Mutex::new(None),
            monitor_stop: Arc::new(AtomicBool::new(false)),
            control_socket: Mutex::new(control_socket),
            recorder,
            cluster: None,
        }
    }
}

pub struct Runtime {
    pub prog: Arc<Program>,
    pub scheduler: Arc<Scheduler>,
    pub registry: Arc<AgentRegistry>,
    /// v0.16: per-agent introspection state (mailbox high-water,
    /// in-flight handler, last-N ring). Populated alongside the
    /// agent descriptor on spawn and torn down on agent removal.
    pub introspect: Arc<IntrospectMap>,
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
    /// v0.16: optional control-socket listener. `None` when the env
    /// var was unset or bind failed. Aborted on shutdown.
    control_socket: Mutex<Option<ControlSocketHandle>>,
    /// v0.18: optional deterministic-replay recorder, installed when
    /// `MTY_RECORD_TRACE=<path>` is set in the environment. Flushed
    /// to disk on `Runtime::shutdown` and on `Drop`.
    pub(crate) recorder: Option<Arc<Recorder>>,
    /// v0.19 Tier 4.1 (continued): optional cluster router. When set,
    /// [`Runtime::send_addr`] / [`Runtime::ask_addr`] consult this on
    /// every call and forward non-local addresses to a peer over the
    /// cluster mesh. `None` is the zero-overhead single-node path —
    /// the existing handle-based [`Runtime::send`] / [`Runtime::ask`]
    /// never touch this field at all, so callers that never opt into
    /// addressed messaging pay zero cost.
    pub(crate) cluster: Option<SharedRouter>,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("agents", &self.registry.len())
            .field("workers", &self.scheduler.worker_count())
            .finish_non_exhaustive()
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

    /// v0.19: install a cluster router so [`Runtime::send_addr`] and
    /// [`Runtime::ask_addr`] can route to remote nodes. Builder shape:
    ///
    /// ```ignore
    /// let mesh = ClusterMesh::from_config(cfg).await?;
    /// let rt = RuntimeBuilder::new().build(prog).with_cluster(mesh);
    /// ```
    ///
    /// Idempotent — replacing the router is fine; in-flight asks
    /// against the previous router resolve via that router's own
    /// shutdown path, not the new one.
    pub fn with_cluster(mut self, router: SharedRouter) -> Self {
        self.cluster = Some(router);
        self
    }

    /// Borrow the installed cluster router, if any. Tests + the
    /// inbound demultiplexer use this; production code calls
    /// [`Runtime::send_addr`] / [`Runtime::ask_addr`] instead.
    pub fn cluster_router(&self) -> Option<&SharedRouter> {
        self.cluster.as_ref()
    }

    /// True iff a cluster router is installed.
    pub fn has_cluster(&self) -> bool {
        self.cluster.is_some()
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
        // v0.16: introspect-state lives next to the descriptor.
        self.introspect
            .insert(id.0, Arc::new(AgentIntrospectState::default()));
        // Pick worker + register route.
        let worker = self.scheduler.assign_worker(affinity);
        self.scheduler.register_route(id.0, worker, affinity);
        self.telemetry.emit(&TelemetryEvent::Spawn {
            name: name.into(),
            agent_id: id.0,
        });
        // v0.16 OTel agent span — RAII guard ends when this fn returns.
        let _otel = crate::telemetry::span_spawn(name);
        // v0.18 replay: record the spawn before the agent loop starts
        // so any subsequent MessageHandled/Exit events have a matching
        // Spawn earlier in the stream (self-consistency requirement).
        {
            let name_for_trace = name.to_string();
            let aid = id.0;
            with_recorder(|r| r.record_spawn(aid, &name_for_trace, None));
        }
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
        // v0.16 OTel agent span — fire-and-forget, no guard.
        crate::telemetry::span_send(msg);
        // v0.18 replay: capture the message-send. Sender is the
        // synthetic "extern" id (0) — this matches the v0.17 wire
        // contract where `0` is the well-known external caller.
        {
            let to_id = target.id.0;
            let msg_owned = msg.to_string();
            let payload = encode_payload_for_trace(&args);
            with_recorder(|r| r.record_message_sent(0, to_id, &msg_owned, payload));
        }
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
        // v0.16 OTel agent span — wraps the round-trip. We use the
        // synchronous-closure shape via a manual RAII guard because
        // span_ask's closure form would need an async-friendly wrapper.
        let _otel = crate::telemetry::span_handler(&target.name, msg);
        // v0.18 replay: capture the ask as a MessageSent event from
        // the synthetic external sender (0). The MessageHandled event
        // is recorded by the agent loop when it dispatches.
        {
            let to_id = target.id.0;
            let msg_owned = msg.to_string();
            let payload = encode_payload_for_trace(&args);
            with_recorder(|r| r.record_message_sent(0, to_id, &msg_owned, payload));
        }
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

    /// v0.19: fire-and-forget send to an [`AgentAddr`].
    ///
    /// Dispatch table:
    /// - `to.is_local()` AND a local agent with this `(name, id)`
    ///   exists in the registry → in-process mailbox path (same shape
    ///   as [`Runtime::send`]). Zero overhead vs single-node path
    ///   when no cluster is installed.
    /// - `to.is_local()` but no matching local agent →
    ///   [`RuntimeError::AgentNotFound`].
    /// - `to` is remote AND a cluster router is installed →
    ///   `router.route_send(...)`.
    /// - `to` is remote AND no cluster router → [`RuntimeError::Trap`]
    ///   with code `MT5030` (no cluster configured).
    ///
    /// The `from` address is recorded for telemetry / replay; pass
    /// [`AgentAddr::local`] from an agent's `self` if you don't have
    /// something more specific.
    pub async fn send_addr(
        &self,
        from: AgentAddr,
        to: AgentAddr,
        msg: &str,
        args: Vec<Value>,
    ) -> RuntimeResult<()> {
        if to.is_local() {
            // Local path: find the handle by (name, id) and use the
            // existing mailbox send.
            let handle = self
                .find_local_handle(&to)
                .ok_or_else(|| RuntimeError::AgentNotFound(to.to_string()))?;
            return self.send(&handle, msg, args).await;
        }
        // Remote: must have a cluster router installed.
        let router = self.cluster.as_ref().ok_or_else(|| RuntimeError::Trap {
            code: "MT5030",
            message: format!(
                "cluster routing requested for remote address {to} but no router is installed"
            ),
        })?;
        let msg_bytes = encode_payload_for_trace(&args);
        router
            .route_send(from, to.clone(), msg.to_string(), msg_bytes)
            .map_err(|e| RuntimeError::Trap {
                code: "MT5031",
                message: format!("cluster send to {to} failed: {e}"),
            })
    }

    /// v0.19: request-reply ask to an [`AgentAddr`]. See
    /// [`Runtime::send_addr`] for the local/remote dispatch table.
    ///
    /// Remote asks block until the reply arrives on the cluster mesh
    /// or `deadline` elapses. The reply is decoded back from opaque
    /// `msg_bytes` into a [`Value::Bytes`] — the runtime + cluster
    /// preserve the v0.18 wire contract where payload bytes are
    /// opaque. Callers that need typed remote replies should layer
    /// their own decoder above this.
    pub async fn ask_addr(
        &self,
        from: AgentAddr,
        to: AgentAddr,
        msg: &str,
        args: Vec<Value>,
        deadline: Option<Duration>,
    ) -> RuntimeResult<Value> {
        if to.is_local() {
            let handle = self
                .find_local_handle(&to)
                .ok_or_else(|| RuntimeError::AgentNotFound(to.to_string()))?;
            return self.ask(&handle, msg, args, deadline).await;
        }
        let router = self.cluster.as_ref().ok_or_else(|| RuntimeError::Trap {
            code: "MT5030",
            message: format!(
                "cluster routing requested for remote address {to} but no router is installed"
            ),
        })?;
        let msg_bytes = encode_payload_for_trace(&args);
        let to_display = to.to_string();
        let fut = router.route_ask(from, to, msg.to_string(), msg_bytes);
        let reply = match deadline {
            Some(d) => {
                let raw = tokio::time::timeout(d, fut)
                    .await
                    .map_err(|_| RuntimeError::DeadlineExceeded(d))?;
                raw.map_err(|e| RuntimeError::Trap {
                    code: "MT5031",
                    message: format!("cluster ask to {to_display} failed: {e}"),
                })?
            }
            None => fut.await.map_err(|e| RuntimeError::Trap {
                code: "MT5031",
                message: format!("cluster ask to {to_display} failed: {e}"),
            })?,
        };
        match reply {
            // The cluster transport speaks opaque bytes; surface them
            // as a `Value::Str` (lossy UTF-8 decode) so callers don't
            // need a Bytes variant. Round-tripping rich values is the
            // job of the layer above (which encoded the args into the
            // opaque `msg_bytes` in the first place).
            RouteReply::Ok { msg_bytes } => Ok(Value::Str(
                String::from_utf8(msg_bytes.clone())
                    .unwrap_or_else(|_| format!("<{}-byte opaque payload>", msg_bytes.len())),
            )),
            RouteReply::Err { kind, message } => Err(RuntimeError::Trap {
                // Remote-side error: propagate as a trap with the
                // structured kind/message so the caller can match
                // on `kind` to distinguish e.g. peer_disconnected
                // from a user-level reply error.
                code: "MT5032",
                message: format!("remote ask returned error [{kind}]: {message}"),
            }),
        }
    }

    /// Locate a local agent by [`AgentAddr`]. Returns the live
    /// [`AgentHandle`] if the descriptor is still registered.
    /// Internal helper for [`Self::send_addr`] / [`Self::ask_addr`].
    fn find_local_handle(&self, addr: &AgentAddr) -> Option<AgentHandle> {
        let desc = self.registry.get(crate::agent::AgentId(addr.agent_id))?;
        // Verify the name matches — addressing the right pid but
        // wrong type would silently target the wrong agent otherwise.
        if desc.name != addr.agent_type {
            return None;
        }
        Some(AgentHandle {
            id: desc.id,
            name: desc.name.clone(),
            mailbox: desc.mailbox.clone(),
        })
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
        // v0.16: abort the control socket listener if it was started,
        // and clean up the socket file. Failure-to-remove is fine —
        // a subsequent run will overwrite it.
        if let Some(handle) = self.control_socket.lock().take() {
            handle.task.abort();
            #[cfg(unix)]
            {
                let _ = std::fs::remove_file(&handle.sock_path);
            }
        }
        self.telemetry.emit(&TelemetryEvent::Shutdown);
        self.telemetry.flush();
        // v0.18 replay: flush the recorder to disk + uninstall the
        // process-wide handle so subsequent Runtime::new calls start
        // fresh. Flush failures are logged but not fatal.
        if let Some(rec) = self.recorder.as_ref() {
            if let Err(e) = rec.flush_to_disk() {
                eprintln!("mty-runtime: replay trace flush failed: {e}");
            }
            let _ = crate::replay::uninstall();
        }
        // Scheduler `Drop` joins worker threads.
        RunOutcome::Ok
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        // v0.18 replay: if the runtime was dropped without an explicit
        // `shutdown().await`, still flush the recorder. Best-effort —
        // we can't do anything about errors during a drop.
        if let Some(rec) = self.recorder.as_ref() {
            let _ = rec.flush_to_disk();
            let _ = crate::replay::uninstall();
        }
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
    let introspect_map = rt.introspect.clone();
    let shutdown = rt.shutdown_token.clone();
    let wall_budget = rt.default_budget.wall;
    let mut rx = desc
        .mailbox
        .take_receiver()
        .expect("mailbox receiver already taken");
    let handle = rt.scheduler.handle_for(worker);
    let capture_bodies = crate::introspect::capture_bodies_enabled();
    handle.spawn(async move {
        let host: Arc<Mutex<Box<dyn Host + Send>>> = Arc::new(Mutex::new(Box::new(
            StdHost::new(desc.budget.clone()).with_agent_id(desc.id.0),
        )));
        let intr = introspect_map.get(desc.id.0);
        let mut exit_reason: Option<String> = None;
        while let Some(frame) = rx.recv().await {
            // v0.16 introspection: mark handler start (and optionally
            // capture the proto-msg body in the ring buffer).
            if let Some(intr) = &intr {
                let body = if capture_bodies {
                    Some(frame.proto_msg.clone())
                } else {
                    None
                };
                intr.note_handler_start(&frame.proto_msg, body);
            }
            // Per-turn cancellation token is a child of the runtime
            // shutdown token. If shutdown fires, every per-turn token
            // is automatically cancelled.
            let per_turn = shutdown.child();
            // v0.18 replay: MessageHandled events are recorded inside
            // `run_one_turn_with_shared_reply` (before the reply is
            // sent on the oneshot channel) so that an `ask()` caller
            // never observes its reply before the trace has the
            // matching handled record. See agent.rs.
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
            if let Some(intr) = &intr {
                intr.note_handler_end();
            }
            match (res, outcome) {
                (Ok(()), TurnOutcome::Completed) => {
                    // happy path
                }
                (Err(e), _outcome) => {
                    telemetry.emit(&TelemetryEvent::BudgetBreach {
                        agent: desc.name.clone(),
                        kind: e.diag_code().into(),
                    });
                    // v0.16 OTel: record on the active handler span if any.
                    crate::telemetry::record_budget_exhausted(e.diag_code());
                    // v0.18 replay: budget / trap exhaustion is the
                    // canonical "agent died" signal — capture both
                    // BudgetExhausted (when relevant) + Exit.
                    {
                        let aid = desc.id.0;
                        let code = e.diag_code();
                        let kind = format!("trap:{}", code);
                        with_recorder(|r| r.record_budget_exhausted(aid, code));
                        exit_reason = Some(kind);
                    }
                    registry.remove(desc.id);
                    introspect_map.remove(desc.id.0);
                    break;
                }
                (Ok(()), TurnOutcome::Cancelled(reason)) => {
                    // Cancellation without an error (shouldn't happen
                    // under current code paths, but record for safety).
                    telemetry.emit(&TelemetryEvent::BudgetBreach {
                        agent: desc.name.clone(),
                        kind: reason.diag_code().into(),
                    });
                    // v0.16 OTel.
                    crate::telemetry::record_budget_exhausted(reason.as_str());
                    // v0.18 replay.
                    {
                        let aid = desc.id.0;
                        let reason_str = reason.as_str().to_string();
                        with_recorder(|r| r.record_budget_exhausted(aid, &reason_str));
                        exit_reason = Some(reason_str);
                    }
                    registry.remove(desc.id);
                    introspect_map.remove(desc.id.0);
                    break;
                }
            }
            if shutdown.is_cancelled() {
                exit_reason.get_or_insert_with(|| "shutdown".into());
                break;
            }
        }
        // v0.16: clean up introspect state on normal loop exit too
        // (e.g. mailbox closed).
        introspect_map.remove(desc.id.0);
        // v0.18 replay: record the Exit event after the loop has
        // drained. "normal" covers both "mailbox closed cleanly" and
        // "agent ran to completion".
        {
            let aid = desc.id.0;
            let reason = exit_reason.unwrap_or_else(|| "normal".into());
            with_recorder(|r| r.record_exit(aid, &reason));
        }
    })
}

/// v0.18 replay helper: serialize a payload slice to opaque bytes for
/// the trace event. The interpreter `Value` doesn't derive Serialize
/// (it carries Host-side references), so we render via `Debug` — the
/// shape is opaque-but-human-readable, which is enough for v0.18
/// trace inspection. v0.19 stretch: structured payload encoding for
/// byte-identical replay. Best-effort: failures fall back to empty.
fn encode_payload_for_trace(args: &[Value]) -> Vec<u8> {
    if !crate::replay::recording_enabled() {
        // Cheap fast-path: skip the format walk when no recorder is
        // attached. Recording call-sites already gate on
        // `with_recorder`, but encoding the payload happens *before*
        // the call, so we double-check here too.
        return Vec::new();
    }
    format!("{:?}", args).into_bytes()
}
