//! Panel member abstraction — one LLM that participates in a swarm.
//!
//! A [`Member`] wraps an `std.llm` client (Anthropic / OpenAI / Gemini
//! / Bedrock) together with the canonical model name to run on it.
//! The variant is the right shape to grow over time: when v0.28 adds a
//! local-inference backend (Ollama, llama.cpp), it slots in as another
//! [`Member`] variant without disturbing the rest of the swarm surface.
//!
//! ## Why a tagged enum, not `Box<dyn LlmProvider>`
//!
//! The swarm dispatches every member's [`Member::ask`] in parallel via
//! `futures::future::join_all`. With a trait-object enum we can
//! `match` on the variant to keep the per-provider error mapping (e.g.
//! Anthropic's rate-limit surfaces as `LlmError::RateLimit` while
//! Bedrock's surfaces under `LlmError::Provider`) without losing the
//! provider-specific weight tables in [`crate::swarm::consensus`].
//! `dyn LlmProvider` would force every member through one path; the
//! enum keeps the swarm decision-loop legible.
//!
//! ## Mock for tests
//!
//! Tests don't carry an `ANTHROPIC_API_KEY`. The [`Member::mock`]
//! constructor returns a deterministic stand-in that replies with a
//! pre-canned body + cost. The variant is opt-in via the `mock`
//! constructor; production callers never accidentally land on it.

use std::sync::Arc;
use std::sync::Mutex;

use crate::llm::anthropic::AnthropicClient;
use crate::llm::bedrock::BedrockClient;
use crate::llm::error::LlmError;
use crate::llm::gemini::GeminiClient;
use crate::llm::message::{Message, ToolUse};
use crate::llm::openai::OpenAiClient;
use crate::llm::provider::{CompletionRequest, LlmProvider};
use crate::swarm::budget::SharedDollarBudget;

/// One participant on the swarm panel.
///
/// Use the named constructors ([`Member::anthropic`], etc.) rather
/// than the raw variants — they pull credentials from the canonical
/// env var per provider and surface a clear [`LlmError::Auth`] when
/// the key is missing.
#[derive(Clone)]
pub enum Member {
    Anthropic {
        client: AnthropicClient,
        model: String,
    },
    OpenAi {
        client: OpenAiClient,
        model: String,
    },
    Gemini {
        client: GeminiClient,
        model: String,
    },
    Bedrock {
        client: BedrockClient,
        model: String,
    },
    /// Deterministic stand-in for tests. Carries the canned reply +
    /// per-call cost so swarm tests can drive every code path
    /// (majority, dissent, budget-trip, ...) without touching a real
    /// provider.
    Mock(MockMember),
}

/// Mock member shape — only used by tests, but lives in the public
/// surface so integration-test files can construct it.
#[derive(Clone)]
pub struct MockMember {
    /// Stable name surfaced in [`MemberReply::member`] + dissent
    /// records. Must be unique within the panel.
    pub name: String,
    /// Canned body the mock returns from [`Member::ask`].
    pub reply_body: String,
    /// Pretend input tokens. Drives budget accounting + the surfaced
    /// `tokens_used` total.
    pub input_tokens: u64,
    /// Pretend output tokens.
    pub output_tokens: u64,
    /// Pretend cost (cents). When `None`, the mock derives a cost
    /// from `output_tokens * 1 cent / 1k tokens` so tests can drive
    /// the budget path implicitly. When `Some`, this is the exact
    /// per-call cost in cents.
    pub forced_cost_cents: Option<u64>,
    /// Optional error to surface instead of a reply. Lets tests
    /// drive the "one member errored, others agreed" path.
    pub forced_error: Option<String>,
    /// v0.32 Track F: canned tool-use blocks the mock surfaces on
    /// [`MemberReply::tool_uses`]. Lets eval + replay tests drive
    /// `Compare::tool_call_set_equal` and the structural-recorder
    /// path without spinning a real LLM provider.
    pub forced_tool_uses: Vec<ToolUse>,
    /// Counter shared across clones so tests can assert how many
    /// times this member was actually asked (i.e. that the
    /// `FirstAgreed` strategy short-circuited the rest).
    pub call_count: Arc<Mutex<u32>>,
}

