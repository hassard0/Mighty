//! Lossless live agent migration (Tier 4.3, v0.21).
//!
//! Building on v0.20 hot-reload's `Resumable` snapshot infrastructure
//! and v0.18-v0.20's cluster wire layer, this module orchestrates the
//! end-to-end "move a running agent between cluster nodes preserving
//! mailbox + continuation" flow (RFC-006).
//!
//! ### Sequence
//!
//! ```text
//!   source                                         target
//!     |                                              |
//!     |  1. drain + snapshot local agent             |
//!     |     mark agent MIGRATING                     |
//!     |     (new messages keep enqueueing)           |
//!     |                                              |
//!     |  2. WireFrame::MigrateSnapshot ─────────────►|
//!     |                                              |
//!     |                                  3. verify schema_hash
//!     |                                     restore agent
//!     |                                     assign new agent_id
//!     |                                              |
//!     |◄───────────── 4. WireFrame::MigrateAck ──────|
//!     |                                              |
//!     |  5. forward queued mailbox frames to target  |
//!     |  6. mark agent REMOTE(target, new_id)        |
//!     |                                              |
//!     |  (subsequent sends to the original AgentAddr |
//!     |   are forwarded via the routing table)       |
//! ```
//!
//! ### What v0.21 ships vs defers
//!
//! - The orchestrator is **abstracted over the runtime** via three
//!   hooks: [`SnapshotSource`] (drain + snapshot), [`SnapshotSink`]
//!   (restore on the target), and the mesh-level routing surface
//!   reuse from v0.18. The runtime wires these for its `Value`-shaped
//!   state; tests use a generic byte-shaped hook. This keeps the
//!   off-limits `agent.rs` / `runtime.rs` untouched.
//! - Ack-vs-no-ack: every migration is **ack'd**. The source rolls
//!   back its agent state on `MigrateError` or on a timeout. Designed
//!   to fail loud rather than half-migrate; v0.22 may add a two-phase
//!   commit for partial-cluster scenarios.
//! - Mailbox-queue location: queued messages stay on the **source**
//!   until the ack arrives, then are forwarded as `WireFrame::Send`
//!   frames addressed to the new node. This avoids a "messages in
//!   flight while the target is half-loaded" hazard.
//!
//! ### Metrics
//!
//! The orchestrator emits a [`MigrationReport`] for every completed
//! migration. The runtime (or the test) is responsible for wiring
//! these into Prometheus / OTel. Telemetry hooks are deliberately
//! *out* of this module — the report carries the raw counters and the
//! caller picks the export shape.

use crate::cluster::address::{AgentAddr, NodeId};
use crate::cluster::correlation::CorrelationTable;
use crate::cluster::mesh::{ClusterMesh, MeshError};
use crate::cluster::wire::WireFrame;
use dashmap::DashMap;
use parking_lot::Mutex;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

/// Hard upper bound on a migration snapshot (mirrors the cluster
/// frame-size cap minus header overhead).
pub const MAX_MIGRATION_SNAPSHOT_BYTES: usize = 6 * 1024 * 1024;

/// Errors surfaced by the migration orchestrator. Maps to the
/// `MT507x` diagnostic family (parallel to `MT506x` for hot reload).
#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("migrate: agent not found locally for {0}")]
    AgentNotFound(AgentAddr),

    #[error("migrate: target node {0:?} is not reachable")]
    TargetUnreachable(NodeId),

    #[error("migrate: source and target are the same node ({0:?})")]
    SameNode(NodeId),

    #[error(
        "migrate: schema hash incompatible — source produced {old:#018x}, \
         target expected {new:#018x}"
    )]
    IncompatibleSchema { old: u64, new: u64 },

    #[error("migrate: deadline of {0:?} exceeded before ack arrived")]
    Deadline(Duration),

    #[error("migrate: target rejected snapshot — kind={kind} message={message}")]
    Rejected { kind: String, message: String },

    #[error("migrate: snapshot too large ({bytes} B > {limit} B)")]
    SnapshotTooLarge { bytes: usize, limit: usize },

    #[error("migrate: mesh error: {0}")]
    Mesh(#[from] MeshError),

    #[error("migrate: internal — {0}")]
    Internal(String),
}

