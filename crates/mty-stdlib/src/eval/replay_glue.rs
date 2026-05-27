//! Replay-runtime glue — thin wrapper over the v0.21 replay machinery
//! that exposes "give me the prompt + recorded reply from this trace"
//! and "re-run this trace's LLM turn against a fresh provider".
//!
//! ## Scope
//!
//! The v0.21 [`mty_runtime::replay::ReplayDriver`] re-executes a
//! recorded `Spawn`/`MessageSent`/`MessageHandled` event stream against
//! a fresh `Runtime` — it gives byte-identical replay of the *agent*
//! layer, but it does not surface the recorded *LLM* turns directly.
//! For v0.28 Track G the eval driver only needs two operations:
//!
//! 1. Decode the recorded prompt + assistant reply out of a trace
//!    file so a `Case::from_trace` can use them as the baseline.
//! 2. Re-run that prompt against a fresh [`crate::swarm::Member`]
//!    under the eval's shared budget.
//!
//! Both operations are exposed by this module. Operation 1 reads a
//! lightweight on-disk format (one JSON object per turn) that the
//! v0.28 `mty trace record` CLI emits in eval-mode; Operation 2 calls
//! straight into `Member::ask`. The v0.21 byte-identical replay
//! machinery is *not* required for v0.28 — the typical eval case is
//! "rerun a 1-turn query on N models", not "byte-replay a multi-agent
//! trace". The full integration is queued under [`V029_BACKLOG`].
//!
//! ## v0.29 backlog
//!
//! See [`V029_BACKLOG`] for the list of replay-runtime hooks the
//! integrator should land in v0.29:
//!
//! 1. `Replay::with_provider(member)` constructor on `ReplayDriver`
//!    that swaps the recorded LLM provider mid-replay so the eval
//!    driver can byte-replay a multi-turn trace + only divert the LLM
//!    calls to a new member.
//! 2. `RecordedTrace::iter_llm_calls()` accessor so the eval driver
//!    can fast-path "just rerun the LLM turns" without spinning a
//!    full `Runtime`.
//! 3. Trace v3 wire format that captures LLM request+response shapes
//!    structurally (prompt + system + tools + reply) so eval reports
//!    can show tool-call diffs without re-parsing model output.
//!
//! The eval driver works against the existing v0.21 replay surface
//! today by reading a minimal JSON-lines trace shape (see
//! [`decode_trace_baseline`]); upgrading to the v3 wire format is a
//! drop-in once the integrator lands the hooks above.

use std::fs;
use std::path::Path;

use thiserror::Error;

use crate::swarm::{Member, MemberReply, SharedDollarBudget};

/// v0.29 backlog items the integrator should land to upgrade
/// `std.eval` from the JSON-lines fast path to the full byte-identical
/// replay-driver integration. Surfaced as a `const &[&str]` so the
/// commit body + the docs page can pull it without duplicating text.
pub const V029_BACKLOG: &[&str] = &[
    "replay-runtime: `Replay::with_provider(member)` constructor on \
     `ReplayDriver` that swaps the recorded `LlmProvider` mid-replay.",
    "replay-runtime: `RecordedTrace::iter_llm_calls()` accessor so \
     std.eval can rerun just the LLM turns without a fresh Runtime.",
    "replay-wire: v3 format capturing LLM request+response shapes \
     structurally (prompt + system + tools + reply text + tool_uses).",
    "std.eval: divergence reporter integration with `mty replay --diff` \
     so eval failures point back at the exact recorded turn.",
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
    /// JSON-lines turn format; future versions can add a magic prefix
    /// + structured wire decoder here.
    #[error("eval-replay: trace at {path} is malformed: {reason}")]
    MalformedTrace { path: String, reason: String },
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
/// `(user-prompt, assistant-reply)` pair. The on-disk format for
/// v0.28 is one JSON object per turn:
///
/// ```text
/// {"type": "user", "content": "What is 2+2?"}
/// {"type": "assistant", "content": "4"}
/// ```
///
/// The decoder ignores other event types (`system`, `tool_use`, ...)
/// so it stays forward-compatible with the v0.29 structured trace
/// wire format. Unknown fields are silently dropped.
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

/// Run a trace's prompt against a fresh member under the supplied
/// budget. The default v0.28 path is just `member.ask(prompt, budget)`
/// — the v0.29 backlog upgrades this to a true byte-identical replay
/// via [`mty_runtime::replay::ReplayDriver`] once the
/// `with_provider` hook lands.
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
    fn v029_backlog_is_non_empty() {
        // Sanity check — the backlog text is what the commit body
        // surfaces. If someone empties it, the commit message would
        // lose the v0.29 follow-up list.
        assert!(!V029_BACKLOG.is_empty());
        assert!(V029_BACKLOG.iter().all(|s| !s.is_empty()));
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
}
