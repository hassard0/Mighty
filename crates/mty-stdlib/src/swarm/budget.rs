//! Shared dollar budget for a swarm panel.
//!
//! Builds on `std.llm`'s [`crate::llm::DollarBudget`] but specialises
//! the shape: every panel member deducts from one shared pool, and
//! when the pool trips mid-flight we want to surface a *partial*
//! consensus with `budget_exhausted: true` rather than poison the
//! whole result. That single-pool semantics is best expressed as a
//! dedicated type so the swarm code never has to remember "did I
//! clone the budget into the [`Member`] before dispatch".
//!
//! ## Why integer cents
//!
//! Fractional-cent drift across `f64` cost additions is the kind of
//! bug that only shows up after the panel has run thousands of times.
//! All accounting in this module is `u64` cents (`saturating_add`),
//! same as `std.llm::DollarBudget`. Callers who want sub-cent
//! precision pass tokens directly to `add_tokens` and let the
//! per-model rate table do the rounding once, centrally.
//!
//! ## Concurrency
//!
//! [`SharedDollarBudget`] is `Clone` — every clone shares the same
//! `Arc<Mutex<Inner>>` pool. The swarm dispatches every member's
//! `ask` in parallel via `futures::future::join_all`; each call
//! charges the shared pool atomically, and the next dispatched member
//! sees the updated `consumed_cents` immediately.

use std::sync::Arc;
use std::sync::Mutex;

/// One shared dollar budget shared across all swarm panel members.
///
/// Construct with [`SharedDollarBudget::new(limit_cents)`] then clone
/// freely — every clone shares the same underlying counter.
#[derive(Debug, Clone)]
pub struct SharedDollarBudget {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Debug)]
struct Inner {
    limit_cents: u64,
    consumed_cents: u64,
}

impl SharedDollarBudget {
    /// New budget capped at `limit_cents` total spend.
    pub fn new(limit_cents: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                limit_cents,
                consumed_cents: 0,
            })),
        }
    }

    /// Effectively no cap. Useful for "I want to observe `cost` but
    /// don't care about a ceiling" tests.
    pub fn unbounded() -> Self {
        Self::new(u64::MAX)
    }

    /// Cents-per-dollar convenience: `SharedDollarBudget::from_dollars(0.50)`
    /// is a 50-cent cap.
    pub fn from_dollars(dollars: f64) -> Self {
        let cents = (dollars * 100.0).round().max(0.0) as u64;
        Self::new(cents)
    }

    pub fn limit_cents(&self) -> u64 {
        self.inner
            .lock()
            .expect("shared budget poisoned")
            .limit_cents
    }

    pub fn consumed_cents(&self) -> u64 {
        self.inner
            .lock()
            .expect("shared budget poisoned")
            .consumed_cents
    }

    /// Returns `true` if any further dispatch would exceed the cap.
    /// The swarm loop polls this between panel members to decide
    /// whether to skip the next one.
    pub fn is_exhausted(&self) -> bool {
        let i = self.inner.lock().expect("shared budget poisoned");
        i.consumed_cents >= i.limit_cents
    }

    /// Charge `cents` against the shared pool. Returns the new
    /// consumed total. The caller is responsible for short-circuiting
    /// on `is_exhausted()` *before* the next dispatch — we record
    /// overshoots (saturating add) so observability tooling sees them.
    pub fn try_charge(&self, cents: u64) -> Result<u64, BudgetTripped> {
        let mut i = self.inner.lock().expect("shared budget poisoned");
        i.consumed_cents = i.consumed_cents.saturating_add(cents);
        if i.consumed_cents > i.limit_cents {
            Err(BudgetTripped {
                limit_cents: i.limit_cents,
                consumed_cents: i.consumed_cents,
            })
        } else {
            Ok(i.consumed_cents)
        }
    }

    /// Convenience: charge a token count for `model` via the canonical
    /// per-million-token rate table.
    pub fn add_tokens(
        &self,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) -> Result<u64, BudgetTripped> {
        let (in_rate, out_rate) = crate::llm::budget::default_pricing_cents_per_million(model);
        let cost = input_tokens.saturating_mul(in_rate) / 1_000_000
            + output_tokens.saturating_mul(out_rate) / 1_000_000;
        self.try_charge(cost)
    }
}

/// Returned by [`SharedDollarBudget::try_charge`] when the new total
/// crosses the cap. The swarm wraps this back into
/// `LlmError::BudgetExhausted` for the [`crate::swarm::Member::ask`]
/// return shape; tests pattern-match on it directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetTripped {
    pub limit_cents: u64,
    pub consumed_cents: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_counter() {
        let a = SharedDollarBudget::new(100);
        let b = a.clone();
        a.try_charge(30).unwrap();
        b.try_charge(40).unwrap();
        assert_eq!(a.consumed_cents(), 70);
        assert_eq!(b.consumed_cents(), 70);
    }

    #[test]
    fn try_charge_trips_when_exceeded() {
        let b = SharedDollarBudget::new(100);
        b.try_charge(60).unwrap();
        let err = b.try_charge(50).err().unwrap();
        assert_eq!(err.limit_cents, 100);
        assert_eq!(err.consumed_cents, 110);
        assert!(b.is_exhausted());
    }

    #[test]
    fn from_dollars_rounds_to_cents() {
        let b = SharedDollarBudget::from_dollars(0.50);
        assert_eq!(b.limit_cents(), 50);
        let b2 = SharedDollarBudget::from_dollars(1.234);
        assert_eq!(b2.limit_cents(), 123);
    }

    #[test]
    fn add_tokens_uses_default_rate_table() {
        let b = SharedDollarBudget::new(10_000);
        // 1M opus input tokens = 1500 cents
        b.add_tokens("claude-opus-4-7", 1_000_000, 0).unwrap();
        assert_eq!(b.consumed_cents(), 1500);
    }

    #[test]
    fn is_exhausted_when_at_limit() {
        let b = SharedDollarBudget::new(100);
        let _ = b.try_charge(100);
        assert!(b.is_exhausted());
    }
}
