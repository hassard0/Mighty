//! Replay-runtime glue — bridges `std.eval` to the native v3 binary
//! replay machinery in `mty_runtime::replay`.
//!
//! ## v0.28 → v0.32
//!
//! v0.28 Track G shipped this module as a JSON-lines shim
//! (`{"type":"user"|"assistant"}` lines), keyed off
//! `decode_trace_baseline`. v0.29 Track F added the v3 native
//! decoders + auto-route. v0.32 Track F closes the loop:
//!
//! * [`Case::from_trace`](crate::eval::case::Case::from_trace) now
//!   reads **only** the v3 binary `.mty-trace` shape — the
//!   JSON-lines fallback that the auto-route used to fall back on
//!   has been retired. Legacy `*.jsonl` fixtures get a clear error
//!   pointing at `MTY_RECORD_TRACE` rather than a silent
//!   "best-effort" decode.
//! * Every `Member::ask` call surfaces structured `tool_uses` on
//!   `MemberReply`, so [`MemberTurnProvider`] forwards them through
//!   to the live provider's [`ProvidedTurn`] without losing the
//!   structural shape.
//! * `MTY_RECORD_TRACE=<path>` now auto-captures every `Member::ask`
//!   call through the [`recorder`](mty_runtime::replay::recorder)
//!   hook in `std.swarm::member` — no eval driver needs to call
//!   `record_llm_call` explicitly.
//!
//! ## What this module exposes
//!
//! * [`decode_trace_baseline`] — the v0.28 JSON-lines decoder. Kept
//!   for `read_jsonl_baseline()`-style explicit calls (tests + tools
//!   that author traces by hand). **Not** wired into `Case::from_trace`
//!   anymore.
//! * [`decode_trace_baseline_native`] — load a v3 binary `.mty-trace`
//!   produced by `MTY_RECORD_TRACE`, iterate
//!   `TraceFile::iter_llm_calls()`, return the first LLM turn as the
//!   baseline.
//! * [`decode_baseline_auto`] — v0.32: enforces the native-only path.
//!   Files without the `MTYTRACE` magic prefix now error rather than
//!   falling through to the JSON-lines shim.
//! * [`MemberTurnProvider`] — adapter that implements
//!   `mty_runtime::replay::TurnProvider` for a `Member`, so the eval
//!   driver can hand a panel member to
//!   `ReplayDriver::with_provider`. v0.32 surfaces the structural
//!   tool_uses through.
//! * [`run_trace_with_member`] — convenience: dispatch the recorded
//!   prompt against a fresh member under the shared budget.

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

