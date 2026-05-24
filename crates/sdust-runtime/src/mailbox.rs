//! Per-agent mailbox slabs. Bounded MPSC carrying MessageFrames.

use sdust_sir::interp::value::Value;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

use crate::error::{RuntimeError, RuntimeResult};

#[derive(Debug, Clone, Copy)]
pub enum SendPolicy {
    /// Sender waits until capacity is available.
    Block,
    /// Drop the message and warn.
    Drop,
    /// Return SD5012 to the sender.
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
}

#[derive(Debug)]
pub struct MessageFrame {
    pub proto_msg: String,
    pub payload: SmallPayload,
    pub reply: Option<oneshot::Sender<RuntimeResult<Value>>>,
    pub deadline: Option<Instant>,
    pub seq: u64,
}

impl MessageFrame {
    pub fn fire_and_forget(msg: &str, payload: SmallPayload) -> Self {
        Self {
            proto_msg: msg.into(),
            payload,
            reply: None,
            deadline: None,
            seq: 0,
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

    pub fn try_send(&self, frame: MessageFrame) -> RuntimeResult<()> {
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
            SendPolicy::Fail => self.try_send(frame),
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
