//! SQLite-backed storage for [`LlmObservation`]s.
//!
//! The shared store lives behind a `OnceLock<Mutex<...>>` so every
//! `record_if_enabled` call hits the same connection. Tests can
//! redirect the path via `MTY_OBSERVE_DB` and the v0.30 must-ship
//! flow is just: set `MTY_OBSERVE=1`, run your program, then read
//! it back with `mty inspect --cost`.

use crate::observe::observation::{now_ms, LlmObservation};

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Storage errors. Kept narrow — the SQLite path either opens or
/// it doesn't; the v0.30 sink either records or silently drops
/// (we never `panic!`).
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("observe feature `{0}` is disabled in this build")]
    FeatureDisabled(&'static str),
    #[error("observe IO: {0}")]
    Io(String),
}

/// Abstract sink so the OTel stub + the SQLite store can share the
/// `record_if_enabled` entry point. Each backend handles its own
/// failure (logs to stderr); the trait returns nothing.
pub trait ObservationStore: Send + Sync {
    fn record(&self, obs: &LlmObservation);
    /// Hook for tests + the inspect CLI: pull every observation back
    /// out. The OTel backend returns `None` (it's write-only).
    fn snapshot(&self) -> Option<Vec<LlmObservation>>;
    /// Wipe everything — only used by tests today.
    fn clear(&self);
}

/// Returns `true` if `MTY_OBSERVE` is set to a non-empty,
/// non-"0"/"false" value.
pub fn is_recording_enabled() -> bool {
    match std::env::var("MTY_OBSERVE") {
        Ok(v) => !matches!(v.as_str(), "" | "0" | "false" | "FALSE" | "no" | "off"),
        Err(_) => false,
    }
}

/// Resolve the storage path. `MTY_OBSERVE_DB` wins; default is
/// `~/.mty/observations.sqlite`.
pub fn default_db_path() -> PathBuf {
    if let Ok(p) = std::env::var("MTY_OBSERVE_DB") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let home = home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mty").join("observations.sqlite")
}

#[cfg(unix)]
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(windows)]
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOMEPATH").map(PathBuf::from))
}

#[cfg(not(any(unix, windows)))]
fn home_dir() -> Option<PathBuf> {
    None
}

// ---------------------------------------------------------------------------
// Process-global store.
// ---------------------------------------------------------------------------

static STORE: OnceLock<Mutex<Option<Box<dyn ObservationStore>>>> = OnceLock::new();

fn lock_store() -> &'static Mutex<Option<Box<dyn ObservationStore>>> {
    STORE.get_or_init(|| Mutex::new(None))
}

/// Install a specific store. Tests use this with an in-memory
/// SQLite path; production never calls it directly — instead
/// [`record_if_enabled`] lazily initialises a SQLite store the
/// first time `MTY_OBSERVE=1` triggers a record.
pub fn install_store(store: Box<dyn ObservationStore>) {
    let mut g = lock_store().lock().expect("observe store mutex poisoned");
    *g = Some(store);
}

/// Pull the active store out for inspection / `clear()`. Used by
/// tests and by `mty inspect --cost`'s in-process happy-path.
pub fn with_storage<R>(f: impl FnOnce(&dyn ObservationStore) -> R) -> Option<R> {
    let g = lock_store().lock().expect("observe store mutex poisoned");
    g.as_ref().map(|s| f(s.as_ref()))
}

/// Drop the installed store (and any in-process connection).
/// Idempotent — calling on an empty slot is a no-op.
pub fn uninstall_store() {
    let mut g = lock_store().lock().expect("observe store mutex poisoned");
    *g = None;
}

/// If recording is enabled and a store is installed (or initialisable),
/// record `obs`. Failure is logged to stderr; never panics.
///
/// **This is the v0.30 hot-path entry point** — every provider's
/// `complete()` calls it with a freshly built [`LlmObservation`].
pub fn record_if_enabled(obs: &LlmObservation) {
    if !is_recording_enabled() {
        return;
    }
    ensure_store_initialised();
    let _ = with_storage(|s| s.record(obs));
}

