//! Framed CBOR wire protocol for the cluster mesh.
//!
//! Each frame on the wire is:
//!
//! ```text
//! +-----------+----------------------------+
//! | u32 BE    | CBOR-encoded WireFrame     |
//! | body len  | (body_len bytes)           |
//! +-----------+----------------------------+
//! ```
//!
//! The length prefix lets the reader allocate exactly the right buffer
//! and defends against partial reads on a streaming socket. We cap the
//! body at [`MAX_FRAME_BYTES`] (8 MiB) so a malicious or buggy peer
//! can't push us to OOM.
//!
//! [`WireFrame`] is serde-derived so adding a new variant in the future
//! is additive on the encoder side; receivers that don't know the
//! variant will fail decode and the peer will be torn down (clean,
//! audible failure beats silent skipping).
//!
//! ### Why ciborium, not serde_cbor?
//!
//! `serde_cbor` is unmaintained (last release 2021, ring buffer of
//! known bugs). `ciborium` is the modern serde-cbor implementation
//! used by `coap-lite`, `rustls-platform-verifier`, and friends. It's
//! already pulled in transitively via `webpki-roots` style chains, so
//! promoting it to a direct dep adds 0 bytes to the dep graph.

use crate::cluster::address::{AgentAddr, NodeId};
use serde::{Deserialize, Serialize};
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Protocol version. Bumped when [`WireFrame`] gains a breaking
/// change; the [`WireFrame::Hello`] handshake refuses peers with a
/// different major version.
pub const WIRE_VERSION: u32 = 1;

/// Hard cap on a single CBOR body. 8 MiB is far larger than any
/// reasonable single agent message (we're not shipping blobs over
/// this) and small enough to keep a misbehaving peer from killing
/// the node.
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

/// One protocol frame on the wire.
///
/// `msg_bytes` is the *already-encoded* user payload. We do NOT
/// re-serialize the runtime's `Value` graph here — the caller hands
/// us bytes (the runtime already has its own canonical CBOR encoding
/// for the value graph via the replay layer). This keeps the cluster
/// module decoupled from the IR.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WireFrame {
    /// Sent by both sides immediately after the TLS handshake. The
    /// receiver checks `version == WIRE_VERSION` and remembers the
    /// peer's `node_id` for routing.
    Hello { node_id: NodeId, version: u32 },

    /// Periodic liveness ping. The remote end answers with another
    /// `Heartbeat`; missed heartbeats trigger reconnect.
    Heartbeat,

    /// Fire-and-forget cross-node send.
    Send {
        from: AgentAddr,
        to: AgentAddr,
        msg: String,
        msg_bytes: Vec<u8>,
    },

    /// Request-reply cross-node ask. The `correlation` is a per-peer
    /// monotonic u64 the requester assigns; the reply carries the same
    /// id back.
    Ask {
        from: AgentAddr,
        to: AgentAddr,
        msg: String,
        msg_bytes: Vec<u8>,
        correlation: u64,
    },

    /// Successful reply for an earlier [`WireFrame::Ask`].
    Reply {
        correlation: u64,
        msg_bytes: Vec<u8>,
    },

    /// Error reply for an earlier [`WireFrame::Ask`] (or an
    /// unsolicited push when the peer wants to report a problem with
    /// an in-flight conversation).
    Error {
        correlation: u64,
        kind: String,
        message: String,
    },

    /// Voluntary teardown. Sent before closing the TCP socket so the
    /// peer can distinguish a clean shutdown from a network blip.
    Goodbye,

    /// v0.21 Tier 4.3: source node ships a paused agent's serialized
    /// state to the target. The target decodes against
    /// [`crate::reload::Resumable::SCHEMA_HASH`], spawns a fresh
    /// instance, and acks via [`WireFrame::MigrateAck`].
    ///
    /// `agent_addr.node` is the source; `target_node` is the routing
    /// target (where the mesh sends this frame).
    MigrateSnapshot {
        agent_addr: AgentAddr,
        target_node: NodeId,
        agent_type: String,
        schema_hash: u64,
        state: Vec<u8>,
    },

    /// v0.21 Tier 4.3: target acknowledges a successful restore.
    /// `route_to` is the source node id (where the ack should go).
    MigrateAck {
        migrating: AgentAddr,
        new: AgentAddr,
        route_to: NodeId,
    },

    /// v0.21 Tier 4.3: target rejected a migration. Source rolls back.
    MigrateError {
        migrating: AgentAddr,
        route_to: NodeId,
        kind: String,
        message: String,
    },
}

/// Errors raised by the framed I/O layer.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error("wire io: {0}")]
    Io(#[from] io::Error),
    #[error("wire cbor encode: {0}")]
    Encode(String),
    #[error("wire cbor decode: {0}")]
    Decode(String),
    #[error("wire frame too large: {0} > {MAX_FRAME_BYTES}")]
    FrameTooLarge(usize),
}

