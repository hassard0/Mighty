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

use crate::cluster::address::NodeId;
use crate::cluster::peer::{
    reconnect_backoff, InboundFrame, Peer, PeerError, RECONNECT_MAX_ATTEMPTS,
};
use crate::cluster::wire::{WireError, WireFrame};
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
    shutdown: Arc<tokio::sync::Notify>,
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
    pub async fn from_config(cfg: ClusterConfig) -> Result<Arc<Self>, MeshError> {
        let (inbox_tx, inbox_rx) = mpsc::channel::<InboundFrame>(MESH_INBOX_CAPACITY);
        let mesh = Arc::new(Self {
            self_node: cfg.node_id.clone(),
            peers: DashMap::new(),
            inbox_tx: inbox_tx.clone(),
            inbox_rx: parking_lot::Mutex::new(Some(inbox_rx)),
            listener_task: parking_lot::Mutex::new(None),
            dialer_tasks: parking_lot::Mutex::new(Vec::new()),
            shutdown: Arc::new(tokio::sync::Notify::new()),
            config: cfg.clone(),
        });

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
        // Drain peers — taking them out of the map drops the Arc,
        // which on the last clone triggers Peer::drop and aborts the
        // worker tasks.
        let peers: Vec<_> = self.peers.iter().map(|e| e.key().clone()).collect();
        for k in peers {
            self.peers.remove(&k);
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
                if let Ok(peer) =
                    crate::cluster::peer::server_handshake(stream, peer_addr, local_id, inbox).await
                {
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
        loop {
            // If we're already connected, sleep + supervise.
            if let Some(p) = mesh.peers.get(&entry.node_id) {
                if p.is_connected() {
                    drop(p);
                    tokio::select! {
                        _ = shutdown.notified() => break,
                        _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {}
                    }
                    continue;
                }
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
            let connect_res = Peer::connect(
                entry.addr,
                server_name,
                mesh.config.tls.connector.clone(),
                mesh.self_node.clone(),
                mesh.inbox_tx.clone(),
                (),
            )
            .await;
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
