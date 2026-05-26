//! Deterministic agent replay — record runtime IO + message
//! exchanges to a binary trace, then re-play the trace for debugging
//! or regression checks (v0.17, Tier 1.4 in
//! `docs/internals/agent-features-roadmap.md`).
//!
//! ## Capabilities shipped in v0.17
//!
//! - **Wire format** ([`wire`]) — versioned, append-only, codec-agnostic.
//! - **Recorder** ([`recorder`]) — opt-in via `MTY_RECORD_TRACE=<path>`;
//!   thread-safe; zero overhead when not installed.
//! - **Replayer** (this module) — loads a trace, validates it, and
//!   walks the event log. Two replay modes:
//!   - [`ReplayMode::DumpJson`] — emit each event as one JSON line to
//!     the given writer (the "always-works" inspection path).
//!   - [`ReplayMode::Step`] — feeds a [`StepHandler`] one event at a
//!     time so callers can mock the runtime, count messages, or hook
//!     into a future step-debugger UI.
//! - **CLI** — `mty replay <trace.bin>` (see
//!   `crates/mty-cli/src/cmd/replay.rs`).
//!
//! ## Replay determinism contract
//!
//! Successful replay does NOT require Mighty re-executes user code.
//! v0.17 ships the recording surface + the deterministic walk over
//! the trace; full re-execution (where the replayer drives the
//! `Runtime` and asserts byte-identical handler output) is the
//! v0.18 stretch. See `dev/history/notes/REPLAY_V0_17_NOTES.md`.

pub mod recorder;
pub mod wire;

pub use recorder::{
    decode, encode, global_recorder, install, install_from_env, uninstall, Recorder, RecorderError,
    TraceCodec, RECORD_ENV,
};
pub use wire::{TraceEvent, TraceFile, TraceSummary, TRACE_MAGIC, TRACE_WIRE_VERSION};

use std::io::Write;
use std::path::Path;

/// Errors surfaced by the replayer.
#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    #[error(transparent)]
    Recorder(#[from] RecorderError),
    #[error("IO error while replaying: {0}")]
    Io(#[from] std::io::Error),
    #[error("handler aborted replay at event #{index}: {message}")]
    HandlerAborted { index: usize, message: String },
}

/// Result type for the replayer.
pub type ReplayResult<T> = Result<T, ReplayError>;

/// How the replayer should consume the trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayMode {
    /// Dump each event as a JSON line to the writer. The default + the
    /// always-works path.
    DumpJson,
    /// Drive a [`StepHandler`] with one event at a time. The handler
    /// returns `Ok(())` to continue, or `Err(_)` to abort.
    Step,
}

/// Callback interface for `ReplayMode::Step`. The default implementation
/// is a no-op — useful for "tick-through-and-count" replay tests.
pub trait StepHandler {
    fn on_event(&mut self, index: usize, event: &TraceEvent) -> Result<(), String>;
}

/// A no-op step handler that counts events. Handy for tests + as the
/// default for `mty replay --step` until v0.18 wires up a real
/// state-machine replay.
#[derive(Debug, Default, Clone)]
pub struct CountingStepHandler {
    pub seen: Vec<&'static str>,
    pub spawn_count: usize,
    pub message_sent_count: usize,
    pub message_handled_count: usize,
    pub io_read_count: usize,
    pub clock_read_count: usize,
    pub random_read_count: usize,
    pub budget_exhausted_count: usize,
    pub exit_count: usize,
}

impl CountingStepHandler {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn total(&self) -> usize {
        self.seen.len()
    }
}

impl StepHandler for CountingStepHandler {
    fn on_event(&mut self, _index: usize, event: &TraceEvent) -> Result<(), String> {
        self.seen.push(event.kind());
        match event {
            TraceEvent::Spawn { .. } => self.spawn_count += 1,
            TraceEvent::MessageSent { .. } => self.message_sent_count += 1,
            TraceEvent::MessageHandled { .. } => self.message_handled_count += 1,
            TraceEvent::IoRead { .. } => self.io_read_count += 1,
            TraceEvent::ClockRead { .. } => self.clock_read_count += 1,
            TraceEvent::RandomRead { .. } => self.random_read_count += 1,
            TraceEvent::BudgetExhausted { .. } => self.budget_exhausted_count += 1,
            TraceEvent::Exit { .. } => self.exit_count += 1,
        }
        Ok(())
    }
}

