//! Trace recorder — captures runtime events into an in-memory buffer
//! and serializes them on shutdown.
//!
//! ## Design
//!
//! The recorder is a thread-safe append-only buffer of [`TraceEvent`]s
//! plus a [`Mutex`]-protected output file path. It's deliberately
//! decoupled from the runtime's hot path: callers explicitly invoke
//! [`Recorder::record`] (or one of the typed `record_*` helpers) at
//! known capture points. The recorder has zero overhead when the
//! optional [`global_recorder`] slot is `None`.
//!
//! ## Opt-in
//!
//! The standard activation path is the `MTY_RECORD_TRACE=<path>` env
//! var. [`install_from_env`] reads it once and installs a process-wide
//! [`Recorder`]; absent the var, the recorder stays uninstalled and
//! every `record_*` call is a no-op.
//!
//! ## Privacy
//!
//! Payload + IO bytes are captured verbatim. The recorder never
//! filters, so callers should redact at the source if the trace will
//! leave the local machine.
//!
//! See `dev/history/notes/REPLAY_V0_17_NOTES.md` for the wire-shape
//! rationale and the v0.18 step-debugger follow-up.

use super::wire::{
    LlmToolUse, ReplayPayload, ReplayValue, TraceEvent, TraceFile, V1TraceFile, TRACE_MAGIC,
    TRACE_WIRE_VERSION,
};
use parking_lot::{Mutex, RwLock};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Process-wide opt-in switch — `Some` after `install_from_env` (or
/// `install`) is called with `MTY_RECORD_TRACE` set.
static GLOBAL: RwLock<Option<Arc<Recorder>>> = RwLock::new(None);

/// Env var that, when set to a non-empty path, opts into trace
/// recording for the lifetime of the process.
pub const RECORD_ENV: &str = "MTY_RECORD_TRACE";

/// Default codec for serialization on disk.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TraceCodec {
    /// JSON — human-readable, used when postcard isn't available or
    /// when the caller asks for a debug-friendly format.
    #[default]
    Json,
}

/// Per-agent message-handled counter — used to populate
/// `MessageHandled.msg_idx` deterministically.
#[derive(Debug, Default)]
struct AgentCounters {
    handled: AtomicU64,
}

/// The recorder itself. Cheap to clone via `Arc`; cheap to call when
/// not installed.
#[derive(Debug)]
pub struct Recorder {
    output_path: PathBuf,
    codec: TraceCodec,
    runtime_seed: u64,
    created_at_ms: u64,
    worker_count: u32,
    buffer: Mutex<Vec<TraceEvent>>,
    counters: dashmap::DashMap<u64, AgentCounters>,
    /// v0.29 wire-v3: monotonic LLM-turn id allocator for the
    /// `TraceEvent::LlmCall` variant.
    next_llm_turn_id: AtomicU64,
}

impl Recorder {
    /// Build a recorder that will serialize to `output_path` on
    /// `flush_to_disk()`. The `runtime_seed` is folded into the
    /// `TraceFile.runtime_seed` field; replay re-seeds from it.
    pub fn new(output_path: impl Into<PathBuf>, runtime_seed: u64, worker_count: u32) -> Self {
        Self {
            output_path: output_path.into(),
            codec: TraceCodec::default(),
            runtime_seed,
            created_at_ms: now_unix_ms(),
            worker_count,
            buffer: Mutex::new(Vec::new()),
            counters: dashmap::DashMap::new(),
            next_llm_turn_id: AtomicU64::new(0),
        }
    }

    /// Switch the on-disk codec. JSON is the default — change to
    /// experiment with future postcard wiring.
    pub fn with_codec(mut self, codec: TraceCodec) -> Self {
        self.codec = codec;
        self
    }

    /// Output path the recorder will serialize to. Returns the path
    /// the caller passed at construction.
    pub fn output_path(&self) -> &Path {
        &self.output_path
    }

    /// Number of events currently buffered.
    pub fn len(&self) -> usize {
        self.buffer.lock().len()
    }

    /// `true` if no events have been recorded yet.
    pub fn is_empty(&self) -> bool {
        self.buffer.lock().is_empty()
    }

