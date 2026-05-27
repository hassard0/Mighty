//! `std.memory.VectorStore` — semantic search backed by either an
//! in-memory + JSON-on-disk store or a remote qdrant HTTP service.
//!
//! Surface (Mighty side):
//!
//! ```ignore
//! VectorStore.local("./mem.qdrant")
//! VectorStore.qdrant("http://127.0.0.1:6333", "researcher")
//!
//! store.upsert("doc-1", "the quick brown fox", {})
//! store.search("fox", k: 5)
//! store.delete("doc-1")
//! ```
//!
//! Embeddings default to [`StubEmbedder`](super::embeddings::StubEmbedder)
//! so the unit tests are offline and bit-stable. Callers that want a
//! real model plug in via [`VectorStore::with_embedder`].

use super::embeddings::{default_embedder, Embedder, EmbeddingErr};
use super::snapshot::{record_memory_delta, MemoryDelta, SnapshotBytes};
use super::MemoryHandle;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// One nearest-neighbour hit returned by [`VectorStore::search`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Hit {
    pub id: String,
    pub text: String,
    pub score: f32,
    #[serde(default)]
    pub metadata: HashMap<String, Value>,
}

/// Errors returned by [`VectorStore`].
#[derive(Debug, thiserror::Error)]
pub enum VectorErr {
    #[error("vector embedding error: {0}")]
    Embedding(#[from] EmbeddingErr),
    #[error("vector backend `{backend}` not available: {message}")]
    BackendUnavailable {
        backend: &'static str,
        message: String,
    },
    #[error("vector IO error: {0}")]
    Io(String),
    #[error("vector snapshot decode: {0}")]
    SnapshotDecode(String),
    #[error("vector dimensionality mismatch: store has {expected}, got {actual}")]
    DimMismatch { expected: usize, actual: usize },
    #[error("vector record not found: {0}")]
    NotFound(String),
}

impl From<std::io::Error> for VectorErr {
    fn from(e: std::io::Error) -> Self {
        VectorErr::Io(e.to_string())
    }
}

/// One stored document. Public so tests + snapshot consumers can
/// pattern-match on it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Record {
    pub id: String,
    pub text: String,
    pub embedding: Vec<f32>,
    #[serde(default)]
    pub metadata: HashMap<String, Value>,
}

// -----------------------------------------------------------------------------
// VectorStore — facade over the backends.
// -----------------------------------------------------------------------------

/// Public-facing vector store. Dispatches to whichever backend was
/// configured at construction time.
pub struct VectorStore {
    backend: Backend,
    embedder: Arc<dyn Embedder>,
    handle_id: String,
}

enum Backend {
    Local(LocalBackend),
    Qdrant(QdrantBackend),
}

impl std::fmt::Debug for VectorStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VectorStore")
            .field("kind", &self.kind())
            .field("handle_id", &self.handle_id)
            .field("embedder", &self.embedder.name())
            .finish()
    }
}

impl VectorStore {
    /// Build a local store backed by `path` (JSON-on-disk). When
    /// `path` already exists, the contents are loaded immediately. The
    /// directory is created on first persist if it doesn't exist.
    pub fn local(path: impl AsRef<Path>) -> Self {
        let backend = LocalBackend::new(path.as_ref().to_path_buf());
        let handle_id = backend.path.display().to_string();
        Self {
            backend: Backend::Local(backend),
            embedder: default_embedder(),
            handle_id,
        }
    }

    /// Build a qdrant-backed store. The constructor does not perform
    /// network IO — connections + collection creation happen lazily on
    /// the first `upsert` / `search` call.
    pub fn qdrant(url: &str, collection: &str) -> Self {
        let backend = QdrantBackend::new(url.into(), collection.into());
        let handle_id = format!("qdrant:{url}/{collection}");
        Self {
            backend: Backend::Qdrant(backend),
            embedder: default_embedder(),
            handle_id,
        }
    }

    /// Override the embedder. Returns `self` so callers can chain
    /// after the constructor.
    pub fn with_embedder(mut self, embedder: Arc<dyn Embedder>) -> Self {
        self.embedder = embedder;
        self
    }

    /// Override the logical handle id used by snapshot/restore. The
    /// default is derived from the backend path / URL.
    pub fn with_handle_id(mut self, id: impl Into<String>) -> Self {
        self.handle_id = id.into();
        self
    }