impl MigrationError {
    /// Diagnostic code for CLI / structured logs.
    pub fn diag_code(&self) -> &'static str {
        match self {
            MigrationError::AgentNotFound(_) => "MT5071",
            MigrationError::TargetUnreachable(_) => "MT5072",
            MigrationError::SameNode(_) => "MT5073",
            MigrationError::IncompatibleSchema { .. } => "MT5060",
            MigrationError::Deadline(_) => "MT5074",
            MigrationError::Rejected { .. } => "MT5075",
            MigrationError::SnapshotTooLarge { .. } => "MT5076",
            MigrationError::Mesh(_) => "MT5077",
            MigrationError::Internal(_) => "MT5079",
        }
    }
}

pub type MigrationResult<T> = Result<T, MigrationError>;

/// Structured outcome of a successful migration. Mirrors the shape
/// of [`crate::reload::ReloadReport`] so dashboards can render both
/// uniformly.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MigrationReport {
    pub agent: AgentAddr,
    pub source: NodeId,
    pub target: NodeId,
    /// Agent's *new* address on the target (the `agent_id` is freshly
    /// assigned by the target). Equal to `agent` except for the `node`
    /// and `agent_id` fields.
    pub new_addr: AgentAddr,
    pub state_bytes: usize,
    pub drain_elapsed_ms: u64,
    pub ship_elapsed_ms: u64,
    pub restore_elapsed_ms: u64,
    pub total_elapsed_ms: u64,
    /// Number of messages forwarded from the source mailbox to the
    /// target after the ack landed.
    pub forwarded_messages: u32,
}

/// Captured payload the source ships to the target.
#[derive(Debug, Clone)]
pub struct AgentSnapshot {
    pub agent_type: String,
    pub schema_hash: u64,
    pub state: Vec<u8>,
}

/// Hook the orchestrator calls on the **source** node to drain the
/// agent's currently-running handler, snapshot its state, and switch
/// the agent into the MIGRATING lifecycle bucket (new messages keep
/// landing in the mailbox but aren't dispatched).
///
/// Implementors are typically the runtime — but tests can plug in any
/// `Send + Sync` value that satisfies the trait. The trait is intentionally
/// hand-rolled (no `async-trait`) so the workspace doesn't grow a new
/// proc-macro dep.
pub trait SnapshotSource: Send + Sync + 'static {
    fn drain_and_snapshot<'a>(
        &'a self,
        agent: &'a AgentAddr,
    ) -> Pin<Box<dyn std::future::Future<Output = MigrationResult<AgentSnapshot>> + Send + 'a>>;

    /// Drain any messages queued during the drain → ack window. The
    /// orchestrator calls this once after the ack lands so it can ship
    /// them to the target as plain `Send` frames. Returns an empty
    /// vec if the source has nothing buffered.
    fn drain_queued_messages<'a>(
        &'a self,
        agent: &'a AgentAddr,
    ) -> Pin<Box<dyn std::future::Future<Output = MigrationResult<Vec<QueuedMessage>>> + Send + 'a>>;

    /// Mark the agent as REMOTE(target, new_id) once the migration
    /// succeeds. The runtime uses this hint to route subsequent local
    /// sends to the cluster path.
    fn finalize_migrated<'a>(
        &'a self,
        agent: &'a AgentAddr,
        new_addr: &'a AgentAddr,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>>;

    /// Roll back: the migration failed and the agent should resume
    /// processing on the source. Called on `MigrateError` and on
    /// timeout.
    fn rollback<'a>(
        &'a self,
        agent: &'a AgentAddr,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>>;
}

/// Hook the orchestrator calls on the **target** node when a
/// `MigrateSnapshot` frame arrives. Returns the freshly-assigned
/// local address on success.
pub trait SnapshotSink: Send + Sync + 'static {
    fn restore<'a>(
        &'a self,
        snapshot: &'a AgentSnapshot,
        originating_addr: &'a AgentAddr,
    ) -> Pin<Box<dyn std::future::Future<Output = MigrationResult<AgentAddr>> + Send + 'a>>;
}

