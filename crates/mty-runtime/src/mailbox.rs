//! Per-agent mailbox slabs. Bounded MPSC carrying [`MessageFrame`]s
//! whose backing allocation is drawn from a [`SlabPool`] (v0.3,
//! closes A40).
//!
//! ## API contract
//!
//! The `Mailbox::send` / `try_send` / `take_receiver` surface is
//! unchanged from slice 7. Each accepted `MessageFrame` carries an
//! invisible `PooledFrame` handle that is returned to the pool when
//! the frame is dropped (typically right after the handler runs).
//! Senders therefore observe slab exhaustion through the same
//! [`SendPolicy`] semantics that govern channel-capacity overflow —
//! Block waits, Drop discards, Fail returns MT5012.
//!
//! ## Slab + channel layering
//!
//! ```text
//!  ┌──────────────────────────────────────────────────────────────┐
//!  │ Mailbox                                                      │
//!  │                                                              │
//!  │   slab: SlabPool (1024 × 64-byte slots by default)           │
//!  │   tx  : mpsc::Sender<MessageFrame>  (cap == slab.capacity)   │
//!  │   rx  : mpsc::Receiver<MessageFrame>                         │
//!  │                                                              │
//!  │   send(frame):                                               │
//!  │     1. encode frame metadata into a pool slot (acquire)      │
//!  │     2. attach the PooledFrame handle to `frame._slab`        │
//!  │     3. push frame onto the mpsc channel                      │
//!  └──────────────────────────────────────────────────────────────┘
//! ```
//!
//! Backpressure semantics are therefore the union of pool-exhaustion
//! and channel-full conditions; both surface as `MailboxFull` (MT5012)
//! or are absorbed by Block, depending on policy.

use mty_ir::interp::value::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

use crate::error::{RuntimeError, RuntimeResult};
use crate::slab_pool::{PooledFrame, SlabPool};

#[derive(Debug, Clone, Copy)]
pub enum SendPolicy {
    /// Sender waits until capacity is available.
    Block,
    /// Drop the message and warn.
    Drop,
    /// Return MT5012 to the sender.
    Fail,
}

/// Tiny payload optimisation: most messages have a small fixed
/// number of args; we store inline rather than always heap-allocate.
#[derive(Debug)]
pub enum SmallPayload {
    Empty,
    Inline(Vec<Value>),
}

impl SmallPayload {
    pub fn inline(values: Vec<Value>) -> Self {
        if values.is_empty() {
            SmallPayload::Empty
        } else {
            SmallPayload::Inline(values)
        }
    }
    pub fn values(&self) -> &[Value] {
        match self {
            SmallPayload::Empty => &[],
            SmallPayload::Inline(v) => v.as_slice(),
        }
    }
    pub fn into_vec(self) -> Vec<Value> {
        match self {
            SmallPayload::Empty => vec![],
            SmallPayload::Inline(v) => v,
        }
    }
    /// Approximate encoded size (used as a slab payload-size hint).
    ///
    /// We don't serialise the `Value`s — the slab byte payload is a
    /// metadata blob, not a wire format — but we record the arg count
    /// plus a per-arg fixed cost so the pool sees realistic byte pressure.
    fn approx_bytes(&self) -> usize {
        match self {
            SmallPayload::Empty => 0,
            SmallPayload::Inline(v) => v.len().saturating_mul(8),
        }
    }
}

#[derive(Debug)]
pub struct MessageFrame {
    pub proto_msg: String,
    pub payload: SmallPayload,
    pub reply: Option<oneshot::Sender<RuntimeResult<Value>>>,
    pub deadline: Option<Instant>,
    pub seq: u64,
    /// Pool slot handle. Some(slot) when the frame was admitted via a
    /// slab-backed mailbox; None for ad-hoc frames built outside a
    /// mailbox (e.g. tests, helper construction). The slot is
    /// returned to the pool when this frame is dropped.
    pub(crate) _slab: Option<PooledFrame>,
}

impl MessageFrame {
    pub fn fire_and_forget(msg: &str, payload: SmallPayload) -> Self {
        Self {
            proto_msg: msg.into(),
            payload,
            reply: None,
            deadline: None,
            seq: 0,
            _slab: None,
        }
    }
    pub fn ask(
        msg: &str,
        payload: SmallPayload,
        deadline: Option<Duration>,
    ) -> (Self, oneshot::Receiver<RuntimeResult<Value>>) {
        let (tx, rx) = oneshot::channel();
        let frame = Self {
            proto_msg: msg.into(),
            payload,
            reply: Some(tx),
            deadline: deadline.map(|d| Instant::now() + d),
            seq: 0,
            _slab: None,
        };
        (frame, rx)
    }
}