/// v0.33 follow-ups — items surfaced during v0.32 Track F that didn't
/// fit the v0.32 scope but are worth tracking for the next track:
///
/// 1. `Member::ask` doesn't yet know which agent spawned it, so the
///    `record_member_turn` hook stamps every recorded turn with the
///    synthetic agent id `0`. v0.33 should plumb the spawning agent's
///    id through the swarm + eval surface so multi-agent traces
///    attribute turns to the right agent.
/// 2. The tool-name list emitted on `TraceEvent::LlmCall.tools` today
///    is the *model* name (one-element vec) because `Member::ask`
///    doesn't carry an advertised tool list at construction. v0.33
///    should add a `Member::with_tools(...)` builder + lift the
///    advertised tool names into the record.
/// 3. `ReplayDriver::replay_all` interleaved with `with_provider`
///    re-emits each LLM call against the live provider but does not
///    yet *re-record* the live turn into the replay's secondary
///    trace. v0.33 should add `--rerecord <path>` so a successful
///    eval can write the new trace as the next baseline.
pub const V033_FOLLOWUPS: &[&str] = &[
    "v0.33: lift spawning-agent id through Member::ask so trace.agent is non-zero for in-runtime calls.",
    "v0.33: thread advertised-tool list (Member::with_tools) through to TraceEvent::LlmCall.tools.",
    "v0.33: ReplayDriver::replay_all --rerecord <path> writes live turns as the next baseline trace.",
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

/// v0.32 Track F: route a trace path through the native v3 binary
/// decoder. The v0.28-era JSON-lines fallback (`decode_trace_baseline`)
/// is no longer auto-invoked from this entry point — `Case::from_trace`
/// is now native-only.
///
/// The auto-route name is kept so existing callers don't churn, but
/// the only path it takes is the v3 binary one. Files without the
/// 8-byte `MTYTRACE` magic prefix surface a clear
/// [`ReplayGlueError::MalformedTrace`] pointing the user at
/// `MTY_RECORD_TRACE` rather than a silent "best-effort JSON-lines"
/// decode.
///
/// Callers that *do* want to read a hand-written JSON-lines fixture
/// should call [`decode_trace_baseline`] explicitly — it stays in the
/// surface for the tools-and-tests use case.
pub fn decode_baseline_auto(path: &Path) -> Result<TraceBaseline, ReplayGlueError> {
    let bytes = fs::read(path).map_err(|e| ReplayGlueError::TraceRead {
        path: path.display().to_string(),
        source: e,
    })?;
    if !bytes.starts_with(TRACE_MAGIC) {
        return Err(ReplayGlueError::MalformedTrace {
            path: path.display().to_string(),
            reason: "missing MTYTRACE magic prefix — Case::from_trace is native-only \
                     since v0.32; record traces via MTY_RECORD_TRACE=<path> or call \
                     decode_trace_baseline() explicitly for legacy JSON-lines fixtures"
                .to_string(),
        });
    }
    let trace = decode_binary_trace(&bytes).map_err(|e| ReplayGlueError::MalformedTrace {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    let first = trace
        .iter_llm_calls()
        .next()
        .ok_or_else(|| ReplayGlueError::NoLlmTurns(path.display().to_string()))?;
    Ok(TraceBaseline {
        prompt: first.prompt.to_string(),
        assistant_reply: first.reply.to_string(),
    })
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
        // v0.32 Track F: `MemberReply.tool_uses` is now populated by
        // every provider's `Member::ask` (lifted from the typed
        // `Message::tool_uses()` block). Translate the structural
        // `crate::llm::ToolUse` shape into the wire-v3
        // `mty_runtime::replay::LlmToolUse` shape so the live turn
        // mirrors the recorded one structurally.
        let tool_uses: Vec<LlmToolUse> = reply
            .tool_uses
            .iter()
            .map(|t| LlmToolUse {
                name: t.name.clone(),
                id: t.id.clone(),
                input_json: serde_json::to_string(&t.input).unwrap_or_else(|_| "{}".to_string()),
            })
            .collect();
        Ok(ProvidedTurn {
            reply: reply.body,
            tool_uses,
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
    fn v033_followups_list_three_items() {
        // The v0.32 closeout surfaces 3 follow-ups for v0.33 — see
        // module-level docs for the rationale of each.
        assert_eq!(V033_FOLLOWUPS.len(), 3);
        assert!(V033_FOLLOWUPS.iter().all(|s| s.starts_with("v0.33:")));
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
    fn decode_baseline_auto_rejects_jsonl_without_native_magic() {
        // v0.32: the JSON-lines fallback is no longer auto-invoked by
        // `decode_baseline_auto`. Files without the MTYTRACE magic
        // prefix surface a clear error pointing the user at
        // `MTY_RECORD_TRACE` or the explicit `decode_trace_baseline`
        // entry point.
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, r#"{{"type":"user","content":"hi"}}"#).unwrap();
        writeln!(tmp, r#"{{"type":"assistant","content":"hello"}}"#).unwrap();
        tmp.flush().unwrap();
        let err = decode_baseline_auto(tmp.path()).unwrap_err();
        match err {
            ReplayGlueError::MalformedTrace { reason, .. } => {
                assert!(reason.contains("MTYTRACE"));
                assert!(reason.contains("MTY_RECORD_TRACE"));
            }
            other => panic!("expected MalformedTrace, got {other:?}"),
        }
    }

    #[test]
    fn decode_trace_baseline_still_reads_jsonl_directly() {
        // The legacy JSON-lines decoder is still exposed for tools
        // and tests that author fixtures by hand — it's just not
        // routed through `decode_baseline_auto` anymore.
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, r#"{{"type":"user","content":"hi"}}"#).unwrap();
        writeln!(tmp, r#"{{"type":"assistant","content":"hello"}}"#).unwrap();
        tmp.flush().unwrap();
        let b = decode_trace_baseline(tmp.path()).unwrap();
        assert_eq!(b.prompt, "hi");
        assert_eq!(b.assistant_reply, "hello");
    }

    #[test]
    fn decode_baseline_auto_reads_native_trace_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("native_end_to_end.mty-trace");
        write_v3_trace_with_one_llm_call(&path);
        let b = decode_baseline_auto(&path).unwrap();
        assert_eq!(b.prompt, "what is 2+2?");
        assert_eq!(b.assistant_reply, "4");
    }

    #[test]
    fn decode_baseline_auto_propagates_recorder_error_on_corrupt_native() {
        // File starts with MTYTRACE but body is junk → native decoder
        // surfaces a `RecorderError::Serde` which propagates through
        // as `MalformedTrace`.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("corrupt.mty-trace");
        let mut bytes = TRACE_MAGIC.to_vec();
        bytes.extend_from_slice(b"not json at all");
        std::fs::write(&path, bytes).unwrap();
        let err = decode_baseline_auto(&path).unwrap_err();
        assert!(matches!(err, ReplayGlueError::MalformedTrace { .. }));
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

    // -------------------------------------------------------------------------
    // v0.32 Track F: native-only Case::from_trace + MemberTurnProvider
    // structural tool_uses

    #[tokio::test(flavor = "multi_thread")]
    async fn member_turn_provider_lifts_structural_tool_uses_through_to_provided_turn() {
        use crate::llm::message::ToolUse;
        let canned = vec![ToolUse {
            id: "tu_1".into(),
            name: "search_web".into(),
            input: serde_json::json!({"q": "rust"}),
        }];
        let provider =
            MemberTurnProvider::unbounded(Member::mock_with_tool_uses("m", "ok", 1, canned));
        let event = TraceEvent::LlmCall {
            agent: 0,
            turn_id: 0,
            prompt: "go search".into(),
            system: None,
            tools: vec![],
            reply: "ignored".into(),
            tool_uses: vec![],
            cost_cents: 0,
        };
        let mut t = TraceFile::new(0, 0, 1);
        t.events.push(event);
        let turn = t.iter_llm_calls().next().unwrap();
        let out = provider.provide(turn).unwrap();
        assert_eq!(out.tool_uses.len(), 1);
        assert_eq!(out.tool_uses[0].name, "search_web");
        assert!(out.tool_uses[0].input_json.contains("\"q\":\"rust\""));
    }

    #[test]
    fn decode_baseline_auto_is_native_only_after_v032() {
        // A v0.28-style JSON-lines fixture used to auto-route through
        // the JSON-lines shim. v0.32 retired that fallback — the
        // surface now surfaces an error pointing at MTY_RECORD_TRACE.
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, r#"{{"type":"user","content":"hi"}}"#).unwrap();
        tmp.flush().unwrap();
        let err = decode_baseline_auto(tmp.path()).unwrap_err();
        let s = err.to_string();
        assert!(s.contains("MTY_RECORD_TRACE") || s.contains("MTYTRACE"));
    }

    #[test]
    fn case_from_trace_now_round_trips_via_native_recorder() {
        // The end-to-end Case::from_trace path now reads only the v3
        // binary shape. This test asserts the typed CaseRun lifts
        // through cleanly.
        use crate::eval::case::Case;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("e2e.mty-trace");
        write_v3_trace_with_one_llm_call(&path);
        let c = Case::from_trace(&path);
        let cr = c.resolve().unwrap();
        assert_eq!(cr.prompt, "what is 2+2?");
        assert_eq!(cr.baseline_reply.as_deref(), Some("4"));
        assert!(cr.source_trace.is_some());
    }

    #[test]
    fn v033_followups_documented_and_starts_with_v033_prefix() {
        for entry in V033_FOLLOWUPS {
            assert!(entry.starts_with("v0.33:"), "entry: {entry}");
            assert!(!entry.is_empty());
        }
    }
}