    /// Read-only snapshot of the events buffer. Useful for tests that
    /// want to assert on captured events without flushing to disk.
    pub fn events_snapshot(&self) -> Vec<TraceEvent> {
        self.buffer.lock().clone()
    }

    /// Append a single event. Lock-protected — safe to call from any
    /// thread. Most callers should prefer the typed `record_*`
    /// helpers below.
    pub fn record(&self, event: TraceEvent) {
        self.buffer.lock().push(event);
    }

    pub fn record_spawn(&self, agent_id: u64, agent_type: &str, supervisor: Option<u64>) {
        self.counters.entry(agent_id).or_default();
        self.record(TraceEvent::Spawn {
            agent_id,
            agent_type: agent_type.to_string(),
            supervisor,
        });
    }

    /// Record a message-send with an *opaque* byte payload. The
    /// v0.18 runtime hot path uses this (with the `format!("{:?}",
    /// args)` byte rendering) because it doesn't pay the structural
    /// `Value` walk when no recorder is installed.
    pub fn record_message_sent(&self, from: u64, to: u64, msg: &str, payload: Vec<u8>) {
        self.record(TraceEvent::MessageSent {
            from,
            to,
            msg: msg.to_string(),
            payload: ReplayPayload::Opaque(payload),
        });
    }

    /// v0.19: record a message-send with a *structural* payload — the
    /// args have been encoded into [`ReplayValue`]s so the replayer
    /// can reconstruct them and feed a fresh `Runtime` with the same
    /// shape. Use this from the [`super::replay_driver`] re-execution
    /// path.
    pub fn record_message_sent_structural(
        &self,
        from: u64,
        to: u64,
        msg: &str,
        values: Vec<ReplayValue>,
    ) {
        self.record(TraceEvent::MessageSent {
            from,
            to,
            msg: msg.to_string(),
            payload: ReplayPayload::Values(values),
        });
    }

    /// v0.19: record a message-send with an already-constructed
    /// [`ReplayPayload`]. Useful for tests + callers that want to
    /// hand-craft the recorded shape.
    pub fn record_message_sent_payload(
        &self,
        from: u64,
        to: u64,
        msg: &str,
        payload: ReplayPayload,
    ) {
        self.record(TraceEvent::MessageSent {
            from,
            to,
            msg: msg.to_string(),
            payload,
        });
    }

    /// Record a handler dispatch. Returns the assigned per-agent
    /// `msg_idx` so the caller can correlate with logs.
    pub fn record_message_handled(&self, agent: u64, msg: &str, elapsed_us: u64) -> u64 {
        let entry = self.counters.entry(agent).or_default();
        let idx = entry.handled.fetch_add(1, Ordering::Relaxed);
        drop(entry);
        self.record(TraceEvent::MessageHandled {
            agent,
            msg_idx: idx,
            msg: msg.to_string(),
            elapsed_us,
        });
        idx
    }

    pub fn record_io_read(&self, agent: u64, source: &str, bytes: Vec<u8>) {
        self.record(TraceEvent::IoRead {
            agent,
            source: source.to_string(),
            bytes,
        });
    }

    pub fn record_clock_read(&self, agent: u64, value_ms: u64) {
        self.record(TraceEvent::ClockRead { agent, value_ms });
    }

    pub fn record_random_read(&self, agent: u64, bytes: Vec<u8>) {
        self.record(TraceEvent::RandomRead { agent, bytes });
    }

    pub fn record_budget_exhausted(&self, agent: u64, reason: &str) {
        self.record(TraceEvent::BudgetExhausted {
            agent,
            reason: reason.to_string(),
        });
    }

    pub fn record_exit(&self, agent: u64, reason: &str) {
        self.record(TraceEvent::Exit {
            agent,
            reason: reason.to_string(),
        });
    }

    /// v0.29 wire-v3: record one structural LLM turn. `turn_id`
    /// auto-allocates (monotonic per-recorder) when callers pass
    /// `None` so a single `mty trace record` session keeps the
    /// `LlmCall.turn_id` field unique without coordination.
    #[allow(clippy::too_many_arguments)]
    pub fn record_llm_call(
        &self,
        agent: u64,
        turn_id: Option<u64>,
        prompt: &str,
        system: Option<&str>,
        tools: Vec<String>,
        reply: &str,
        tool_uses: Vec<LlmToolUse>,
        cost_cents: u64,
    ) -> u64 {
        let turn_id =
            turn_id.unwrap_or_else(|| self.next_llm_turn_id.fetch_add(1, Ordering::Relaxed));
        self.record(TraceEvent::LlmCall {
            agent,
            turn_id,
            prompt: prompt.to_string(),
            system: system.map(|s| s.to_string()),
            tools,
            reply: reply.to_string(),
            tool_uses,
            cost_cents,
        });
        turn_id
    }