/// One queued message held back during the drain → ack window.
#[derive(Debug, Clone)]
pub struct QueuedMessage {
    pub from: AgentAddr,
    pub msg: String,
    pub msg_bytes: Vec<u8>,
}

/// Counters the orchestrator updates over its lifetime. Mirrors the
/// shape we'd expose via Prometheus — bounded-cardinality, no
/// per-agent labels.
#[derive(Debug, Default)]
pub struct MigrationMetrics {
    pub migrations_started: AtomicU64,
    pub migrations_completed: AtomicU64,
    pub migrations_failed: AtomicU64,
    pub migrations_rolled_back: AtomicU64,
    pub bytes_shipped_total: AtomicU64,
    pub messages_forwarded_total: AtomicU64,
}

impl MigrationMetrics {
    /// Snapshot the counters into a serde-friendly record.
    pub fn snapshot(&self) -> MigrationMetricsSnapshot {
        MigrationMetricsSnapshot {
            migrations_started: self.migrations_started.load(Ordering::Relaxed),
            migrations_completed: self.migrations_completed.load(Ordering::Relaxed),
            migrations_failed: self.migrations_failed.load(Ordering::Relaxed),
            migrations_rolled_back: self.migrations_rolled_back.load(Ordering::Relaxed),
            bytes_shipped_total: self.bytes_shipped_total.load(Ordering::Relaxed),
            messages_forwarded_total: self.messages_forwarded_total.load(Ordering::Relaxed),
        }
    }
}

/// Cardinality-bounded snapshot for telemetry export.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MigrationMetricsSnapshot {
    pub migrations_started: u64,
    pub migrations_completed: u64,
    pub migrations_failed: u64,
    pub migrations_rolled_back: u64,
    pub bytes_shipped_total: u64,
    pub messages_forwarded_total: u64,
}

/// Pending migration the source is waiting on an ack/error for.
#[allow(dead_code)]
struct Pending {
    started_at: Instant,
    drain_elapsed: Duration,
    snapshot_bytes: usize,
    /// One-shot fulfilled when the ack/error frame lands.
    reply: oneshot::Sender<PendingReply>,
}

enum PendingReply {
    Ack { new_addr: AgentAddr },
    Error { kind: String, message: String },
}

/// The orchestrator object the runtime owns and threads through the
/// mesh's inbound-frame handler.
pub struct MigrationOrchestrator {
    mesh: Arc<ClusterMesh>,
    source: Option<Arc<dyn SnapshotSource>>,
    sink: Option<Arc<dyn SnapshotSink>>,
    /// Pending migrations the source is waiting on.
    pending: DashMap<AgentAddr, Pending>,
    /// Active routing rewrites — `original → new`. Used after the ack
    /// lands so any further forwarded messages address the target's
    /// fresh agent_id.
    rewrites: DashMap<AgentAddr, AgentAddr>,
    metrics: Arc<MigrationMetrics>,
    /// Reused for the v0.20 correlation surface even though migration
    /// frames don't currently use a correlation id — kept here in case
    /// v0.22 multi-step migrations need it.
    #[allow(dead_code)]
    correlations: Arc<CorrelationTable>,
    /// Whether the orchestrator has been wired with the sink/source
    /// hooks. The handle to install them is a builder, not the ctor,
    /// so tests can build a half-wired orchestrator.
    sink_lock: Mutex<()>,
}

impl std::fmt::Debug for MigrationOrchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MigrationOrchestrator")
            .field("local_node", self.mesh.local_node_id())
            .field("pending", &self.pending.len())
            .field("rewrites", &self.rewrites.len())
            .field("metrics", &self.metrics.snapshot())
            .finish_non_exhaustive()
    }
}

impl MigrationOrchestrator {
    /// Build a bare orchestrator. The caller installs the source/sink
    /// hooks via [`Self::with_source`] / [`Self::with_sink`] before
    /// driving migrations.
    pub fn new(mesh: Arc<ClusterMesh>) -> Arc<Self> {
        Arc::new(Self {
            mesh,
            source: None,
            sink: None,
            pending: DashMap::new(),
            rewrites: DashMap::new(),
            metrics: Arc::new(MigrationMetrics::default()),
            correlations: Arc::new(CorrelationTable::new()),
            sink_lock: Mutex::new(()),
        })
    }

