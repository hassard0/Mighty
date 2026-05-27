//! `std.memory.Episodic` — append-only ring buffer of recorded
//! `(timestamp, key, value)` triples.
//!
//! Two backends ship in v0.26:
//!
//! - [`Episodic::in_memory`] — bounded `VecDeque` ring; oldest entry
//!   evicted when `max` is exceeded.
//! - [`Episodic::sqlite`] — same surface but persisted via `rusqlite`.
//!   Gated behind the `memory-sqlite` feature (default-on); when the
//!   feature is off, calling `sqlite()` returns
//!   [`EpisodicErr::FeatureDisabled`].
//!
//! The handle is intentionally tiny — episodic memory is "I want to
//! recall recent interactions"; richer structured logging lives in
//! `std.log` (not yet shipped).

use super::snapshot::{record_memory_delta, MemoryDelta, SnapshotBytes};
use super::MemoryHandle;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// One episodic entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    /// Unix milliseconds when the entry was recorded.
    pub timestamp_ms: u64,
    /// Caller-supplied key (e.g. a query, a task id).
    pub key: String,
    /// Caller-supplied value — any JSON-shaped data.
    pub value: Value,
}

/// Errors returned by [`Episodic`].
#[derive(Debug, thiserror::Error)]
pub enum EpisodicErr {
    #[error("episodic backend `{0}` is disabled in this build (feature flag off)")]
    FeatureDisabled(&'static str),
    #[error("episodic IO error: {0}")]
    Io(String),
    #[error("episodic snapshot decode: {0}")]
    SnapshotDecode(String),
}

/// Public-facing episodic memory handle. Dispatches to whichever
/// backend was configured.
pub struct Episodic {
    backend: EpisodicBackend,
    handle_id: String,
    /// Bounded max size. `usize::MAX` means unbounded (the sqlite
    /// backend uses this when callers want a "log everything" view).
    max: usize,
}

enum EpisodicBackend {
    InMemory(VecDeque<Entry>),
    #[cfg(feature = "memory-sqlite")]
    Sqlite(SqliteBackend),
}

impl std::fmt::Debug for Episodic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Episodic")
            .field("kind", &self.kind())
            .field("len", &self.len())
            .field("max", &self.max)
            .field("handle_id", &self.handle_id)
            .finish()
    }
}

impl Episodic {
    /// Build an in-memory bounded ring buffer. `max` is the hard cap;
    /// when exceeded the oldest entry is evicted.
    pub fn in_memory(max: usize) -> Self {
        let max = max.max(1);
        Self {
            backend: EpisodicBackend::InMemory(VecDeque::with_capacity(max)),
            handle_id: "episodic.in_memory".into(),
            max,
        }
    }

    /// Build a sqlite-backed episodic store at `path`. The schema is
    /// created on first call. `max` bounds the number of rows kept;
    /// pass `usize::MAX` for "no bound".
    #[cfg(feature = "memory-sqlite")]
    pub fn sqlite(path: impl AsRef<std::path::Path>, max: usize) -> Result<Self, EpisodicErr> {
        let path = path.as_ref().to_path_buf();
        let backend = SqliteBackend::open(&path)?;
        let handle_id = format!("sqlite:{}", path.display());
        Ok(Self {
            backend: EpisodicBackend::Sqlite(backend),
            handle_id,
            max: max.max(1),
        })
    }

    /// Sqlite stub for builds without the `memory-sqlite` feature.
    /// Returns [`EpisodicErr::FeatureDisabled`].
    #[cfg(not(feature = "memory-sqlite"))]
    pub fn sqlite(_path: impl AsRef<std::path::Path>, _max: usize) -> Result<Self, EpisodicErr> {
        Err(EpisodicErr::FeatureDisabled("memory-sqlite"))
    }

    /// Override the logical handle id used by snapshot/restore.
    pub fn with_handle_id(mut self, id: impl Into<String>) -> Self {
        self.handle_id = id.into();
        self
    }

    /// Current entry count.
    pub fn len(&self) -> usize {
        match &self.backend {
            EpisodicBackend::InMemory(q) => q.len(),
            #[cfg(feature = "memory-sqlite")]
            EpisodicBackend::Sqlite(b) => b.count().unwrap_or(0),
        }
    }