impl std::fmt::Debug for Member {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Member::Anthropic { model, .. } => write!(f, "Member::Anthropic({model})"),
            Member::OpenAi { model, .. } => write!(f, "Member::OpenAi({model})"),
            Member::Gemini { model, .. } => write!(f, "Member::Gemini({model})"),
            Member::Bedrock { model, .. } => write!(f, "Member::Bedrock({model})"),
            Member::Mock(m) => write!(f, "Member::Mock({})", m.name),
        }
    }
}

impl Member {
    /// New Anthropic member. Reads `ANTHROPIC_API_KEY` from the env;
    /// panics if it's missing — swarms are an explicit choice and the
    /// "silent fall-back to no member" failure mode is worse than the
    /// loud error at construction time.
    ///
    /// Use [`Member::anthropic_with_client`] when you've already
    /// built a client (typically with `with_base_url(...)` for tests).
    pub fn anthropic(model: &str) -> Self {
        let client = AnthropicClient::from_env()
            .unwrap_or_else(|e| panic!("Member::anthropic: {e} — set ANTHROPIC_API_KEY"));
        Self::Anthropic {
            client,
            model: model.into(),
        }
    }

    pub fn anthropic_with_client(client: AnthropicClient, model: impl Into<String>) -> Self {
        Self::Anthropic {
            client,
            model: model.into(),
        }
    }

    /// New OpenAI member. Reads `OPENAI_API_KEY`. Today the OpenAI
    /// client is a v0.26 stub that returns a placeholder text body —
    /// fine for shaping the swarm primitive; the real wire-up moves
    /// to v0.28 alongside Track C's full OpenAI rollout.
    pub fn openai(model: &str) -> Self {
        let client = OpenAiClient::from_env()
            .unwrap_or_else(|e| panic!("Member::openai: {e} — set OPENAI_API_KEY"));
        Self::OpenAi {
            client,
            model: model.into(),
        }
    }

    pub fn openai_with_client(client: OpenAiClient, model: impl Into<String>) -> Self {
        Self::OpenAi {
            client,
            model: model.into(),
        }
    }

    pub fn gemini(model: &str) -> Self {
        let client = GeminiClient::from_env().unwrap_or_else(|e| {
            panic!("Member::gemini: {e} — set GEMINI_API_KEY or GOOGLE_API_KEY")
        });
        Self::Gemini {
            client,
            model: model.into(),
        }
    }

    pub fn gemini_with_client(client: GeminiClient, model: impl Into<String>) -> Self {
        Self::Gemini {
            client,
            model: model.into(),
        }
    }

    pub fn bedrock(model: &str) -> Self {
        let client = BedrockClient::from_env()
            .unwrap_or_else(|e| panic!("Member::bedrock: {e} — set AWS_BEDROCK_API_TOKEN"));
        Self::Bedrock {
            client,
            model: model.into(),
        }
    }

    pub fn bedrock_with_client(client: BedrockClient, model: impl Into<String>) -> Self {
        Self::Bedrock {
            client,
            model: model.into(),
        }
    }

    /// Build a mock member from a canned reply + per-call cost. Used
    /// exclusively by tests; the `mock` constructor is public so
    /// `tests/swarm_*.rs` can construct it.
    pub fn mock(name: impl Into<String>, reply_body: impl Into<String>, cost_cents: u64) -> Self {
        Self::Mock(MockMember {
            name: name.into(),
            reply_body: reply_body.into(),
            input_tokens: 10,
            output_tokens: 20,
            forced_cost_cents: Some(cost_cents),
            forced_error: None,
            forced_tool_uses: Vec::new(),
            call_count: Arc::new(Mutex::new(0)),
        })
    }

