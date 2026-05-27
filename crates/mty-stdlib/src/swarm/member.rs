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
use crate::llm::message::Message;
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
            return Ok(MemberReply {
                member: m.name.clone(),
                body: m.reply_body.clone(),
                tokens_used: (m.input_tokens + m.output_tokens) as u32,
                cost_cents: cost,
            });
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

        Ok(MemberReply {
            member: self.label(),
            body: reply.text(),
            tokens_used: approx_tokens as u32,
            cost_cents: cost,
        })
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
}

impl MemberReply {
    /// Dollar cost as a `f64` for ergonomic logging. The integer
    /// `cost_cents` remains the source of truth.
    pub fn cost(&self) -> f64 {
        self.cost_cents as f64 / 100.0
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
}