/// Encode a frame into a fresh `Vec<u8>` (length prefix + CBOR body).
pub fn encode_frame(frame: &WireFrame) -> Result<Vec<u8>, WireError> {
    let mut body = Vec::with_capacity(64);
    ciborium::into_writer(frame, &mut body).map_err(|e| WireError::Encode(e.to_string()))?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(WireError::FrameTooLarge(body.len()));
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Decode the next length-prefixed frame from a `&[u8]`. Used by the
/// roundtrip tests; the streaming reader lives in
/// [`read_frame_async`].
pub fn decode_frame(buf: &[u8]) -> Result<(WireFrame, usize), WireError> {
    if buf.len() < 4 {
        return Err(WireError::Decode(
            "buffer shorter than length prefix".into(),
        ));
    }
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(WireError::FrameTooLarge(len));
    }
    if buf.len() < 4 + len {
        return Err(WireError::Decode(format!(
            "buffer shorter than body (need {}+4, have {})",
            len,
            buf.len()
        )));
    }
    let frame: WireFrame =
        ciborium::from_reader(&buf[4..4 + len]).map_err(|e| WireError::Decode(e.to_string()))?;
    Ok((frame, 4 + len))
}

/// Write a frame to an `AsyncWrite` sink.
pub async fn write_frame_async<W>(w: &mut W, frame: &WireFrame) -> Result<(), WireError>
where
    W: AsyncWrite + Unpin,
{
    let bytes = encode_frame(frame)?;
    w.write_all(&bytes).await?;
    w.flush().await?;
    Ok(())
}

/// Read one frame from an `AsyncRead` source. Returns `Ok(None)` on
/// clean EOF (length prefix at byte-zero), otherwise the decoded
/// frame or a decode error.
pub async fn read_frame_async<R>(r: &mut R) -> Result<Option<WireFrame>, WireError>
where
    R: AsyncRead + Unpin,
{
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(WireError::FrameTooLarge(len));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    let frame: WireFrame =
        ciborium::from_reader(&body[..]).map_err(|e| WireError::Decode(e.to_string()))?;
    Ok(Some(frame))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_send() -> WireFrame {
        WireFrame::Send {
            from: AgentAddr::remote("a", "Sender", 1),
            to: AgentAddr::remote("b", "Receiver", 2),
            msg: "ping".into(),
            msg_bytes: b"hello".to_vec(),
        }
    }

    #[test]
    fn hello_roundtrips() {
        let frame = WireFrame::Hello {
            node_id: NodeId::new("node-x"),
            version: WIRE_VERSION,
        };
        let bytes = encode_frame(&frame).unwrap();
        let (decoded, n) = decode_frame(&bytes).unwrap();
        assert_eq!(n, bytes.len());
        assert_eq!(decoded, frame);
    }

    #[test]
    fn send_roundtrips() {
        let frame = sample_send();
        let bytes = encode_frame(&frame).unwrap();
        let (decoded, _) = decode_frame(&bytes).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn ask_reply_correlation_preserved() {
        let ask = WireFrame::Ask {
            from: AgentAddr::remote("a", "S", 1),
            to: AgentAddr::remote("b", "R", 2),
            msg: "ask".into(),
            msg_bytes: b"q".to_vec(),
            correlation: 42,
        };
        let reply = WireFrame::Reply {
            correlation: 42,
            msg_bytes: b"r".to_vec(),
        };
        let ask_b = encode_frame(&ask).unwrap();
        let reply_b = encode_frame(&reply).unwrap();
        let (ask_d, _) = decode_frame(&ask_b).unwrap();
        let (reply_d, _) = decode_frame(&reply_b).unwrap();
        assert_eq!(ask_d, ask);
        assert_eq!(reply_d, reply);
    }

    #[test]
    fn frame_too_large_rejected_on_encode() {
        let huge = WireFrame::Send {
            from: AgentAddr::remote("a", "S", 1),
            to: AgentAddr::remote("b", "R", 2),
            msg: "huge".into(),
            msg_bytes: vec![0u8; MAX_FRAME_BYTES + 1024],
        };
        let err = encode_frame(&huge).unwrap_err();
        matches!(err, WireError::FrameTooLarge(_));
    }

    #[tokio::test]
    async fn async_roundtrip_via_duplex() {
        let (mut a, mut b) = tokio::io::duplex(4096);
        let frame = sample_send();
        write_frame_async(&mut a, &frame).await.unwrap();
        let got = read_frame_async(&mut b).await.unwrap().unwrap();
        assert_eq!(got, frame);
    }
}
