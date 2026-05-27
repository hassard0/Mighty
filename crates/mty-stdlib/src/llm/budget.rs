//! Typed token + dollar budgets for `std.llm`.
//!
//! Plug-in to the [`crate::llm::LlmProvider`] trait — every
//! `CompletionRequest` takes an optional [`TokenBudget`] and
//! [`DollarBudget`]. Each completed turn (or each streaming delta)
//! updates the consumed counters; once either trips, the next call
//! short-circuits with [`crate::llm::error::LlmError::BudgetExhausted`].
//!
//! ## Why typed, not just env-variables
//!
//! Agents in the wild blow budgets in two failure modes:
//!
//! 1. **Streaming loops** that don't notice the tokens-per-second draw
//!    until the user's invoice is 5x what was estimated.
//! 2. **Tool-use chains** that fan out 8 concurrent provider calls
//!    against a single conceptual user request — each one within
//!    "budget", together over it.
//!
//! Both fixes need a *typed, ref-counted handle* the caller can hand
//! down through the agent graph so child calls deduct from the same
//! pool. Hence [`Arc<Mutex<...>>`] internally — sharing a budget is
//! `budget.clone()`, not "remember to pass the same `u64` around".
//!
//! ## Pricing model (v0.26)
//!
//! Per-1k-token rates are baked in for the canonical Anthropic /
//! OpenAI / Gemini / Bedrock models. The table is intentionally tiny
//! and shaped for editing; we don't try to track real-time pricing.
//! Callers who want the latest pricing pass an explicit
//! `(input_cents_per_million, output_cents_per_million)` tuple to
//! [`DollarBudget::with_pricing`].

use std::sync::{Arc, Mutex};

use crate::llm::error::BudgetExhausted;

/// Cap the total tokens (input + output, summed) a chain of
/// completions may consume.
///
/// Clone freely — every clone shares the same underlying counter, so
/// child agents can deduct from the parent's pool.
#[derive(Debug, Clone)]
pub struct TokenBudget {
    inner: Arc<Mutex<TokenInner>>,
}

#[derive(Debug)]
struct TokenInner {
    limit: u64,
    consumed: u64,
}

impl TokenBudget {
    /// New budget capped at `limit` total tokens.
    pub fn new(limit: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TokenInner { limit, consumed: 0 })),
        }
    }

    /// Effectively no cap. Useful for tests that just want to observe
    /// the *consumed* counter.
    pub fn unbounded() -> Self {
        Self::new(u64::MAX)
    }

    /// How many tokens have been added so far.
    pub fn consumed(&self) -> u64 {
        self.inner
            .lock()
            .expect("token-budget mutex poisoned")
            .consumed
    }

    /// Configured ceiling.
    pub fn limit(&self) -> u64 {
        self.inner
            .lock()
            .expect("token-budget mutex poisoned")
            .limit
    }

    /// Returns `Ok(())` if there's still headroom, or
    /// [`BudgetExhausted`] when adding `tokens` would cross the cap.
    /// Either way, the counter is incremented — exceeding budgets are
    /// recorded so observability dashboards see the overshoot.
    pub fn try_consume(&self, tokens: u64) -> Result<(), BudgetExhausted> {
        let mut i = self.inner.lock().expect("token-budget mutex poisoned");
        i.consumed = i.consumed.saturating_add(tokens);
        if i.consumed > i.limit {
            Err(BudgetExhausted::tokens(i.limit, i.consumed))
        } else {
            Ok(())
        }
    }

    /// Returns true if any further consumption would exceed the cap.
    /// Streaming loops poll this between deltas to decide whether to
    /// drop the rest of the stream.
    pub fn is_exhausted(&self) -> bool {
        let i = self.inner.lock().expect("token-budget mutex poisoned");
        i.consumed >= i.limit
    }
}

/// Cap the dollar cost (in *cents*, integer) a chain of completions
/// may consume. Pricing is per-model; see the
/// `default_pricing_cents_per_million` lookup below for the v0.26
/// table.
#[derive(Debug, Clone)]
pub struct DollarBudget {
    inner: Arc<Mutex<DollarInner>>,
}

#[derive(Debug)]
struct DollarInner {
    limit_cents: u64,
    consumed_cents: u64,
    /// Optional caller override; otherwise we hit
    /// `default_pricing_cents_per_million(model)`.
    pricing_override: Option<(u64, u64)>,
}

