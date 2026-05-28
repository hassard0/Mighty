//! The typed [`LlmObservation`] record one [`record_now`] call writes.
//!
//! Kept in its own module so the shape doesn't tangle with the
//! storage/query plumbing — tests round-trip this struct directly
//! via `serde_json`, and `mty inspect --cost --json` (future) can
//! emit it verbatim.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// One recorded LLM completion.
///
/// Fields are public — call sites build them directly (no builder),
/// the SQLite + OTel exporters read them all, and tests pattern-match
/// shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmObservation {
    /// `"anthropic"`, `"openai"`, `"gemini"`, `"bedrock"` — matches
    /// the `LlmProvider` impl module name.
    pub provider: String,
    /// Provider-specific model id (e.g. `claude-opus-4-7`).
    pub model: String,
    /// Tokens billed for the input/system prompt.
    pub prompt_tokens: u64,
    /// Tokens billed for the assistant's reply.
    pub completion_tokens: u64,
    /// Cost in **integer cents** computed from
    /// [`crate::observe::pricing::cost_cents_for`]. `i64` so a sum
    /// across millions of rows doesn't drift; negative is used by
    /// the override path to encode "unknown model, conservative
    /// guess" sentinel observations (the override file may flip
    /// the sign — see the override docs).
    pub cost_cents: i64,
    /// Wall-clock latency of the call.
    pub latency_ms: u64,
    /// Unix milliseconds at the moment the call *started*. Records
    /// are sortable by `started_at_ms` even when [`recorded_at_ms`]
    /// drifts across hosts. Note: also serialised to SQLite as an
    /// ISO-8601 string (`started_at` column) so the table is
    /// human-greppable.
    pub started_at_ms: u64,
    /// Optional agent id from the runtime, set when the call ran
    /// inside an `Agent.handler`. `None` for top-level
    /// `Member.ask`/REPL calls.
    pub agent_id: Option<u64>,
    /// Sub-observations for tool-use turns. v0.30 fills in
    /// `(name, latency_ms)` only; tool I/O bytes land in v0.31.
    pub tool_calls: Vec<ToolCallObservation>,
    /// Optional free-form error code when the call failed (e.g.
    /// `"rate_limit"`, `"budget_exhausted"`). `None` for the
    /// success path. Failed calls still record observations so
    /// "why did this 429" dashboards work.
    pub error_kind: Option<String>,
}

/// One tool invocation inside a single LLM turn.
///
/// v0.30 is the minimal shape — name + duration — enough for the
/// "which tool dominates the budget" rollup. v0.31 adds bytes-in /
/// bytes-out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallObservation {
    pub name: String,
    pub latency_ms: u64,
    /// `true` if the model called the tool but the tool errored.
    /// Distinguished from a model-side parse failure (which is an
    /// `error_kind` on the parent [`LlmObservation`]).
    pub failed: bool,
}

impl LlmObservation {
    /// New observation with the canonical "now" timestamp + an
    /// auto-computed `cost_cents` from the default pricing table.
    /// Tests + provider hooks build directly via this helper instead
    /// of memorising the struct's field order.
    pub fn new(
        provider: impl Into<String>,
        model: impl Into<String>,
        prompt_tokens: u64,
        completion_tokens: u64,
        latency_ms: u64,
    ) -> Self {
        let model = model.into();
        let cost_cents =
            crate::observe::pricing::cost_cents_for(&model, prompt_tokens, completion_tokens, None);
        Self {
            provider: provider.into(),
            model,
            prompt_tokens,
            completion_tokens,
            cost_cents,
            latency_ms,
            started_at_ms: now_ms().saturating_sub(latency_ms),
            agent_id: None,
            tool_calls: Vec::new(),
            error_kind: None,
        }
    }

    /// Builder-style: attach an agent id.
    #[must_use]
    pub fn with_agent_id(mut self, id: u64) -> Self {
        self.agent_id = Some(id);
        self
    }

    /// Builder-style: attach an error kind (the call failed but
    /// usage was still billed, e.g. for partial streams).
    #[must_use]
    pub fn with_error_kind(mut self, k: impl Into<String>) -> Self {
        self.error_kind = Some(k.into());
        self
    }

    /// Builder-style: attach tool-call observations.
    #[must_use]
    pub fn with_tool_calls(mut self, calls: Vec<ToolCallObservation>) -> Self {
        self.tool_calls = calls;
        self
    }

    /// Pin the start time explicitly. Useful when the caller already
    /// captured an `Instant` and wants `started_at_ms = end - latency`.
    #[must_use]
    pub fn with_started_at_ms(mut self, ms: u64) -> Self {
        self.started_at_ms = ms;
        self
    }
}

/// Wall-clock unix milliseconds. Returns 0 on the (in-test) shim
/// where `SystemTime::now()` is somehow before `UNIX_EPOCH` — we
/// pick the safe "0" rather than panic because observability code
/// must NEVER break the user's program.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_observation_auto_costs_known_model() {
        // 1M opus input tokens = $15 = 1500 cents. We pass 100k input,
        // expect 150 cents (rounding to nearest integer cent).
        let o = LlmObservation::new("anthropic", "claude-opus-4-7", 100_000, 0, 50);
        assert_eq!(o.cost_cents, 150);
        assert_eq!(o.prompt_tokens, 100_000);
        assert_eq!(o.latency_ms, 50);
    }

    #[test]
    fn builders_chain() {
        let o = LlmObservation::new("openai", "gpt-5", 10, 5, 1)
            .with_agent_id(42)
            .with_error_kind("rate_limit")
            .with_tool_calls(vec![ToolCallObservation {
                name: "search".into(),
                latency_ms: 9,
                failed: false,
            }]);
        assert_eq!(o.agent_id, Some(42));
        assert_eq!(o.error_kind.as_deref(), Some("rate_limit"));
        assert_eq!(o.tool_calls.len(), 1);
    }

    #[test]
    fn started_at_defaults_to_now_minus_latency() {
        let before = now_ms();
        let o = LlmObservation::new("openai", "gpt-5", 0, 0, 100);
        let after = now_ms();
        // started_at_ms should be in the window [before-100, after-100+epsilon].
        assert!(o.started_at_ms + 100 >= before);
        assert!(o.started_at_ms <= after);
    }

    #[test]
    fn round_trips_through_json() {
        let o = LlmObservation::new("gemini", "gemini-2.5-pro", 1000, 2000, 33).with_agent_id(7);
        let s = serde_json::to_string(&o).unwrap();
        let back: LlmObservation = serde_json::from_str(&s).unwrap();
        assert_eq!(o, back);
    }
}
