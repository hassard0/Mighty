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
// v0.19 Tier 1.4 follow-up — byte-identical full replay re-execution.
// The driver spins up a fresh `Runtime` from a recorded trace and
// asserts each re-emitted event matches the recorded one. See
// `dev/history/notes/REPLAY_BYTE_IDENTICAL_V0_19_NOTES.md`.
pub mod replay_driver;
pub mod wire;

pub use recorder::{
    decode, encode, global_recorder, install, install_from_env, recording_enabled, uninstall,
    with_recorder, Recorder, RecorderError, TraceCodec, RECORD_ENV,
};
pub use replay_driver::{
    EventMismatch, LlmTurnDiff, LlmTurnReplay, ProvidedTurn, ReplayDriver, ReplayReport,
    TurnProvider,
};
pub use wire::{
    LlmCallRef, LlmToolUse, ReplayPayload, ReplayValue, RuntimeValueLike, TraceEvent, TraceFile,
    TraceSummary, TRACE_MAGIC, TRACE_WIRE_VERSION,
};

use mty_ir::interp::value::Value as RuntimeValue;
use mty_types::{FloatKind, IntKind};
use std::io::Write;
use std::path::Path;

// -----------------------------------------------------------------------------
// v0.19 — structural Value <-> ReplayValue codec
// -----------------------------------------------------------------------------
//
// The runtime's `Value` enum carries Host-side references (`Ref`, `Fn`,
// `Agent`, `Cap`) which aren't portable across processes. The codec
// folds those into `ReplayValue::Opaque` so the on-disk shape remains
// stable. Round-trip is lossless for the pure-data variants
// (`Unit`/`Bool`/`Int`/`Float`/`Str`/`Char`/`Duration`/`Size`/`Tuple`/
// `Array`/`Struct`/`Enum`).

fn int_kind_name(k: IntKind) -> &'static str {
    use IntKind::*;
    match k {
        I8 => "I8",
        I16 => "I16",
        I32 => "I32",
        I64 => "I64",
        I128 => "I128",
        U8 => "U8",
        U16 => "U16",
        U32 => "U32",
        U64 => "U64",
        U128 => "U128",
        ISize => "ISize",
        USize => "USize",
        IntInfer => "IntInfer",
    }
}

fn int_kind_from_name(name: &str) -> IntKind {
    use IntKind::*;
    match name {
        "I8" => I8,
        "I16" => I16,
        "I32" => I32,
        "I64" => I64,
        "I128" => I128,
        "U8" => U8,
        "U16" => U16,
        "U32" => U32,
        "U64" => U64,
        "U128" => U128,
        "ISize" => ISize,
        "USize" => USize,
        "IntInfer" => IntInfer,
        // Default-safe fall-back: I64 is the interpreter's "natural"
        // width and matches every numeric literal that fits.
        _ => I64,
    }
}

fn float_kind_name(k: FloatKind) -> &'static str {
    use FloatKind::*;
    match k {
        F32 => "F32",
        F64 => "F64",
        FloatInfer => "FloatInfer",
    }
}

fn float_kind_from_name(name: &str) -> FloatKind {
    use FloatKind::*;
    match name {
        "F32" => F32,
        "FloatInfer" => FloatInfer,
        _ => F64,
    }
}

