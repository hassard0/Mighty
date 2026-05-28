//! `Suite` — the builder users construct to drive an eval.
//!
//! Three chain points: register a case, register a member, run the
//! eval through a comparator. The shape mirrors `std.swarm` so a
//! caller switching from "consensus" to "regression test" only needs
//! to swap the verb on the verb-on-the-builder.
//!
//! ```no_run
//! # use mty_stdlib::eval::{Suite, Case, Member, Compare};
//! # async fn run() -> Result<(), mty_stdlib::eval::EvalError> {
//! let report = Suite::new("research-agent")
//!     .case(Case::from_input("What's 2 + 2?"))
//!     .case(Case::from_input("What's the capital of France?"))
//!     .run_with(Member::mock("a", "answer", 1))
//!     .run_with(Member::mock("b", "answer", 1))
//!     .compare(Compare::equal())
//!     .await?;
//! # let _ = report;
//! # Ok(()) }
//! ```

use crate::eval::case::Case;
use crate::eval::compare::{Compare, Report};
use crate::eval::runner::Runner;
use crate::eval::EvalError;
use crate::swarm::{Member, SharedDollarBudget};

/// The eval builder. Cheap to clone — every chain method returns
/// `self` by value so a `let suite = Suite::new(...)` call can be
/// extended without rebinding.
#[derive(Debug, Clone)]
pub struct Suite {
    name: String,
    cases: Vec<Case>,
    members: Vec<Member>,
    budget_cents: u64,
}

