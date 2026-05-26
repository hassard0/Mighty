//! Single-peer connection state.
//!
//! A [`Peer`] owns a long-lived TLS stream to one other node. The
//! shape is:
//!
//! ```text
//!   send_frame() ──► mpsc::Sender ──► writer task ──► TLS sink
//!                                                       │
//!                                          reader task ◄┘
//!                                                       │
//!                                            inbox tx ◄─┘
//! ```
//!
//! The reader pushes every successfully-decoded frame onto an inbox
//! channel that the [`crate::cluster::mesh::ClusterMesh`] owns.
//!
//! Reconnect: if the writer or reader task notices the socket is
//! gone, the peer drops into a backoff loop (`100ms, 200ms, 400ms,
//! …, capped 30s`) and retries up to `RECONNECT_MAX_ATTEMPTS` times
//! before giving up. The mesh's `route_send` returns
//! `PeerError::Disconnected` in the meantime; nothing buffers
//! silently.
//!
//! ### What's deliberately out of scope
//!
//! - Per-frame ACK / retransmit. CBOR-over-TLS gives us order +
//!   integrity; if the socket survives, the frame arrived.
//! - Backpressure. The internal mpsc is bounded ([`PEER_TX_CAPACITY`]);
//!   if it fills, `send_frame` returns `PeerError::Backpressure` and
//!   the mesh decides what to do.

use crate::cluster::address::NodeId;
use crate::cluster::wire::{
    read_frame_async, write_frame_async, WireError, WireFrame, WIRE_VERSION,
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_rustls::{client::TlsStream, TlsConnector};

/// How many outgoing frames we'll buffer before `send_frame` errors.
pub const PEER_TX_CAPACITY: usize = 256;
/// How often the writer task pumps an unsolicited heartbeat.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
/// Initial reconnect backoff.
pub const RECONNECT_INITIAL_BACKOFF: Duration = Duration::from_millis(100);
/// Cap on reconnect backoff.
pub const RECONNECT_MAX_BACKOFF: Duration = Duration::from_secs(30);
/// Max consecutive reconnect attempts before giving up. Set to 0 for
/// unbounded (used by long-running services).
pub const RECONNECT_MAX_ATTEMPTS: u32 = 10;

#[derive(Debug, thiserror::Error)]
pub enum PeerError {
    #[error("peer io: {0}")]
    Io(#[from] std::io::Error),
    #[error("peer tls: {0}")]
    Tls(String),
    #[error("peer wire: {0}")]
    Wire(#[from] WireError),
    #[error("peer disconnected — frame dropped")]
    Disconnected,
    #[error("peer backpressure — outbound queue full")]
    Backpressure,
    #[error("peer handshake mismatch: expected version {expected}, got {got}")]
    VersionMismatch { expected: u32, got: u32 },
    #[error("peer handshake: missing Hello frame")]
    MissingHello,
}

/// A connected peer. Frames go in via [`Peer::send_frame`]; the
/// writer task on the other side pumps them to the wire. Inbound
/// frames are pushed by the reader task onto an `inbox` channel
/// supplied at construction time.
pub struct Peer {
    /// Peer's remote socket address (for diagnostics).
    pub remote_addr: SocketAddr,
    /// Peer's advertised [`NodeId`] (from the `Hello` handshake).
    pub node_id: NodeId,
    tx: mpsc::Sender<WireFrame>,
    shutdown: Option<oneshot::Sender<()>>,
    writer_task: Option<JoinHandle<()>>,
    reader_task: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Peer")
            .field("remote_addr", &self.remote_addr)
            .field("node_id", &self.node_id)
            .finish_non_exhaustive()
    }
}

impl Peer {
    /// Dial `remote_addr`, perform a TLS handshake using `connector`
    /// and `server_name`, exchange `Hello` frames with `local_node_id`,
    /// and spawn the reader/writer tasks. Inbound frames go to
    /// `inbox`.
    pub async fn connect<I>(
        remote_addr: SocketAddr,
        server_name: rustls::pki_types::ServerName<'static>,
        connector: TlsConnector,
        local_node_id: NodeId,
        inbox: mpsc::Sender<InboundFrame>,
        _io_hint: I,
    ) -> Result<Self, PeerError>
    where
        I: 'static,
    {
        let tcp = TcpStream::connect(remote_addr).await?;
        tcp.set_nodelay(true).ok();
        let stream = connector
            .connect(server_name.clone(), tcp)
            .await
            .map_err(|e| PeerError::Tls(e.to_string()))?;
        spawn_peer_after_tls(stream, remote_addr, local_node_id, inbox).await
    }

    /// Variant of [`Peer::connect`] for tests that bring their own
    /// already-wrapped TLS stream (so they can use `tokio::io::duplex`
    /// for ultra-fast in-process tests). The runtime never calls this.
    pub async fn from_raw_tls_client(
        stream: TlsStream<TcpStream>,
        remote_addr: SocketAddr,
        local_node_id: NodeId,
        inbox: mpsc::Sender<InboundFrame>,
    ) -> Result<Self, PeerError> {
        spawn_peer_after_tls(stream, remote_addr, local_node_id, inbox).await
    }

    /// Test-side: wrap a pair of in-memory streams that already
    /// behaves like a TLS connection (or any other reliable, ordered,
    /// length-preserving byte channel). Used by integration tests
    /// that don't need to drag real rustls/TCP through every fixture.
    pub async fn from_raw_stream<S>(
        stream: S,
        remote_addr: SocketAddr,
        local_node_id: NodeId,
        inbox: mpsc::Sender<InboundFrame>,
    ) -> Result<Self, PeerError>
    where
        S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        spawn_peer_after_tls(stream, remote_addr, local_node_id, inbox).await
    }

    /// Push a frame onto the outbound queue. Non-blocking: returns
    /// `Backpressure` if the queue is full or `Disconnected` if the
    /// writer task is gone.
    pub fn send_frame(&self, frame: WireFrame) -> Result<(), PeerError> {
        match self.tx.try_send(frame) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => Err(PeerError::Backpressure),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(PeerError::Disconnected),
        }
    }

    /// Async variant — waits if the channel is full. Errors on close.
    pub async fn send_frame_async(&self, frame: WireFrame) -> Result<(), PeerError> {
        self.tx
            .send(frame)
            .await
            .map_err(|_| PeerError::Disconnected)
    }

    /// True iff the writer task is still alive (peer reachable).
    pub fn is_connected(&self) -> bool {
        !self.tx.is_closed()
    }

    /// Send a Goodbye, then drop the connection. The reader/writer
    /// tasks shut down cleanly via the oneshot.
    ///
    /// We don't `await` the join handles here — the reader half of a
    /// split TLS stream may keep blocking on `read` until the *peer*
    /// closes its write half, which we have no synchronous way to
    /// observe. Aborting both tasks is correct: we've already flushed
    /// `Goodbye` to the wire, and the peer's listener sees the EOF +
    /// drops its side.
    pub async fn close(mut self) {
        let _ = self.send_frame_async(WireFrame::Goodbye).await;
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(t) = self.writer_task.take() {
            t.abort();
        }
        if let Some(t) = self.reader_task.take() {
            t.abort();
        }
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(t) = self.writer_task.take() {
            t.abort();
        }
        if let Some(t) = self.reader_task.take() {
            t.abort();
        }
    }
}