    /// `true` if no entries are recorded.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Append an entry. Evicts the oldest if `max` would be exceeded.
    /// Emits a `MemoryDelta::Patch { op: "record" }` into the replay
    /// trace.
    pub fn record(&mut self, key: &str, value: &Value) {
        let entry = Entry {
            timestamp_ms: now_ms(),
            key: key.to_string(),
            value: value.clone(),
        };
        match &mut self.backend {
            EpisodicBackend::InMemory(q) => {
                if q.len() >= self.max {
                    q.pop_front();
                }
                q.push_back(entry.clone());
            }
            #[cfg(feature = "memory-sqlite")]
            EpisodicBackend::Sqlite(b) => {
                let _ = b.insert(&entry);
                let _ = b.enforce_max(self.max);
            }
        }
        record_memory_delta(
            0,
            &MemoryDelta::Patch {
                handle_kind: self.kind().to_string(),
                handle_id: self.handle_id.clone(),
                op: "record".into(),
                bytes: serde_json::to_vec(&entry).unwrap_or_default(),
            },
        );
    }

    /// Most-recent `n` entries (newest first).
    pub fn recent(&self, n: usize) -> Vec<Entry> {
        let mut all = self.collect_entries();
        all.reverse();
        all.truncate(n);
        all
    }

    /// Entries whose key starts with `prefix`. Order matches insertion.
    pub fn search_by_key(&self, prefix: &str) -> Vec<Entry> {
        self.collect_entries()
            .into_iter()
            .filter(|e| e.key.starts_with(prefix))
            .collect()
    }

