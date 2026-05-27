//! Replay-runtime glue — bridges `std.eval` to the v0.29 native
//! replay machinery in `mty_runtime::replay`.
//!
//! ## v0.28 vs v0.29
//!
//! v0.28 Track G shipped this module as a JSON-lines shim: trace
//! files were single-purpose `{"type":"user"|"assistant"}` lines
//! decoded by [`decode_trace_baseline`], and eval re-runs went
//! straight through `Member::ask` without involving the replay
//! driver at all. The v0.29 backlog (the `V029_BACKLOG` constant
//! below, kept for historical reference) called out four upgrades.
//! Track F lands them:
//!
//! 1. **`ReplayDriver::with_provider`** — `mty_runtime` now exposes
//!    a [`TurnProvider`] trait + a `ReplayDriver::with_provider`
//!    method that swaps the recorded LLM provider for a fresh one
//!    mid-replay. See `crates/mty-runtime/src/replay/replay_driver.rs`.
//! 2. **`TraceFile::iter_llm_calls`** — borrowed iterator over every
//!    [`mty_runtime::replay::TraceEvent::LlmCall`] event in a trace.
//! 3. **Wire v3** — the recorder now emits `TraceEvent::LlmCall`
//!    structurally (prompt + system + tools + reply + tool_uses).
//!    `TRACE_WIRE_VERSION` bumped 2 → 3.
//! 4. **`mty replay --diff`** — the CLI surfaces
//!    [`mty_runtime::replay::ReplayDriver::diff_llm_turn`] under
//!    `mty replay --diff <trace> --turn <id>` so an eval divergence
//!    points back at the exact recorded turn.
//!
//! ## What this module exposes
//!
//! * [`decode_trace_baseline`] — the v0.28 JSON-lines decoder, kept
//!   as a fallback for trace files written before wire-v3 (and used
//!   by [`Case::from_trace`] for `.jsonl`-style traces).
//! * [`decode_trace_baseline_native`] — the v0.29 path: load a
//!   binary `.mty-trace` produced by `MTY_RECORD_TRACE`, iterate
//!   `TraceFile::iter_llm_calls()`, return the first LLM turn as the
//!   baseline.
//! * [`decode_baseline_auto`] — try the native path first; fall back
//!   to JSON-lines if the file doesn't carry the trace magic prefix.
//!   This is what `Case::from_trace` calls.
//! * [`MemberTurnProvider`] — adapter that implements
//!   `mty_runtime::replay::TurnProvider` for a `Member`, so the eval
//!   driver can hand a panel member to
//!   `ReplayDriver::with_provider`.
//! * [`run_trace_with_member`] — convenience: dispatch the recorded
//!   prompt against a fresh member under the shared budget.
//!
//! ## Mixed wire support
//!
//! The auto-decoder accepts both the lightweight JSON-lines shim
//! (for hand-written eval fixtures + older recordings) and the v3
//! binary trace. The `Case::from_trace` constructor doesn't care
//! which one a fixture uses — both surface as
//! [`TraceBaseline { prompt, assistant_reply }`].

use std::fs;
use std::path::Path;
use std::sync::Arc;

use thiserror::Error;
use tokio::runtime::Handle;

use mty_runtime::replay::{
    decode as decode_binary_trace, LlmCallRef, LlmToolUse, ProvidedTurn, RecorderError, TraceFile,
    TurnProvider, TRACE_MAGIC,
};

use crate::swarm::{Member, MemberReply, SharedDollarBudget};

/// v0.29 backlog — kept for historical reference. **All four items
/// were landed by v0.29 Track F.** The constant lives on so the
/// docs page + commit-body can cite it; the items are now marked
/// shipped via the `[shipped]` prefix.
pub const V029_BACKLOG: &[&str] = &[
    "[shipped v0.29] replay-runtime: `ReplayDriver::with_provider` + `TurnProvider` trait.",
    "[shipped v0.29] replay-runtime: `TraceFile::iter_llm_calls` borrowed iterator.",
    "[shipped v0.29] replay-wire: wire-v3 `TraceEvent::LlmCall` (prompt + system + tools + reply + tool_uses).",
    "[shipped v0.29] std.eval: `mty replay --diff <trace> --turn <id>` integration.",
];

