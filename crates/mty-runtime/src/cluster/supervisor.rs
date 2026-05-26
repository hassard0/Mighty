//! Cluster-wide supervisor (Tier 4.2, v0.20).
//!
//! The in-process [`crate::supervisor`] tree restarts agents that crash
//! inside this node. The cluster supervisor lifts that one level up:
//! its children can live on **remote** nodes, and the events it reacts
//! to include "peer disconnected → every child on that node is now
//! `:noproc`".
//!
//! ### What v0.20 ships
//!
//! - [`ClusterSupervisor`] — a flat parent that knows its children's
//!   addresses ([`AgentAddr`]), restart policies, and the strategy
//!   binding siblings together ([`OneForOne`], [`OneForAll`],
//!   [`RestForOne`]).
//! - Three event entry points:
//!   - [`ClusterSupervisor::add_child`] / [`Self::add_children`].
//!   - [`Self::on_node_disconnect`] — every child on the dead node
//!     transitions to [`ChildState::NoProc`]; siblings on other nodes
//!     also get woken per strategy.
//!   - [`Self::on_child_exit`] — single-child crash on the local node.
//! - A circuit breaker: more than `max_restarts` restarts within
//!   `window_ms` triggers [`SupervisorEvent::CircuitBreakerTripped`]
//!   instead of another restart.
//!
//! ### What v0.20 deliberately defers (→ v0.21)
//!
//! - **Cross-node fail-over.** When `node-b` goes down, the v0.20 path
//!   records "all of B's children are :noproc" and emits a restart
//!   event PER CHILD — but it does NOT re-place them on a different
//!   node. The runtime caller decides what "restart" means (try B
//!   again when it reconnects, or pick a new node from a placement
//!   policy that we'll write later).
//! - **Lossless live migration.** Tier 4.3.
//!
//! ### Decoupling from the rest of the runtime
//!
//! The supervisor doesn't import `Runtime` or `agent::AgentHandle`. It
//! talks in [`AgentAddr`] (the cluster-wide address) and emits
//! [`SupervisorEvent`]s the caller drains. The caller wires those
//! events back into whatever local restart machinery it has — e.g. the
//! existing [`crate::supervisor_orchestrator`] for in-process children,
//! a placement service for cross-node restarts later.
//!
//! ```ignore
//! let sup = ClusterSupervisor::new(RestartStrategy::OneForOne);
//! sup.add_child(ChildSpec { addr, restart: RestartPolicy::Permanent, max_restarts: 5, window_ms: 30_000 });
//!
//! // Wire mesh disconnect events into the supervisor:
//! mesh.register_supervisor(Arc::new(sup.clone()));
//!
//! // Drain restart events:
//! while let Some(ev) = sup.next_event().await {
//!     match ev {
//!         SupervisorEvent::RestartRequested { child, .. } => /* re-spawn */,
//!         SupervisorEvent::CircuitBreakerTripped { .. }   => /* give up */,
//!         SupervisorEvent::NodeDisconnect { .. }          => { /* log */ }
//!     }
//! }
//! ```

use crate::cluster::address::{AgentAddr, NodeId};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::{mpsc, Mutex as AsyncMutex};

/// What kind of restart a single child wants when it (or a sibling
/// covered by the supervisor's strategy) goes down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPolicy {
    /// Always restart (default for long-lived service children).
    Permanent,
    /// Restart only on abnormal termination (crash, `:noproc`, …).
    Transient,
    /// Never restart (e.g. one-shot bootstrap children).
    Temporary,
}

impl RestartPolicy {
    /// Does this policy want a restart given the exit reason?
    pub fn should_restart(self, reason: &ExitReason) -> bool {
        match self {
            RestartPolicy::Permanent => true,
            RestartPolicy::Temporary => false,
            RestartPolicy::Transient => !matches!(reason, ExitReason::Normal),
        }
    }
}

/// How a child went away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitReason {
    /// Voluntary, clean exit.
    Normal,
    /// Child panicked / trapped.
    Crashed(String),
    /// The node hosting this child went away — child is unreachable.
    NoProc,
}

/// The three sibling-restart strategies. Mirrors
/// [`crate::supervisor::Strategy`] but lives next to the cluster
/// supervisor to keep this module self-contained (and to keep the
/// `OneForOne` literal at index 0 — we don't want the cluster API
/// silently inheriting the in-process tree's `Escalate` variant, which
/// has no meaning when "the parent" lives in a different process).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RestartStrategy {
    /// Only the failing child is restarted.
    #[default]
    OneForOne,
    /// All siblings are restarted alongside the failing child.
    OneForAll,
    /// The failing child and all siblings registered AFTER it (in
    /// insertion order) are restarted; earlier siblings are untouched.
    RestForOne,
}