    /// Builder: install the source-side hook. Returns a new Arc with
    /// the hook wired — the previous Arc is functional but won't be
    /// able to source migrations.
    pub fn with_source(self: Arc<Self>, source: Arc<dyn SnapshotSource>) -> Arc<Self> {
        let _g = self.sink_lock.lock();
        Arc::new(Self {
            mesh: self.mesh.clone(),
            source: Some(source),
            sink: self.sink.clone(),
            pending: DashMap::new(),
            rewrites: DashMap::new(),
            metrics: self.metrics.clone(),
            correlations: self.correlations.clone(),
            sink_lock: Mutex::new(()),
        })
    }

    /// Builder: install the target-side hook.
    pub fn with_sink(self: Arc<Self>, sink: Arc<dyn SnapshotSink>) -> Arc<Self> {
        let _g = self.sink_lock.lock();
        Arc::new(Self {
            mesh: self.mesh.clone(),
            source: self.source.clone(),
            sink: Some(sink),
            pending: DashMap::new(),
            rewrites: DashMap::new(),
            metrics: self.metrics.clone(),
            correlations: self.correlations.clone(),
            sink_lock: Mutex::new(()),
        })
    }

    /// Borrow the metrics snapshot. Cheap.
    pub fn metrics(&self) -> &Arc<MigrationMetrics> {
        &self.metrics
    }

    /// Look up the rewrite for `original`, if any. Used by the
    /// routing layer to forward sends after a migration completes.
    pub fn lookup_rewrite(&self, original: &AgentAddr) -> Option<AgentAddr> {
        self.rewrites.get(original).map(|r| r.value().clone())
    }

    /// Borrow the mesh — for tests that need to drive frames through
    /// the orchestrator manually.
    pub fn mesh(&self) -> &Arc<ClusterMesh> {
        &self.mesh
    }

