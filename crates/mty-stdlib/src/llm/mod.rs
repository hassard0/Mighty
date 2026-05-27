//! `std.llm` — typed LLM provider abstraction.
//!
//! v0.26 Track A shipped Anthropic full + three skeletons. v0.27
//! Track C promotes the three skeletons to full implementations.
//! One trait surface ([`LlmProvider`](provider::LlmProvider)), four
//! shipping implementations:
//!
//! | Provider | Status | Module |
//! |---|---|---|
//! | Anthropic Messages | **full** (HTTP + streaming + tool-use + budgets) | [`anthropic`] |
//! | OpenAI Responses | **full** (HTTP + SSE streaming + tools + budgets) | [`openai`] |
//! | Google Gemini `generateContent` | **full** (HTTP + `alt=sse` streaming + tools + safety) | [`gemini`] |
//! | AWS Bedrock Converse | **full** (SigV4 + ConverseStream event-stream + tools + budgets) | [`bedrock`] |
//!
//! Mighty source consumes this module via the permissive method
//! table (`std.llm.anthropic.messages(...)`); the typed shapes
//! ([`message::Message`], [`message::ContentBlock`],
//! [`tools::Tool`]) serialize directly to the providers' wire
//! formats. The `model` effect is registered in
//! `mty_types::prelude::build_prelude` so Mighty source can write
//! `effect {net, model}` on the call site.
//!
//! ## Quickstart (Rust)
//!
//! ```no_run
//! use mty_stdlib::llm::{
//!     anthropic::AnthropicClient,
//!     provider::{CompletionRequest, LlmProvider},
//!     message::Message,
//! };
//!
//! # async fn run() -> Result<(), mty_stdlib::llm::error::LlmError> {
//! let client = AnthropicClient::from_env()?;
//! let req = CompletionRequest::new(
//!     "claude-opus-4-7",
//!     vec![Message::user_text("hi")],
//! )
//! .with_system("You are a careful code reviewer.")
//! .with_max_tokens(1024);
//! let reply = client.complete(req).await?;
//! println!("{}", reply.text());
//! # Ok(()) }
//! ```
//!
//! ## Quickstart (Mighty)
//!
//! ```mty
//! use std.llm
//!
//! let reply = anthropic.messages(
//!   model: "claude-opus-4-7",
//!   system: "You are a careful code reviewer.",
//!   messages: history,
//!   tools: [search_tool, write_tool],
//! ) effect {net, model}
//! ```
//!
//! ## Streaming
//!
//! `complete_stream` returns a typed
//! [`streaming::MessageStream`] of [`message::MessageDelta`]
//! events. Token + dollar budgets short-circuit between deltas, so
//! agent loops that watch the budget tile can drop the rest of a
//! runaway stream without waiting for the upstream to time out.
//!
//! See `docs/reference/stdlib/llm.md` for the full surface +
//! `dev/history/notes/STD_LLM_V0_26_NOTES.md` for design rationale.

pub mod anthropic;
pub mod bedrock;
pub mod budget;
pub mod error;
pub mod gemini;
pub mod message;
pub mod openai;
pub mod provider;
pub mod streaming;
pub mod tools;

/// v0.29 Track E: resolve a provider base URL from the environment.
///
/// Resolution order:
///   1. `<provider_var>` (e.g. `ANTHROPIC_BASE_URL`) — exact override
///      for one provider. Used by tests that mock a single provider.
///   2. `MTY_LLM_BASE_URL` — universal fallback for all providers.
///      Useful for redirecting *every* LLM call at a single mock or
///      observability proxy during integration tests.
///   3. `default_url` — the hard-coded production endpoint, applied as
///      the last-resort fallback.
///
/// Returned URLs are passed through verbatim — no trailing-slash
/// normalisation or scheme validation. The provider clients trim
/// trailing slashes when composing endpoints, so callers MAY include
/// or omit the trailing `/`.
///
/// Empty strings count as unset (typically the result of `EnvVar=` with
/// nothing on the right). This keeps a stray empty env var from
/// silently redirecting traffic to `""/v1/messages`.
pub(crate) fn resolve_base_url(provider_var: &str, default_url: &str) -> String {
    if let Ok(v) = std::env::var(provider_var) {
        if !v.is_empty() {
            return v;
        }
    }
    if let Ok(v) = std::env::var("MTY_LLM_BASE_URL") {
        if !v.is_empty() {
            return v;
        }
    }
    default_url.to_string()
}

// Re-exports the most-used surface at the top level so call sites
// can write `mty_stdlib::llm::Message` instead of digging through
// submodules.
pub use anthropic::AnthropicClient;
pub use bedrock::{AwsCredentials, BedrockClient};
pub use budget::{DollarBudget, TokenBudget};
pub use error::{BudgetExhausted, LlmError, RateLimitError};
pub use gemini::GeminiClient;
pub use message::{ContentBlock, ImageSource, Message, MessageDelta, Role, ToolResult, ToolUse};
pub use openai::OpenAiClient;
pub use provider::{CompletionRequest, LlmProvider};
pub use streaming::MessageStream;
pub use tools::{Tool, ToolChoice};
