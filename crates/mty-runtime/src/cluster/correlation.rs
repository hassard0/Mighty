//! Per-mesh request/reply correlation table.
//!
//! When a runtime issues an `Ask` to a remote agent, the resulting
//! [`WireFrame::Ask`](crate::cluster::wire::WireFrame::Ask) carries a
//! `correlation: u64` field. The remote responds with a matching
//! [`WireFrame::Reply`](crate::cluster::wire::WireFrame::Reply) or
//! [`WireFrame::Error`](crate::cluster::wire::WireFrame::Error) that
//! quotes back the same id. The local side has to remember which
//! oneshot belongs to which correlation id while the request is
//! in-flight.
//!
//! That's all this module is: an `AtomicU64` for handing out fresh
//! ids and a `DashMap<u64, oneshot::Sender<WireFrame>>` for resolving
//! them when the reply arrives.
//!
//! ### Design notes
//!
//! - **Per-node, not per-peer.** v0.18 left the door open for a per-
//!   peer counter, but a node-wide counter is simpler and the u64
//!   space is effectively unbounded (a node would need to issue >18
//!   quintillion asks before wrapping). One table means the inbound
//!   `Reply` demultiplexer doesn't have to know which peer the reply
//!   came from to find the waker.
//!
//! - **Oneshot, not broadcast.** Exactly one consumer of the reply —
//!   the `ask()` caller. If the caller drops its `Receiver` (e.g.
//!   `tokio::time::timeout` fired), the table is still cleaned up
//!   when the corresponding `Reply` lands and the `send` errors.
//!   The `cleanup` helper handles the timeout-side teardown.
//!
//! - **Peer-disconnect cleanup.** When a peer dies mid-ask, the mesh
//!   calls [`CorrelationTable::fail_all_with`] to drop every pending
//!   oneshot with a synthesised `Error` frame. The caller's `await`
//!   resolves to a clean "peer disconnected" instead of hanging
//!   forever.

use crate::cluster::wire::WireFrame;
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::oneshot;

/// Request/reply correlation table.
///
/// Cheap to clone via `Arc<Self>` — every field is internally
/// shareable (`Atomic*` + `DashMap`).
#[derive(Debug, Default)]
pub struct CorrelationTable {
    next_id: AtomicU64,
    pending: DashMap<u64, oneshot::Sender<WireFrame>>,
    /// Side map: which target node each pending correlation targets.
    /// Used by [`Self::fail_targeting_node`] to wake every ask
    /// directed at a node that just disconnected.
    targets: DashMap<u64, String>,
}