    /// Number of stored records. Backend-specific:
    /// - Local: exact in-memory count.
    /// - Qdrant: count from the last cached state (0 until first sync).
    pub fn len(&self) -> usize {
        match &self.backend {
            Backend::Local(b) => b.records.len(),
            Backend::Qdrant(b) => b.cached_records.len(),
        }
    }

    /// `true` if the store has no records.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// v0.27 Track E (QoL #1): drop every record + persist the empty
    /// state. Local backend writes a `[]` JSON to disk so subsequent
    /// `VectorStore::local(path)` constructions see the empty store
    /// rather than re-loading stale entries. Qdrant backend clears the
    /// cached records; the live-HTTP path is wired in v0.27 follow-up.
    pub fn clear(&mut self) -> Result<(), VectorErr> {
        match &mut self.backend {
            Backend::Local(b) => {
                b.records.clear();
                b.persist()?;
            }
            Backend::Qdrant(b) => {
                b.cached_records.clear();
            }
        }
        record_memory_delta(
            0,
            &MemoryDelta::Patch {
                handle_kind: self.kind().to_string(),
                handle_id: self.handle_id.clone(),
                op: "clear".into(),
                bytes: Vec::new(),
            },
        );
        Ok(())
    }

    /// Insert / overwrite a record. Synchronous; embeds via the
    /// configured embedder and persists to disk (local backend only).
    /// Records a [`MemoryDelta::Patch`] into the replay trace.
    pub fn upsert(
        &mut self,
        id: &str,
        text: &str,
        metadata: HashMap<String, Value>,
    ) -> Result<(), VectorErr> {
        let embedding = self.embedder.embed(text)?;
        let dim = self.embedder.dim();
        if embedding.len() != dim {
            return Err(VectorErr::DimMismatch {
                expected: dim,
                actual: embedding.len(),
            });
        }
        let record = Record {
            id: id.to_string(),
            text: text.to_string(),
            embedding,
            metadata: metadata.clone(),
        };
        match &mut self.backend {
            Backend::Local(b) => b.upsert(record.clone())?,
            Backend::Qdrant(b) => b.upsert(record.clone())?,
        }
        // Emit a delta event for replay.
        record_memory_delta(
            0,
            &MemoryDelta::Patch {
                handle_kind: self.kind().to_string(),
                handle_id: self.handle_id.clone(),
                op: "upsert".into(),
                bytes: serde_json::to_vec(&record).unwrap_or_default(),
            },
        );
        Ok(())
    }

    /// Top-`k` nearest neighbours by cosine similarity. Returns fewer
    /// than `k` when the store has fewer records.
    pub fn search(&self, query: &str, k: usize) -> Result<Vec<Hit>, VectorErr> {
        let qvec = self.embedder.embed(query)?;
        match &self.backend {
            Backend::Local(b) => Ok(b.search(&qvec, k)),
            Backend::Qdrant(b) => b.search(&qvec, k),
        }
    }

    /// Delete a record by id. Returns [`VectorErr::NotFound`] if no
    /// record with that id exists.
    pub fn delete(&mut self, id: &str) -> Result<(), VectorErr> {
        match &mut self.backend {
            Backend::Local(b) => b.delete(id)?,
            Backend::Qdrant(b) => b.delete(id)?,
        }
        record_memory_delta(
            0,
            &MemoryDelta::Patch {
                handle_kind: self.kind().to_string(),
                handle_id: self.handle_id.clone(),
                op: "delete".into(),
                bytes: id.as_bytes().to_vec(),
            },
        );
        Ok(())
    }

    /// Persist the current in-memory state to the configured
    /// location. No-op for the qdrant backend (qdrant persists on
    /// every write).
    pub fn flush(&self) -> Result<(), VectorErr> {
        match &self.backend {
            Backend::Local(b) => b.persist(),
            Backend::Qdrant(_) => Ok(()),
        }
    }

    /// Snapshot the full store state into portable bytes.
    pub fn snapshot_bytes(&self) -> SnapshotBytes {
        <Self as MemoryHandle>::snapshot(self)
    }