/// Errors returned by the replay-glue layer.
#[derive(Debug, Error)]
pub enum ReplayGlueError {
    /// The trace file at the configured path was missing or
    /// unreadable.
    #[error("eval-replay: cannot read trace at {path}: {source}")]
    TraceRead {
        path: String,
        #[source]
        source: std::io::Error,
    },
    /// The trace file was readable but didn't contain a user-prompt
    /// turn we could extract a baseline from.
    #[error("eval-replay: trace at {0} does not contain a user prompt")]
    NoUserPrompt(String),
    /// The trace file's wire shape was unrecognised. v0.28 reads a
    /// JSON-lines turn format; v0.29 adds the v3 binary trace decoder.
    #[error("eval-replay: trace at {path} is malformed: {reason}")]
    MalformedTrace { path: String, reason: String },
    /// The binary trace decoded but didn't contain any `LlmCall`
    /// events — caller's [`Case::from_trace`] needs at least one.
    #[error(
        "eval-replay: trace at {0} is a valid v3 trace but contains no LlmCall events; \
         either record an LLM turn via `MTY_RECORD_TRACE` + a `std.eval` driver, or use the \
         JSON-lines fallback shape"
    )]
    NoLlmTurns(String),
}

impl From<RecorderError> for ReplayGlueError {
    fn from(err: RecorderError) -> Self {
        ReplayGlueError::MalformedTrace {
            path: "<binary trace>".to_string(),
            reason: err.to_string(),
        }
    }
}

/// Decoded baseline pulled from a trace file. The eval driver uses
/// `prompt` to drive each member's `ask` + `assistant_reply` as the
/// comparator's reference column.
#[derive(Debug, Clone)]
pub struct TraceBaseline {
    pub prompt: String,
    pub assistant_reply: String,
}

/// Read a trace file off disk and extract the first
/// `(user-prompt, assistant-reply)` pair. The on-disk format
/// accepted by this decoder is one JSON object per turn:
///
/// ```text
/// {"type": "user", "content": "What is 2+2?"}
/// {"type": "assistant", "content": "4"}
/// ```
///
/// The decoder ignores other event types (`system`, `tool_use`, ...)
/// so it stays forward-compatible with the v0.29 structured trace
/// wire format. Unknown fields are silently dropped.
///
/// For native v3 binary traces produced by `MTY_RECORD_TRACE`, use
/// [`decode_trace_baseline_native`] (or [`decode_baseline_auto`],
/// which routes both shapes).
pub fn decode_trace_baseline(path: &Path) -> Result<TraceBaseline, ReplayGlueError> {
    let body = fs::read_to_string(path).map_err(|e| ReplayGlueError::TraceRead {
        path: path.display().to_string(),
        source: e,
    })?;

    let mut prompt: Option<String> = None;
    let mut reply: Option<String> = None;

    for (lineno, raw) in body.lines().enumerate() {
        if raw.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value =
            serde_json::from_str(raw).map_err(|e| ReplayGlueError::MalformedTrace {
                path: path.display().to_string(),
                reason: format!("line {}: invalid JSON: {}", lineno + 1, e),
            })?;
        let ty = v.get("type").and_then(|x| x.as_str()).unwrap_or_default();
        let content = v
            .get("content")
            .and_then(|x| x.as_str())
            .unwrap_or_default();
        match ty {
            "user" if prompt.is_none() => prompt = Some(content.to_string()),
            "assistant" if reply.is_none() => reply = Some(content.to_string()),
            _ => {}
        }
        if prompt.is_some() && reply.is_some() {
            break;
        }
    }

    let prompt = prompt.ok_or_else(|| ReplayGlueError::NoUserPrompt(path.display().to_string()))?;
    Ok(TraceBaseline {
        prompt,
        assistant_reply: reply.unwrap_or_default(),
    })
}

/// v0.29 native path — decode a binary `.mty-trace` produced by the
/// `MTY_RECORD_TRACE` recorder + return the first `LlmCall` event's
/// `(prompt, reply)` as the baseline.
///
/// Routes through [`mty_runtime::replay::TraceFile::iter_llm_calls`]
/// (v0.29 backlog item #2) so the eval driver no longer parses a
/// trace-specific JSON shape — it consumes the same wire format the
/// runtime's `mty replay` CLI does.
pub fn decode_trace_baseline_native(path: &Path) -> Result<TraceBaseline, ReplayGlueError> {
    let trace = read_binary_trace(path)?;
    let first = trace
        .iter_llm_calls()
        .next()
        .ok_or_else(|| ReplayGlueError::NoLlmTurns(path.display().to_string()))?;
    Ok(TraceBaseline {
        prompt: first.prompt.to_string(),
        assistant_reply: first.reply.to_string(),
    })
}