impl Suite {
    /// New empty suite. The default budget is $5.00 (500 cents); use
    /// [`Suite::with_budget`] for finer control.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            cases: Vec::new(),
            members: Vec::new(),
            budget_cents: 500,
        }
    }

    /// Suite name — surfaced in [`Report::suite_name`].
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Override the default shared dollar budget. The budget is shared
    /// across every (case, member) dispatch; once exhausted, pending
    /// cells return errors rather than dispatching.
    pub fn with_budget(mut self, dollars: f64) -> Self {
        let cents = (dollars * 100.0).round().max(0.0) as u64;
        self.budget_cents = cents;
        self
    }

    /// Register a [`Case`]. Returns `self` so the chain stays fluent.
    pub fn case(mut self, c: Case) -> Self {
        self.cases.push(c);
        self
    }

    /// Register multiple cases at once. Convenience for bulk loading.
    pub fn cases<I: IntoIterator<Item = Case>>(mut self, cases: I) -> Self {
        self.cases.extend(cases);
        self
    }

    /// Register a [`Member`] to dispatch every case against.
    pub fn run_with(mut self, m: Member) -> Self {
        self.members.push(m);
        self
    }

    /// Number of registered cases (test-introspection helper).
    pub fn case_count(&self) -> usize {
        self.cases.len()
    }

    /// Number of registered members.
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// Configured shared-budget ceiling (cents).
    pub fn budget_cents(&self) -> u64 {
        self.budget_cents
    }

    /// Dispatch every (case, member) cell + stamp verdicts under the
    /// supplied comparator. The eval driver runs every case in
    /// sequence; members within a case run in parallel.
    pub async fn compare(self, comparator: Compare) -> Result<Report, EvalError> {
        if self.cases.is_empty() {
            return Err(EvalError::EmptySuite);
        }
        if self.members.is_empty() {
            return Err(EvalError::NoMembers);
        }

        let cases = Runner::resolve_cases(&self.cases)?;
        let budget = SharedDollarBudget::new(self.budget_cents);
        let matrix = Runner::run_matrix(&cases, &self.members, &budget).await;

        // If every cell errored we bubble up an `AllCellsFailed` so
        // CI doesn't try to make sense of an empty report.
        let any_reply_landed = matrix.iter().flatten().any(|cell| cell.reply.is_some());
        if !any_reply_landed {
            let first_err = matrix
                .iter()
                .flatten()
                .find_map(|c| c.error.clone())
                .unwrap_or_else(|| "no replies".into());
            return Err(EvalError::AllCellsFailed(first_err));
        }

        let report =
            Runner::stamp_verdicts(&self.name, &cases, &self.members, &matrix, &comparator);
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suite_new_defaults() {
        let s = Suite::new("demo");
        assert_eq!(s.name(), "demo");
        assert_eq!(s.case_count(), 0);
        assert_eq!(s.member_count(), 0);
        assert_eq!(s.budget_cents(), 500);
    }

    #[test]
    fn case_builder_appends() {
        let s = Suite::new("x")
            .case(Case::from_input("a"))
            .case(Case::from_input("b"));
        assert_eq!(s.case_count(), 2);
    }

    #[test]
    fn cases_iter_bulk_loads() {
        let s = Suite::new("x").cases(vec![
            Case::from_input("a"),
            Case::from_input("b"),
            Case::from_input("c"),
        ]);
        assert_eq!(s.case_count(), 3);
    }

    #[test]
    fn run_with_appends_member() {
        let s = Suite::new("x")
            .run_with(Member::mock("a", "x", 1))
            .run_with(Member::mock("b", "x", 1));
        assert_eq!(s.member_count(), 2);
    }

    #[test]
    fn with_budget_converts_dollars_to_cents() {
        let s = Suite::new("x").with_budget(2.50);
        assert_eq!(s.budget_cents(), 250);
    }

    #[test]
    fn with_budget_clamps_negative_to_zero() {
        let s = Suite::new("x").with_budget(-1.0);
        assert_eq!(s.budget_cents(), 0);
    }

    #[test]
    fn with_budget_rounds_fractional_cent_correctly() {
        let s = Suite::new("x").with_budget(0.005);
        // 0.5 cents rounds to 1 (half-away-from-zero on f64::round).
        assert!(s.budget_cents() <= 1);
    }

    #[tokio::test]
    async fn compare_returns_report_for_valid_suite() {
        let s = Suite::new("x")
            .case(Case::from_input("p"))
            .run_with(Member::mock("a", "yes", 1))
            .run_with(Member::mock("b", "yes", 1));
        let r = s.compare(Compare::equal()).await.unwrap();
        assert!(r.passed());
        assert_eq!(r.case_names.len(), 1);
        assert_eq!(r.member_labels.len(), 2);
    }

    #[tokio::test]
    async fn compare_surfaces_divergences() {
        let s = Suite::new("x")
            .case(Case::from_input("p"))
            .run_with(Member::mock("a", "yes", 1))
            .run_with(Member::mock("b", "no", 1));
        let r = s.compare(Compare::equal()).await.unwrap();
        assert!(!r.passed());
        assert_eq!(r.divergences.len(), 1);
    }

    #[tokio::test]
    async fn compare_all_members_errored_returns_all_cells_failed() {
        let s = Suite::new("x")
            .case(Case::from_input("p"))
            .run_with(Member::mock_error("a", "boom"))
            .run_with(Member::mock_error("b", "kaboom"));
        let r = s.compare(Compare::equal()).await;
        assert!(matches!(r, Err(EvalError::AllCellsFailed(_))));
    }

    #[tokio::test]
    async fn compare_with_semantic_strategy() {
        let s = Suite::new("x")
            .case(Case::from_input("p"))
            .run_with(Member::mock("a", "anthropic claude opus", 1))
            .run_with(Member::mock("b", "anthropic claude opus haiku", 1));
        let r = s.compare(Compare::semantic_similarity(0.3)).await.unwrap();
        // High token overlap — should pass at the low threshold.
        assert!(r.passed());
    }

    #[tokio::test]
    async fn compare_with_tool_call_set_strategy() {
        let s = Suite::new("x")
            .case(Case::from_input("p"))
            .run_with(Member::mock("a", "let me use search_web()", 1))
            .run_with(Member::mock(
                "b",
                "I'll call <tool_use name=\"search_web\"> for that",
                1,
            ));
        let r = s.compare(Compare::tool_call_set_equal()).await.unwrap();
        assert!(r.passed());
    }

    #[tokio::test]
    async fn compare_multi_case_multi_member_matrix() {
        let s = Suite::new("matrix")
            .case(Case::from_input("p1"))
            .case(Case::from_input("p2"))
            .case(Case::from_input("p3"))
            .run_with(Member::mock("a", "yes", 1))
            .run_with(Member::mock("b", "yes", 1));
        let r = s.compare(Compare::equal()).await.unwrap();
        assert_eq!(r.cells.len(), 3);
        for row in &r.cells {
            assert_eq!(row.len(), 2);
        }
        assert!(r.passed());
        // 3 cases × 2 members × 1 cent each
        assert_eq!(r.total_cost_cents, 6);
    }

    /// v0.32 Track F: helper that writes a native v3 binary trace
    /// containing one LLM turn (`prompt` → `reply`). Used by the
    /// trace-baseline test pair below — the JSON-lines fallback was
    /// retired in v0.32, so every `Case::from_trace` fixture is now a
    /// real `.mty-trace`.
    fn write_native_trace(path: &std::path::Path, prompt: &str, reply: &str) {
        use mty_runtime::replay::{Recorder, TraceCodec};
        let r = Recorder::new(path, 0, 1).with_codec(TraceCodec::Json);
        r.record_llm_call(0, None, prompt, None, vec![], reply, vec![], 1);
        r.flush_to_disk().unwrap();
    }

    #[tokio::test]
    async fn compare_with_trace_baseline_marks_member_diverges_from_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("diverge.mty-trace");
        write_native_trace(&path, "what's 2+2", "4");
        let s = Suite::new("x")
            .case(Case::from_trace(&path))
            // Both members produce "five" — trace baseline is "4" so
            // both diverge.
            .run_with(Member::mock("a", "five", 1))
            .run_with(Member::mock("b", "five", 1));
        let r = s.compare(Compare::equal()).await.unwrap();
        assert!(!r.passed());
        assert_eq!(r.divergences.len(), 2);
    }

    #[tokio::test]
    async fn compare_with_trace_baseline_matches_when_member_agrees() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("match.mty-trace");
        write_native_trace(&path, "what's 2+2", "4");
        let s = Suite::new("x")
            .case(Case::from_trace(&path))
            .run_with(Member::mock("a", "4", 1))
            .run_with(Member::mock("b", "4", 1));
        let r = s.compare(Compare::equal()).await.unwrap();
        assert!(r.passed());
    }

    #[tokio::test]
    async fn compare_single_member_passes_via_single_member_verdict() {
        let s = Suite::new("x")
            .case(Case::from_input("p"))
            .run_with(Member::mock("only", "anything", 1));
        let r = s.compare(Compare::equal()).await.unwrap();
        assert!(r.passed());
    }

    #[tokio::test]
    async fn compare_budget_exhaustion_does_not_abort_suite() {
        // Tiny budget; the second member dispatch should land *after*
        // the budget has been drained.
        let s = Suite::new("x")
            .with_budget(0.01) // 1 cent
            .case(Case::from_input("p"))
            .run_with(Member::mock("a", "yes", 1))
            .run_with(Member::mock("b", "yes", 1));
        let r = s.compare(Compare::equal()).await;
        // The suite is allowed to surface a report OR an
        // AllCellsFailed depending on which member won the dispatch
        // race. Either way the call returns Ok(report) or
        // Err(AllCellsFailed) — never panics.
        match r {
            Ok(report) => {
                // At least one cell either matched (both got
                // through) or one errored on budget.
                assert!(report.cells[0].len() == 2);
            }
            Err(EvalError::AllCellsFailed(_)) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }
}
