//! Multi-peer cluster mesh.
//!
//! The [`ClusterMesh`] is the single object the runtime hands a frame
//! to when it needs to leave the local node. It owns:
//!
//! - The TLS configuration (one [`Arc<TlsConnector>`] + one
//!   [`Arc<TlsAcceptor>`]) shared by every peer.
//! - A `DashMap<NodeId, PeerSlot>` of outbound peers.
//! - A listener task accepting inbound TLS connections, performing the
//!   `Hello` handshake, and registering the resulting peer in the same
//!   map.
//! - The single `inbox` mpsc receiver every reader task pushes into.
//!
//! ### Routing
//!
//! - `route_send(frame)` extracts `frame.to.node`. If it's the local
//!   node, the mesh returns `MeshError::WouldLoopLocal` — the caller
//!   should have taken the in-process path. Otherwise it looks up the
//!   peer and pushes the frame.
//! - Inbound frames are popped by the runtime via [`ClusterMesh::take_inbox`].
//!   The runtime decides what to do with them (route to a local
//!   mailbox, etc.); this module is transport-only.
//!
//! ### What we deliberately do NOT do
//!
//! - We don't reach into `Runtime::send`. Wiring is via the
//!   [`ClusterRouter`] trait in `crate::cluster`, which the runtime
//!   may consult.
//! - We don't open peer connections eagerly to dead peers — the static
//!   list says "these are my friends," not "they MUST be up." A peer
//!   that's down at start-up gets a background reconnect loop.

use crate::cluster::address::{AgentAddr, NodeId};
use crate::cluster::correlation::CorrelationTable;
use crate::cluster::peer::{
    reconnect_backoff, InboundFrame, Peer, PeerError, RECONNECT_MAX_ATTEMPTS,
};
use crate::cluster::wire::{WireError, WireFrame};
use crate::cluster::RouteReply;
use dashmap::DashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_rustls::{rustls, TlsAcceptor, TlsConnector};

/// Capacity of the mesh's central inbox. Inbound frames from every
/// peer multiplex through here.
pub const MESH_INBOX_CAPACITY: usize = 4096;

#[derive(Debug, thiserror::Error)]
pub enum MeshError {
    #[error("mesh: peer for node {0:?} is not connected")]
    PeerDisconnected(NodeId),
    #[error("mesh: unknown peer node {0:?}")]
    UnknownNode(NodeId),
    #[error("mesh: send to self would loop ({0:?})")]
    WouldLoopLocal(NodeId),
    #[error("mesh: peer error: {0}")]
    Peer(#[from] PeerError),
    #[error("mesh: wire error: {0}")]
    Wire(#[from] WireError),
    #[error("mesh: bind/listen: {0}")]
    Listen(std::io::Error),
}

/// Static configuration for the mesh. The "peer list" is typically
/// read from `mighty.toml`; this struct is the in-memory form.
///
/// v0.20: the mTLS flag does NOT live here — it would be a breaking
/// addition for v0.18 / v0.19 callers that still build `ClusterConfig`
/// via struct-literal syntax. Instead, mTLS is enabled at mesh
/// construction time via [`ClusterMesh::from_config_mtls`], which
/// flips an internal flag the listener + dialer paths consult before
/// installing peers.
#[derive(Debug, Clone)]
pub struct ClusterConfig {
    pub node_id: NodeId,
    pub listen_addr: Option<SocketAddr>,
    pub peers: Vec<PeerEntry>,
    pub tls: TlsConfig,
}

#[derive(Debug, Clone)]
pub struct PeerEntry {
    pub node_id: NodeId,
    pub addr: SocketAddr,
    /// SNI / server-name to use when dialing this peer. Defaults to
    /// the node id if unset.
    pub server_name: Option<String>,
}

#[derive(Clone)]
pub struct TlsConfig {
    pub connector: TlsConnector,
    pub acceptor: TlsAcceptor,
}

impl std::fmt::Debug for TlsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsConfig").finish_non_exhaustive()
    }
}