    /// Mock that surfaces an error from [`ask`]. The error string is
    /// embedded in `LlmError::Provider { status: 500, body: ... }`.
    pub fn mock_error(name: impl Into<String>, body: impl Into<String>) -> Self {
        Self::Mock(MockMember {
            name: name.into(),
            reply_body: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            forced_cost_cents: Some(0),
            forced_error: Some(body.into()),
            forced_tool_uses: Vec::new(),
            call_count: Arc::new(Mutex::new(0)),
        })
    }

    /// v0.32 Track F: build a mock that surfaces structured tool-use
    /// blocks alongside its reply text. Mirrors [`Member::mock`] but
    /// pre-seeds [`MockMember::forced_tool_uses`] so eval tests can
    /// drive `Compare::tool_call_set_equal` against a deterministic
    /// shape.
    pub fn mock_with_tool_uses(
        name: impl Into<String>,
        reply_body: impl Into<String>,
        cost_cents: u64,
        tool_uses: Vec<ToolUse>,
    ) -> Self {
        Self::Mock(MockMember {
            name: name.into(),
            reply_body: reply_body.into(),
            input_tokens: 10,
            output_tokens: 20,
            forced_cost_cents: Some(cost_cents),
            forced_error: None,
            forced_tool_uses: tool_uses,
            call_count: Arc::new(Mutex::new(0)),
        })
    }

    /// Stable, human-readable label. The swarm decision-loop uses this
    /// to attribute dissents and to dedupe members within a panel.
    pub fn label(&self) -> String {
        match self {
            Member::Anthropic { model, .. } => format!("anthropic:{model}"),
            Member::OpenAi { model, .. } => format!("openai:{model}"),
            Member::Gemini { model, .. } => format!("gemini:{model}"),
            Member::Bedrock { model, .. } => format!("bedrock:{model}"),
            Member::Mock(m) => m.name.clone(),
        }
    }

    /// Model name the member will run on (raw; not prefixed with the
    /// provider).
    pub fn model(&self) -> &str {
        match self {
            Member::Anthropic { model, .. }
            | Member::OpenAi { model, .. }
            | Member::Gemini { model, .. }
            | Member::Bedrock { model, .. } => model,
            Member::Mock(m) => &m.name,
        }
    }