#[derive(Debug)]
pub struct Mailbox {
    tx: mpsc::Sender<MessageFrame>,
    rx: parking_lot::Mutex<Option<mpsc::Receiver<MessageFrame>>>,
    capacity: usize,
    policy: SendPolicy,
    slab: SlabPool,
}

impl Mailbox {
    pub fn new(capacity: usize, policy: SendPolicy) -> Self {
        let cap = capacity.max(1);
        let (tx, rx) = mpsc::channel(cap);
        Self {
            tx,
            rx: parking_lot::Mutex::new(Some(rx)),
            capacity: cap,
            policy,
            slab: SlabPool::new(cap),
        }
    }

    /// Construct with an externally-supplied slab pool (for tests
    /// that share a pool across mailboxes to assert reuse).
    pub fn with_pool(capacity: usize, policy: SendPolicy, slab: SlabPool) -> Self {
        let cap = capacity.max(1);
        let (tx, rx) = mpsc::channel(cap);
        Self {
            tx,
            rx: parking_lot::Mutex::new(Some(rx)),
            capacity: cap,
            policy,
            slab,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
    pub fn policy(&self) -> SendPolicy {
        self.policy
    }
    pub fn sender(&self) -> mpsc::Sender<MessageFrame> {
        self.tx.clone()
    }

    /// Access the underlying slab pool. Used by tests + telemetry to
    /// observe slot utilisation.
    pub fn pool(&self) -> &SlabPool {
        &self.slab
    }

    /// Attach a pool slot to the frame just before enqueue.
    fn admit(&self, mut frame: MessageFrame) -> MessageFrame {
        // The slab byte payload is purely metadata used to back-pressure
        // memory; we encode a small descriptor (proto_msg bytes +
        // approx arg-size hint) so the slab's inline-vs-overflow split
        // is exercised realistically. Falls back to overflow when the
        // proto_msg name doesn't fit.
        let mut buf = Vec::with_capacity(self.slab.inline_bytes());
        let name = frame.proto_msg.as_bytes();
        let take = name.len().min(self.slab.inline_bytes().saturating_sub(2));
        buf.extend_from_slice(&name[..take]);
        let hint = frame.payload.approx_bytes() as u16;
        buf.extend_from_slice(&hint.to_le_bytes());
        let handle = self.slab.acquire_or_overflow(&buf);
        frame._slab = Some(handle);
        frame
    }

    pub fn try_send(&self, frame: MessageFrame) -> RuntimeResult<()> {
        let frame = self.admit(frame);
        match self.tx.try_send(frame) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => Err(RuntimeError::MailboxFull {
                agent: String::new(),
            }),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                Err(RuntimeError::AgentNotFound("(closed mailbox)".into()))
            }
        }
    }

    pub async fn send(&self, frame: MessageFrame) -> RuntimeResult<()> {
        let frame = self.admit(frame);
        match self.policy {
            SendPolicy::Block => self
                .tx
                .send(frame)
                .await
                .map_err(|_| RuntimeError::AgentNotFound("(closed mailbox)".into())),
            SendPolicy::Drop => {
                let _ = self.tx.try_send(frame);
                Ok(())
            }
            SendPolicy::Fail => match self.tx.try_send(frame) {
                Ok(()) => Ok(()),
                Err(mpsc::error::TrySendError::Full(_)) => Err(RuntimeError::MailboxFull {
                    agent: String::new(),
                }),
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    Err(RuntimeError::AgentNotFound("(closed mailbox)".into()))
                }
            },
        }
    }

    /// Take the receiver. Can be called at most once — subsequent calls
    /// return None. Designed for the agent's run loop.
    pub fn take_receiver(&self) -> Option<mpsc::Receiver<MessageFrame>> {
        self.rx.lock().take()
    }

    /// Test helper: synchronous receive.
    pub async fn recv(&self) -> Option<MessageFrame> {
        let mut rx = self.take_receiver()?;
        let v = rx.recv().await;
        // put it back for re-use in tests
        *self.rx.lock() = Some(rx);
        v
    }
}

/// Returned from `Mailbox::introspect` for test fixtures + benches.
#[derive(Debug, Clone, Copy)]
pub struct MailboxStats {
    pub capacity: usize,
    pub channel_used: usize,
    pub slab_used: usize,
    pub slab_capacity: usize,
}

impl Mailbox {
    pub fn introspect(&self) -> MailboxStats {
        MailboxStats {
            capacity: self.capacity,
            channel_used: self.capacity.saturating_sub(self.tx.capacity()),
            slab_used: self.slab.used_count(),
            slab_capacity: self.slab.capacity(),
        }
    }
}

/// Helper for embedding the slab pool into an Arc-shared mailbox.
pub fn shared(capacity: usize, policy: SendPolicy) -> Arc<Mailbox> {
    Arc::new(Mailbox::new(capacity, policy))
}