/// The mesh.
pub struct ClusterMesh {
    self_node: NodeId,
    config: ClusterConfig,
    peers: DashMap<NodeId, Arc<Peer>>,
    inbox_tx: mpsc::Sender<InboundFrame>,
    inbox_rx: parking_lot::Mutex<Option<mpsc::Receiver<InboundFrame>>>,
    listener_task: parking_lot::Mutex<Option<JoinHandle<()>>>,
    dialer_tasks: parking_lot::Mutex<Vec<JoinHandle<()>>>,
    demux_task: parking_lot::Mutex<Option<JoinHandle<()>>>,
    /// v0.19: ask/reply correlation table. The reply demultiplexer
    /// task drains the central inbox, peels `Reply` / `Error` frames
    /// off into this table, and forwards the rest onto the
    /// caller-facing `inbox_rx`.
    correlations: Arc<CorrelationTable>,
    shutdown: Arc<tokio::sync::Notify>,
    /// v0.20: gate for the mTLS code path. When `true`, every accepted
    /// listener connection is required to present a client cert and
    /// every dialer pulls the listener's server cert post-handshake;
    /// both sides verify the CN matches the `Hello.node_id` claim.
    require_mtls: bool,
    /// v0.20: cluster supervisors registered with this mesh. The mesh
    /// notifies every supervisor when a peer disconnects so it can mark
    /// the dead node's children `:noproc` and trigger the configured
    /// restart strategy. Boxed so the mesh stays agnostic to the
    /// concrete supervisor type.
    supervisors: parking_lot::RwLock<Vec<Arc<dyn crate::cluster::supervisor::SupervisorHook>>>,
}

impl std::fmt::Debug for ClusterMesh {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClusterMesh")
            .field("self_node", &self.self_node)
            .field("peer_count", &self.peers.len())
            .finish_non_exhaustive()
    }
}

impl ClusterMesh {
    /// Build a mesh from a config. Spawns the listener task if
    /// `listen_addr` is set and a background dialer per configured
    /// peer. None of these block the call site — peers that are down
    /// keep retrying in the background.
    ///
    /// Uses **server-only TLS** (v0.18 / v0.19 shape). For the v0.20
    /// mTLS + CN-bound identity path, see [`Self::from_config_mtls`].
    pub async fn from_config(cfg: ClusterConfig) -> Result<Arc<Self>, MeshError> {
        Self::from_config_inner(cfg, false).await
    }

    /// v0.20: build an mTLS-bound mesh. Same shape as [`Self::from_config`]
    /// but every accepted connection MUST present a client cert (the
    /// caller is responsible for building `cfg.tls.acceptor` from a
    /// [`crate::cluster::tls::ClusterTlsConfig`] with
    /// `require_client_cert = true`), and the listener / dialer paths
    /// both verify the peer cert CN against the `Hello.node_id`.
    pub async fn from_config_mtls(cfg: ClusterConfig) -> Result<Arc<Self>, MeshError> {
        Self::from_config_inner(cfg, true).await
    }