/// Auto-route between native v3 binary traces and the JSON-lines
/// fallback. Detection is by the 8-byte `MTYTRACE` magic prefix —
/// any file that starts with it routes through the native decoder,
/// every other file goes through the JSON-lines path.
///
/// `Case::from_trace` calls this so existing eval fixtures
/// (JSON-lines) keep working while new recordings produced by
/// `MTY_RECORD_TRACE` flow through the native v3 path.
pub fn decode_baseline_auto(path: &Path) -> Result<TraceBaseline, ReplayGlueError> {
    let bytes = fs::read(path).map_err(|e| ReplayGlueError::TraceRead {
        path: path.display().to_string(),
        source: e,
    })?;
    if bytes.starts_with(TRACE_MAGIC) {
        let trace = decode_binary_trace(&bytes).map_err(|e| ReplayGlueError::MalformedTrace {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
        let first = trace
            .iter_llm_calls()
            .next()
            .ok_or_else(|| ReplayGlueError::NoLlmTurns(path.display().to_string()))?;
        return Ok(TraceBaseline {
            prompt: first.prompt.to_string(),
            assistant_reply: first.reply.to_string(),
        });
    }
    decode_trace_baseline(path)
}

/// Load the full `TraceFile` from disk — used by callers that want
/// to iterate every recorded turn, not just the first. `std.eval`
/// uses this when the eval driver wants to walk an entire
/// multi-turn trace via [`MemberTurnProvider`].
pub fn read_binary_trace(path: &Path) -> Result<TraceFile, ReplayGlueError> {
    let bytes = fs::read(path).map_err(|e| ReplayGlueError::TraceRead {
        path: path.display().to_string(),
        source: e,
    })?;
    decode_binary_trace(&bytes).map_err(|e| ReplayGlueError::MalformedTrace {
        path: path.display().to_string(),
        reason: e.to_string(),
    })
}

/// Adapter — implements [`mty_runtime::replay::TurnProvider`] for a
/// `std.eval` [`Member`], so the eval driver can hand a panel member
/// straight to
/// [`mty_runtime::replay::ReplayDriver::with_provider`] (v0.29
/// backlog item #1).
///
/// The provider serialises through the active tokio runtime
/// (`Handle::current().block_on(...)`) because `TurnProvider::provide`
/// is sync at the surface — `mty-runtime` doesn't want to drag an
/// async runtime through its trait. Callers must invoke
/// `replay_llm_turns` from within a `#[tokio::main]` / `block_in_place`
/// context for this to work.
pub struct MemberTurnProvider {
    member: Member,
    budget: Arc<SharedDollarBudget>,
}

impl MemberTurnProvider {
    pub fn new(member: Member, budget: SharedDollarBudget) -> Self {
        Self {
            member,
            budget: Arc::new(budget),
        }
    }

    /// Convenience: build a provider backed by an unlimited budget.
    /// Useful in tests + when the caller has already capped cost
    /// elsewhere.
    pub fn unbounded(member: Member) -> Self {
        Self::new(member, SharedDollarBudget::new(u64::MAX))
    }
}

impl TurnProvider for MemberTurnProvider {
    fn provide(&self, turn: LlmCallRef<'_>) -> Result<ProvidedTurn, String> {
        let prompt = turn.prompt.to_string();
        let member = self.member.clone();
        let budget = self.budget.clone();
        // Run the async ask inside the current tokio runtime. If the
        // caller invoked us from a sync context (`#[test]` without
        // `#[tokio::test]`), `Handle::try_current` returns `Err` and
        // we surface a helpful message rather than panicking on
        // `block_on`.
        let reply: Result<MemberReply, String> = match Handle::try_current() {
            Ok(handle) => match handle.runtime_flavor() {
                tokio::runtime::RuntimeFlavor::CurrentThread => {
                    // We're inside a single-threaded runtime — blocking
                    // would deadlock. Spawn a fresh runtime on a
                    // dedicated thread instead.
                    block_on_isolated(async move { member.ask(&prompt, &budget).await })
                        .map_err(|e| e.to_string())
                }
                _ => tokio::task::block_in_place(|| {
                    handle.block_on(async move { member.ask(&prompt, &budget).await })
                })
                .map_err(|e| e.to_string()),
            },
            Err(_) => block_on_isolated(async move { member.ask(&prompt, &budget).await })
                .map_err(|e| e.to_string()),
        };
        let reply = reply?;
        Ok(ProvidedTurn {
            reply: reply.body,
            // The streaming adapter surfaces tool_uses via the LLM
            // layer, not via `MemberReply` — v0.29 surfaces only the
            // text. A v0.30 follow-up lifts tool_uses up through
            // `Member::ask` so the provider can return them
            // structurally.
            tool_uses: Vec::<LlmToolUse>::new(),
            cost_cents: reply.cost_cents,
        })
    }
}

/// Run an async future on a dedicated single-thread runtime —
/// guaranteed to not deadlock against the current runtime, at the
/// cost of one short-lived OS thread per call.
fn block_on_isolated<F, T>(fut: F) -> T
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("eval-replay: isolated tokio runtime build failed");
        let v = rt.block_on(fut);
        let _ = tx.send(v);
    });
    rx.recv()
        .expect("eval-replay: isolated runtime thread dropped its channel")
}

