//! Distributed-agent cluster (Tier 4.1).
//!
//! v0.18 ships the *transport layer* — a typed wire protocol, a
//! reconnecting peer connection, and a multi-peer mesh — without
//! invasively rewriting the existing single-process [`crate::Runtime`].
//! Integration with `Runtime::send` / `Runtime::ask` is the next slice:
//! the runtime will look up an optional `dyn ClusterRouter` and call
//! [`ClusterRouter::route`] when the target is non-local.
//!
//! See `docs/internals/cluster.md` for the public-facing architecture
//! diagram and operational notes; `dev/history/notes/CLUSTER_V0_18_NOTES.md`
//! for the design rationale.

pub mod address;
pub mod correlation;
pub mod mesh;
pub mod peer;
// v0.20 Tier 4.2 — cluster mTLS hardening + cluster-wide supervisor.
pub mod supervisor;
pub mod tls;
pub mod wire;
// v0.21 Tier 4.3 — lossless live agent migration + placement policy.
pub mod migration;
pub mod placement;

pub use address::{current_node_id, AgentAddr, NodeId};
pub use correlation::CorrelationTable;
pub use mesh::{ClusterConfig, ClusterMesh, MeshError, PeerEntry, TlsConfig, MESH_INBOX_CAPACITY};
pub use migration::{
    AgentSnapshot, MigrationError, MigrationMetrics, MigrationMetricsSnapshot,
    MigrationOrchestrator, MigrationReport, MigrationResult, QueuedMessage, SnapshotSink,
    SnapshotSource, MAX_MIGRATION_SNAPSHOT_BYTES,
};
pub use peer::{InboundFrame, Peer, PeerError};
pub use placement::{
    LeastLoadedPolicy, PlacementContext, PlacementPolicy, StaticPolicy, StickyPolicy,
};
pub use supervisor::{
    ChildSpec, ChildState, ClusterSupervisor, ExitReason, RestartPolicy, RestartStrategy,
    SupervisorEvent, SupervisorHook, SUPERVISOR_EVENT_CAPACITY,
};
pub use tls::{
    build_acceptor as tls_build_acceptor, build_connector as tls_build_connector,
    build_pair as tls_build_pair, cert_node_id, verify_peer_identity, ClusterTlsConfig, TlsError,
};
pub use wire::{
    decode_frame, encode_frame, read_frame_async, write_frame_async, WireError, WireFrame,
    MAX_FRAME_BYTES, WIRE_VERSION,
};

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Reply shape returned by [`ClusterRouter::route_ask`]: either a
/// raw payload (success — corresponds to `WireFrame::Reply`) or a
/// structured error (corresponds to `WireFrame::Error` or a
/// transport-level failure).
#[derive(Debug, Clone)]
pub enum RouteReply {
    Ok { msg_bytes: Vec<u8> },
    Err { kind: String, message: String },
}

/// Routing handle the runtime consults when sending to a non-local
/// address. Implemented by [`ClusterMesh`]; the runtime never sees
/// the concrete type so we can swap in a different backend later
/// (e.g. a UDP/QUIC mesh) without touching `runtime.rs`.
pub trait ClusterRouter: Send + Sync + 'static {
    /// Local node identifier.
    fn local_node(&self) -> &NodeId;

    /// Push a `Send` or `Ask` frame to the appropriate peer.
    /// Returns `Err(MeshError)` if the peer is unreachable or the
    /// frame can't be encoded.
    fn route(&self, frame: WireFrame) -> Result<(), MeshError>;

    /// Fire-and-forget a `Send` to a remote agent. Higher-level than
    /// [`Self::route`] — constructs the wire frame from the caller's
    /// `from` / `to` + opaque message envelope and routes it.
    fn route_send(
        &self,
        from: AgentAddr,
        to: AgentAddr,
        msg: String,
        msg_bytes: Vec<u8>,
    ) -> Result<(), MeshError>;

    /// Request-reply `Ask` to a remote agent. Reserves a fresh
    /// correlation id internally, sends the frame, and resolves the
    /// returned future when the matching `Reply` or `Error` arrives
    /// (or the peer drops mid-flight). The future is `Send` so it can
    /// cross await points on multi-threaded runtimes.
    #[allow(clippy::type_complexity)]
    fn route_ask(
        &self,
        from: AgentAddr,
        to: AgentAddr,
        msg: String,
        msg_bytes: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<RouteReply, MeshError>> + Send + '_>>;

    /// True iff `node` is the local node.
    fn is_local(&self, node: &NodeId) -> bool {
        node == self.local_node()
    }
}

impl ClusterRouter for ClusterMesh {
    fn local_node(&self) -> &NodeId {
        self.local_node_id()
    }
    fn route(&self, frame: WireFrame) -> Result<(), MeshError> {
        ClusterMesh::route(self, frame)
    }
    fn route_send(
        &self,
        from: AgentAddr,
        to: AgentAddr,
        msg: String,
        msg_bytes: Vec<u8>,
    ) -> Result<(), MeshError> {
        ClusterMesh::route(
            self,
            WireFrame::Send {
                from,
                to,
                msg,
                msg_bytes,
            },
        )
    }
    fn route_ask(
        &self,
        from: AgentAddr,
        to: AgentAddr,
        msg: String,
        msg_bytes: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<RouteReply, MeshError>> + Send + '_>> {
        Box::pin(ClusterMesh::route_ask_impl(self, from, to, msg, msg_bytes))
    }
}

/// Boxed router for type erasure at the `Runtime` integration
/// boundary. The integration in `runtime.rs` consults this on every
/// `send`/`ask`:
///
/// ```ignore
/// pub fn with_cluster(mut self, router: SharedRouter) -> Self { ... }
/// // in `send_addr`:
/// if let Some(router) = &self.cluster {
///     if !router.is_local(&to.node) {
///         return router.route_send(from, to, msg, bytes);
///     }
/// }
/// ```
///
/// Shape is additive: a `None` slot means "no cluster"; existing
/// agents and tests are completely unaffected.
pub type SharedRouter = Arc<dyn ClusterRouter>;