    /// Build a [`TraceFile`] from the buffered events without
    /// writing to disk. Used by tests + `flush_to_disk`.
    pub fn to_trace_file(&self) -> TraceFile {
        let events = self.buffer.lock().clone();
        TraceFile {
            version: TRACE_WIRE_VERSION,
            created_at_ms: self.created_at_ms,
            runtime_seed: self.runtime_seed,
            worker_count: self.worker_count,
            events,
        }
    }

    /// Serialize the trace to `output_path`. Always uses the JSON
    /// codec for v0.17 (postcard is gated behind a follow-up dep
    /// bump — see REPLAY_V0_17_NOTES.md).
    pub fn flush_to_disk(&self) -> Result<(), RecorderError> {
        let trace = self.to_trace_file();
        let bytes = encode(&trace, self.codec)?;
        if let Some(parent) = self.output_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(RecorderError::Io)?;
            }
        }
        std::fs::write(&self.output_path, bytes).map_err(RecorderError::Io)?;
        Ok(())
    }
}

/// Encode a [`TraceFile`] to bytes. The JSON codec prepends the
/// 8-byte MAGIC so the file is self-describing even when the caller
/// gives it a generic extension.
pub fn encode(trace: &TraceFile, codec: TraceCodec) -> Result<Vec<u8>, RecorderError> {
    match codec {
        TraceCodec::Json => {
            let mut out = Vec::with_capacity(256);
            out.extend_from_slice(TRACE_MAGIC);
            let json = serde_json::to_vec(trace).map_err(RecorderError::Serde)?;
            out.extend_from_slice(&json);
            Ok(out)
        }
    }
}

/// Decode bytes produced by [`encode`]. Strips + verifies the magic.
///
/// v0.19: detects the on-disk wire version up-front. **v1** traces
/// (where `MessageSent.payload` was a flat `Vec<u8>`) are decoded via
/// [`V1TraceFile`] and lifted into the current shape by wrapping each
/// payload in a [`ReplayPayload::Opaque`]. **v2** traces decode
/// directly. Traces with a `version` newer than this binary supports
/// are rejected with [`RecorderError::UnsupportedVersion`].
pub fn decode(bytes: &[u8]) -> Result<TraceFile, RecorderError> {
    if bytes.len() < TRACE_MAGIC.len() {
        return Err(RecorderError::BadMagic);
    }
    let (prefix, rest) = bytes.split_at(TRACE_MAGIC.len());
    if prefix != TRACE_MAGIC {
        return Err(RecorderError::BadMagic);
    }

    // Peek at the wire version without committing to either shape.
    // The header is tiny (`{"version":N,...`), so a partial-parse
    // probe is plenty fast and avoids backtracking on full decode.
    #[derive(serde::Deserialize)]
    struct VersionProbe {
        version: u32,
    }
    let probe: VersionProbe = serde_json::from_slice(rest).map_err(RecorderError::Serde)?;

    if probe.version > TRACE_WIRE_VERSION {
        return Err(RecorderError::UnsupportedVersion {
            found: probe.version,
            supported: TRACE_WIRE_VERSION,
        });
    }

    if probe.version == 1 {
        // v1 backwards-read: lift the flat-`Vec<u8>` payloads.
        let v1: V1TraceFile = serde_json::from_slice(rest).map_err(RecorderError::Serde)?;
        return Ok(v1.into_v2());
    }

    // v2 (or any future version <= TRACE_WIRE_VERSION).
    let trace: TraceFile = serde_json::from_slice(rest).map_err(RecorderError::Serde)?;
    Ok(trace)
}

