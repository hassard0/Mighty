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
pub mod mesh;
pub mod peer;
pub mod wire;

pub use address::{current_node_id, AgentAddr, NodeId};
pub use mesh::{ClusterConfig, ClusterMesh, MeshError, PeerEntry, TlsConfig, MESH_INBOX_CAPACITY};
pub use peer::{InboundFrame, Peer, PeerError};
pub use wire::{
    decode_frame, encode_frame, read_frame_async, write_frame_async, WireError, WireFrame,
    MAX_FRAME_BYTES, WIRE_VERSION,
};

use std::sync::Arc;

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
}

/// Boxed router for type erasure at the `Runtime` integration
/// boundary. The integration in `runtime.rs` (v0.19) will be:
///
/// ```ignore
/// pub fn install_cluster_router(&mut self, router: SharedRouter) { ... }
/// // in `send`:
/// if let Some(router) = &self.cluster {
///     if !router.is_local(&target.addr().node) {
///         return router.route(WireFrame::Send { ... });
///     }
/// }
/// ```
///
/// Shape is additive: a `None` slot means "no cluster"; existing
/// agents and tests are completely unaffected.
pub type SharedRouter = Arc<dyn ClusterRouter>;