/// One supervised child.
#[derive(Debug, Clone)]
pub struct ChildSpec {
    pub addr: AgentAddr,
    pub restart: RestartPolicy,
    /// Max restarts allowed within [`Self::window_ms`] before the
    /// supervisor trips the circuit breaker.
    pub max_restarts: u32,
    pub window_ms: u64,
}

/// Runtime view of a child's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChildState {
    Running,
    /// Child exited and a restart is in flight (caller has been told,
    /// but no new exit reported yet).
    Restarting,
    /// The node hosting this child is unreachable.
    NoProc,
    /// Permanent failure — circuit breaker tripped or the policy says
    /// don't restart. The supervisor stops emitting events for this
    /// child until [`ClusterSupervisor::add_child`] re-installs it.
    Dead(String),
}

/// Events emitted by the supervisor that the caller is expected to
/// drain via [`ClusterSupervisor::next_event`]. The events are
/// fire-and-forget — losing one is bad but not corrupting (the
/// supervisor still has the authoritative `children` + state map).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorEvent {
    /// Caller should re-spawn / re-connect to `child`. `siblings`
    /// lists every other child the strategy says to restart at the
    /// same time (empty for OneForOne).
    RestartRequested {
        child: AgentAddr,
        siblings: Vec<AgentAddr>,
        reason: ExitReason,
    },
    /// Circuit breaker tripped — supervisor has stopped trying to
    /// restart this child. Operator action required.
    CircuitBreakerTripped {
        child: AgentAddr,
        attempts: u32,
        window_ms: u64,
    },
    /// One of our hosting nodes went away. Always emitted alongside
    /// the per-child RestartRequested events for that node.
    NodeDisconnect { node: NodeId, lost_children: u32 },
}

/// Hook the cluster mesh calls when a peer disconnects. The
/// supervisor's `&self` API wires straight into this; the trait
/// indirection keeps the mesh from depending on `ClusterSupervisor`
/// concretely (tests can also drop in a no-op hook).
///
/// Hand-rolled `Future + Send` shape to avoid pulling `async-trait`
/// into the workspace just for one method.
pub trait SupervisorHook: Send + Sync {
    fn on_node_disconnect<'a>(
        &'a self,
        node: &'a NodeId,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>>;
}

#[derive(Debug)]
struct ChildRecord {
    spec: ChildSpec,
    state: ChildState,
    /// Recent restart timestamps used by the circuit breaker.
    restart_history: Vec<Instant>,
    /// Insertion order — needed for `RestForOne` semantics.
    sequence: u64,
}

/// The cluster supervisor.
///
/// `Arc<Self>`-friendly: every mutating method takes `&self` and uses
/// an internal lock. The intent is that a single supervisor instance
/// is shared between the application code that registers children and
/// the mesh code that calls [`Self::on_node_disconnect`] via the
/// [`SupervisorHook`] trait.
pub struct ClusterSupervisor {
    inner: Mutex<Inner>,
    /// Outbox the caller drains via `next_event` — bounded so a
    /// stalled drainer back-pressures the supervisor instead of
    /// growing unbounded.
    event_tx: mpsc::Sender<SupervisorEvent>,
    event_rx: AsyncMutex<mpsc::Receiver<SupervisorEvent>>,
}

#[derive(Debug, Default)]
struct Inner {
    children: HashMap<AgentAddr, ChildRecord>,
    strategy: RestartStrategy,
    next_sequence: u64,
}

/// Capacity of the supervisor event channel. Set to a small constant
/// because in steady state we expect very few events (a typical
/// supervisor handles a handful of restarts per minute, not per ms).
pub const SUPERVISOR_EVENT_CAPACITY: usize = 256;

impl ClusterSupervisor {
    pub fn new(strategy: RestartStrategy) -> Self {
        let (tx, rx) = mpsc::channel(SUPERVISOR_EVENT_CAPACITY);
        Self {
            inner: Mutex::new(Inner {
                strategy,
                ..Inner::default()
            }),
            event_tx: tx,
            event_rx: AsyncMutex::new(rx),
        }
    }

    /// Borrow the configured strategy.
    pub fn strategy(&self) -> RestartStrategy {
        self.inner.lock().strategy
    }