/// Synchronous record — bypasses the env flag, used by tests + by
/// users who installed an explicit store. Returns `Err(...)` if no
/// store is installed (callers can `unwrap_or_default` to keep the
/// hot path quiet).
pub fn record_now(obs: &LlmObservation) -> Result<(), StorageError> {
    match with_storage(|s| s.record(obs)) {
        Some(()) => Ok(()),
        None => Err(StorageError::Io("no observation store installed".into())),
    }
}

fn ensure_store_initialised() {
    if with_storage(|_| ()).is_some() {
        return;
    }
    // First-time init: try OTel exporter env var first, then SQLite.
    if let Ok(endpoint) = std::env::var("MTY_OBSERVE_OTEL") {
        if !endpoint.is_empty() {
            let store = crate::observe::otel::OtelStore::new(endpoint);
            install_store(Box::new(store));
            return;
        }
    }
    let path = default_db_path();
    match SqliteStore::open(&path) {
        Ok(s) => install_store(Box::new(s)),
        Err(e) => {
            eprintln!(
                "mty observe: failed to open SQLite store at {}: {e}",
                path.display()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Manual span guard — `std.observe.span("name")`.
// ---------------------------------------------------------------------------

/// Returned by [`span`] — drops record the elapsed wall-clock time
/// as a 0-token, 0-cost [`LlmObservation`] tagged with provider
/// `"_span"` so it appears in the same DB but doesn't pollute the
/// LLM-cost rollups.
#[must_use = "the span guard records elapsed time when dropped — bind it to a variable"]
pub struct SpanGuard {
    name: String,
    started: Instant,
    started_at_ms: u64,
}

impl SpanGuard {
    /// Manually finish — useful when you want the duration to
    /// stop *before* the surrounding scope ends.
    pub fn finish(self) {
        // Just drop self; the Drop impl records.
        drop(self);
    }
}

impl Drop for SpanGuard {
    fn drop(&mut self) {
        let elapsed_ms = self.started.elapsed().as_millis() as u64;
        let obs = LlmObservation {
            provider: "_span".into(),
            model: self.name.clone(),
            prompt_tokens: 0,
            completion_tokens: 0,
            cost_cents: 0,
            latency_ms: elapsed_ms,
            started_at_ms: self.started_at_ms,
            agent_id: None,
            tool_calls: Vec::new(),
            error_kind: None,
        };
        record_if_enabled(&obs);
    }
}

/// Start a manual span. The returned [`SpanGuard`] records elapsed
/// time when dropped. No-op when `MTY_OBSERVE` is off.
pub fn span(name: impl Into<String>) -> SpanGuard {
    SpanGuard {
        name: name.into(),
        started: Instant::now(),
        started_at_ms: now_ms(),
    }
}

// ---------------------------------------------------------------------------
// SQLite backend — gated behind `observe-sqlite` (default-on).
// ---------------------------------------------------------------------------

#[cfg(feature = "observe-sqlite")]
pub struct SqliteStore {
    inner: Mutex<rusqlite::Connection>,
    #[allow(dead_code)]
    path: PathBuf,
}

#[cfg(feature = "observe-sqlite")]
impl SqliteStore {
    pub fn open(path: &std::path::Path) -> Result<Self, StorageError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| StorageError::Io(e.to_string()))?;
            }
        }
        let conn = rusqlite::Connection::open(path).map_err(|e| StorageError::Io(e.to_string()))?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS observations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                prompt_tokens INTEGER NOT NULL,
                completion_tokens INTEGER NOT NULL,
                cost_cents INTEGER NOT NULL,
                latency_ms INTEGER NOT NULL,
                started_at TEXT NOT NULL,
                started_at_ms INTEGER NOT NULL,
                agent_id INTEGER,
                error_kind TEXT,
                tool_calls TEXT NOT NULL DEFAULT '[]',
                recorded_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            CREATE INDEX IF NOT EXISTS idx_obs_started ON observations(started_at_ms);
            CREATE INDEX IF NOT EXISTS idx_obs_provider ON observations(provider);
            CREATE INDEX IF NOT EXISTS idx_obs_model ON observations(model);
            "#,
        )
        .map_err(|e| StorageError::Io(e.to_string()))?;
        Ok(Self {
            inner: Mutex::new(conn),
            path: path.to_path_buf(),
        })
    }

    /// Convenience: open an in-memory store. Used by tests + by
    /// the `MTY_OBSERVE_DB=:memory:` shortcut.
    pub fn in_memory() -> Result<Self, StorageError> {
        let conn =
            rusqlite::Connection::open_in_memory().map_err(|e| StorageError::Io(e.to_string()))?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS observations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                prompt_tokens INTEGER NOT NULL,
                completion_tokens INTEGER NOT NULL,
                cost_cents INTEGER NOT NULL,
                latency_ms INTEGER NOT NULL,
                started_at TEXT NOT NULL,
                started_at_ms INTEGER NOT NULL,
                agent_id INTEGER,
                error_kind TEXT,
                tool_calls TEXT NOT NULL DEFAULT '[]',
                recorded_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            "#,
        )
        .map_err(|e| StorageError::Io(e.to_string()))?;
        Ok(Self {
            inner: Mutex::new(conn),
            path: PathBuf::from(":memory:"),
        })
    }
}