    /// Send `prompt` to this member, deducting cost against `budget`.
    ///
    /// Returns [`MemberReply`] with the body + per-call cost on
    /// success. If `budget` is already exhausted on entry, returns
    /// `LlmError::BudgetExhausted` *without* dispatching the request.
    pub async fn ask(
        &self,
        prompt: &str,
        budget: &SharedDollarBudget,
    ) -> Result<MemberReply, LlmError> {
        // Short-circuit before the upstream call so we don't waste
        // tokens against a tripped budget.
        if budget.is_exhausted() {
            return Err(LlmError::BudgetExhausted(
                crate::llm::error::BudgetExhausted::dollars(
                    budget.limit_cents(),
                    budget.consumed_cents(),
                ),
            ));
        }

        if let Member::Mock(m) = self {
            // Bump the call counter under the lock so tests can
            // assert short-circuit behaviour.
            *m.call_count.lock().expect("mock call_count poisoned") += 1;
            if let Some(body) = &m.forced_error {
                return Err(LlmError::Provider {
                    status: 500,
                    body: body.clone(),
                });
            }
            let cost = m
                .forced_cost_cents
                .unwrap_or_else(|| (m.output_tokens / 1000).max(1));
            // Charge the shared budget *before* returning so a panel
            // that overshoots mid-flight surfaces `budget_exhausted:
            // true` on the consensus.
            let _ = budget.try_charge(cost);
            let reply = MemberReply {
                member: m.name.clone(),
                body: m.reply_body.clone(),
                tokens_used: (m.input_tokens + m.output_tokens) as u32,
                cost_cents: cost,
                tool_uses: m.forced_tool_uses.clone(),
            };
            // v0.32 Track F: surface mock turns to the global trace
            // recorder when `MTY_RECORD_TRACE` is set so eval +
            // replay tests that drive mocks see the same recording
            // shape they'd get from the real providers.
            record_member_turn(self, prompt, &reply);
            return Ok(reply);
        }

        let req = CompletionRequest::new(self.model(), vec![Message::user_text(prompt)]);

        // Per-provider dispatch — every real backend exposes a
        // `complete()` through the `LlmProvider` trait.
        let reply: Message = match self {
            Member::Anthropic { client, .. } => client.complete(req).await?,
            Member::OpenAi { client, .. } => client.complete(req).await?,
            Member::Gemini { client, .. } => client.complete(req).await?,
            Member::Bedrock { client, .. } => client.complete(req).await?,
            Member::Mock(_) => unreachable!("handled above"),
        };

        // The real provider clients don't surface per-call token
        // counts in their typed `Message` — they push usage into the
        // `DollarBudget` on the `CompletionRequest`. The swarm primitive
        // owns a *shared* budget, not a per-member one, so we charge
        // a conservative estimate here. v0.28's full provider rollout
        // will plumb the actual usage through.
        let approx_tokens = reply.text().len() as u64 / 4 + 1;
        let cost = approx_token_cost_cents(self.model(), approx_tokens);
        let _ = budget.try_charge(cost);

        // v0.32 Track F: lift every tool-use block the provider
        // emitted onto `MemberReply.tool_uses` so eval comparators
        // and the structural recorder can see them.
        let tool_uses: Vec<ToolUse> = reply.tool_uses().into_iter().cloned().collect();
        let body = reply.text();

        let member_reply = MemberReply {
            member: self.label(),
            body,
            tokens_used: approx_tokens as u32,
            cost_cents: cost,
            tool_uses,
        };
        record_member_turn(self, prompt, &member_reply);
        Ok(member_reply)
    }
}

/// One member's response to a prompt. Carries the `member` label
/// (typically `<provider>:<model>`) so dissents can be attributed.
#[derive(Debug, Clone)]
pub struct MemberReply {
    /// `<provider>:<model>` label, or the mock's `name`.
    pub member: String,
    /// The model's reply text. For free-form prompts this is the full
    /// body; for yes/no the consensus layer trims + lower-cases it.
    pub body: String,
    /// Approximate input + output tokens for this single call. v0.27
    /// uses a `len/4` estimate; v0.28's real-provider rollout will
    /// surface the actual usage counts.
    pub tokens_used: u32,
    /// Per-call cost in cents (always integer to avoid fractional-cent
    /// drift on a shared budget).
    pub cost_cents: u64,
    /// v0.32 Track F: tool-use blocks the assistant emitted in this
    /// turn. Lifted verbatim from the provider's typed
    /// [`Message::tool_uses`]; mirrors the wire-v3
    /// [`mty_runtime::replay::LlmToolUse`] shape so eval comparators
    /// + the structural recorder can hand them through unchanged.
    ///
    /// Empty `Vec` when:
    /// * the model returned no tool uses (the most common case),
    /// * the member is a [`Member::Mock`] without `forced_tool_uses` set,
    /// * a test or older caller constructed `MemberReply` via struct
    ///   literal (the field defaults to `Vec::new()` via the typed
    ///   constructor below).
    #[doc(alias = "tool_calls")]
    pub tool_uses: Vec<ToolUse>,
}

impl MemberReply {
    /// Dollar cost as a `f64` for ergonomic logging. The integer
    /// `cost_cents` remains the source of truth.
    pub fn cost(&self) -> f64 {
        self.cost_cents as f64 / 100.0
    }

    /// v0.32 Track F: borrowed accessor for the structured tool-use
    /// blocks the assistant emitted on this turn. Returns an empty
    /// slice when the reply carried no tool uses.
    pub fn tool_uses(&self) -> &[ToolUse] {
        &self.tool_uses
    }

