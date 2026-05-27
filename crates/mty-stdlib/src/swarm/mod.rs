//! `std.swarm` — multi-LLM consensus primitive.
//!
//! v0.27 Track D. One trait, four consensus strategies, one shared
//! budget. Builds on v0.26's [`crate::llm`] providers + the
//! [`crate::llm::DollarBudget`] shape.
//!
//! ## What it is
//!
//! A *swarm* sends one prompt to every member of a panel in parallel,
//! collects the replies, runs them through a [`ConsensusStrategy`],
//! and surfaces the verdict + every dissent. Concretely:
//!
//! ```no_run
//! use mty_stdlib::swarm::{swarm, Member, ConsensusStrategy, SharedDollarBudget};
//!
//! # async fn run() -> Result<(), mty_stdlib::swarm::SwarmError> {
//! let consensus = swarm(
//!     "Is this code safe? fn main() { unsafe { /* ... */ } }",
//!     vec![
//!         Member::anthropic("claude-opus-4-7"),
//!         Member::openai("gpt-5"),
//!         Member::gemini("gemini-2.5-pro"),
//!     ],
//!     SharedDollarBudget::from_dollars(0.50),
//!     ConsensusStrategy::Majority,
//! )
//! .await?;
//!
//! println!("verdict: {:?}", consensus.majority);
//! for d in &consensus.dissents {
//!     println!("- {} disagrees: {}", d.member, d.body);
//! }
//! # Ok(()) }
//! ```
//!
//! ## Surfaces
//!
//! | Module | Shape |
//! |---|---|
//! | [`member`] | `Member` enum (Anthropic/OpenAi/Gemini/Bedrock/Mock) |
//! | [`consensus`] | `ConsensusStrategy` (Majority/Unanimous/Weighted/FirstAgreed), `Consensus` result |
//! | [`budget`] | `SharedDollarBudget` — single integer-cents pool shared across the panel |
//! | [`vote`] | Answer-comparison: `SimilarityMode::Exact` (yes/no) + `TokenSet` (free-form) |
//!
//! ## Mighty surface
//!
//! ```mty
//! use std.swarm
//!
//! let consensus = swarm(
//!   prompt: "Is this code safe?",
//!   panel: vec![
//!     Member.anthropic("claude-opus-4-7"),
//!     Member.openai("gpt-5"),
//!     Member.gemini("gemini-2.5-pro"),
//!   ],
//!   budget: DollarBudget.new(0.50),
//!   strategy: ConsensusStrategy.Majority,
//! ).await
//! ```
//!
//! See `docs/reference/stdlib/swarm.md` for the full doc page and
//! `dev/history/notes/STD_SWARM_V0_27_NOTES.md` for design rationale.

pub mod budget;
pub mod consensus;
pub mod member;
pub mod vote;

pub use budget::{BudgetTripped, SharedDollarBudget};
pub use consensus::{resolve, Consensus, ConsensusStrategy};
pub use member::{Member, MemberReply, MockMember};
pub use vote::{cluster_replies, similarity, Cluster, SimilarityMode};

use crate::llm::error::LlmError;
use thiserror::Error;

/// Top-level error returned by [`swarm`]. Wraps [`LlmError`] (one
/// member's per-call failure) + the swarm-level configuration errors.
#[derive(Debug, Error)]
pub enum SwarmError {
    /// The panel had no members. Swarms require at least one — the
    /// degenerate single-member case is supported (Majority returns
    /// that member's reply with no dissents), but zero is an error.
    #[error("swarm: panel cannot be empty")]
    EmptyPanel,

    /// Every member errored. Carries the first error so callers can
    /// inspect it without dragging the whole vector around.
    #[error("swarm: every member failed; first error: {0}")]
    AllMembersFailed(String),