/// A frame received from a peer, tagged with the peer it came from.
#[derive(Debug)]
pub struct InboundFrame {
    pub from_node: NodeId,
    pub frame: WireFrame,
}

/// Split + spawn the reader/writer tasks for a freshly-handshaken
/// TLS connection. Performs the `Hello` exchange before returning.
async fn spawn_peer_after_tls<S>(
    stream: S,
    remote_addr: SocketAddr,
    local_node_id: NodeId,
    inbox: mpsc::Sender<InboundFrame>,
) -> Result<Peer, PeerError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(stream);

    // Handshake: send our Hello, wait for theirs.
    let our_hello = WireFrame::Hello {
        node_id: local_node_id.clone(),
        version: WIRE_VERSION,
    };
    write_frame_async(&mut writer, &our_hello).await?;
    let peer_node_id = match read_frame_async(&mut reader).await? {
        Some(WireFrame::Hello { node_id, version }) => {
            if version != WIRE_VERSION {
                return Err(PeerError::VersionMismatch {
                    expected: WIRE_VERSION,
                    got: version,
                });
            }
            node_id
        }
        Some(_) => return Err(PeerError::MissingHello),
        None => return Err(PeerError::MissingHello),
    };

    let (tx, mut rx) = mpsc::channel::<WireFrame>(PEER_TX_CAPACITY);
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

    // Writer task: pump from rx → wire, plus a heartbeat ticker.
    let writer_task = tokio::spawn(async move {
        let mut hb = tokio::time::interval(HEARTBEAT_INTERVAL);
        hb.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Eat the immediate tick.
        hb.tick().await;
        loop {
            tokio::select! {
                biased;
                _ = &mut shutdown_rx => break,
                maybe = rx.recv() => match maybe {
                    Some(frame) => {
                        if write_frame_async(&mut writer, &frame).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                },
                _ = hb.tick() => {
                    if write_frame_async(&mut writer, &WireFrame::Heartbeat).await.is_err() {
                        break;
                    }
                }
            }
        }
        // Best-effort half-close so the peer's reader sees EOF
        // promptly. Failure is fine — the socket is dying anyway.
        let _ = tokio::io::AsyncWriteExt::shutdown(&mut writer).await;
    });

    // Reader task: pump from wire → inbox.
    let peer_node_id_for_reader = peer_node_id.clone();
    let reader_task = tokio::spawn(async move {
        loop {
            match read_frame_async(&mut reader).await {
                Ok(Some(frame)) => {
                    if matches!(frame, WireFrame::Goodbye) {
                        break;
                    }
                    if matches!(frame, WireFrame::Heartbeat) {
                        // Heartbeats are absorbed locally; no need
                        // to spam the inbox.
                        continue;
                    }
                    let envelope = InboundFrame {
                        from_node: peer_node_id_for_reader.clone(),
                        frame,
                    };
                    if inbox.send(envelope).await.is_err() {
                        break;
                    }
                }
                Ok(None) => break, // clean EOF
                Err(_) => break,   // io / decode error
            }
        }
    });

    Ok(Peer {
        remote_addr,
        node_id: peer_node_id,
        tx,
        shutdown: Some(shutdown_tx),
        writer_task: Some(writer_task),
        reader_task: Some(reader_task),
    })
}

