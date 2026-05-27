//! Typed errors for `std.llm`.
//!
//! Surface every provider through one error enum so Mighty source can
//! write `match err { LlmError.RateLimit(..) => ..., _ => ... }` without
//! caring whether it came from Anthropic, OpenAI, Gemini, or Bedrock.
//!
//! ## Design
//!
//! Three error families are first-class because callers genuinely need
//! to branch on them:
//!
//! - [`RateLimitError`] — the provider asked us to back off. Carries a
//!   suggested `retry_after_secs` when the upstream sent one
//!   (`Retry-After` header on Anthropic / OpenAI), `None` otherwise.
//! - [`BudgetExhausted`] — the *caller's* typed [`crate::llm::budget`]
//!   budget tripped. Surfaced before the next provider call goes out so
//!   long completions can be cancelled deterministically.
//! - [`LlmError::Auth`] — the provider rejected the API key (401/403).
//!
//! Everything else (timeouts, malformed responses, transport blow-ups)
//! collapses into [`LlmError::Transport`] / [`LlmError::Decode`] so the
//! enum doesn't grow combinatorially.

use thiserror::Error;

/// One error type across every provider in `std.llm`.
#[derive(Debug, Error)]
pub enum LlmError {
    /// The provider returned 401/403 — the API key is missing or
    /// invalid. Surface a short, untyped message; never echo the key.
    #[error("llm auth: {0}")]
    Auth(String),

    /// HTTP 429 with optional `Retry-After`. See [`RateLimitError`].
    #[error("llm rate-limited")]
    RateLimit(RateLimitError),

    /// The caller-supplied [`crate::llm::budget`] tripped. See
    /// [`BudgetExhausted`].
    #[error("llm budget exhausted")]
    BudgetExhausted(BudgetExhausted),

    /// The provider returned a non-success status (other than 401/403/429).
    /// `status` is the HTTP code, `body` is the (truncated) error body.
    #[error("llm provider {status}: {body}")]
    Provider { status: u16, body: String },

    /// Network / TCP / TLS error — anything below the application
    /// layer. Wraps the underlying error's `Display` impl.
    #[error("llm transport: {0}")]
    Transport(String),

    /// The provider returned a 2xx response but we couldn't parse it
    /// into the typed [`crate::llm::message::Message`] / streaming
    /// delta shapes. Usually means the provider rolled out a new
    /// schema we don't know about yet.
    #[error("llm decode: {0}")]
    Decode(String),

    /// The provider's model name isn't registered in our endpoint map.
    /// Useful as a distinct variant so callers can `?` it without
    /// catching genuine transport failures.
    #[error("llm: unknown model {0}")]
    UnknownModel(String),

    /// Catch-all for surfaces that haven't been wired yet — e.g. an
    /// OpenAI streaming call against the v0.26 skeleton.
    #[error("llm: not implemented in v0.26 — {0}")]
    NotImplemented(&'static str),
}

/// Rate-limit detail. Pulled out so `match` arms can read
/// `retry_after_secs` without unpacking the enum payload twice.
#[derive(Debug, Clone)]
pub struct RateLimitError {
    /// Suggested wait before retrying. `None` when the upstream
    /// didn't set a `Retry-After` header.
    pub retry_after_secs: Option<u64>,
    /// Short human-readable string from the upstream's error body.
    /// May be empty.
    pub message: String,
}

impl RateLimitError {
    pub fn new(retry_after_secs: Option<u64>, message: impl Into<String>) -> Self {
        Self {
            retry_after_secs,
            message: message.into(),
        }
    }
}

/// Budget-exhaustion detail. Carries enough info that downstream
/// observability (metrics, traces) can record *which* budget tripped.
#[derive(Debug, Clone)]
pub struct BudgetExhausted {
    /// `"tokens"` or `"dollars"` — the kind of budget that tripped.
    /// Pinned as `&'static str` so the variant is cheap to construct
    /// from streaming hot paths.
    pub kind: &'static str,
    /// Limit the caller set when the budget was created.
    pub limit: u64,
    /// Amount actually consumed when the trip fired.
    pub consumed: u64,
}

impl BudgetExhausted {
    pub fn tokens(limit: u64, consumed: u64) -> Self {
        Self {
            kind: "tokens",
            limit,
            consumed,
        }
    }

    pub fn dollars(limit_cents: u64, consumed_cents: u64) -> Self {
        Self {
            kind: "dollars",
            limit: limit_cents,
            consumed: consumed_cents,
        }
    }
}

impl From<std::io::Error> for LlmError {
    fn from(e: std::io::Error) -> Self {
        LlmError::Transport(e.to_string())
    }
}

impl From<serde_json::Error> for LlmError {
    fn from(e: serde_json::Error) -> Self {
        LlmError::Decode(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_carries_retry_after() {
        let r = RateLimitError::new(Some(7), "slow down");
        assert_eq!(r.retry_after_secs, Some(7));
        assert_eq!(r.message, "slow down");
    }

    #[test]
    fn budget_exhausted_distinguishes_token_vs_dollar() {
        let t = BudgetExhausted::tokens(1000, 1001);
        let d = BudgetExhausted::dollars(500, 600);
        assert_eq!(t.kind, "tokens");
        assert_eq!(d.kind, "dollars");
        assert_eq!(t.limit, 1000);
        assert_eq!(d.consumed, 600);
    }

    #[test]
    fn display_includes_status_for_provider_err() {
        let e = LlmError::Provider {
            status: 500,
            body: "boom".into(),
        };
        assert!(e.to_string().contains("500"));
        assert!(e.to_string().contains("boom"));
    }
}
