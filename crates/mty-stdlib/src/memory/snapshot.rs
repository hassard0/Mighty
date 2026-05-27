//! Snapshot integration with the v0.19 byte-identical replay layer.
//!
//! The on-disk snapshot type is intentionally a thin newtype around
//! `Vec<u8>` — every backend chooses its own encoding (the local
//! vector store uses canonical JSON, episodic uses the same, the
//! qdrant backend stores a tiny URL/collection descriptor) and just
//! has to be deterministic about it.
//!
//! ### Replay wiring
//!
//! Every `record_*` call on a memory handle emits a [`MemoryDelta`]
//! event into the v0.19 trace via [`record_memory_delta`]. The
//! [`MemoryDelta::Snapshot`] variant carries the full snapshot bytes;
//! [`MemoryDelta::Patch`] carries the operation name + arbitrary
//! bytes so a backend can implement a cheaper delta encoding later
//! without breaking the wire format.
//!
//! Until the runtime ships a dedicated `TraceEvent::Memory…` variant
//! (planned for v0.27), the integration piggy-backs on the existing
//! [`mty_runtime::replay::with_recorder`] hook by serializing the
//! delta as JSON bytes and routing it through `record_io_read` with
//! the synthetic source label `"memory:<handle_kind>"`. Replay can
//! filter on that prefix to reconstruct the deltas.

use serde::{Deserialize, Serialize};

/// Portable byte-encoded snapshot of a memory handle. Wrapped so
/// signatures stay legible and we can add `Debug`-shape helpers
/// without changing the byte payload.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SnapshotBytes(pub Vec<u8>);

impl SnapshotBytes {
    /// Wrap a byte buffer as a `SnapshotBytes`.
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Borrow the raw bytes.
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// Consume into the raw bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// `true` if the snapshot has no bytes.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<Vec<u8>> for SnapshotBytes {
    fn from(v: Vec<u8>) -> Self {
        Self(v)
    }
}

/// One memory mutation captured into the replay trace.
///
/// The replayer reconstructs handle state by either:
/// 1. Replaying every `Patch` event in order (cheap delta encoding,
///    used by backends that have one).
/// 2. Reading the last `Snapshot` event before the target frame
///    (always works, used by backends that don't yet implement
///    `Patch` — currently all of them).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MemoryDelta {
    /// Full handle state snapshot.
    Snapshot {
        /// Stable backend kind (e.g. `"vector.local"`).
        handle_kind: String,
        /// Logical handle id assigned by the agent / runtime — when
        /// multiple memory handles co-exist on one agent the replayer
        /// uses this to route deltas to the right handle.
        handle_id: String,
        /// Snapshot bytes.
        snapshot: SnapshotBytes,
    },
    /// Incremental patch — opaque to the snapshot layer; each backend
    /// decides what its `op` strings mean. Currently only used by
    /// tests; production backends emit full `Snapshot` events.
    Patch {
        handle_kind: String,
        handle_id: String,
        /// Operation name (e.g. `"upsert"`, `"delete"`, `"record"`).
        op: String,
        /// Operation bytes — backend-defined encoding.
        bytes: Vec<u8>,
    },
}

impl MemoryDelta {
    /// JSON encode for the trace event payload. Deterministic — uses
    /// `serde_json` which sorts struct fields in declaration order.
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// Decode from JSON bytes (the inverse of [`encode`]).
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(bytes).map_err(|e| e.to_string())
    }
}

/// Synthetic `IoRead.source` prefix used to route memory deltas
/// through the v0.19 recorder until v0.27 adds a dedicated
/// `TraceEvent::Memory*` variant.
pub const MEMORY_SOURCE_PREFIX: &str = "memory:";

/// Build the `IoRead.source` label for a given handle kind.
pub fn memory_source_label(handle_kind: &str) -> String {
    format!("{MEMORY_SOURCE_PREFIX}{handle_kind}")
}

/// Forward a [`MemoryDelta`] into the process-wide recorder, if one
/// is installed. Zero-overhead when recording is disabled.
///
/// The `agent` argument is the agent id the recorder will attribute
/// the event to. Callers that don't have an agent context can pass
/// `0` (the synthetic external-sender id used elsewhere in the trace
/// wire format).
pub fn record_memory_delta(agent: u64, delta: &MemoryDelta) {
    let handle_kind = match delta {
        MemoryDelta::Snapshot { handle_kind, .. } | MemoryDelta::Patch { handle_kind, .. } => {
            handle_kind.clone()
        }
    };
    let source = memory_source_label(&handle_kind);
    let bytes = delta.encode();
    mty_runtime::replay::with_recorder(|rec| {
        rec.record_io_read(agent, &source, bytes.clone());
    });
}

/// Filter helper for replay: returns `true` if a given `IoRead.source`
/// label was produced by [`record_memory_delta`].
pub fn is_memory_event(source: &str) -> bool {
    source.starts_with(MEMORY_SOURCE_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_bytes_round_trips_through_serde() {
        let s = SnapshotBytes::new(b"hello".to_vec());
        let js = serde_json::to_string(&s).unwrap();
        let back: SnapshotBytes = serde_json::from_str(&js).unwrap();
        assert_eq!(s, back);
        assert_eq!(back.as_slice(), b"hello");
        assert_eq!(back.len(), 5);
        assert!(!back.is_empty());
    }

    #[test]
    fn delta_snapshot_round_trips() {
        let d = MemoryDelta::Snapshot {
            handle_kind: "vector.local".into(),
            handle_id: "h1".into(),
            snapshot: SnapshotBytes::new(b"payload".to_vec()),
        };
        let bytes = d.encode();
        let back = MemoryDelta::decode(&bytes).unwrap();
        assert_eq!(d, back);
    }

    #[test]
    fn delta_patch_round_trips() {
        let d = MemoryDelta::Patch {
            handle_kind: "episodic.in_memory".into(),
            handle_id: "ep1".into(),
            op: "record".into(),
            bytes: b"k=>v".to_vec(),
        };
        let back = MemoryDelta::decode(&d.encode()).unwrap();
        assert_eq!(d, back);
    }

    #[test]
    fn memory_source_label_uses_prefix() {
        let l = memory_source_label("vector.local");
        assert!(is_memory_event(&l));
        assert!(!is_memory_event("file:/etc/foo"));
    }
}