/// The replayer: a thin wrapper around a loaded [`TraceFile`]. The
/// trace itself is the source of truth — the replayer is stateless
/// across calls beyond holding the loaded file.
#[derive(Debug)]
pub struct Replayer {
    trace: TraceFile,
}

impl Replayer {
    /// Construct from an already-decoded trace.
    pub fn new(trace: TraceFile) -> Self {
        Self { trace }
    }

    /// Convenience: load from a path on disk. Verifies the magic +
    /// wire version.
    pub fn from_path(path: impl AsRef<Path>) -> ReplayResult<Self> {
        let bytes = std::fs::read(path.as_ref())?;
        let trace = decode(&bytes)?;
        Ok(Self::new(trace))
    }

    /// Borrow the underlying trace.
    pub fn trace(&self) -> &TraceFile {
        &self.trace
    }

    /// Borrow the summary.
    pub fn summary(&self) -> TraceSummary {
        self.trace.summary()
    }

    /// Iterate every event in order, writing one JSON object per line
    /// to `out`. The "always-works" fallback when full re-execution
    /// isn't possible.
    pub fn dump_json<W: Write>(&self, mut out: W) -> ReplayResult<usize> {
        for (i, ev) in self.trace.events.iter().enumerate() {
            let mut obj = serde_json::Map::new();
            obj.insert("index".into(), serde_json::Value::from(i as u64));
            let value = serde_json::to_value(ev)
                .map_err(|e| ReplayError::Recorder(RecorderError::Serde(e)))?;
            obj.insert("event".into(), value);
            let line = serde_json::to_string(&obj)
                .map_err(|e| ReplayError::Recorder(RecorderError::Serde(e)))?;
            writeln!(out, "{}", line)?;
        }
        Ok(self.trace.events.len())
    }

    /// Drive a [`StepHandler`] with one event at a time. Aborts on
    /// the first error returned by the handler.
    pub fn step<H: StepHandler>(&self, handler: &mut H) -> ReplayResult<usize> {
        for (i, ev) in self.trace.events.iter().enumerate() {
            handler
                .on_event(i, ev)
                .map_err(|message| ReplayError::HandlerAborted { index: i, message })?;
        }
        Ok(self.trace.events.len())
    }