    /// Insert / replace a child.
    pub fn add_child(&self, spec: ChildSpec) {
        let mut g = self.inner.lock();
        let seq = g.next_sequence;
        g.next_sequence += 1;
        g.children.insert(
            spec.addr.clone(),
            ChildRecord {
                spec,
                state: ChildState::Running,
                restart_history: Vec::new(),
                sequence: seq,
            },
        );
    }

    /// Convenience: bulk-insert at construction time.
    pub fn add_children(&self, specs: impl IntoIterator<Item = ChildSpec>) {
        for s in specs {
            self.add_child(s);
        }
    }

    /// Number of currently-tracked children.
    pub fn child_count(&self) -> usize {
        self.inner.lock().children.len()
    }

    /// Inspect a child's current state. Returns `None` if the address
    /// isn't tracked.
    pub fn state_of(&self, addr: &AgentAddr) -> Option<ChildState> {
        self.inner
            .lock()
            .children
            .get(addr)
            .map(|c| c.state.clone())
    }

    /// Mark every child whose `addr.node == node` as [`ChildState::NoProc`]
    /// and apply the strategy to wake siblings. Idempotent — calling
    /// twice for the same `node` doesn't re-restart already-NoProc
    /// children.
    pub async fn on_node_disconnect(&self, node: &NodeId) {
        let events = {
            let mut g = self.inner.lock();
            let mut lost: Vec<AgentAddr> = Vec::new();
            for record in g.children.values_mut() {
                if &record.spec.addr.node == node && record.state != ChildState::NoProc {
                    record.state = ChildState::NoProc;
                    lost.push(record.spec.addr.clone());
                }
            }
            if lost.is_empty() {
                return;
            }
            let mut evs = Vec::with_capacity(lost.len() + 1);
            evs.push(SupervisorEvent::NodeDisconnect {
                node: node.clone(),
                lost_children: lost.len() as u32,
            });
            for addr in lost {
                Self::plan_restart_locked(&mut g, &addr, ExitReason::NoProc, &mut evs);
            }
            evs
        };
        for ev in events {
            // Best-effort: if the caller's drainer is gone or stalled
            // beyond the channel capacity, we drop the event silently.
            // The authoritative state lives in the supervisor itself.
            let _ = self.event_tx.send(ev).await;
        }
    }

    /// Single-child exit. Used both for in-process crashes (the
    /// caller noticed an agent trapped) and for cross-node exits
    /// reported by some future placement service.
    pub async fn on_child_exit(&self, addr: AgentAddr, reason: ExitReason) {
        let events = {
            let mut g = self.inner.lock();
            let mut evs = Vec::new();
            if let Some(record) = g.children.get_mut(&addr) {
                if record.state != ChildState::NoProc {
                    record.state = ChildState::Restarting;
                }
            } else {
                return;
            }
            Self::plan_restart_locked(&mut g, &addr, reason, &mut evs);
            evs
        };
        for ev in events {
            let _ = self.event_tx.send(ev).await;
        }
    }

    /// Drain the next supervisor event, blocking until one arrives.
    /// Returns `None` when every Arc to this supervisor is dropped
    /// (channel closed) — that's the "supervisor was destroyed"
    /// signal for the drainer task.
    pub async fn next_event(&self) -> Option<SupervisorEvent> {
        let mut rx = self.event_rx.lock().await;
        rx.recv().await
    }

    /// Non-blocking drain. Used by tests to assert on emitted events
    /// without awaiting the channel.
    pub fn try_next_event(&self) -> Option<SupervisorEvent> {
        // Acquire the async mutex synchronously. We're using the
        // `try_lock` shape because the caller is a test asserting
        // immediately after an op; the lock is uncontended.
        let mut rx = self.event_rx.try_lock().ok()?;
        rx.try_recv().ok()
    }