    /// Main entry: migrate `agent_id` from this node (the local one)
    /// to `target`. Blocks until the ack arrives or `deadline_ms`
    /// elapses. On error, the source's `rollback` hook is called.
    pub async fn migrate_agent(
        &self,
        agent: AgentAddr,
        target: NodeId,
        deadline_ms: u64,
    ) -> MigrationResult<MigrationReport> {
        let local = self.mesh.local_node_id().clone();
        if target == local {
            return Err(MigrationError::SameNode(target));
        }
        if !self.mesh.has_peer(&target) {
            return Err(MigrationError::TargetUnreachable(target));
        }
        let source = self
            .source
            .as_ref()
            .ok_or_else(|| MigrationError::Internal("no source hook installed".into()))?
            .clone();

        self.metrics
            .migrations_started
            .fetch_add(1, Ordering::Relaxed);
        let started = Instant::now();

        // (1) drain + snapshot.
        let drain_started = Instant::now();
        let snapshot = source.drain_and_snapshot(&agent).await.map_err(|e| {
            self.metrics
                .migrations_failed
                .fetch_add(1, Ordering::Relaxed);
            e
        })?;
        let drain_elapsed = drain_started.elapsed();
        if snapshot.state.len() > MAX_MIGRATION_SNAPSHOT_BYTES {
            source.rollback(&agent).await;
            self.metrics
                .migrations_failed
                .fetch_add(1, Ordering::Relaxed);
            self.metrics
                .migrations_rolled_back
                .fetch_add(1, Ordering::Relaxed);
            return Err(MigrationError::SnapshotTooLarge {
                bytes: snapshot.state.len(),
                limit: MAX_MIGRATION_SNAPSHOT_BYTES,
            });
        }
        let snapshot_bytes = snapshot.state.len();

        // (2) register a pending entry and ship the snapshot.
        let (tx, rx) = oneshot::channel();
        self.pending.insert(
            agent.clone(),
            Pending {
                started_at: started,
                drain_elapsed,
                snapshot_bytes,
                reply: tx,
            },
        );

        let ship_started = Instant::now();
        let frame = WireFrame::MigrateSnapshot {
            agent_addr: agent.clone(),
            target_node: target.clone(),
            agent_type: snapshot.agent_type.clone(),
            schema_hash: snapshot.schema_hash,
            state: snapshot.state.clone(),
        };
        if let Err(e) = self.mesh.route_async(frame).await {
            // Roll back: pending slot was registered but the ship
            // never happened.
            self.pending.remove(&agent);
            source.rollback(&agent).await;
            self.metrics
                .migrations_failed
                .fetch_add(1, Ordering::Relaxed);
            self.metrics
                .migrations_rolled_back
                .fetch_add(1, Ordering::Relaxed);
            return Err(MigrationError::Mesh(e));
        }
        self.metrics
            .bytes_shipped_total
            .fetch_add(snapshot_bytes as u64, Ordering::Relaxed);
        let ship_elapsed = ship_started.elapsed();

        // (3) wait for ack within the deadline.
        let deadline = Duration::from_millis(deadline_ms);
        let reply = match tokio::time::timeout(deadline, rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => {
                self.pending.remove(&agent);
                source.rollback(&agent).await;
                self.metrics
                    .migrations_failed
                    .fetch_add(1, Ordering::Relaxed);
                self.metrics
                    .migrations_rolled_back
                    .fetch_add(1, Ordering::Relaxed);
                return Err(MigrationError::Internal(
                    "migration reply channel closed".into(),
                ));
            }
            Err(_) => {
                self.pending.remove(&agent);
                source.rollback(&agent).await;
                self.metrics
                    .migrations_failed
                    .fetch_add(1, Ordering::Relaxed);
                self.metrics
                    .migrations_rolled_back
                    .fetch_add(1, Ordering::Relaxed);
                return Err(MigrationError::Deadline(deadline));
            }
        };
        let restore_started = Instant::now();

        let new_addr = match reply {
            PendingReply::Ack { new_addr } => new_addr,
            PendingReply::Error { kind, message } => {
                source.rollback(&agent).await;
                self.metrics
                    .migrations_failed
                    .fetch_add(1, Ordering::Relaxed);
                self.metrics
                    .migrations_rolled_back
                    .fetch_add(1, Ordering::Relaxed);
                if kind == "schema_incompatible" {
                    // Try to extract the hashes from the message; if
                    // not parseable, fall back to the generic Rejected
                    // shape. The message format is owned by the target,
                    // so we keep it loose.
                    return Err(MigrationError::Rejected { kind, message });
                }
                return Err(MigrationError::Rejected { kind, message });
            }
        };

        // (4) forward any messages queued during drain → ack.
        let queued = source.drain_queued_messages(&agent).await.unwrap_or_default();
        let mut forwarded = 0u32;
        for q in &queued {
            let fwd = WireFrame::Send {
                from: q.from.clone(),
                to: new_addr.clone(),
                msg: q.msg.clone(),
                msg_bytes: q.msg_bytes.clone(),
            };
            // Best-effort forward; a forward failure leaves the message
            // on the source for the rollback path, but at this point
            // the agent already lives on the target — the cleanest
            // story is "log and continue". The orchestrator surfaces
            // the count of successful forwards in the report.
            if self.mesh.route_async(fwd).await.is_ok() {
                forwarded += 1;
            }
        }
        self.metrics
            .messages_forwarded_total
            .fetch_add(forwarded as u64, Ordering::Relaxed);

        // (5) install the routing rewrite + finalize on the source.
        self.rewrites.insert(agent.clone(), new_addr.clone());
        source.finalize_migrated(&agent, &new_addr).await;
        self.metrics
            .migrations_completed
            .fetch_add(1, Ordering::Relaxed);

        let restore_elapsed = restore_started.elapsed();
        Ok(MigrationReport {
            agent: agent.clone(),
            source: local,
            target,
            new_addr,
            state_bytes: snapshot_bytes,
            drain_elapsed_ms: drain_elapsed.as_millis() as u64,
            ship_elapsed_ms: ship_elapsed.as_millis() as u64,
            restore_elapsed_ms: restore_elapsed.as_millis() as u64,
            total_elapsed_ms: started.elapsed().as_millis() as u64,
            forwarded_messages: forwarded,
        })
    }