#[cfg(feature = "observe-sqlite")]
impl ObservationStore for SqliteStore {
    fn record(&self, obs: &LlmObservation) {
        let Ok(conn) = self.inner.lock() else {
            eprintln!("mty observe: SQLite mutex poisoned");
            return;
        };
        let tool_calls_json =
            serde_json::to_string(&obs.tool_calls).unwrap_or_else(|_| "[]".to_string());
        let started_iso = format_unix_ms_iso(obs.started_at_ms);
        let res = conn.execute(
            "INSERT INTO observations (provider, model, prompt_tokens, completion_tokens, \
                 cost_cents, latency_ms, started_at, started_at_ms, agent_id, error_kind, tool_calls) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                &obs.provider,
                &obs.model,
                obs.prompt_tokens as i64,
                obs.completion_tokens as i64,
                obs.cost_cents,
                obs.latency_ms as i64,
                &started_iso,
                obs.started_at_ms as i64,
                obs.agent_id.map(|a| a as i64),
                obs.error_kind.as_deref(),
                &tool_calls_json,
            ],
        );
        if let Err(e) = res {
            eprintln!("mty observe: insert failed: {e}");
        }
    }

    fn snapshot(&self) -> Option<Vec<LlmObservation>> {
        let conn = self.inner.lock().ok()?;
        let mut stmt = conn
            .prepare(
                "SELECT provider, model, prompt_tokens, completion_tokens, cost_cents, \
                     latency_ms, started_at_ms, agent_id, error_kind, tool_calls \
                     FROM observations ORDER BY id ASC",
            )
            .ok()?;
        let iter = stmt
            .query_map([], |row| {
                let tool_calls_str: String = row.get(9)?;
                let tool_calls = serde_json::from_str(&tool_calls_str).unwrap_or_default();
                let agent_id: Option<i64> = row.get(7)?;
                Ok(LlmObservation {
                    provider: row.get(0)?,
                    model: row.get(1)?,
                    prompt_tokens: row.get::<_, i64>(2)? as u64,
                    completion_tokens: row.get::<_, i64>(3)? as u64,
                    cost_cents: row.get(4)?,
                    latency_ms: row.get::<_, i64>(5)? as u64,
                    started_at_ms: row.get::<_, i64>(6)? as u64,
                    agent_id: agent_id.map(|a| a as u64),
                    error_kind: row.get(8)?,
                    tool_calls,
                })
            })
            .ok()?;
        let mut out = Vec::new();
        for r in iter.flatten() {
            out.push(r);
        }
        Some(out)
    }

    fn clear(&self) {
        if let Ok(conn) = self.inner.lock() {
            let _ = conn.execute("DELETE FROM observations", []);
        }
    }
}

#[cfg(not(feature = "observe-sqlite"))]
pub struct SqliteStore;

#[cfg(not(feature = "observe-sqlite"))]
impl SqliteStore {
    pub fn open(_path: &std::path::Path) -> Result<Self, StorageError> {
        Err(StorageError::FeatureDisabled("observe-sqlite"))
    }
    pub fn in_memory() -> Result<Self, StorageError> {
        Err(StorageError::FeatureDisabled("observe-sqlite"))
    }
}