impl CorrelationTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a fresh correlation id and return it along with the
    /// receiver the caller should await. The id is monotonically
    /// increasing within this table; the first id is `1` (we reserve
    /// `0` as a sentinel for "no correlation" in case future code
    /// wants to lift the same enum onto the async path).
    pub fn register(&self) -> (u64, oneshot::Receiver<WireFrame>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = oneshot::channel();
        self.pending.insert(id, tx);
        (id, rx)
    }

    /// Like [`Self::register`] but records the target node for later
    /// peer-disconnect fan-out. Used by the mesh; tests can stick to
    /// the bare [`Self::register`].
    pub fn register_for_node(&self, node: &str) -> (u64, oneshot::Receiver<WireFrame>) {
        let (id, rx) = self.register();
        self.targets.insert(id, node.to_string());
        (id, rx)
    }

    /// Fail every pending ask that targets `node`. Used by the mesh
    /// when a peer disconnects mid-ask so the caller's `await`
    /// resolves to a clean `Error` instead of hanging.
    pub fn fail_targeting_node(&self, node: &str) {
        let ids: Vec<u64> = self
            .targets
            .iter()
            .filter(|e| e.value() == node)
            .map(|e| *e.key())
            .collect();
        for id in ids {
            self.targets.remove(&id);
            if let Some((_, tx)) = self.pending.remove(&id) {
                let _ = tx.send(WireFrame::Error {
                    correlation: id,
                    kind: "peer_disconnected".into(),
                    message: format!("peer for node {node} disconnected mid-ask"),
                });
            }
        }
    }

    /// Resolve a pending correlation with the reply frame. Returns
    /// `true` iff a matching pending entry was found. Late or
    /// duplicate replies (caller already timed out, or a buggy peer
    /// double-replied) are silently dropped.
    pub fn complete(&self, id: u64, frame: WireFrame) -> bool {
        self.targets.remove(&id);
        if let Some((_, tx)) = self.pending.remove(&id) {
            // `tx.send` only fails if the Receiver was already
            // dropped (caller timed out). That's fine — we're
            // tearing down the slot either way.
            let _ = tx.send(frame);
            true
        } else {
            false
        }
    }

    /// Drop a pending entry without delivering a reply. Used by the
    /// `ask` caller's timeout branch to keep the map small.
    pub fn cleanup(&self, id: u64) {
        self.targets.remove(&id);
        self.pending.remove(&id);
    }

    /// Resolve every pending correlation with a synthesised error
    /// frame. Used by mesh shutdown so callers don't hang forever.
    ///
    /// The frame factory is invoked once per pending id so each
    /// caller gets their own (otherwise we'd have to clone, which
    /// `WireFrame::Error` supports but is wasteful for a hot path).
    pub fn fail_all_with<F: Fn(u64) -> WireFrame>(&self, frame_for: F) {
        // Drain via an explicit collect to avoid holding the shard
        // lock across the oneshot send (which is cheap, but the
        // shape keeps the lock window minimal).
        let ids: Vec<u64> = self.pending.iter().map(|e| *e.key()).collect();
        for id in ids {
            self.targets.remove(&id);
            if let Some((_, tx)) = self.pending.remove(&id) {
                let _ = tx.send(frame_for(id));
            }
        }
    }

    /// Number of in-flight asks (for diagnostics + tests).
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::wire::WireFrame;
    use std::sync::Arc;

    fn sample_reply(correlation: u64) -> WireFrame {
        WireFrame::Reply {
            correlation,
            msg_bytes: b"ok".to_vec(),
        }
    }

    #[tokio::test]
    async fn register_returns_unique_increasing_ids() {
        let t = CorrelationTable::new();
        let (a, _ra) = t.register();
        let (b, _rb) = t.register();
        let (c, _rc) = t.register();
        assert!(a < b);
        assert!(b < c);
        assert_eq!(t.pending_count(), 3);
    }

    #[tokio::test]
    async fn complete_resolves_receiver() {
        let t = CorrelationTable::new();
        let (id, rx) = t.register();
        assert!(t.complete(id, sample_reply(id)));
        let got = rx.await.unwrap();
        assert_eq!(got, sample_reply(id));
        assert_eq!(t.pending_count(), 0);
    }

    #[tokio::test]
    async fn complete_unknown_id_returns_false() {
        let t = CorrelationTable::new();
        assert!(!t.complete(999, sample_reply(999)));
    }

    #[tokio::test]
    async fn cleanup_drops_without_send() {
        let t = CorrelationTable::new();
        let (id, _rx) = t.register();
        assert_eq!(t.pending_count(), 1);
        t.cleanup(id);
        assert_eq!(t.pending_count(), 0);
    }

    #[tokio::test]
    async fn fail_all_with_resolves_every_pending() {
        let t = Arc::new(CorrelationTable::new());
        let (id1, rx1) = t.register();
        let (id2, rx2) = t.register();
        t.fail_all_with(|cid| WireFrame::Error {
            correlation: cid,
            kind: "peer_disconnected".into(),
            message: "peer dropped mid-ask".into(),
        });
        assert_eq!(t.pending_count(), 0);
        let got1 = rx1.await.unwrap();
        let got2 = rx2.await.unwrap();
        for (frame, expected_id) in [(got1, id1), (got2, id2)] {
            match frame {
                WireFrame::Error {
                    correlation, kind, ..
                } => {
                    assert_eq!(correlation, expected_id);
                    assert_eq!(kind, "peer_disconnected");
                }
                other => panic!("expected Error frame, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn concurrent_register_complete_resolves_correctly() {
        let t = Arc::new(CorrelationTable::new());
        let n = 100usize;
        let mut handles = Vec::with_capacity(n);
        for _ in 0..n {
            let (id, rx) = t.register();
            handles.push((id, rx));
        }
        // Complete in reverse order to ensure no implicit ordering
        // dependency.
        for (id, _) in handles.iter().rev() {
            t.complete(*id, sample_reply(*id));
        }
        for (id, rx) in handles {
            let got = rx.await.unwrap();
            match got {
                WireFrame::Reply { correlation, .. } => assert_eq!(correlation, id),
                other => panic!("expected Reply, got {other:?}"),
            }
        }
        assert_eq!(t.pending_count(), 0);
    }
}