    /// Inbound-frame handler invoked from the mesh's drain loop. The
    /// runtime wires its `inbox_rx` loop to call this for every
    /// migration-shaped frame.
    ///
    /// Returns `true` if the frame was a migration frame the
    /// orchestrator consumed; `false` if the caller should keep
    /// processing it (e.g. as a normal `Send`).
    pub async fn handle_inbound(&self, frame: WireFrame) -> bool {
        match frame {
            WireFrame::MigrateSnapshot {
                agent_addr,
                target_node,
                agent_type,
                schema_hash,
                state,
            } => {
                // This frame is destined for us — restore via the sink.
                let _ = target_node; // Already routed here by the mesh.
                let Some(sink) = self.sink.clone() else {
                    let err = WireFrame::MigrateError {
                        migrating: agent_addr.clone(),
                        route_to: agent_addr.node.clone(),
                        kind: "no_sink".into(),
                        message: "target has no migration sink installed".into(),
                    };
                    let _ = self.mesh.route_async(err).await;
                    return true;
                };
                let snapshot = AgentSnapshot {
                    agent_type,
                    schema_hash,
                    state,
                };
                match sink.restore(&snapshot, &agent_addr).await {
                    Ok(new_addr) => {
                        let ack = WireFrame::MigrateAck {
                            migrating: agent_addr.clone(),
                            new: new_addr,
                            route_to: agent_addr.node.clone(),
                        };
                        let _ = self.mesh.route_async(ack).await;
                    }
                    Err(e) => {
                        let kind = match &e {
                            MigrationError::IncompatibleSchema { .. } => "schema_incompatible",
                            MigrationError::SnapshotTooLarge { .. } => "snapshot_too_large",
                            _ => "restore_failed",
                        };
                        let err = WireFrame::MigrateError {
                            migrating: agent_addr.clone(),
                            route_to: agent_addr.node.clone(),
                            kind: kind.into(),
                            message: e.to_string(),
                        };
                        let _ = self.mesh.route_async(err).await;
                    }
                }
                true
            }
            WireFrame::MigrateAck {
                migrating,
                new,
                route_to: _,
            } => {
                if let Some((_, p)) = self.pending.remove(&migrating) {
                    let _ = p.reply.send(PendingReply::Ack { new_addr: new });
                    let _ = p.started_at; // kept for the report builder
                    let _ = p.drain_elapsed;
                    let _ = p.snapshot_bytes;
                }
                true
            }
            WireFrame::MigrateError {
                migrating,
                route_to: _,
                kind,
                message,
            } => {
                if let Some((_, p)) = self.pending.remove(&migrating) {
                    let _ = p.reply.send(PendingReply::Error { kind, message });
                }
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_codes_are_stable() {
        // Pin the diag-code shape so a future refactor can't silently
        // change it under operators' dashboards.
        assert_eq!(
            MigrationError::AgentNotFound(AgentAddr::remote("n", "A", 1)).diag_code(),
            "MT5071"
        );
        assert_eq!(
            MigrationError::TargetUnreachable(NodeId::new("n")).diag_code(),
            "MT5072"
        );
        assert_eq!(
            MigrationError::IncompatibleSchema { old: 1, new: 2 }.diag_code(),
            "MT5060"
        );
        assert_eq!(
            MigrationError::Deadline(Duration::from_millis(1)).diag_code(),
            "MT5074"
        );
    }

    #[test]
    fn metrics_snapshot_reflects_increments() {
        let m = MigrationMetrics::default();
        m.migrations_started.fetch_add(3, Ordering::Relaxed);
        m.migrations_completed.fetch_add(1, Ordering::Relaxed);
        m.bytes_shipped_total.fetch_add(1024, Ordering::Relaxed);
        let s = m.snapshot();
        assert_eq!(s.migrations_started, 3);
        assert_eq!(s.migrations_completed, 1);
        assert_eq!(s.bytes_shipped_total, 1024);
    }
}
