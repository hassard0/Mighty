//! `std.eval` — typed LLM-eval driver on top of v0.21 replay machinery.
//!
//! v0.28 Track G. One typed builder, two case sources (raw input
//! string plus recorded `.mty-trace` file), four "members" (the same
//! Anthropic, OpenAI, Gemini, Bedrock providers `std.swarm` uses),
//! three comparison strategies (byte-equal, semantic-similarity over
//! the [`crate::memory::Embedder`] surface, tool-call-set equality).
//! Builds on the v0.21 byte-identical replay driver
//! ([`mty_runtime::replay::ReplayDriver`]) so a trace-sourced case
//! re-runs deterministically against each panel member.
//!
//! ## What it is
//!
//! "Regression-test LLM agents like any other code." Record one
//! agent run, then re-run that recording (or a fresh input prompt)
//! against many model variants and assert their replies cluster
//! according to a chosen comparator. The README's Why-Mighty section
//! already promises this; v0.28 Track G is the surface that delivers.
//!
//! ```no_run
//! use mty_stdlib::eval::{Case, Compare, Member, Suite};
//!
//! # async fn run() -> Result<(), mty_stdlib::eval::EvalError> {
//! let suite = Suite::new("research-agent")
//!     .case(Case::from_input("What's the population of France?"))
//!     .case(Case::from_input("Capital of Australia?"));
//!
//! let report = suite
//!     .run_with(Member::mock("baseline", "Paris", 1))
//!     .run_with(Member::mock("challenger", "Paris", 1))
//!     .compare(Compare::semantic_similarity(0.85))
//!     .await?;
//!
//! assert!(report.passed());
//! # Ok(()) }
//! ```
//!
//! ## Surfaces
//!
//! | Module | Shape |
//! |---|---|
//! | [`suite`] | `Suite` builder — `.case(...)` / `.run_with(...)` / `.compare(...)` |
//! | [`case`] | `Case` — an eval input: either a raw prompt or a recorded trace |
//! | [`runner`] | `Runner` — internal dispatch + per-(case,member) result table |
//! | [`compare`] | `Compare` strategy + `Report` shape with divergence tallies |
//! | [`replay_glue`] | Thin wrapper over [`mty_runtime::replay::ReplayDriver`] |
//!
//! ## Mighty surface
//!
//! ```mty
//! use std.eval.{Suite, Case, Member, Compare}
//!
//! let suite = Suite.new("research-agent")
//!   .case(Case.from_trace("traces/research-001.mty-trace"))
//!   .case(Case.from_input("What's the population of France?"));
//!
//! let report = suite
//!   .run_with(Member.anthropic("claude-opus-4-7"))
//!   .run_with(Member.openai("gpt-5"))
//!   .compare(Compare.semantic_similarity(threshold: 0.85))
//!   .await
//! ```
//!
//! ## Replay-runtime hooks
//!
//! Two hooks the v0.21 [`mty_runtime::replay::ReplayDriver`] doesn't
//! yet expose are stubbed in [`replay_glue`] and surfaced in the v0.29
//! backlog (see `replay_glue::V029_BACKLOG`):
//!
//! 1. A `Replay::with_provider(member)` constructor that swaps the
//!    recorded `LlmProvider` calls for a fresh member's calls without
//!    re-running unrelated runtime IO.
//! 2. A `RecordedTrace::iter_llm_calls()` accessor so the eval driver
//!    can fast-path "just rerun the LLM turns" without spinning a
//!    full [`Runtime`](mty_runtime::Runtime).
//!
//! For v0.28 we work around (1) by re-dispatching the recorded prompt
//! through the live member (the typical case is "rerun a 1-turn
//! query"); the deterministic re-execution path is exercised when
//! [`mty_runtime::replay::ReplayDriver::with_program`] is wired through
//! [`replay_glue::run_trace_with_member`]'s `program: Option<...>`
//! argument.
//!
//! See `docs/internals/std-eval.md` for the design rationale + the
//! complete divergence-reporting contract.

pub mod case;
pub mod compare;
pub mod replay_glue;
pub mod runner;
pub mod suite;

pub use case::{Case, CaseKind, CaseRun};
pub use compare::{Compare, Divergence, Report, Verdict};
pub use replay_glue::{
    decode_baseline_auto, decode_trace_baseline, decode_trace_baseline_native, read_binary_trace,
    MemberTurnProvider, ReplayGlueError, TraceBaseline, V029_BACKLOG,
};
pub use runner::{Runner, RunnerCell};
pub use suite::Suite;

// Re-export the `Member` shape from `std.swarm` so callers writing
// `use std.eval.Member` don't need to also pull in `std.swarm`.
pub use crate::swarm::{Member, MemberReply};

use thiserror::Error;

/// Top-level error returned by [`Suite::compare`].
#[derive(Debug, Error)]
pub enum EvalError {
    /// The suite was created with no cases. Eval needs at least one.
    #[error("eval: suite must contain at least one case")]
    EmptySuite,
    /// No members were registered via `.run_with(...)`. Every comparator
    /// needs at least two reply columns to compare; we accept a
    /// single-member suite (returns `Verdict::SingleMember`) but raise
    /// here for zero.
    #[error("eval: suite must register at least one member via run_with")]
    NoMembers,
    /// A `Case::from_trace` pointed at a file we couldn't read or
    /// decode. Carries the underlying replay-glue error.
    #[error("eval: trace case error: {0}")]
    TraceCase(#[from] ReplayGlueError),
    /// One member failed mid-eval — surfaced as a soft mismatch on
    /// the per-cell result rather than aborting the whole suite, but
    /// if *every* member fails on *every* case the suite returns this
    /// at the top.
    #[error("eval: every (case, member) cell errored; first: {0}")]
    AllCellsFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_suite_errors() {
        let suite = Suite::new("x");
        let r = suite.compare(Compare::equal()).await;
        assert!(matches!(r, Err(EvalError::EmptySuite)));
    }

    #[tokio::test]
    async fn no_members_errors() {
        let suite = Suite::new("x").case(Case::from_input("hello"));
        let r = suite.compare(Compare::equal()).await;
        assert!(matches!(r, Err(EvalError::NoMembers)));
    }
}