/// Errors surfaced by the recorder.
#[derive(Debug, thiserror::Error)]
pub enum RecorderError {
    #[error("trace IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("trace serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("trace file magic header missing or invalid")]
    BadMagic,
    #[error(
        "trace wire version {found} is newer than the runtime supports ({supported}); upgrade `mty` to read it"
    )]
    UnsupportedVersion { found: u32, supported: u32 },
}

/// Install `recorder` as the process-wide active recorder. Replaces
/// any prior installation.
pub fn install(recorder: Arc<Recorder>) {
    *GLOBAL.write() = Some(recorder);
}

/// Remove the process-wide recorder (no-op if none was installed).
pub fn uninstall() -> Option<Arc<Recorder>> {
    GLOBAL.write().take()
}

/// Return a handle to the process-wide recorder, if one is installed.
/// Callers in the runtime hot path use this to avoid cloning the `Arc`
/// when no recording is in progress.
pub fn global_recorder() -> Option<Arc<Recorder>> {
    GLOBAL.read().clone()
}

/// Fire-and-forget hook for runtime instrumentation sites: invoke `f`
/// with the process-wide recorder if one is installed, otherwise no-op.
///
/// This is the v0.18 hot-path entry point. The read-lock is taken
/// briefly to grab the `Arc`, then released before `f` runs — so `f`
/// may freely call back into other recorder helpers without deadlock.
///
/// Zero-overhead when disabled: a single `RwLock::read` + `Option::is_none`
/// check; the `Arc` clone only happens when recording is active.
#[inline]
pub fn with_recorder<F: FnOnce(&Recorder)>(f: F) {
    if let Some(rec) = global_recorder() {
        f(&rec);
    }
}

/// `true` if a process-wide recorder is currently installed. Cheap;
/// useful for fast-pathing instrumentation sites that need to compute
/// expensive arguments (e.g. cloning a payload) before calling
/// `with_recorder`.
#[inline]
pub fn recording_enabled() -> bool {
    GLOBAL.read().is_some()
}