    /// Convenience: collect the names of every tool the assistant
    /// invoked, in order. Used by `Compare::ToolCallSetEqual` +
    /// the divergence reporter to render `tool: search_web, calc`
    /// in human-readable diffs.
    pub fn tool_names(&self) -> Vec<String> {
        self.tool_uses.iter().map(|t| t.name.clone()).collect()
    }
}

/// Estimate per-call cost from a token count. Used when the upstream
/// provider doesn't surface a `usage` field (every v0.26 skeleton —
/// OpenAI/Gemini/Bedrock — falls through here).
///
/// Pricing matches `crate::llm::budget::default_pricing_cents_per_million`
/// shape but applies a single combined rate (input ~= output for the
/// estimate) since the swarm doesn't know the split.
fn approx_token_cost_cents(model: &str, total_tokens: u64) -> u64 {
    let (in_rate, out_rate) = crate::llm::budget::default_pricing_cents_per_million(model);
    // Blend 50/50 between input/output rates — good enough for the
    // estimate; callers who care use `DollarBudget::with_pricing` on
    // the underlying client.
    let avg = u64::midpoint(in_rate, out_rate);
    total_tokens.saturating_mul(avg) / 1_000_000
}

/// v0.32 Track F: record one `Member::ask` turn into the global trace
/// recorder when `MTY_RECORD_TRACE` is set.
///
/// The hook is **zero-overhead** when no recorder is installed —
/// [`mty_runtime::replay::recording_enabled`] takes a single
/// `RwLock::read` + `Option::is_none` check before we walk the typed
/// reply. When a recorder *is* installed, every call to
/// [`Member::ask`] surfaces one [`mty_runtime::replay::TraceEvent::LlmCall`]
/// event with the prompt + reply + tool_uses + cost so `std.eval` can
/// replay the recording structurally against a fresh provider.
///
/// `agent` is the synthetic `0` ("CLI / driver" id) because
/// `Member::ask` doesn't know which agent (if any) invoked it. v0.33
/// can plumb the spawning agent's id through if user code asks for
/// it.
fn record_member_turn(member: &Member, prompt: &str, reply: &MemberReply) {
    use mty_runtime::replay::{recording_enabled, with_recorder, LlmToolUse};

    if !recording_enabled() {
        return;
    }

    // Convert our typed `ToolUse` shapes into the wire-v3 record.
    let tool_uses: Vec<LlmToolUse> = reply
        .tool_uses
        .iter()
        .map(|t| LlmToolUse {
            name: t.name.clone(),
            id: t.id.clone(),
            input_json: serde_json::to_string(&t.input).unwrap_or_else(|_| "{}".to_string()),
        })
        .collect();

    let model = member.model().to_string();
    with_recorder(|rec| {
        rec.record_llm_call(
            0,
            None,
            prompt,
            None,
            // Tool list (the *advertised* tools, not the called ones);
            // `Member::ask` today only ships a single-prompt request
            // with no tools, so we emit empty. v0.33 lifts the
            // configured-tools list through `Member::ask` so this
            // mirrors the request shape exactly.
            vec![model.clone()],
            &reply.body,
            tool_uses.clone(),
            reply.cost_cents,
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_ask_charges_budget_and_returns_canned_body() {
        let m = Member::mock("alpha", "yes", 5);
        let b = SharedDollarBudget::new(100);
        let r = m.ask("anything", &b).await.unwrap();
        assert_eq!(r.body, "yes");
        assert_eq!(r.cost_cents, 5);
        assert_eq!(b.consumed_cents(), 5);
    }

    #[tokio::test]
    async fn mock_ask_short_circuits_when_budget_exhausted() {
        let m = Member::mock("alpha", "yes", 5);
        let b = SharedDollarBudget::new(10);
        // Manually push the budget over the line.
        let _ = b.try_charge(20);
        let err = m.ask("anything", &b).await.err().unwrap();
        assert!(matches!(err, LlmError::BudgetExhausted(_)));
        // The mock's call counter stayed at zero — we never dispatched.
        if let Member::Mock(inner) = &m {
            assert_eq!(*inner.call_count.lock().unwrap(), 0);
        }
    }

    #[tokio::test]
    async fn mock_error_surfaces_as_provider_500() {
        let m = Member::mock_error("alpha", "kaboom");
        let b = SharedDollarBudget::new(100);
        let err = m.ask("anything", &b).await.err().unwrap();
        match err {
            LlmError::Provider { status, body } => {
                assert_eq!(status, 500);
                assert!(body.contains("kaboom"));
            }
            _ => panic!("expected provider error"),
        }
    }

    #[test]
    fn label_includes_provider_and_model() {
        let c = AnthropicClient::with_api_key("k");
        let m = Member::anthropic_with_client(c, "claude-opus-4-7");
        assert_eq!(m.label(), "anthropic:claude-opus-4-7");
    }

    #[test]
    fn approx_token_cost_known_model_nonzero() {
        let c = approx_token_cost_cents("claude-opus-4-7", 1_000_000);
        // (1500 + 7500) / 2 = 4500 cents/M tokens
        assert_eq!(c, 4500);
    }

    // -------------------------------------------------------------------------
    // v0.32 Track F: structural tool_uses on MemberReply + recorder
    // integration for Member::ask.

    #[tokio::test]
    async fn mock_with_tool_uses_surfaces_them_on_member_reply() {
        let canned = vec![ToolUse {
            id: "tu_01".into(),
            name: "search_web".into(),
            input: serde_json::json!({"q": "rust"}),
        }];
        let m = Member::mock_with_tool_uses("alpha", "let me search", 3, canned.clone());
        let b = SharedDollarBudget::new(100);
        let r = m.ask("anything", &b).await.unwrap();
        assert_eq!(r.body, "let me search");
        assert_eq!(r.tool_uses.len(), 1);
        assert_eq!(r.tool_uses[0].name, "search_web");
        assert_eq!(r.tool_uses[0].id, "tu_01");
        // Backward-compat: the named accessor returns the same slice.
        assert_eq!(r.tool_uses().len(), 1);
        assert_eq!(r.tool_names(), vec!["search_web".to_string()]);
    }

    #[tokio::test]
    async fn member_reply_tool_uses_defaults_to_empty_for_plain_mock() {
        let m = Member::mock("alpha", "yes", 1);
        let b = SharedDollarBudget::new(100);
        let r = m.ask("anything", &b).await.unwrap();
        assert!(r.tool_uses.is_empty());
        assert!(r.tool_names().is_empty());
    }

    // Tests below serialise on the process-wide recorder slot to
    // avoid racing with each other when run in parallel — the
    // recorder is a global. `tokio::sync::Mutex` is the async-aware
    // mutex (its guard is fine across `await`s under clippy's
    // `await_holding_lock` lint); fine for tests because the
    // critical section is short.
    fn recorder_lock() -> &'static tokio::sync::Mutex<()> {
        use std::sync::OnceLock;
        static M: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        M.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn member_ask_records_turn_into_global_recorder_when_installed() {
        use mty_runtime::replay::{install, uninstall, Recorder};
        use std::sync::Arc;

        let _g = recorder_lock().lock().await;
        let _ = uninstall();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rec.mty-trace");
        let rec = Arc::new(Recorder::new(&path, 0, 1));
        install(rec.clone());

        let m = Member::mock("alpha", "hello world", 2);
        let b = SharedDollarBudget::new(100);
        let prompt = "member-recorder-unique-greet?";
        let _ = m.ask(prompt, &b).await.unwrap();

        // Confirm our LlmCall event landed in the recorder buffer. The
        // recorder is process-wide, so unrelated parallel tests may also
        // record turns while this recorder is installed.
        let events = rec.events_snapshot();
        let found = events.iter().any(|e| {
            matches!(
                e,
                mty_runtime::replay::TraceEvent::LlmCall {
                    prompt: p,
                    reply,
                    cost_cents,
                    ..
                } if p == prompt && reply == "hello world" && *cost_cents == 2
            )
        });
        assert!(
            found,
            "expected the unique greet LlmCall in the recorder buffer"
        );

        let _ = uninstall();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn member_ask_records_no_event_when_recorder_uninstalled() {
        use mty_runtime::replay::{recording_enabled, uninstall};
        let _g = recorder_lock().lock().await;
        let _ = uninstall();
        assert!(!recording_enabled());

        let m = Member::mock("alpha", "hello world", 2);
        let b = SharedDollarBudget::new(100);
        let _ = m.ask("greet?", &b).await.unwrap();
        // No recorder → no event. Just confirm `recording_enabled`
        // remains false (i.e. ask() didn't sneak-install one).
        assert!(!recording_enabled());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn member_ask_recorder_carries_structural_tool_uses() {
        use mty_runtime::replay::{install, uninstall, Recorder, TraceEvent};
        use std::sync::Arc;

        let _g = recorder_lock().lock().await;
        let _ = uninstall();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rec_tu.mty-trace");
        let rec = Arc::new(Recorder::new(&path, 0, 1));
        install(rec.clone());

        let canned = vec![ToolUse {
            id: "tu_xx".into(),
            name: "calc".into(),
            input: serde_json::json!({"x": 7}),
        }];
        let m = Member::mock_with_tool_uses("alpha", "computing", 1, canned);
        let b = SharedDollarBudget::new(100);
        let prompt = "member-recorder-unique-tool-use?";
        let _ = m.ask(prompt, &b).await.unwrap();

        let events = rec.events_snapshot();
        let mut found = false;
        for ev in &events {
            if let TraceEvent::LlmCall {
                prompt,
                reply,
                tool_uses,
                cost_cents,
                ..
            } = ev
            {
                if prompt != "member-recorder-unique-tool-use?" {
                    continue;
                }
                assert_eq!(prompt, "member-recorder-unique-tool-use?");
                assert_eq!(reply, "computing");
                assert_eq!(tool_uses.len(), 1);
                assert_eq!(tool_uses[0].name, "calc");
                assert_eq!(tool_uses[0].id, "tu_xx");
                // input is stored as JSON-string in the wire shape.
                assert!(tool_uses[0].input_json.contains("\"x\""));
                assert_eq!(*cost_cents, 1);
                found = true;
            }
        }
        assert!(found, "expected an LlmCall event in the recorder buffer");
        let _ = uninstall();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn member_ask_records_multi_turn_in_buffer_order() {
        use mty_runtime::replay::{install, uninstall, Recorder, TraceEvent};
        use std::sync::Arc;

        let _g = recorder_lock().lock().await;
        let _ = uninstall();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rec_multi.mty-trace");
        let rec = Arc::new(Recorder::new(&path, 0, 1));
        install(rec.clone());

        let m = Member::mock("alpha", "reply-a", 1);
        let b = SharedDollarBudget::new(100);
        let _ = m.ask("member-recorder-unique-q1", &b).await.unwrap();
        let _ = m.ask("member-recorder-unique-q2", &b).await.unwrap();
        let _ = m.ask("member-recorder-unique-q3", &b).await.unwrap();

        let events = rec.events_snapshot();
        let prompts: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                TraceEvent::LlmCall { prompt, .. } => Some(prompt.clone()),
                _ => None,
            })
            .filter(|prompt| prompt.starts_with("member-recorder-unique-q"))
            .collect();
        assert_eq!(
            prompts,
            vec![
                "member-recorder-unique-q1",
                "member-recorder-unique-q2",
                "member-recorder-unique-q3"
            ]
        );
        let _ = uninstall();
    }
}
