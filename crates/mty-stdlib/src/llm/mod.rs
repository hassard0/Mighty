//! `std.llm` — typed LLM provider abstraction.
//!
//! v0.26 Track A. One trait surface
//! ([`LlmProvider`](provider::LlmProvider)), four implementations:
//!
//! | Provider | Status | Module |
//! |---|---|---|
//! | Anthropic Messages | **full** (HTTP + streaming + tool-use + budgets) | [`anthropic`] |
//! | OpenAI Responses | skeleton (auth + request shaping; body parse TODO v0.27) | [`openai`] |
//! | Google Gemini `generateContent` | skeleton | [`gemini`] |
//! | AWS Bedrock Converse | skeleton (bearer-token; SigV4 TODO v0.27) | [`bedrock`] |
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

// Re-exports the most-used surface at the top level so call sites
// can write `mty_stdlib::llm::Message` instead of digging through
// submodules.
pub use anthropic::AnthropicClient;
pub use bedrock::BedrockClient;
pub use budget::{DollarBudget, TokenBudget};
pub use error::{BudgetExhausted, LlmError, RateLimitError};
pub use gemini::GeminiClient;
pub use message::{ContentBlock, ImageSource, Message, MessageDelta, Role, ToolResult, ToolUse};
pub use openai::OpenAiClient;
pub use provider::{CompletionRequest, LlmProvider};
pub use streaming::MessageStream;
pub use tools::{Tool, ToolChoice};