    /// Verify the trace would replay byte-identical against itself.
    ///
    /// v0.17 ships the recording surface; full runtime re-execution is
    /// v0.18 work. The check below is a *self-consistency* test: we
    /// confirm the per-agent `msg_idx` sequence is monotonic per
    /// agent, the recipient of every `MessageSent` later appears in a
    /// `MessageHandled` (when possible), and no event references an
    /// agent that wasn't spawned. This is the v0.17 byte-identical
    /// contract: a trace either passes self-consistency or the
    /// replayer rejects it.
    pub fn verify_self_consistent(&self) -> ReplayResult<()> {
        use std::collections::{HashMap, HashSet};
        let mut spawned: HashSet<u64> = HashSet::new();
        let mut last_idx_per_agent: HashMap<u64, u64> = HashMap::new();
        for (i, ev) in self.trace.events.iter().enumerate() {
            match ev {
                TraceEvent::Spawn { agent_id, .. } => {
                    spawned.insert(*agent_id);
                }
                TraceEvent::MessageSent { from, to, .. } => {
                    // The sender of a message may be the synthetic
                    // "extern" sender (id 0) which is not in the
                    // spawned set; we only require the recipient.
                    if !spawned.contains(to) {
                        return Err(ReplayError::HandlerAborted {
                            index: i,
                            message: format!(
                                "MessageSent (from={from}) targets unspawned agent #{to}"
                            ),
                        });
                    }
                }
                TraceEvent::MessageHandled { agent, msg_idx, .. } => {
                    if !spawned.contains(agent) {
                        return Err(ReplayError::HandlerAborted {
                            index: i,
                            message: format!("MessageHandled for unspawned agent #{agent}"),
                        });
                    }
                    let next_expected = last_idx_per_agent.get(agent).map(|v| v + 1).unwrap_or(0);
                    if *msg_idx != next_expected {
                        return Err(ReplayError::HandlerAborted {
                            index: i,
                            message: format!(
                                "agent #{agent} msg_idx out of order: expected {next_expected}, got {msg_idx}"
                            ),
                        });
                    }
                    last_idx_per_agent.insert(*agent, *msg_idx);
                }
                TraceEvent::IoRead { agent, .. }
                | TraceEvent::ClockRead { agent, .. }
                | TraceEvent::RandomRead { agent, .. }
                | TraceEvent::BudgetExhausted { agent, .. }
                | TraceEvent::Exit { agent, .. } => {
                    if !spawned.contains(agent) {
                        return Err(ReplayError::HandlerAborted {
                            index: i,
                            message: format!("{} for unspawned agent #{agent}", ev.kind()),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_trace() -> TraceFile {
        let mut t = TraceFile::new(123, 1_000, 1);
        t.events.push(TraceEvent::Spawn {
            agent_id: 1,
            agent_type: "Echo".into(),
            supervisor: None,
        });
        t.events.push(TraceEvent::MessageSent {
            from: 0,
            to: 1,
            msg: "Ping".into(),
            payload: vec![],
        });
        t.events.push(TraceEvent::MessageHandled {
            agent: 1,
            msg_idx: 0,
            msg: "Ping".into(),
            elapsed_us: 5,
        });
        t.events.push(TraceEvent::ClockRead {
            agent: 1,
            value_ms: 1_005,
        });
        t.events.push(TraceEvent::Exit {
            agent: 1,
            reason: "normal".into(),
        });
        t
    }

    #[test]
    fn summary_passes_through() {
        let r = Replayer::new(sample_trace());
        let s = r.summary();
        assert_eq!(s.event_count, 5);
        assert_eq!(s.spawn_count, 1);
        assert_eq!(s.message_handled_count, 1);
    }

    #[test]
    fn dump_json_writes_one_line_per_event() {
        let r = Replayer::new(sample_trace());
        let mut buf = Vec::new();
        let n = r.dump_json(&mut buf).unwrap();
        assert_eq!(n, 5);
        let s = String::from_utf8(buf).unwrap();
        // Five distinct lines.
        assert_eq!(s.lines().count(), 5);
        // Each line contains the event index field.
        assert!(s.lines().all(|l| l.contains("\"index\":")));
        // First event is spawn.
        let first: serde_json::Value = serde_json::from_str(s.lines().next().unwrap()).unwrap();
        assert_eq!(first["index"], 0);
        assert!(first["event"]["Spawn"]["agent_id"] == 1);
    }

    #[test]
    fn step_handler_counts_events_correctly() {
        let r = Replayer::new(sample_trace());
        let mut h = CountingStepHandler::new();
        let n = r.step(&mut h).unwrap();
        assert_eq!(n, 5);
        assert_eq!(h.total(), 5);
        assert_eq!(h.spawn_count, 1);
        assert_eq!(h.message_sent_count, 1);
        assert_eq!(h.message_handled_count, 1);
        assert_eq!(h.clock_read_count, 1);
        assert_eq!(h.exit_count, 1);
    }

    #[test]
    fn step_propagates_handler_error() {
        struct Abort;
        impl StepHandler for Abort {
            fn on_event(&mut self, _i: usize, _e: &TraceEvent) -> Result<(), String> {
                Err("nope".into())
            }
        }
        let r = Replayer::new(sample_trace());
        let err = r.step(&mut Abort).unwrap_err();
        match err {
            ReplayError::HandlerAborted { index, message } => {
                assert_eq!(index, 0);
                assert_eq!(message, "nope");
            }
            other => panic!("expected HandlerAborted, got {other:?}"),
        }
    }

    #[test]
    fn self_consistent_passes_for_clean_trace() {
        let r = Replayer::new(sample_trace());
        r.verify_self_consistent().unwrap();
    }

    #[test]
    fn self_consistent_rejects_out_of_order_msg_idx() {
        let mut t = sample_trace();
        // Re-handle msg_idx=2 without 1.
        t.events.insert(
            3,
            TraceEvent::MessageHandled {
                agent: 1,
                msg_idx: 2,
                msg: "Pong".into(),
                elapsed_us: 1,
            },
        );
        let r = Replayer::new(t);
        let err = r.verify_self_consistent().unwrap_err();
        match err {
            ReplayError::HandlerAborted { message, .. } => {
                assert!(message.contains("msg_idx"));
            }
            other => panic!("expected HandlerAborted, got {other:?}"),
        }
    }

    #[test]
    fn self_consistent_rejects_unspawned_recipient() {
        let mut t = TraceFile::new(0, 0, 1);
        t.events.push(TraceEvent::MessageSent {
            from: 0,
            to: 99,
            msg: "Ping".into(),
            payload: vec![],
        });
        let r = Replayer::new(t);
        assert!(r.verify_self_consistent().is_err());
    }
}