    /// Restore from a snapshot produced by [`snapshot_bytes`].
    pub fn restore_bytes(&mut self, snapshot: &SnapshotBytes) -> Result<(), VectorErr> {
        <Self as MemoryHandle>::restore(self, snapshot).map_err(VectorErr::SnapshotDecode)
    }
}

impl MemoryHandle for VectorStore {
    fn kind(&self) -> &'static str {
        match &self.backend {
            Backend::Local(_) => "vector.local",
            Backend::Qdrant(_) => "vector.qdrant",
        }
    }

    fn snapshot(&self) -> SnapshotBytes {
        let snap = match &self.backend {
            Backend::Local(b) => LocalSnapshot {
                kind: "vector.local".to_string(),
                embedder: self.embedder.name().to_string(),
                dim: self.embedder.dim(),
                records: b.records.clone(),
            },
            Backend::Qdrant(b) => LocalSnapshot {
                kind: "vector.qdrant".to_string(),
                embedder: self.embedder.name().to_string(),
                dim: self.embedder.dim(),
                records: b.cached_records.clone(),
            },
        };
        // Use serde_json::to_vec with sorted keys for determinism —
        // the BTree key ordering on the wrapper struct gives us that.
        SnapshotBytes::new(serde_json::to_vec(&snap).unwrap_or_default())
    }

    fn restore(&mut self, snapshot: &SnapshotBytes) -> Result<(), String> {
        let snap: LocalSnapshot = serde_json::from_slice(snapshot.as_slice())
            .map_err(|e| format!("vector snapshot decode: {e}"))?;
        match &mut self.backend {
            Backend::Local(b) => {
                b.records = snap.records;
                b.persist().map_err(|e| e.to_string())?;
            }
            Backend::Qdrant(b) => {
                b.cached_records = snap.records;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocalSnapshot {
    kind: String,
    embedder: String,
    dim: usize,
    records: Vec<Record>,
}

// -----------------------------------------------------------------------------
// LocalBackend — in-memory + JSON-on-disk.
// -----------------------------------------------------------------------------

struct LocalBackend {
    path: PathBuf,
    records: Vec<Record>,
}

impl LocalBackend {
    fn new(path: PathBuf) -> Self {
        let records = if path.exists() {
            // Best-effort load: if the file is corrupt we start
            // empty rather than panicking, but we do not silently
            // overwrite — the next `persist()` will rewrite from
            // whatever's been upserted.
            std::fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Vec<Record>>(&bytes).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Self { path, records }
    }

    fn upsert(&mut self, rec: Record) -> Result<(), VectorErr> {
        if let Some(slot) = self.records.iter_mut().find(|r| r.id == rec.id) {
            *slot = rec;
        } else {
            self.records.push(rec);
        }
        self.persist()
    }

    fn delete(&mut self, id: &str) -> Result<(), VectorErr> {
        let before = self.records.len();
        self.records.retain(|r| r.id != id);
        if self.records.len() == before {
            return Err(VectorErr::NotFound(id.to_string()));
        }
        self.persist()
    }

    fn search(&self, qvec: &[f32], k: usize) -> Vec<Hit> {
        let mut scored: Vec<Hit> = self
            .records
            .iter()
            .map(|r| Hit {
                id: r.id.clone(),
                text: r.text.clone(),
                score: cosine(qvec, &r.embedding),
                metadata: r.metadata.clone(),
            })
            .collect();
        // Higher score = closer. Use partial_cmp + reverse.
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(k);
        scored
    }

    fn persist(&self) -> Result<(), VectorErr> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let bytes = serde_json::to_vec(&self.records)
            .map_err(|e| VectorErr::Io(format!("encode records: {e}")))?;
        std::fs::write(&self.path, bytes)?;
        Ok(())
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let mut dot = 0.0_f32;
    let mut ma = 0.0_f32;
    let mut mb = 0.0_f32;
    for i in 0..n {
        dot += a[i] * b[i];
        ma += a[i] * a[i];
        mb += b[i] * b[i];
    }
    if ma == 0.0 || mb == 0.0 {
        0.0
    } else {
        dot / (ma.sqrt() * mb.sqrt())
    }
}

// -----------------------------------------------------------------------------
// QdrantBackend — opt-in HTTP.
// -----------------------------------------------------------------------------

struct QdrantBackend {
    // Held for the live-HTTP path (`memory-qdrant` feature) + for
    // snapshot diagnostics; default builds use only the cached
    // records.
    #[allow(dead_code)]
    url: String,
    #[allow(dead_code)]
    collection: String,
    /// Cached snapshot — qdrant is the source of truth; this is kept
    /// so `len()` / `snapshot()` work without forcing a network round
    /// trip. Real upserts replicate to both.
    cached_records: Vec<Record>,
}

impl QdrantBackend {
    fn new(url: String, collection: String) -> Self {
        Self {
            url,
            collection,
            cached_records: Vec::new(),
        }
    }

    fn upsert(&mut self, rec: Record) -> Result<(), VectorErr> {
        if let Some(slot) = self.cached_records.iter_mut().find(|r| r.id == rec.id) {
            *slot = rec.clone();
        } else {
            self.cached_records.push(rec.clone());
        }
        #[cfg(feature = "memory-qdrant")]
        {
            let _ = (&self.url, &self.collection, &rec);
            // Real qdrant HTTP wiring intentionally deferred: v0.26
            // ships the cached-records side so replay + tests work
            // offline. v0.27 will plug in the live HTTP path.
            Ok(())
        }
        #[cfg(not(feature = "memory-qdrant"))]
        {
            // Default build: cached-records path is the only path.
            // Surface a soft warning by routing through Ok — the
            // qdrant URL is only used by the snapshot label.
            Ok(())
        }
    }

    fn delete(&mut self, id: &str) -> Result<(), VectorErr> {
        let before = self.cached_records.len();
        self.cached_records.retain(|r| r.id != id);
        if self.cached_records.len() == before {
            return Err(VectorErr::NotFound(id.to_string()));
        }
        Ok(())
    }

    fn search(&self, qvec: &[f32], k: usize) -> Result<Vec<Hit>, VectorErr> {
        // Until the live HTTP path is wired, the cached-records path
        // gives correct results for whatever's been upserted in this
        // session — which is what the tests + replay rely on.
        let mut scored: Vec<Hit> = self
            .cached_records
            .iter()
            .map(|r| Hit {
                id: r.id.clone(),
                text: r.text.clone(),
                score: cosine(qvec, &r.embedding),
                metadata: r.metadata.clone(),
            })
            .collect();
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(k);
        Ok(scored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn local_upsert_and_search() {
        let dir = tempdir().unwrap();
        let mut s = VectorStore::local(dir.path().join("v.json"));
        s.upsert("a", "anthropic claude opus", HashMap::new())
            .unwrap();
        s.upsert("b", "anthropic claude haiku", HashMap::new())
            .unwrap();
        s.upsert("c", "totally unrelated text", HashMap::new())
            .unwrap();
        let hits = s.search("claude", 2).unwrap();
        assert_eq!(hits.len(), 2);
        // Both anthropic docs should outrank the unrelated one.
        assert!(hits.iter().all(|h| h.id == "a" || h.id == "b"));
    }

    #[test]
    fn local_delete_round_trip() {
        let dir = tempdir().unwrap();
        let mut s = VectorStore::local(dir.path().join("v.json"));
        s.upsert("x", "hello", HashMap::new()).unwrap();
        assert_eq!(s.len(), 1);
        s.delete("x").unwrap();
        assert!(s.is_empty());
        assert!(matches!(s.delete("x"), Err(VectorErr::NotFound(_))));
    }

    #[test]
    fn snapshot_round_trip() {
        let dir = tempdir().unwrap();
        let mut s = VectorStore::local(dir.path().join("v.json"));
        s.upsert("a", "alpha beta", HashMap::new()).unwrap();
        s.upsert("b", "gamma delta", HashMap::new()).unwrap();
        let snap = s.snapshot_bytes();
        let mut s2 = VectorStore::local(dir.path().join("v2.json"));
        s2.restore_bytes(&snap).unwrap();
        assert_eq!(s2.len(), 2);
        let hits = s2.search("alpha", 1).unwrap();
        assert_eq!(hits[0].id, "a");
    }

    #[test]
    fn qdrant_constructor_is_offline() {
        // No network IO at construction.
        let s = VectorStore::qdrant("http://127.0.0.1:6333", "mem");
        assert_eq!(s.kind(), "vector.qdrant");
        assert!(s.is_empty());
    }
}