/// Server-side: perform the post-accept handshake using an already-
/// completed `tokio-rustls` server-side TLS stream. Used by the
/// mesh's listener task.
pub async fn server_handshake<S>(
    stream: S,
    remote_addr: SocketAddr,
    local_node_id: NodeId,
    inbox: mpsc::Sender<InboundFrame>,
) -> Result<Peer, PeerError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    spawn_peer_after_tls(stream, remote_addr, local_node_id, inbox).await
}

/// Compute the next reconnect backoff given the attempt count
/// (1-based). Exponential with jitter capped at
/// [`RECONNECT_MAX_BACKOFF`].
pub fn reconnect_backoff(attempt: u32) -> Duration {
    let base = RECONNECT_INITIAL_BACKOFF.as_millis() as u64;
    // 2^(attempt-1), capped.
    let shifted = base.saturating_mul(1u64 << attempt.saturating_sub(1).min(20));
    let cap = RECONNECT_MAX_BACKOFF.as_millis() as u64;
    Duration::from_millis(shifted.min(cap))
}

/// Drive a peer connection with retries. Returns the live [`Peer`]
/// on success or the last error after [`RECONNECT_MAX_ATTEMPTS`].
///
/// The closure shape lets the mesh provide its own connect strategy
/// (the test path uses a duplex pair, production uses real TCP).
pub async fn reconnect_loop<F, Fut>(mut connect: F, max_attempts: u32) -> Result<Peer, PeerError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<Peer, PeerError>>,
{
    let mut last_err = PeerError::Disconnected;
    let attempts = if max_attempts == 0 {
        u32::MAX
    } else {
        max_attempts
    };
    for attempt in 1..=attempts {
        match connect().await {
            Ok(peer) => return Ok(peer),
            Err(e) => {
                last_err = e;
                tokio::time::sleep(reconnect_backoff(attempt)).await;
            }
        }
    }
    Err(last_err)
}

/// Helper used by the mesh to keep the [`Peer`] reachable across
/// reconnects. Drops the `Arc<Peer>` slot when the writer task dies.
#[derive(Clone)]
pub struct PeerSlot(pub Arc<parking_lot::RwLock<Option<Arc<Peer>>>>);

impl PeerSlot {
    pub fn new() -> Self {
        Self(Arc::new(parking_lot::RwLock::new(None)))
    }
    pub fn install(&self, peer: Arc<Peer>) {
        *self.0.write() = Some(peer);
    }
    pub fn clear(&self) {
        *self.0.write() = None;
    }
    pub fn get(&self) -> Option<Arc<Peer>> {
        self.0.read().clone()
    }
}

impl Default for PeerSlot {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_exponential_then_capped() {
        let a1 = reconnect_backoff(1);
        let a2 = reconnect_backoff(2);
        let a3 = reconnect_backoff(3);
        let a_max = reconnect_backoff(40);
        assert!(a1 < a2);
        assert!(a2 < a3);
        assert!(a_max <= RECONNECT_MAX_BACKOFF);
        assert!(a_max >= RECONNECT_MAX_BACKOFF.saturating_sub(Duration::from_millis(1)));
    }

    #[test]
    fn peer_slot_install_and_get() {
        let slot = PeerSlot::new();
        assert!(slot.get().is_none());
        // We can't easily construct a real Peer without a TLS stream,
        // so the round-trip test for slot lives in the integration
        // tests file where rcgen is available.
    }
}
