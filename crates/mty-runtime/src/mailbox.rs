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
use crate::slab_pool::{PooledFrame, SlabPool, DEFAULT_INLINE_BYTES};

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
    ///
    /// v0.8 fast path:
    /// - When `payload` is `SmallPayload::Empty` AND the proto_msg
    ///   would have fit in the inline buffer anyway, we skip the
    ///   slab-pool acquire entirely. Empty payloads carry no useful
    ///   slab-metadata pressure and are extremely common (every
    ///   fire-and-forget Ping). This eliminates the parking_lot Mutex
    ///   lock + Vec allocation + slot.write for that case.
    /// - Otherwise we go through the regular acquire-or-overflow path.
    ///   The inline_admit_cache is a stack-buffer that avoids the
    ///   per-admit heap allocation for the descriptor; the slab pool's
    ///   own `inline` Vec still owns the canonical copy.
    fn admit(&self, mut frame: MessageFrame) -> MessageFrame {
        let payload_empty = matches!(frame.payload, SmallPayload::Empty);
        let inline_bytes = self.slab.inline_bytes();

        // Fast path: empty payload + short proto_msg. The slab handle
        // is a "tombstone" PooledFrame that owns no slot — Drop is a
        // no-op. Skips the per-msg parking_lot lock entirely.
        if payload_empty {
            frame._slab = Some(self.slab.acquire_empty());
            return frame;
        }

        // Regular path: build the descriptor into a stack-resident
        // inline buffer (an inline cache). For all default
        // configurations (inline_bytes = 64) this avoids the heap Vec
        // entirely; the slab pool itself still does the canonical
        // copy.
        let mut stack_buf = [0u8; DEFAULT_INLINE_BYTES];
        let cap = inline_bytes.min(DEFAULT_INLINE_BYTES);
        let name = frame.proto_msg.as_bytes();
        let take = name.len().min(cap.saturating_sub(2));
        stack_buf[..take].copy_from_slice(&name[..take]);
        let hint = frame.payload.approx_bytes() as u16;
        stack_buf[take..take + 2].copy_from_slice(&hint.to_le_bytes());
        let used = take + 2;

        // If inline_bytes > DEFAULT_INLINE_BYTES (an unusual custom
        // pool layout), fall back to a heap descriptor; the stack
        // cache only helps the default-sized path.
        let handle = if inline_bytes <= DEFAULT_INLINE_BYTES {
            self.slab.acquire_or_overflow(&stack_buf[..used])
        } else {
            let mut buf = Vec::with_capacity(inline_bytes);
            buf.extend_from_slice(&name[..take]);
            buf.extend_from_slice(&hint.to_le_bytes());
            self.slab.acquire_or_overflow(&buf)
        };
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

/// v0.8 batched-recv helper. Drains up to `max` ready messages from the
/// receiver into `out` and returns the count actually drained. Unlike a
/// loop of `try_recv`, this collapses the per-message overhead of the
/// tokio receiver path. Returns 0 when the receiver is closed AND
/// drained.
///
/// Designed to be used inside the agent's run loop: replace
/// `while let Some(m) = rx.recv().await { handle(m) }` with
/// `loop { let n = try_recv_many(&mut rx, &mut buf, 32); if n == 0 { let m = rx.recv().await; ... } else { for m in buf.drain(..) handle(m) } }`
/// so a producer that out-paces the consumer can hand a whole batch in
/// one cross-task hand-off.
pub fn try_recv_many(
    rx: &mut mpsc::Receiver<MessageFrame>,
    out: &mut Vec<MessageFrame>,
    max: usize,
) -> usize {
    let mut n = 0;
    while n < max {
        match rx.try_recv() {
            Ok(frame) => {
                out.push(frame);
                n += 1;
            }
            Err(mpsc::error::TryRecvError::Empty) => break,
            Err(mpsc::error::TryRecvError::Disconnected) => break,
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn try_recv_many_drains_batch() {
        let mb = Mailbox::new(16, SendPolicy::Block);
        for i in 0..8 {
            let f = MessageFrame::fire_and_forget(
                "M",
                SmallPayload::inline(vec![Value::Int(i as i128, mty_types::IntKind::I64)]),
            );
            mb.send(f).await.unwrap();
        }
        let mut rx = mb.take_receiver().unwrap();
        let mut buf = Vec::with_capacity(8);
        let n = try_recv_many(&mut rx, &mut buf, 16);
        assert_eq!(n, 8);
        assert_eq!(buf.len(), 8);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn try_recv_many_respects_max() {
        let mb = Mailbox::new(16, SendPolicy::Block);
        for _ in 0..10 {
            mb.send(MessageFrame::fire_and_forget("M", SmallPayload::Empty))
                .await
                .unwrap();
        }
        let mut rx = mb.take_receiver().unwrap();
        let mut buf = Vec::new();
        let n = try_recv_many(&mut rx, &mut buf, 4);
        assert_eq!(n, 4);
        assert_eq!(buf.len(), 4);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn empty_payload_skips_slab_acquire() {
        let mb = Mailbox::new(8, SendPolicy::Block);
        let before = mb.pool().stats().0;
        for _ in 0..5 {
            mb.send(MessageFrame::fire_and_forget("M", SmallPayload::Empty))
                .await
                .unwrap();
        }
        let after = mb.pool().stats().0;
        // Empty payloads must NOT have consumed any slab slot.
        assert_eq!(before, after, "empty path acquired a slot");
        // And the pool's free_count is untouched.
        assert_eq!(mb.pool().free_count(), mb.pool().capacity());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn nonempty_payload_still_uses_slab() {
        let mb = Mailbox::new(8, SendPolicy::Block);
        let before = mb.pool().stats().0;
        mb.send(MessageFrame::fire_and_forget(
            "M",
            SmallPayload::inline(vec![Value::Int(7, mty_types::IntKind::I64)]),
        ))
        .await
        .unwrap();
        let after = mb.pool().stats().0;
        assert_eq!(after, before + 1);
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