impl DollarBudget {
    /// New budget capped at `limit_cents` total spend.
    pub fn new(limit_cents: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(DollarInner {
                limit_cents,
                consumed_cents: 0,
                pricing_override: None,
            })),
        }
    }

    /// Override the per-million-token pricing for all subsequent
    /// `add_usage` calls on this budget. Useful when the canonical
    /// table drifts vs. the caller's contracted rate.
    #[must_use]
    pub fn with_pricing(self, input_cents_per_million: u64, output_cents_per_million: u64) -> Self {
        let mut i = self.inner.lock().expect("dollar-budget mutex poisoned");
        i.pricing_override = Some((input_cents_per_million, output_cents_per_million));
        drop(i);
        self
    }

    pub fn consumed_cents(&self) -> u64 {
        self.inner
            .lock()
            .expect("dollar-budget mutex poisoned")
            .consumed_cents
    }

    pub fn limit_cents(&self) -> u64 {
        self.inner
            .lock()
            .expect("dollar-budget mutex poisoned")
            .limit_cents
    }

    /// Record `input_tokens` + `output_tokens` against `model`'s
    /// pricing. Returns [`BudgetExhausted`] if the new total crosses
    /// the cap.
    pub fn add_usage(
        &self,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) -> Result<(), BudgetExhausted> {
        let mut i = self.inner.lock().expect("dollar-budget mutex poisoned");
        let (in_rate, out_rate) = i
            .pricing_override
            .unwrap_or_else(|| default_pricing_cents_per_million(model));
        // cost = (tokens / 1_000_000) * rate_cents — multiply first so
        // small per-call deltas don't underflow to zero.
        let cost = input_tokens.saturating_mul(in_rate) / 1_000_000
            + output_tokens.saturating_mul(out_rate) / 1_000_000;
        i.consumed_cents = i.consumed_cents.saturating_add(cost);
        if i.consumed_cents > i.limit_cents {
            Err(BudgetExhausted::dollars(i.limit_cents, i.consumed_cents))
        } else {
            Ok(())
        }
    }

    pub fn is_exhausted(&self) -> bool {
        let i = self.inner.lock().expect("dollar-budget mutex poisoned");
        i.consumed_cents >= i.limit_cents
    }
}

/// Per-model pricing in cents-per-million-tokens.
///
/// The numbers below are **placeholder** and drift over time — the
/// shipped UX is: pass `with_pricing(...)` when you care, or use the
/// table when you don't (and trust the small overshoot).
///
/// Returns `(input_cents_per_million, output_cents_per_million)`.
///
/// Unknown models fall through to a conservative `(1500, 7500)` rate
/// (frontier-class) so the budget *over-* rather than *under-*
/// estimates.
pub fn default_pricing_cents_per_million(model: &str) -> (u64, u64) {
    // We only branch on canonical prefixes — full version strings
    // (`claude-opus-4-7-20251101`) match by `starts_with`.
    if model.starts_with("claude-opus") {
        (1500, 7500)
    } else if model.starts_with("claude-sonnet") {
        (300, 1500)
    } else if model.starts_with("claude-haiku") {
        (80, 400)
    } else if model.starts_with("gpt-5") || model.starts_with("gpt-4.1") {
        (300, 1200)
    } else if model.starts_with("gpt-4o-mini") {
        (15, 60)
    } else if model.starts_with("gpt-4o") {
        (250, 1000)
    } else if model.starts_with("gemini-2.5-pro") {
        (125, 500)
    } else if model.starts_with("gemini-2.5-flash") {
        (8, 30)
    } else {
        // Conservative frontier-class default.
        (1500, 7500)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_budget_trips_only_after_exceeding_limit() {
        let b = TokenBudget::new(100);
        assert!(b.try_consume(50).is_ok());
        assert!(b.try_consume(40).is_ok());
        assert_eq!(b.consumed(), 90);
        // 90 + 20 = 110 > 100 -> trip
        assert!(b.try_consume(20).is_err());
        assert!(b.is_exhausted());
    }

    #[test]
    fn token_budget_clone_shares_counter() {
        let a = TokenBudget::new(1000);
        let b = a.clone();
        a.try_consume(100).unwrap();
        b.try_consume(200).unwrap();
        // Both views agree.
        assert_eq!(a.consumed(), 300);
        assert_eq!(b.consumed(), 300);
    }

    #[test]
    fn dollar_budget_uses_default_pricing_for_known_model() {
        // 1M input tokens for opus = 1500 cents = $15
        let b = DollarBudget::new(10_000); // $100 cap
        b.add_usage("claude-opus-4-7", 1_000_000, 0).unwrap();
        assert_eq!(b.consumed_cents(), 1500);
    }

    #[test]
    fn dollar_budget_trips_when_exceeded() {
        let b = DollarBudget::new(100); // $1 cap
                                        // 1M opus output tokens = 7500 cents -> way over
        assert!(b.add_usage("claude-opus-4-7", 0, 1_000_000).is_err());
        assert!(b.is_exhausted());
    }

    #[test]
    fn dollar_budget_pricing_override_supersedes_table() {
        // Caller-set $1/million for both directions.
        let b = DollarBudget::new(10_000).with_pricing(100, 100);
        b.add_usage("unknown-model", 1_000_000, 1_000_000).unwrap();
        assert_eq!(b.consumed_cents(), 200);
    }

    #[test]
    fn unknown_model_falls_back_to_conservative_rate() {
        let (i, o) = default_pricing_cents_per_million("some-future-model-9");
        assert_eq!(i, 1500);
        assert_eq!(o, 7500);
    }
}
