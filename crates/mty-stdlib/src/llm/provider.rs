//! Provider-trait + [`CompletionRequest`] shared by every backend.
//!
//! Pulled out of `mod.rs` so the trait can sit next to the request
//! shape it operates on (helps grepping; the file stays small).
//!
//! The trait is intentionally *async*-flavoured with `async_trait`
//! rather than GAT-based-`Future` because:
//!
//! - Every provider already returns a `Future`-typed `complete()`;
//!   the surface gain from an associated `Future` type doesn't pay
//!   for the readability cost.
//! - Tracks B (`@tool` macro) + C (memory backends) need to write
//!   `dyn LlmProvider` for trait-object dispatch in agent harnesses.
//!   `async_trait` is the obvious path; GATs don't trait-object yet.
//!
//! `MessageStream` is the typed stream, *not* the trait's associated
//! type, again because trait-object friendliness matters more than
//! a single allocation per streaming response.

use serde::{Deserialize, Serialize};

use crate::llm::budget::{DollarBudget, TokenBudget};
use crate::llm::error::LlmError;
use crate::llm::message::Message;
use crate::llm::streaming::MessageStream;
use crate::llm::tools::{Tool, ToolChoice};

/// One completion request.
///
/// All fields are public — providers serialise their wire shape from
/// this struct; tests and helpers in `tests/` build it directly.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompletionRequest {
    /// Provider-specific model name. Anthropic: `claude-opus-4-7`.
    /// OpenAI: `gpt-5`. Gemini: `gemini-2.5-pro`. Bedrock: an
    /// inference profile ARN OR the canonical model id.
    pub model: String,

    /// System prompt (top-of-conversation instructions). For
    /// Anthropic this becomes the top-level `system` field; for
    /// OpenAI it becomes the first `developer`-role message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,

    /// Conversation history. The current user turn is the last entry.
    pub messages: Vec<Message>,

    /// Tools the model is allowed to call this turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Tool>,

    /// How tools may be invoked. See [`ToolChoice`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,

    /// Upper bound on output tokens. Anthropic requires this; OpenAI +
    /// Gemini default it; we default-fill to 1024 at the provider
    /// boundary if unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Sampling temperature. `None` -> provider default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Whether this is a streaming request. `complete()` ignores this
    /// (it always returns a fully-materialised `Message`);
    /// `complete_stream()` forces it true.
    #[serde(default, skip_serializing_if = "is_false")]
    pub stream: bool,

    /// Typed token budget shared across one agent invocation. Not
    /// serialised — it's a runtime-side handle.
    #[serde(skip)]
    pub token_budget: Option<TokenBudget>,

    /// Typed dollar budget shared across one agent invocation. Not
    /// serialised.
    #[serde(skip)]
    pub dollar_budget: Option<DollarBudget>,
}

// Serde requires a `&T -> bool` signature for `skip_serializing_if`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !b
}

impl CompletionRequest {
    /// New request with `model` + conversation `messages`. Use the
    /// `with_*` builders to set tools / system prompt / budgets.
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    #[must_use]
    pub fn with_tools(mut self, tools: Vec<Tool>) -> Self {
        self.tools = tools;
        self
    }

    #[must_use]
    pub fn with_max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = Some(n);
        self
    }

    #[must_use]
    pub fn with_token_budget(mut self, b: TokenBudget) -> Self {
        self.token_budget = Some(b);
        self
    }

    #[must_use]
    pub fn with_dollar_budget(mut self, b: DollarBudget) -> Self {
        self.dollar_budget = Some(b);
        self
    }
}

/// The provider abstraction every `std.llm.<vendor>` backend
/// implements.
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    /// One-shot completion — runs the request to completion and
    /// returns the assembled [`Message`]. The default implementation
    /// of `complete_stream` (provided by `MessageStream::from_vec`)
    /// is *not* derived from this; each provider implements both so
    /// the streaming path can hit the upstream's SSE endpoint
    /// directly.
    async fn complete(&self, req: CompletionRequest) -> Result<Message, LlmError>;

    /// Streaming completion — returns a [`MessageStream`] of typed
    /// deltas. Providers that haven't wired streaming yet (v0.26
    /// skeletons) return an empty stream; their `complete()` path is
    /// still functional.
    async fn complete_stream(&self, req: CompletionRequest) -> Result<MessageStream, LlmError>;

    /// Provider-specific serialisation of a [`Tool`] — e.g. Anthropic
    /// wraps it as `{name, description, input_schema}` while OpenAI
    /// nests it under `{type:"function", function:{...}}`.
    fn schema_for_tool(&self, tool: &Tool) -> serde_json::Value;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_round_trips_field_set() {
        let r = CompletionRequest::new("m", vec![])
            .with_system("be helpful")
            .with_max_tokens(99)
            .with_tools(vec![Tool::no_args("noop", "noop")]);
        assert_eq!(r.model, "m");
        assert_eq!(r.system.as_deref(), Some("be helpful"));
        assert_eq!(r.max_tokens, Some(99));
        assert_eq!(r.tools.len(), 1);
    }

    #[test]
    fn default_request_omits_optional_fields_in_json() {
        let r = CompletionRequest::new("m", vec![]);
        let j = serde_json::to_value(&r).unwrap();
        assert!(j.get("system").is_none());
        assert!(j.get("tools").is_none());
        assert!(j.get("max_tokens").is_none());
        assert!(j.get("stream").is_none());
    }
}