    /// Drop every entry. Persistent backends drop the rows too.
    pub fn clear(&mut self) {
        match &mut self.backend {
            EpisodicBackend::InMemory(q) => q.clear(),
            #[cfg(feature = "memory-sqlite")]
            EpisodicBackend::Sqlite(b) => {
                let _ = b.clear();
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
    }

    /// Snapshot the full entry list into portable bytes.
    pub fn snapshot_bytes(&self) -> SnapshotBytes {
        <Self as MemoryHandle>::snapshot(self)
    }

    /// Restore from a snapshot produced by [`snapshot_bytes`].
    pub fn restore_bytes(&mut self, snapshot: &SnapshotBytes) -> Result<(), EpisodicErr> {
        <Self as MemoryHandle>::restore(self, snapshot).map_err(EpisodicErr::SnapshotDecode)
    }

    fn collect_entries(&self) -> Vec<Entry> {
        match &self.backend {
            EpisodicBackend::InMemory(q) => q.iter().cloned().collect(),
            #[cfg(feature = "memory-sqlite")]
            EpisodicBackend::Sqlite(b) => b.all().unwrap_or_default(),
        }
    }
}

impl MemoryHandle for Episodic {
    fn kind(&self) -> &'static str {
        match &self.backend {
            EpisodicBackend::InMemory(_) => "episodic.in_memory",
            #[cfg(feature = "memory-sqlite")]
            EpisodicBackend::Sqlite(_) => "episodic.sqlite",
        }
    }

    fn snapshot(&self) -> SnapshotBytes {
        let snap = EpisodicSnapshot {
            kind: self.kind().to_string(),
            max: self.max,
            entries: self.collect_entries(),
        };
        SnapshotBytes::new(serde_json::to_vec(&snap).unwrap_or_default())
    }

    fn restore(&mut self, snapshot: &SnapshotBytes) -> Result<(), String> {
        let snap: EpisodicSnapshot = serde_json::from_slice(snapshot.as_slice())
            .map_err(|e| format!("episodic snapshot decode: {e}"))?;
        self.max = snap.max.max(1);
        match &mut self.backend {
            EpisodicBackend::InMemory(q) => {
                q.clear();
                for e in snap.entries {
                    if q.len() >= self.max {
                        q.pop_front();
                    }
                    q.push_back(e);
                }
            }
            #[cfg(feature = "memory-sqlite")]
            EpisodicBackend::Sqlite(b) => {
                b.clear().map_err(|e| e.to_string())?;
                for e in snap.entries {
                    b.insert(&e).map_err(|e| e.to_string())?;
                }
                b.enforce_max(self.max).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EpisodicSnapshot {
    kind: String,
    max: usize,
    entries: Vec<Entry>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// -----------------------------------------------------------------------------
// SqliteBackend — gated behind `memory-sqlite`.
// -----------------------------------------------------------------------------

#[cfg(feature = "memory-sqlite")]
struct SqliteBackend {
    conn: rusqlite::Connection,
    #[allow(dead_code)]
    path: PathBuf,
}

#[cfg(feature = "memory-sqlite")]
impl SqliteBackend {
    fn open(path: &std::path::Path) -> Result<Self, EpisodicErr> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| EpisodicErr::Io(e.to_string()))?;
            }
        }
        let conn = rusqlite::Connection::open(path).map_err(|e| EpisodicErr::Io(e.to_string()))?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS episodic (
              rowid INTEGER PRIMARY KEY AUTOINCREMENT,
              ts_ms INTEGER NOT NULL,
              key TEXT NOT NULL,
              value TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_episodic_key ON episodic(key);
            "#,
        )
        .map_err(|e| EpisodicErr::Io(e.to_string()))?;
        Ok(Self {
            conn,
            path: path.to_path_buf(),
        })
    }

    fn insert(&self, entry: &Entry) -> Result<(), EpisodicErr> {
        let value_str =
            serde_json::to_string(&entry.value).map_err(|e| EpisodicErr::Io(e.to_string()))?;
        self.conn
            .execute(
                "INSERT INTO episodic (ts_ms, key, value) VALUES (?1, ?2, ?3)",
                rusqlite::params![entry.timestamp_ms as i64, &entry.key, &value_str],
            )
            .map_err(|e| EpisodicErr::Io(e.to_string()))?;
        Ok(())
    }

    fn enforce_max(&self, max: usize) -> Result<(), EpisodicErr> {
        if max == usize::MAX {
            return Ok(());
        }
        // Delete everything but the most recent `max` rows.
        self.conn
            .execute(
                "DELETE FROM episodic WHERE rowid NOT IN (\
                 SELECT rowid FROM episodic ORDER BY rowid DESC LIMIT ?1)",
                rusqlite::params![max as i64],
            )
            .map_err(|e| EpisodicErr::Io(e.to_string()))?;
        Ok(())
    }

    fn count(&self) -> Result<usize, EpisodicErr> {
        let mut stmt = self
            .conn
            .prepare("SELECT COUNT(*) FROM episodic")
            .map_err(|e| EpisodicErr::Io(e.to_string()))?;
        let n: i64 = stmt
            .query_row([], |r| r.get(0))
            .map_err(|e| EpisodicErr::Io(e.to_string()))?;
        Ok(n as usize)
    }

    fn all(&self) -> Result<Vec<Entry>, EpisodicErr> {
        let mut stmt = self
            .conn
            .prepare("SELECT ts_ms, key, value FROM episodic ORDER BY rowid ASC")
            .map_err(|e| EpisodicErr::Io(e.to_string()))?;
        let iter = stmt
            .query_map([], |row| {
                let ts: i64 = row.get(0)?;
                let key: String = row.get(1)?;
                let value: String = row.get(2)?;
                Ok((ts, key, value))
            })
            .map_err(|e| EpisodicErr::Io(e.to_string()))?;
        let mut out = Vec::new();
        for row in iter {
            let (ts, key, value) = row.map_err(|e| EpisodicErr::Io(e.to_string()))?;
            let parsed: Value =
                serde_json::from_str(&value).map_err(|e| EpisodicErr::Io(e.to_string()))?;
            out.push(Entry {
                timestamp_ms: ts as u64,
                key,
                value: parsed,
            });
        }
        Ok(out)
    }

    fn clear(&self) -> Result<(), EpisodicErr> {
        self.conn
            .execute("DELETE FROM episodic", [])
            .map_err(|e| EpisodicErr::Io(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_records_then_recalls() {
        let mut e = Episodic::in_memory(10);
        e.record("k1", &Value::String("v1".into()));
        e.record("k2", &Value::String("v2".into()));
        assert_eq!(e.len(), 2);
        let recent = e.recent(10);
        assert_eq!(recent[0].key, "k2");
        assert_eq!(recent[1].key, "k1");
    }

    #[test]
    fn ring_evicts_oldest_when_full() {
        let mut e = Episodic::in_memory(3);
        for i in 0..5 {
            e.record(&format!("k{i}"), &Value::Number(i.into()));
        }
        assert_eq!(e.len(), 3);
        let keys: Vec<String> = e.collect_entries().iter().map(|x| x.key.clone()).collect();
        assert_eq!(keys, vec!["k2", "k3", "k4"]);
    }

    #[test]
    fn search_by_key_prefix() {
        let mut e = Episodic::in_memory(10);
        e.record("alpha:1", &Value::Null);
        e.record("alpha:2", &Value::Null);
        e.record("beta:1", &Value::Null);
        let hits = e.search_by_key("alpha");
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn clear_empties() {
        let mut e = Episodic::in_memory(5);
        e.record("x", &Value::Null);
        e.clear();
        assert!(e.is_empty());
    }

    #[test]
    fn snapshot_round_trip() {
        let mut e = Episodic::in_memory(4);
        e.record("k", &Value::String("v".into()));
        let snap = e.snapshot_bytes();
        let mut e2 = Episodic::in_memory(4);
        e2.restore_bytes(&snap).unwrap();
        assert_eq!(e2.len(), 1);
    }
}