    async fn from_config_inner(
        cfg: ClusterConfig,
        require_mtls: bool,
    ) -> Result<Arc<Self>, MeshError> {
        // Two-stage inbox plumbing:
        //   peer reader tasks ─► `raw_tx` (capacity MESH_INBOX_CAPACITY)
        //                       │
        //                       │ demux task pops, splits Reply/Error
        //                       │ frames into the correlation table,
        //                       │ forwards everything else onto:
        //                       ▼
        //   runtime/take_inbox() ◄─ `inbox_rx`
        //
        // The raw channel is what every `Peer` is handed. The
        // user-facing `inbox_rx` keeps the same shape as v0.18 (callers
        // see Send/Ask frames only), so the v0.18 cluster integration
        // tests are unaffected.
        let (raw_tx, mut raw_rx) = mpsc::channel::<InboundFrame>(MESH_INBOX_CAPACITY);
        let (inbox_tx, inbox_rx) = mpsc::channel::<InboundFrame>(MESH_INBOX_CAPACITY);
        let correlations = Arc::new(CorrelationTable::new());

        let mesh = Arc::new(Self {
            self_node: cfg.node_id.clone(),
            peers: DashMap::new(),
            inbox_tx: raw_tx.clone(),
            inbox_rx: parking_lot::Mutex::new(Some(inbox_rx)),
            listener_task: parking_lot::Mutex::new(None),
            dialer_tasks: parking_lot::Mutex::new(Vec::new()),
            demux_task: parking_lot::Mutex::new(None),
            correlations: correlations.clone(),
            shutdown: Arc::new(tokio::sync::Notify::new()),
            config: cfg.clone(),
            require_mtls,
            supervisors: parking_lot::RwLock::new(Vec::new()),
        });

        // Demux task: split replies into the correlation table,
        // forward everything else to the user-facing inbox.
        let demux = {
            let correlations = correlations.clone();
            tokio::spawn(async move {
                while let Some(env) = raw_rx.recv().await {
                    match &env.frame {
                        WireFrame::Reply { correlation, .. }
                        | WireFrame::Error { correlation, .. } => {
                            // Late or unknown replies are dropped
                            // silently — see CorrelationTable::complete.
                            correlations.complete(*correlation, env.frame);
                        }
                        _ => {
                            if inbox_tx.send(env).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            })
        };
        *mesh.demux_task.lock() = Some(demux);

        // Spawn listener if configured.
        if let Some(addr) = cfg.listen_addr {
            let listener = TcpListener::bind(addr).await.map_err(MeshError::Listen)?;
            let task = spawn_listener_task(mesh.clone(), listener);
            *mesh.listener_task.lock() = Some(task);
        }

        // Spawn background dialer per peer.
        for entry in cfg.peers.iter().cloned() {
            let task = spawn_dialer_task(mesh.clone(), entry);
            mesh.dialer_tasks.lock().push(task);
        }

        Ok(mesh)
    }

    /// Borrow the shared correlation table. Mostly for tests + the
    /// `ClusterRouter` impl; the runtime never touches it directly.
    pub fn correlations(&self) -> &Arc<CorrelationTable> {
        &self.correlations
    }

    /// Higher-level `Ask` routing used by the
    /// [`crate::cluster::ClusterRouter`] impl. Reserves a fresh
    /// correlation id, builds the `WireFrame::Ask`, hands it to the
    /// peer writer, and awaits the matching `Reply` / `Error` on a
    /// oneshot wired through the correlation table.
    ///
    /// Cancel-safety: if the caller's future is dropped before the
    /// reply arrives (e.g. an outer `timeout` fired), the slot is
    /// cleaned up here via the `_guard` RAII helper so the table
    /// doesn't leak entries.
    pub async fn route_ask_impl(
        &self,
        from: AgentAddr,
        to: AgentAddr,
        msg: String,
        msg_bytes: Vec<u8>,
    ) -> Result<RouteReply, MeshError> {
        if to.node == self.self_node {
            return Err(MeshError::WouldLoopLocal(to.node));
        }
        let target_node = to.node.as_str().to_string();
        let (correlation, rx) = self.correlations.register_for_node(&target_node);
        let _guard = AskGuard {
            correlations: self.correlations.clone(),
            correlation,
            armed: true,
        };
        let frame = WireFrame::Ask {
            from,
            to,
            msg,
            msg_bytes,
            correlation,
        };
        self.route_async(frame).await?;
        let reply = rx.await.map_err(|_| {
            MeshError::Wire(WireError::Decode("ask correlation oneshot closed".into()))
        })?;
        // The guard's job is to clean up the slot on drop (timeout
        // path). The happy path completed the slot via the demux
        // task, so disarm it here to skip the extra remove().
        std::mem::forget(_guard);
        match reply {
            WireFrame::Reply { msg_bytes, .. } => Ok(RouteReply::Ok { msg_bytes }),
            WireFrame::Error { kind, message, .. } => Ok(RouteReply::Err { kind, message }),
            // The demux task only routes Reply/Error here, so anything
            // else is a programming error in the mesh.
            other => Err(MeshError::Wire(WireError::Decode(format!(
                "unexpected reply frame: {other:?}"
            )))),
        }
    }

    /// Borrow the local node id.
    pub fn local_node_id(&self) -> &NodeId {
        &self.self_node
    }

    /// Take the inbox receiver. Can only be called once.
    pub fn take_inbox(&self) -> Option<mpsc::Receiver<InboundFrame>> {
        self.inbox_rx.lock().take()
    }

    /// Borrow the inbox-tx side. Test helpers use this to inject
    /// frames as if they came from a real peer; production code goes
    /// through the real reader task.
    pub fn inbox_sender(&self) -> mpsc::Sender<InboundFrame> {
        self.inbox_tx.clone()
    }

    /// Number of currently-connected peers.
    pub fn connected_peer_count(&self) -> usize {
        self.peers
            .iter()
            .filter(|e| e.value().is_connected())
            .count()
    }

    /// True iff at least one peer for `node` is reachable.
    pub fn has_peer(&self, node: &NodeId) -> bool {
        self.peers.get(node).is_some_and(|p| p.is_connected())
    }

    /// Install a peer directly. Used by the listener / dialer tasks
    /// and by tests.
    pub fn install_peer(&self, peer: Arc<Peer>) {
        self.peers.insert(peer.node_id.clone(), peer);
    }

    /// Test-only accessor for the peer map. v0.19 integration tests
    /// need this to grab an inbound peer (accepted by the listener)
    /// and inject a reply frame on its writer half. Production code
    /// goes through [`Self::route`] / [`Self::route_async`].
    pub fn peers_for_test(&self) -> &DashMap<NodeId, Arc<Peer>> {
        &self.peers
    }

    /// Route a frame to its `to` node. Returns:
    /// - `Ok(())` if the frame was handed off to the peer's writer
    ///   task. (Delivery is NOT confirmed — TLS gives us order +
    ///   integrity, that's it.)
    /// - `WouldLoopLocal` if `to == self`.
    /// - `UnknownNode` if no peer entry exists.
    /// - `PeerDisconnected` if the peer is offline right now.
    pub fn route(&self, frame: WireFrame) -> Result<(), MeshError> {
        let to_node = frame_target_node(&frame).ok_or_else(|| {
            MeshError::Wire(WireError::Decode("frame has no addressable target".into()))
        })?;
        if to_node == &self.self_node {
            return Err(MeshError::WouldLoopLocal(to_node.clone()));
        }
        let peer = self
            .peers
            .get(to_node)
            .ok_or_else(|| MeshError::UnknownNode(to_node.clone()))?;
        if !peer.is_connected() {
            return Err(MeshError::PeerDisconnected(to_node.clone()));
        }
        peer.send_frame(frame)?;
        Ok(())
    }

    /// Async variant of [`Self::route`] — waits if the peer's
    /// outbound queue is full instead of returning `Backpressure`.
    pub async fn route_async(&self, frame: WireFrame) -> Result<(), MeshError> {
        let to_node = frame_target_node(&frame).ok_or_else(|| {
            MeshError::Wire(WireError::Decode("frame has no addressable target".into()))
        })?;
        if to_node == &self.self_node {
            return Err(MeshError::WouldLoopLocal(to_node.clone()));
        }
        let peer = self
            .peers
            .get(to_node)
            .map(|p| p.value().clone())
            .ok_or_else(|| MeshError::UnknownNode(to_node.clone()))?;
        if !peer.is_connected() {
            return Err(MeshError::PeerDisconnected(to_node.clone()));
        }
        peer.send_frame_async(frame).await?;
        Ok(())
    }

    /// v0.20: register a cluster supervisor with this mesh. The mesh
    /// notifies every registered hook when a peer disconnects so it can
    /// mark the dead node's children `:noproc` and apply its restart
    /// strategy. Multiple supervisors may share one mesh.
    pub fn register_supervisor(&self, hook: Arc<dyn crate::cluster::supervisor::SupervisorHook>) {
        self.supervisors.write().push(hook);
    }

    /// v0.20: deliver a "node disconnected" event to every registered
    /// supervisor. Called by the dialer task when it notices a peer
    /// has gone away (writer/reader tasks died). Also exposed for
    /// tests that need to simulate a disconnect without driving the
    /// actual TCP teardown.
    pub async fn notify_node_disconnect(&self, node: &NodeId) {
        // Clone Arcs out under the lock, then call hooks lock-free so
        // a slow hook can't stall the dialer.
        let hooks: Vec<_> = self.supervisors.read().iter().cloned().collect();
        for h in hooks {
            h.on_node_disconnect(node).await;
        }
    }

    /// Tear down the mesh: close every peer, abort the listener +
    /// dialers, drop the inbox. Idempotent.
    pub async fn shutdown(self: Arc<Self>) {
        self.shutdown.notify_waiters();
        if let Some(t) = self.listener_task.lock().take() {
            t.abort();
        }
        for t in self.dialer_tasks.lock().drain(..) {
            t.abort();
        }
        if let Some(t) = self.demux_task.lock().take() {
            t.abort();
        }
        // Resolve any in-flight asks so they don't hang forever.
        self.correlations.fail_all_with(|cid| WireFrame::Error {
            correlation: cid,
            kind: "mesh_shutdown".into(),
            message: "cluster mesh shutting down".into(),
        });
        // Drain peers — taking them out of the map drops the Arc,
        // which on the last clone triggers Peer::drop and aborts the
        // worker tasks.
        let peers: Vec<_> = self.peers.iter().map(|e| e.key().clone()).collect();
        for k in peers {
            self.peers.remove(&k);
        }
    }
}

/// RAII cleanup helper for [`ClusterMesh::route_ask_impl`]. When the
/// ask future is dropped before the reply lands (cancellation /
/// timeout), this guard purges the correlation slot so the table
/// doesn't leak entries.
///
/// The happy path `mem::forget`s the guard after the reply arrives —
/// the demux task already removed the slot via `complete()`.
struct AskGuard {
    correlations: Arc<CorrelationTable>,
    correlation: u64,
    armed: bool,
}

impl Drop for AskGuard {
    fn drop(&mut self) {
        if self.armed {
            self.correlations.cleanup(self.correlation);
        }
    }
}

/// Extract the target node from a frame, if any. Some frames
/// (`Heartbeat`, `Goodbye`, `Hello`) don't have a routable target —
/// those are peer-local and never go through `route()`.
fn frame_target_node(frame: &WireFrame) -> Option<&NodeId> {
    match frame {
        WireFrame::Send { to, .. } => Some(&to.node),
        WireFrame::Ask { to, .. } => Some(&to.node),
        // Replies and errors are routed by correlation, not by
        // address — the mesh shouldn't be asked to route those via
        // `route()`. Peers send them directly on the same socket.
        WireFrame::Reply { .. } | WireFrame::Error { .. } => None,
        WireFrame::Hello { .. } | WireFrame::Heartbeat | WireFrame::Goodbye => None,
        // v0.21 Tier 4.3 — migration frames carry their own target
        // node distinct from the agent address.
        WireFrame::MigrateSnapshot { target_node, .. } => Some(target_node),
        WireFrame::MigrateAck { route_to, .. } => Some(route_to),
        WireFrame::MigrateError { route_to, .. } => Some(route_to),
    }
}

fn spawn_listener_task(mesh: Arc<ClusterMesh>, listener: TcpListener) -> JoinHandle<()> {
    let shutdown = mesh.shutdown.clone();
    tokio::spawn(async move {
        loop {
            let accept = tokio::select! {
                _ = shutdown.notified() => break,
                acc = listener.accept() => acc,
            };
            let Ok((tcp, peer_addr)) = accept else {
                continue;
            };
            tcp.set_nodelay(true).ok();
            let mesh = mesh.clone();
            tokio::spawn(async move {
                let Ok(stream) = mesh.config.tls.acceptor.accept(tcp).await else {
                    return;
                };
                let inbox = mesh.inbox_tx.clone();
                let local_id = mesh.self_node.clone();
                let peer = if mesh.require_mtls {
                    // mTLS path: pull peer certs off the freshly-
                    // completed handshake BEFORE splitting the stream
                    // for I/O. rustls only exposes the chain via the
                    // `ServerConnection`; tokio-rustls 0.26 surfaces it
                    // through `get_ref().1`.
                    let peer_certs = stream
                        .get_ref()
                        .1
                        .peer_certificates()
                        .map(|c| c.to_vec())
                        .unwrap_or_default();
                    crate::cluster::peer::server_handshake_mtls(
                        stream, peer_addr, local_id, inbox, peer_certs,
                    )
                    .await
                } else {
                    crate::cluster::peer::server_handshake(stream, peer_addr, local_id, inbox).await
                };
                if let Ok(peer) = peer {
                    mesh.install_peer(Arc::new(peer));
                }
            });
        }
    })
}

fn spawn_dialer_task(mesh: Arc<ClusterMesh>, entry: PeerEntry) -> JoinHandle<()> {
    let shutdown = mesh.shutdown.clone();
    tokio::spawn(async move {
        let mut attempt: u32 = 0;
        let mut was_connected = false;
        loop {
            // If we're already connected, sleep + supervise.
            if let Some(p) = mesh.peers.get(&entry.node_id) {
                if p.is_connected() {
                    was_connected = true;
                    drop(p);
                    tokio::select! {
                        _ = shutdown.notified() => break,
                        _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {}
                    }
                    continue;
                }
            }
            // We just transitioned from "connected" to "not connected" —
            // wake every in-flight ask targeting this node so they
            // resolve cleanly instead of hanging. v0.20 also notifies
            // every cluster supervisor so it can mark this node's
            // children `:noproc` and apply its restart strategy.
            if was_connected {
                mesh.correlations
                    .fail_targeting_node(entry.node_id.as_str());
                mesh.notify_node_disconnect(&entry.node_id).await;
                was_connected = false;
            }
            attempt = attempt.saturating_add(1);
            if RECONNECT_MAX_ATTEMPTS != 0 && attempt > RECONNECT_MAX_ATTEMPTS {
                break;
            }
            let server_name_str = entry
                .server_name
                .clone()
                .unwrap_or_else(|| entry.node_id.as_str().to_string());
            let Ok(server_name) = rustls::pki_types::ServerName::try_from(server_name_str) else {
                break;
            };
            let connect_res = if mesh.require_mtls {
                Peer::connect_mtls(
                    entry.addr,
                    server_name,
                    mesh.config.tls.connector.clone(),
                    mesh.self_node.clone(),
                    Some(entry.node_id.clone()),
                    mesh.inbox_tx.clone(),
                )
                .await
            } else {
                Peer::connect(
                    entry.addr,
                    server_name,
                    mesh.config.tls.connector.clone(),
                    mesh.self_node.clone(),
                    mesh.inbox_tx.clone(),
                    (),
                )
                .await
            };
            match connect_res {
                Ok(peer) => {
                    mesh.install_peer(Arc::new(peer));
                    attempt = 0;
                }
                Err(_) => {
                    let backoff = reconnect_backoff(attempt);
                    tokio::select! {
                        _ = shutdown.notified() => break,
                        _ = tokio::time::sleep(backoff) => {}
                    }
                }
            }
        }
    })
}