    /// An individual member's error escaped — only surfaced when the
    /// swarm dispatch path itself can't recover (e.g. a single-member
    /// panel where that one member errored). For multi-member
    /// panels, member errors are *collected* into the consensus
    /// result rather than aborting the swarm.
    #[error("swarm: llm error: {0}")]
    Llm(#[from] LlmError),
}

/// Dispatch `prompt` to every member of `panel` in parallel, deduct
/// each call's cost from the shared `budget`, then resolve the replies
/// through `strategy` into a [`Consensus`].
///
/// ## Short-circuit semantics
///
/// - `FirstAgreed` stops dispatching new members once two land in the
///   same cluster. Pending member calls are dropped before they spawn.
/// - When the shared `budget` trips mid-flight, any pending members
///   are dropped and the [`Consensus`] surfaces with
///   `budget_exhausted: true`. The members that already ran are
///   still resolved through `strategy`.
///
/// ## Single-member panels
///
/// A panel of one member is permitted; the consensus is that member's
/// reply with zero dissents. Use this to keep the call site uniform
/// while a swarm is being scaled up.
pub async fn swarm(
    prompt: &str,
    panel: Vec<Member>,
    budget: SharedDollarBudget,
    strategy: ConsensusStrategy,
) -> Result<Consensus, SwarmError> {
    if panel.is_empty() {
        return Err(SwarmError::EmptyPanel);
    }

    // Special-case the single-member panel for the simplest path
    // (and so a panel of one doesn't allocate the join_all future
    // machinery for nothing).
    if panel.len() == 1 {
        let m = &panel[0];
        let reply = m.ask(prompt, &budget).await?;
        return Ok(consensus::resolve(
            vec![reply],
            &strategy,
            budget.is_exhausted(),
        ));
    }

    // `FirstAgreed` strategy: dispatch members *sequentially* (cheaper
    // than parallel — we want to drop pending dispatches on a hit),
    // checking after each reply whether two replies clustered. As
    // soon as they do, stop and resolve.
    if strategy.supports_early_exit() {
        return run_first_agreed(prompt, panel, budget, strategy).await;
    }

    // Parallel dispatch for every other strategy. Each member's
    // `ask` charges the shared budget atomically.
    let mut handles = Vec::with_capacity(panel.len());
    for m in &panel {
        let prompt = prompt.to_string();
        let budget = budget.clone();
        let m = m.clone();
        handles.push(tokio::spawn(async move { m.ask(&prompt, &budget).await }));
    }

    let mut replies: Vec<MemberReply> = Vec::with_capacity(panel.len());
    let mut errors: Vec<String> = Vec::new();
    for h in handles {
        match h.await {
            Ok(Ok(reply)) => replies.push(reply),
            Ok(Err(e)) => errors.push(e.to_string()),
            Err(join_err) => errors.push(format!("join: {join_err}")),
        }
    }

    let budget_exhausted = budget.is_exhausted();

    // If *every* member errored, surface that as a swarm-level error
    // rather than a "no consensus" result — there's no signal to
    // resolve.
    if replies.is_empty() {
        let first = errors
            .into_iter()
            .next()
            .unwrap_or_else(|| "no replies".into());
        return Err(SwarmError::AllMembersFailed(first));
    }

    Ok(consensus::resolve(replies, &strategy, budget_exhausted))
}

async fn run_first_agreed(
    prompt: &str,
    panel: Vec<Member>,
    budget: SharedDollarBudget,
    strategy: ConsensusStrategy,
) -> Result<Consensus, SwarmError> {
    let mut replies: Vec<MemberReply> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut budget_exhausted = false;
    for m in &panel {
        if budget.is_exhausted() {
            budget_exhausted = true;
            break;
        }
        match m.ask(prompt, &budget).await {
            Ok(r) => {
                replies.push(r);
                // Did the last reply join an existing cluster?
                let bodies: Vec<String> = replies.iter().map(|r| r.body.clone()).collect();
                let mode = if bodies.iter().all(|b| b.len() <= 24) {
                    vote::SimilarityMode::Exact
                } else {
                    vote::SimilarityMode::TokenSet
                };
                let clusters = vote::cluster_replies(&bodies, mode, 0.6);
                if clusters.iter().any(|c| c.size() >= 2) {
                    break;
                }
            }
            Err(e) => {
                errors.push(e.to_string());
                // Don't break — give the remaining members a chance.
            }
        }
    }
    if budget.is_exhausted() {
        budget_exhausted = true;
    }
    if replies.is_empty() {
        let first = errors
            .into_iter()
            .next()
            .unwrap_or_else(|| "no replies".into());
        return Err(SwarmError::AllMembersFailed(first));
    }
    Ok(consensus::resolve(replies, &strategy, budget_exhausted))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_panel_errors() {
        let r = swarm(
            "x",
            vec![],
            SharedDollarBudget::new(100),
            ConsensusStrategy::Majority,
        )
        .await;
        assert!(matches!(r, Err(SwarmError::EmptyPanel)));
    }

    #[tokio::test]
    async fn single_member_panel_short_circuits() {
        let panel = vec![Member::mock("a", "yes", 1)];
        let c = swarm(
            "x",
            panel,
            SharedDollarBudget::new(100),
            ConsensusStrategy::Majority,
        )
        .await
        .unwrap();
        assert_eq!(c.majority.as_deref(), Some("yes"));
        assert!(c.dissents.is_empty());
    }

    #[tokio::test]
    async fn all_members_errored_returns_swarm_error() {
        let panel = vec![
            Member::mock_error("a", "boom"),
            Member::mock_error("b", "kaboom"),
        ];
        let r = swarm(
            "x",
            panel,
            SharedDollarBudget::new(100),
            ConsensusStrategy::Majority,
        )
        .await;
        assert!(matches!(r, Err(SwarmError::AllMembersFailed(_))));
    }
}