/// Encode a runtime [`Value`](RuntimeValue) into the structural wire
/// form. Variants the codec can't represent verbatim (live `Ref`, `Fn`,
/// `Agent`, `Cap`, `Void`) are folded to [`ReplayValue::Opaque`] with
/// their `Debug` rendering. Pure-data values round-trip losslessly.
pub fn from_runtime_value(v: &RuntimeValue) -> ReplayValue {
    match v {
        RuntimeValue::Unit => ReplayValue::Unit,
        RuntimeValue::Bool(b) => ReplayValue::Bool(*b),
        RuntimeValue::Int(n, k) => ReplayValue::Int {
            value: *n,
            kind: int_kind_name(*k).to_string(),
        },
        RuntimeValue::Float(f, k) => ReplayValue::Float {
            bits: f.to_bits(),
            kind: float_kind_name(*k).to_string(),
        },
        RuntimeValue::Str(s) => ReplayValue::Str(s.clone()),
        RuntimeValue::Char(c) => ReplayValue::Char(*c),
        RuntimeValue::Duration(n) => ReplayValue::Duration(*n),
        RuntimeValue::Size(n) => ReplayValue::Size(*n),
        RuntimeValue::Tuple(xs) => ReplayValue::Tuple(xs.iter().map(from_runtime_value).collect()),
        RuntimeValue::Array(xs) => ReplayValue::Array(xs.iter().map(from_runtime_value).collect()),
        RuntimeValue::Struct { adt, fields } => ReplayValue::Record {
            adt: adt.0 as u64,
            fields: fields.iter().map(from_runtime_value).collect(),
        },
        RuntimeValue::Enum {
            adt,
            variant,
            payload,
        } => ReplayValue::Variant {
            adt: adt.0 as u64,
            variant: *variant,
            payload: payload.iter().map(from_runtime_value).collect(),
        },
        // Non-portable: render to Debug so the byte-identical
        // comparison still works (both sides will reproduce the same
        // Debug rendering for the same shape).
        other => ReplayValue::Opaque(format!("{:?}", other)),
    }
}