/// Run a trace's prompt against a fresh member under the supplied
/// budget. Equivalent to a single-turn dispatch through
/// [`MemberTurnProvider`]; kept as a separate helper because the
/// most common eval-case path (`Case::from_trace` + 1-turn fixture)
/// only needs one ask.
pub async fn run_trace_with_member(
    prompt: &str,
    member: &Member,
    budget: &SharedDollarBudget,
) -> Result<MemberReply, crate::llm::error::LlmError> {
    member.ask(prompt, budget).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use mty_runtime::replay::{Recorder, TraceCodec, TraceEvent};
    use std::io::Write;

    #[test]
    fn decode_baseline_picks_first_user_and_assistant() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, r#"{{"type":"user","content":"q1"}}"#).unwrap();
        writeln!(tmp, r#"{{"type":"assistant","content":"a1"}}"#).unwrap();
        writeln!(tmp, r#"{{"type":"user","content":"q2"}}"#).unwrap();
        writeln!(tmp, r#"{{"type":"assistant","content":"a2"}}"#).unwrap();
        tmp.flush().unwrap();
        let b = decode_trace_baseline(tmp.path()).unwrap();
        assert_eq!(b.prompt, "q1");
        assert_eq!(b.assistant_reply, "a1");
    }

    #[test]
    fn decode_baseline_skips_unknown_event_types() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, r#"{{"type":"system","content":"you are helpful"}}"#).unwrap();
        writeln!(tmp, r#"{{"type":"tool_use","content":"search"}}"#).unwrap();
        writeln!(tmp, r#"{{"type":"user","content":"hello"}}"#).unwrap();
        writeln!(tmp, r#"{{"type":"assistant","content":"hi"}}"#).unwrap();
        tmp.flush().unwrap();
        let b = decode_trace_baseline(tmp.path()).unwrap();
        assert_eq!(b.prompt, "hello");
        assert_eq!(b.assistant_reply, "hi");
    }

    #[test]
    fn decode_baseline_missing_user_errors() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, r#"{{"type":"assistant","content":"orphan"}}"#).unwrap();
        tmp.flush().unwrap();
        let r = decode_trace_baseline(tmp.path());
        assert!(matches!(r, Err(ReplayGlueError::NoUserPrompt(_))));
    }

    #[test]
    fn decode_baseline_missing_file_errors() {
        let r = decode_trace_baseline(Path::new("/nonexistent/never.mty-trace"));
        assert!(matches!(r, Err(ReplayGlueError::TraceRead { .. })));
    }

    #[test]
    fn decode_baseline_malformed_json_errors() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "not json at all").unwrap();
        tmp.flush().unwrap();
        let r = decode_trace_baseline(tmp.path());
        assert!(matches!(r, Err(ReplayGlueError::MalformedTrace { .. })));
    }

    #[test]
    fn decode_baseline_assistant_reply_empty_when_only_user() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, r#"{{"type":"user","content":"q"}}"#).unwrap();
        tmp.flush().unwrap();
        let b = decode_trace_baseline(tmp.path()).unwrap();
        assert_eq!(b.prompt, "q");
        assert_eq!(b.assistant_reply, "");
    }

    #[test]
    fn v029_backlog_marks_every_item_as_shipped() {
        // Sanity check — every backlog entry now starts with the
        // shipped-marker (Track F closed all four items).
        assert!(!V029_BACKLOG.is_empty());
        assert!(V029_BACKLOG
            .iter()
            .all(|s| s.starts_with("[shipped v0.29]")));
    }

    #[tokio::test]
    async fn run_trace_with_member_dispatches_to_mock() {
        let member = Member::mock("m", "paris", 1);
        let budget = SharedDollarBudget::new(100);
        let r = run_trace_with_member("capital of france?", &member, &budget)
            .await
            .unwrap();
        assert_eq!(r.body, "paris");
        assert_eq!(r.cost_cents, 1);
    }

    // -------------------------------------------------------------------------
    // v0.29 Track F: native v3 binary trace decoder + auto-routing
    // -------------------------------------------------------------------------

    fn write_v3_trace_with_one_llm_call(path: &Path) {
        let r = Recorder::new(path, 0, 1).with_codec(TraceCodec::Json);
        r.record_llm_call(
            0,
            None,
            "what is 2+2?",
            Some("you are a calculator"),
            vec!["calc".into()],
            "4",
            vec![LlmToolUse {
                name: "calc".into(),
                id: "tu-1".into(),
                input_json: "{\"x\":2}".into(),
            }],
            1,
        );
        r.flush_to_disk().unwrap();
    }

    #[test]
    fn decode_baseline_native_reads_v3_binary_trace() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("eval.mty-trace");
        write_v3_trace_with_one_llm_call(&path);
        let b = decode_trace_baseline_native(&path).unwrap();
        assert_eq!(b.prompt, "what is 2+2?");
        assert_eq!(b.assistant_reply, "4");
    }

    #[test]
    fn decode_baseline_native_errors_when_no_llm_turns() {
        // Build a v3 trace that contains a Spawn but no LlmCall.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no-llm.mty-trace");
        let r = Recorder::new(&path, 0, 1).with_codec(TraceCodec::Json);
        r.record_spawn(1, "Echo", None);
        r.flush_to_disk().unwrap();
        let err = decode_trace_baseline_native(&path).unwrap_err();
        assert!(matches!(err, ReplayGlueError::NoLlmTurns(_)));
    }

    #[test]
    fn decode_baseline_auto_routes_binary_to_native_decoder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auto.mty-trace");
        write_v3_trace_with_one_llm_call(&path);
        let b = decode_baseline_auto(&path).unwrap();
        assert_eq!(b.prompt, "what is 2+2?");
        assert_eq!(b.assistant_reply, "4");
    }

    #[test]
    fn decode_baseline_auto_routes_jsonl_to_shim_decoder() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, r#"{{"type":"user","content":"hi"}}"#).unwrap();
        writeln!(tmp, r#"{{"type":"assistant","content":"hello"}}"#).unwrap();
        tmp.flush().unwrap();
        let b = decode_baseline_auto(tmp.path()).unwrap();
        assert_eq!(b.prompt, "hi");
        assert_eq!(b.assistant_reply, "hello");
    }

    #[test]
    fn read_binary_trace_round_trips_every_llm_call() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("multi.mty-trace");
        let r = Recorder::new(&path, 0, 1).with_codec(TraceCodec::Json);
        r.record_llm_call(0, None, "q1", None, vec![], "a1", vec![], 1);
        r.record_llm_call(0, None, "q2", None, vec![], "a2", vec![], 2);
        r.record_llm_call(0, None, "q3", None, vec![], "a3", vec![], 3);
        r.flush_to_disk().unwrap();

        let trace = read_binary_trace(&path).unwrap();
        let calls: Vec<_> = trace.iter_llm_calls().collect();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].prompt, "q1");
        assert_eq!(calls[2].reply, "a3");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn member_turn_provider_dispatches_recorded_turn_against_member() {
        // Build a small trace with one LLM turn, then drive it via
        // MemberTurnProvider against a mock member. The provider
        // bypasses tokio bookkeeping (block_in_place) on the
        // multi-thread runtime so we can serialise the async ask.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("provider.mty-trace");
        write_v3_trace_with_one_llm_call(&path);
        let trace = read_binary_trace(&path).unwrap();

        let provider = MemberTurnProvider::unbounded(Member::mock("m", "fresh-reply", 7));
        let turn = trace.iter_llm_calls().next().unwrap();
        let out = provider.provide(turn).unwrap();
        assert_eq!(out.reply, "fresh-reply");
        assert_eq!(out.cost_cents, 7);
    }

    #[test]
    fn member_turn_provider_surfaces_member_errors_as_strings() {
        // Sync context — provider falls through to an isolated
        // runtime. The mock-error member returns an LlmError that
        // the provider converts to its `String` failure shape.
        let provider = MemberTurnProvider::unbounded(Member::mock_error("m", "boom"));
        // Synthesise a one-shot LlmCallRef by hand for the test —
        // the call site doesn't need a full TraceFile.
        let event = TraceEvent::LlmCall {
            agent: 0,
            turn_id: 0,
            prompt: "p".into(),
            system: None,
            tools: vec![],
            reply: "ignored".into(),
            tool_uses: vec![],
            cost_cents: 0,
        };
        // Borrow the call ref out of a trace.
        let mut t = TraceFile::new(0, 0, 1);
        t.events.push(event);
        let turn = t.iter_llm_calls().next().unwrap();
        let r = provider.provide(turn);
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("boom"));
    }
}