/// Convenience: read `MTY_RECORD_TRACE` and, if set to a non-empty
/// path, install a [`Recorder`] writing to that path. Returns the
/// installed handle so the caller (typically the CLI / runtime
/// bootstrap) can flush it during shutdown.
pub fn install_from_env(runtime_seed: u64, worker_count: u32) -> Option<Arc<Recorder>> {
    let path = std::env::var(RECORD_ENV).ok().filter(|s| !s.is_empty())?;
    let rec = Arc::new(Recorder::new(path, runtime_seed, worker_count));
    install(rec.clone());
    Some(rec)
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    // Tests that touch the process-wide GLOBAL recorder must take this
    // lock to avoid racing with each other. Tests that only build
    // local recorders don't need it.
    fn global_lock() -> &'static parking_lot::Mutex<()> {
        static M: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
        &M
    }

    // Tests use a unique env-var-name-per-test pattern via per-test
    // tempdirs to avoid the global recorder state leaking across
    // parallel tests.
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    fn tmp_path(label: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!("mty-replay-rec-{}-{}.bin", label, n));
        p
    }

    #[test]
    fn recorder_starts_empty() {
        let r = Recorder::new(tmp_path("starts_empty"), 0, 1);
        assert!(r.is_empty());
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn record_helpers_append_typed_events() {
        let r = Recorder::new(tmp_path("typed"), 7, 2);
        r.record_spawn(1, "Echo", None);
        r.record_message_sent(0, 1, "Ping", vec![1, 2, 3]);
        let idx = r.record_message_handled(1, "Ping", 250);
        r.record_clock_read(1, 500);
        r.record_random_read(1, vec![0xAA, 0xBB]);
        r.record_io_read(1, "file:foo", vec![]);
        r.record_budget_exhausted(1, "mem");
        r.record_exit(1, "normal");

        let evs = r.events_snapshot();
        assert_eq!(evs.len(), 8);
        assert_eq!(idx, 0);
        assert!(matches!(evs[0], TraceEvent::Spawn { agent_id: 1, .. }));
        assert!(matches!(
            evs[1],
            TraceEvent::MessageSent { from: 0, to: 1, .. }
        ));
    }

    #[test]
    fn message_handled_counter_per_agent() {
        let r = Recorder::new(tmp_path("counter"), 0, 1);
        assert_eq!(r.record_message_handled(1, "X", 1), 0);
        assert_eq!(r.record_message_handled(1, "X", 1), 1);
        assert_eq!(r.record_message_handled(2, "X", 1), 0);
        assert_eq!(r.record_message_handled(1, "X", 1), 2);
    }

    #[test]
    fn record_llm_call_appends_and_assigns_monotonic_turn_id() {
        let r = Recorder::new(tmp_path("llmcall"), 0, 1);
        let id0 = r.record_llm_call(
            5,
            None,
            "what's 2+2?",
            Some("you are a calculator"),
            vec!["calc".into()],
            "4",
            vec![],
            1,
        );
        let id1 = r.record_llm_call(
            5,
            None,
            "what's 3+3?",
            None,
            vec![],
            "6",
            vec![LlmToolUse {
                name: "calc".into(),
                id: "tu-1".into(),
                input_json: "{\"x\":3}".into(),
            }],
            2,
        );
        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
        // Caller-supplied turn_id wins over the auto allocator.
        let id_force = r.record_llm_call(5, Some(42), "x", None, vec![], "y", vec![], 0);
        assert_eq!(id_force, 42);

        let snap = r.events_snapshot();
        assert_eq!(snap.len(), 3);
        let calls = r.to_trace_file();
        let llm: Vec<_> = calls.iter_llm_calls().collect();
        assert_eq!(llm.len(), 3);
        assert_eq!(llm[0].prompt, "what's 2+2?");
        assert_eq!(llm[0].reply, "4");
        assert_eq!(llm[1].tool_uses.len(), 1);
        assert_eq!(llm[2].turn_id, 42);
    }

    #[test]
    fn flush_then_decode_round_trips() {
        let path = tmp_path("rt");
        let r = Recorder::new(&path, 99, 1);
        r.record_spawn(1, "Echo", None);
        r.record_message_sent(0, 1, "Ping", vec![]);
        r.record_message_handled(1, "Ping", 10);
        r.flush_to_disk().unwrap();

        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(TRACE_MAGIC));
        let back = decode(&bytes).unwrap();
        assert_eq!(back.version, TRACE_WIRE_VERSION);
        assert_eq!(back.runtime_seed, 99);
        assert_eq!(back.events.len(), 3);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn decode_rejects_bad_magic() {
        let err = decode(b"not-a-trace-file").unwrap_err();
        matches!(err, RecorderError::BadMagic);
    }

    #[test]
    fn decode_rejects_future_version() {
        let mut trace = TraceFile::new(0, 0, 1);
        trace.version = TRACE_WIRE_VERSION + 1;
        let bytes = encode(&trace, TraceCodec::Json).unwrap();
        let err = decode(&bytes).unwrap_err();
        matches!(err, RecorderError::UnsupportedVersion { .. });
    }

    #[test]
    fn with_recorder_is_noop_when_uninstalled() {
        let _g = global_lock().lock();
        // Ensure global is clear (other tests may have installed).
        let prev = uninstall();
        let mut called = false;
        with_recorder(|_| called = true);
        assert!(!called);
        assert!(!recording_enabled());
        // Restore (defensive — should be empty anyway).
        if let Some(r) = prev {
            install(r);
        }
    }

    #[test]
    fn with_recorder_runs_when_installed() {
        let _g = global_lock().lock();
        let prev = uninstall();
        let r = Arc::new(Recorder::new(tmp_path("with"), 0, 1));
        install(r.clone());
        assert!(recording_enabled());
        let mut seen = 0;
        with_recorder(|rec| {
            rec.record_spawn(99, "Hot", None);
            seen = rec.len();
        });
        assert_eq!(seen, 1);
        let _ = uninstall();
        if let Some(r) = prev {
            install(r);
        }
    }

    #[test]
    fn install_uninstall_cycle() {
        let _g = global_lock().lock();
        // Don't depend on env var here — we install directly.
        let prev = uninstall();
        assert!(prev.is_none() || prev.is_some()); // no precondition
        let r = Arc::new(Recorder::new(tmp_path("install"), 0, 1));
        install(r.clone());
        let got = global_recorder().expect("installed");
        assert!(Arc::ptr_eq(&got, &r));
        let removed = uninstall().expect("should remove");
        assert!(Arc::ptr_eq(&removed, &r));
        assert!(global_recorder().is_none());
    }
}