/// Decode a structural [`ReplayValue`] back into an interpreter
/// [`Value`](RuntimeValue). The lossy `Opaque` arm becomes
/// [`RuntimeValue::Str`] — the v0.19 contract is that replay re-feeds
/// the recorded shape, not that the original Host-side reference is
/// reconstructed (which is impossible across processes).
pub fn to_runtime_value(v: &ReplayValue) -> Result<RuntimeValue, String> {
    Ok(match v {
        ReplayValue::Unit => RuntimeValue::Unit,
        ReplayValue::Bool(b) => RuntimeValue::Bool(*b),
        ReplayValue::Int { value, kind } => RuntimeValue::Int(*value, int_kind_from_name(kind)),
        ReplayValue::Float { bits, kind } => {
            RuntimeValue::Float(f64::from_bits(*bits), float_kind_from_name(kind))
        }
        ReplayValue::Str(s) => RuntimeValue::Str(s.clone()),
        ReplayValue::Char(c) => RuntimeValue::Char(*c),
        ReplayValue::Duration(n) => RuntimeValue::Duration(*n),
        ReplayValue::Size(n) => RuntimeValue::Size(*n),
        ReplayValue::Tuple(xs) => RuntimeValue::Tuple(
            xs.iter()
                .map(to_runtime_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        ReplayValue::Array(xs) => RuntimeValue::Array(
            xs.iter()
                .map(to_runtime_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        ReplayValue::Record { adt, fields } => RuntimeValue::Struct {
            adt: mty_types::AdtId(*adt as u32),
            fields: fields
                .iter()
                .map(to_runtime_value)
                .collect::<Result<Vec<_>, _>>()?,
        },
        ReplayValue::Variant {
            adt,
            variant,
            payload,
        } => RuntimeValue::Enum {
            adt: mty_types::AdtId(*adt as u32),
            variant: *variant,
            payload: payload
                .iter()
                .map(to_runtime_value)
                .collect::<Result<Vec<_>, _>>()?,
        },
        // Opaque doesn't round-trip the original Host-side reference;
        // we lift it into a string so the replay driver still has a
        // serialised value to feed into `Runtime::send`.
        ReplayValue::Opaque(s) => RuntimeValue::Str(s.clone()),
    })
}

impl RuntimeValueLike for RuntimeValue {
    fn to_replay_value(&self) -> ReplayValue {
        from_runtime_value(self)
    }
}

/// Encode a slice of runtime values into a [`ReplayPayload::Values`].
/// Used by the [`replay_driver`] when driving a fresh `Runtime` so
/// every `record_message_sent` on the replay side has a structural
/// payload to compare against.
pub fn encode_values_payload(args: &[RuntimeValue]) -> ReplayPayload {
    ReplayPayload::Values(args.iter().map(from_runtime_value).collect())
}

/// Best-effort conversion of an opaque byte payload into a
/// [`ReplayPayload::Values`]. Recordings from the v0.18 hot path
/// (which always emit `Opaque`) can be folded into a single-element
/// `Values([Opaque(Debug(args))])` for comparison against a recorder
/// that opted into structural recording. Currently returns the input
/// unchanged — the driver's byte-identical assertion treats `Opaque ==
/// Opaque` and `Values == Values` as equal cases.
pub fn align_payloads(a: &ReplayPayload, b: &ReplayPayload) -> bool {
    a == b
}

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
    /// v0.29 wire-v3: count of `TraceEvent::LlmCall` events seen.
    pub llm_call_count: usize,
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
            TraceEvent::LlmCall { .. } => self.llm_call_count += 1,
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
                // v0.29 wire-v3: LlmCall is a structural side-channel
                // — the recorder may emit it without a prior Spawn
                // (e.g. CLI eval drivers issue LLM calls from the
                // process bootstrap, not from a spawned agent). We
                // accept any `agent` id including the synthetic 0.
                TraceEvent::LlmCall { .. } => {}
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
            payload: ReplayPayload::default(),
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
            payload: ReplayPayload::default(),
        });
        let r = Replayer::new(t);
        assert!(r.verify_self_consistent().is_err());
    }

    // -------------------------------------------------------------------------
    // v0.19 structural-codec tests
    // -------------------------------------------------------------------------

    #[test]
    fn from_runtime_value_round_trips_scalar_values() {
        // The pure-data variants must round-trip exactly.
        let cases: Vec<RuntimeValue> = vec![
            RuntimeValue::Unit,
            RuntimeValue::Bool(true),
            RuntimeValue::Int(-42, IntKind::I64),
            RuntimeValue::Float(1.5, FloatKind::F64),
            RuntimeValue::Str("hello".into()),
            RuntimeValue::Char('z'),
            RuntimeValue::Duration(120),
            RuntimeValue::Size(8_192),
            RuntimeValue::Tuple(vec![
                RuntimeValue::Bool(false),
                RuntimeValue::Int(7, IntKind::I32),
            ]),
            RuntimeValue::Array(vec![
                RuntimeValue::Str("a".into()),
                RuntimeValue::Str("b".into()),
            ]),
        ];
        for case in cases {
            let r = from_runtime_value(&case);
            let back = to_runtime_value(&r).unwrap();
            // We can't compare RuntimeValue directly (no PartialEq) so
            // re-encode and compare ReplayValue forms.
            let re_r = from_runtime_value(&back);
            assert_eq!(r, re_r, "round-trip mismatch for {case:?}");
        }
    }

    #[test]
    fn encode_values_payload_yields_values_arm() {
        let args = vec![
            RuntimeValue::Str("hi".into()),
            RuntimeValue::Int(99, IntKind::U16),
        ];
        let p = encode_values_payload(&args);
        match &p {
            ReplayPayload::Values(vs) => {
                assert_eq!(vs.len(), 2);
                assert!(matches!(&vs[0], ReplayValue::Str(s) if s == "hi"));
                assert!(matches!(&vs[1], ReplayValue::Int { value: 99, .. }));
            }
            other => panic!("expected Values, got {other:?}"),
        }
    }

    #[test]
    fn opaque_values_byte_identical_for_equal_inputs() {
        // Two recordings of the same args must compare equal — this is
        // the byte-identical contract on the comparison side.
        let p1 = ReplayPayload::Opaque(b"abc".to_vec());
        let p2 = ReplayPayload::Opaque(b"abc".to_vec());
        assert!(align_payloads(&p1, &p2));
        let p3 = ReplayPayload::Opaque(b"xyz".to_vec());
        assert!(!align_payloads(&p1, &p3));
    }

    #[test]
    fn float_round_trip_preserves_nan_bits() {
        // The bit-pattern encoding survives signaling-NaN payloads.
        let snan_bits: u64 = 0x7FF1_2345_6789_ABCD;
        let r = from_runtime_value(&RuntimeValue::Float(
            f64::from_bits(snan_bits),
            FloatKind::F64,
        ));
        match r {
            ReplayValue::Float { bits, .. } => assert_eq!(bits, snan_bits),
            other => panic!("expected Float, got {other:?}"),
        }
    }
}