#[cfg(not(feature = "observe-sqlite"))]
impl ObservationStore for SqliteStore {
    fn record(&self, _obs: &LlmObservation) {}
    fn snapshot(&self) -> Option<Vec<LlmObservation>> {
        None
    }
    fn clear(&self) {}
}

/// Format unix ms as ISO-8601 UTC (second precision). Standalone so
/// the SQLite-feature-off build also compiles.
pub(crate) fn format_unix_ms_iso(ms: u64) -> String {
    let secs = ms / 1000;
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let h = rem / 3600;
    let mi = (rem % 3600) / 60;
    let s = rem % 60;
    let mut z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr = if mo <= 2 { y + 1 } else { y };
    z = yr;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        z as u64, mo as u64, d as u64, h, mi, s
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observe::observation::LlmObservation;

    fn fresh_store() -> SqliteStore {
        SqliteStore::in_memory().expect("in-memory sqlite open")
    }

    #[test]
    #[cfg(feature = "observe-sqlite")]
    fn round_trip_record_then_snapshot() {
        let s = fresh_store();
        let obs =
            LlmObservation::new("anthropic", "claude-opus-4-7", 100, 50, 250).with_agent_id(7);
        s.record(&obs);
        let back = s.snapshot().unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].provider, "anthropic");
        assert_eq!(back[0].agent_id, Some(7));
        assert_eq!(back[0].prompt_tokens, 100);
        assert_eq!(back[0].completion_tokens, 50);
    }

    #[test]
    #[cfg(feature = "observe-sqlite")]
    fn snapshot_preserves_tool_calls_json() {
        let s = fresh_store();
        let obs = LlmObservation::new("openai", "gpt-5", 0, 0, 1).with_tool_calls(vec![
            crate::observe::observation::ToolCallObservation {
                name: "search".into(),
                latency_ms: 12,
                failed: false,
            },
        ]);
        s.record(&obs);
        let back = s.snapshot().unwrap();
        assert_eq!(back[0].tool_calls.len(), 1);
        assert_eq!(back[0].tool_calls[0].name, "search");
    }

    #[test]
    #[cfg(feature = "observe-sqlite")]
    fn clear_wipes_table() {
        let s = fresh_store();
        s.record(&LlmObservation::new("gemini", "gemini-2.5-pro", 1, 1, 1));
        assert_eq!(s.snapshot().unwrap().len(), 1);
        s.clear();
        assert!(s.snapshot().unwrap().is_empty());
    }

    #[test]
    fn observe_disabled_is_noop() {
        std::env::remove_var("MTY_OBSERVE");
        // No store installed, recording disabled — must not panic.
        record_if_enabled(&LlmObservation::new("x", "y", 0, 0, 0));
        // Still no store installed.
        assert!(with_storage(|_| ()).is_none());
    }

    #[test]
    #[cfg(feature = "observe-sqlite")]
    fn install_store_installs_then_records() {
        // Use install_store + record_now (which bypass the env flag),
        // then snapshot.
        let store = SqliteStore::in_memory().unwrap();
        install_store(Box::new(store));
        record_now(&LlmObservation::new(
            "anthropic",
            "claude-opus-4-7",
            10,
            5,
            1,
        ))
        .unwrap();
        let n = with_storage(|s| s.snapshot().unwrap().len()).unwrap();
        assert_eq!(n, 1);
        uninstall_store();
    }

    #[test]
    fn iso_formatter_round_trips_epoch() {
        assert_eq!(format_unix_ms_iso(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn is_recording_enabled_respects_falsey_values() {
        std::env::set_var("MTY_OBSERVE", "0");
        assert!(!is_recording_enabled());
        std::env::set_var("MTY_OBSERVE", "false");
        assert!(!is_recording_enabled());
        std::env::set_var("MTY_OBSERVE", "1");
        assert!(is_recording_enabled());
        std::env::set_var("MTY_OBSERVE", "on");
        assert!(is_recording_enabled());
        std::env::remove_var("MTY_OBSERVE");
    }
}