    /// Internal: decide which children to wake given the strategy,
    /// add the `RestartRequested` event(s), and respect the
    /// per-child circuit breaker.
    fn plan_restart_locked(
        inner: &mut Inner,
        failed: &AgentAddr,
        reason: ExitReason,
        out: &mut Vec<SupervisorEvent>,
    ) {
        let strategy = inner.strategy;
        let failed_sequence = inner.children.get(failed).map(|c| c.sequence);

        // Determine which siblings the strategy wants to wake.
        let wake_with: Vec<AgentAddr> = match strategy {
            RestartStrategy::OneForOne => Vec::new(),
            RestartStrategy::OneForAll => inner
                .children
                .keys()
                .filter(|k| *k != failed)
                .cloned()
                .collect(),
            RestartStrategy::RestForOne => {
                let Some(failed_seq) = failed_sequence else {
                    return;
                };
                inner
                    .children
                    .values()
                    .filter(|c| c.sequence > failed_seq && &c.spec.addr != failed)
                    .map(|c| c.spec.addr.clone())
                    .collect()
            }
        };

        // The failed child gets first crack.
        let mut targets = vec![failed.clone()];
        targets.extend(wake_with.iter().cloned());

        // Circuit breaker check on the failing child.
        if let Some(record) = inner.children.get_mut(failed) {
            if !record.spec.restart.should_restart(&reason) {
                record.state = ChildState::Dead(format!(
                    "restart policy {:?} declined to restart on reason {:?}",
                    record.spec.restart, reason
                ));
                return;
            }
            let now = Instant::now();
            let window = std::time::Duration::from_millis(record.spec.window_ms);
            record
                .restart_history
                .retain(|t| now.duration_since(*t) < window);
            if (record.restart_history.len() as u32) >= record.spec.max_restarts {
                let attempts = record.restart_history.len() as u32;
                let window_ms = record.spec.window_ms;
                record.state = ChildState::Dead(format!(
                    "circuit breaker: {} restarts in {} ms",
                    attempts, window_ms
                ));
                out.push(SupervisorEvent::CircuitBreakerTripped {
                    child: failed.clone(),
                    attempts,
                    window_ms,
                });
                return;
            }
            record.restart_history.push(now);
            // Preserve NoProc — the child is genuinely unreachable;
            // calling it Restarting would obscure the truth and break
            // the "node-b went down" diagnostic story. The caller is
            // responsible for transitioning back to Running once it
            // succeeds in placing the child somewhere.
            if record.state != ChildState::NoProc {
                record.state = ChildState::Restarting;
            }
        } else {
            return;
        }

        // Move sibling state too (cosmetic — they're about to be told
        // to restart, so tracking the transition makes
        // `state_of(sibling)` consistent with the emitted event).
        for sib in &wake_with {
            if let Some(rec) = inner.children.get_mut(sib) {
                if !matches!(rec.state, ChildState::NoProc) {
                    rec.state = ChildState::Restarting;
                }
            }
        }

        out.push(SupervisorEvent::RestartRequested {
            child: failed.clone(),
            siblings: wake_with,
            reason,
        });
    }
}

impl std::fmt::Debug for ClusterSupervisor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let g = self.inner.lock();
        f.debug_struct("ClusterSupervisor")
            .field("strategy", &g.strategy)
            .field("child_count", &g.children.len())
            .finish_non_exhaustive()
    }
}

// Implement the mesh-facing hook so a supervisor can be registered
// directly without an extra wrapper type.
impl SupervisorHook for ClusterSupervisor {
    fn on_node_disconnect<'a>(
        &'a self,
        node: &'a NodeId,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            ClusterSupervisor::on_node_disconnect(self, node).await;
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn child(node: &str, ty: &str, id: u64) -> AgentAddr {
        AgentAddr::remote(node, ty, id)
    }

    fn spec(addr: AgentAddr, max: u32, window: u64) -> ChildSpec {
        ChildSpec {
            addr,
            restart: RestartPolicy::Permanent,
            max_restarts: max,
            window_ms: window,
        }
    }

    #[tokio::test]
    async fn add_child_starts_in_running_state() {
        let s = ClusterSupervisor::new(RestartStrategy::OneForOne);
        let a = child("n", "A", 1);
        s.add_child(spec(a.clone(), 5, 30_000));
        assert_eq!(s.state_of(&a), Some(ChildState::Running));
    }

    #[tokio::test]
    async fn one_for_one_emits_only_failed_child() {
        let s = ClusterSupervisor::new(RestartStrategy::OneForOne);
        let a = child("n", "A", 1);
        let b = child("n", "B", 2);
        s.add_child(spec(a.clone(), 5, 30_000));
        s.add_child(spec(b.clone(), 5, 30_000));
        s.on_child_exit(a.clone(), ExitReason::Crashed("boom".into()))
            .await;
        match s.try_next_event().unwrap() {
            SupervisorEvent::RestartRequested {
                child, siblings, ..
            } => {
                assert_eq!(child, a);
                assert!(siblings.is_empty(), "OneForOne should not wake siblings");
            }
            other => panic!("unexpected event {other:?}"),
        }
    }
}
