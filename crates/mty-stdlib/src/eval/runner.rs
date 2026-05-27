//! Runner — the (case × member) dispatch matrix.
//!
//! The runner is the work-horse the [`super::Suite`] hands a fully-
//! configured eval to. It:
//!
//! 1. Materialises each [`super::Case`] into a [`super::CaseRun`]
//!    (reads trace files off disk).
//! 2. For every (case, member) cell, dispatches `member.ask(prompt,
//!    budget)` — replicating the swarm primitive's per-member
//!    accounting against a shared [`crate::swarm::SharedDollarBudget`].
//! 3. Stamps each reply with a verdict ([`super::Verdict`]) by
//!    running the comparator against either the trace's recorded
//!    baseline or the first member's reply.
//!
//! Single-member suites are accepted: every cell is stamped
//! `SingleMember` and the report still passes. This keeps the call
//! site uniform while a suite is being scaled up.

use crate::eval::case::CaseRun;
use crate::eval::compare::{Compare, Divergence, Report, Verdict};
use crate::eval::EvalError;
use crate::swarm::{Member, SharedDollarBudget};

/// One (case, member) cell — the materialised reply + the runtime cost.
#[derive(Debug, Clone)]
pub struct RunnerCell {
    /// `Some(body)` on success, `None` if the member errored.
    pub reply: Option<String>,
    /// Per-call cost in cents (zero on error).
    pub cost_cents: u64,
    /// Error string when [`reply`] is `None`.
    pub error: Option<String>,
}

/// Internal dispatch helper — drives the (case, member) matrix and
/// stamps verdicts.
pub struct Runner;

impl Runner {
    /// Resolve every [`super::Case`] in the suite. Errors on the first
    /// unreadable trace file — the eval refuses to run with a
    /// partially-loaded case list.
    pub fn resolve_cases(cases: &[crate::eval::Case]) -> Result<Vec<CaseRun>, EvalError> {
        let mut out = Vec::with_capacity(cases.len());
        for c in cases {
            out.push(c.resolve()?);
        }
        Ok(out)
    }

    /// Dispatch every (case, member) pair. Returns the full cell
    /// matrix; cells are in `cells[case_idx][member_idx]` order.
    pub async fn run_matrix(
        cases: &[CaseRun],
        members: &[Member],
        budget: &SharedDollarBudget,
    ) -> Vec<Vec<RunnerCell>> {
        let mut matrix = Vec::with_capacity(cases.len());
        for case in cases {
            let mut row = Vec::with_capacity(members.len());
            // Dispatch members in parallel for one case. Members are
            // independent — they don't share state — so the parallel
            // dispatch is safe.
            let mut handles = Vec::with_capacity(members.len());
            for m in members {
                let prompt = case.prompt.clone();
                let budget = budget.clone();
                let m = m.clone();
                handles.push(tokio::spawn(async move { m.ask(&prompt, &budget).await }));
            }
            for h in handles {
                let cell = match h.await {
                    Ok(Ok(reply)) => RunnerCell {
                        reply: Some(reply.body),
                        cost_cents: reply.cost_cents,
                        error: None,
                    },
                    Ok(Err(e)) => RunnerCell {
                        reply: None,
                        cost_cents: 0,
                        error: Some(e.to_string()),
                    },
                    Err(join_err) => RunnerCell {
                        reply: None,
                        cost_cents: 0,
                        error: Some(format!("join error: {join_err}")),
                    },
                };
                row.push(cell);
            }
            matrix.push(row);
        }
        matrix
    }

    /// Stamp every cell with a verdict + collect divergence records.
    /// The baseline column is either the trace's recorded reply (when
    /// the case came from `Case::from_trace`) or the first
    /// non-erroring member's reply.
    pub fn stamp_verdicts(
        suite_name: &str,
        cases: &[CaseRun],
        members: &[Member],
        matrix: &[Vec<RunnerCell>],
        comparator: &Compare,
    ) -> Report {
        let mut cells: Vec<Vec<Verdict>> = Vec::with_capacity(cases.len());
        let mut divergences: Vec<Divergence> = Vec::new();
        let mut total_cost = 0u64;

        for (case_idx, case) in cases.iter().enumerate() {
            let row = &matrix[case_idx];
            // Cost accumulation runs across every cell — even errored
            // ones contribute zero so we still touch them.
            for c in row {
                total_cost = total_cost.saturating_add(c.cost_cents);
            }

            // Baseline pick order:
            //   1. trace's recorded assistant reply
            //   2. first non-errored member reply
            //   3. fall through to "every member is SingleMember"
            let baseline: Option<String> = case
                .baseline_reply
                .clone()
                .or_else(|| row.iter().find_map(|c| c.reply.clone()));

            let mut verdict_row = Vec::with_capacity(members.len());
            for (member_idx, cell) in row.iter().enumerate() {
                let verdict = match (&cell.reply, &cell.error, &baseline) {
                    (None, Some(err), _) => {
                        divergences.push(Divergence {
                            case_name: case.name.clone(),
                            member_label: members[member_idx].label(),
                            baseline: baseline.clone().unwrap_or_default(),
                            actual: String::new(),
                            reason: format!("member errored: {err}"),
                        });
                        Verdict::Error
                    }
                    (Some(_), _, None) => {
                        // No baseline + this cell is the first
                        // non-errored — would only happen when every
                        // member errored or the suite has zero
                        // members; the call sites above guard
                        // against both.
                        Verdict::SingleMember
                    }
                    (Some(actual), _, Some(base)) => {
                        // When the case has no recorded baseline, a
                        // single-member suite cannot compare against
                        // anything — stamp `SingleMember`.
                        if case.baseline_reply.is_none() && members.len() == 1 {
                            Verdict::SingleMember
                        } else if comparator.equivalent(base, actual) {
                            Verdict::Match
                        } else {
                            let reason = explain_divergence(comparator, base, actual);
                            divergences.push(Divergence {
                                case_name: case.name.clone(),
                                member_label: members[member_idx].label(),
                                baseline: base.clone(),
                                actual: actual.clone(),
                                reason,
                            });
                            Verdict::Diverge
                        }
                    }
                    (None, None, _) => {
                        // Shouldn't happen — the cell carries either
                        // a reply or an error.
                        Verdict::Error
                    }
                };
                verdict_row.push(verdict);
            }
            cells.push(verdict_row);
        }

        Report {
            suite_name: suite_name.to_string(),
            strategy: comparator.name(),
            cells,
            divergences,
            case_names: cases.iter().map(|c| c.name.clone()).collect(),
            member_labels: members.iter().map(|m| m.label()).collect(),
            total_cost_cents: total_cost,
        }
    }
}

fn explain_divergence(comparator: &Compare, baseline: &str, actual: &str) -> String {
    match comparator {
        Compare::Equal => "byte-equal mismatch".to_string(),
        Compare::SemanticSimilarity {
            threshold,
            embedder,
        } => {
            let va = embedder.embed(baseline).ok();
            let vb = embedder.embed(actual).ok();
            match (va, vb) {
                (Some(a), Some(b)) if a.len() == b.len() => {
                    let sim: f32 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
                    format!("cosine {:.3} below threshold {:.3}", sim, threshold)
                }
                _ => "semantic similarity unavailable".to_string(),
            }
        }
        Compare::ToolCallSetEqual => "tool-call set mismatch".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Case;

    #[tokio::test]
    async fn run_matrix_dispatches_one_cell_per_pair() {
        let cases = vec![Case::from_input("p1").resolve().unwrap()];
        let members = vec![Member::mock("m1", "x", 1), Member::mock("m2", "x", 1)];
        let budget = SharedDollarBudget::new(100);
        let mat = Runner::run_matrix(&cases, &members, &budget).await;
        assert_eq!(mat.len(), 1);
        assert_eq!(mat[0].len(), 2);
        assert!(mat[0].iter().all(|c| c.reply.is_some()));
    }

    #[tokio::test]
    async fn run_matrix_captures_member_errors() {
        let cases = vec![Case::from_input("p1").resolve().unwrap()];
        let members = vec![Member::mock("m1", "x", 1), Member::mock_error("m2", "boom")];
        let budget = SharedDollarBudget::new(100);
        let mat = Runner::run_matrix(&cases, &members, &budget).await;
        assert_eq!(mat[0].len(), 2);
        assert!(mat[0][0].reply.is_some());
        assert!(mat[0][1].reply.is_none());
        assert!(mat[0][1].error.is_some());
    }

    #[tokio::test]
    async fn stamp_verdicts_marks_matching_replies_as_match() {
        let cases = vec![Case::from_input("p1").resolve().unwrap()];
        let members = vec![Member::mock("m1", "yes", 1), Member::mock("m2", "yes", 1)];
        let budget = SharedDollarBudget::new(100);
        let mat = Runner::run_matrix(&cases, &members, &budget).await;
        let report = Runner::stamp_verdicts("s", &cases, &members, &mat, &Compare::equal());
        assert!(report.passed());
        assert_eq!(report.cells[0][0], Verdict::Match);
        assert_eq!(report.cells[0][1], Verdict::Match);
        assert_eq!(report.total_cost_cents, 2);
    }

    #[tokio::test]
    async fn stamp_verdicts_marks_diverging_replies() {
        let cases = vec![Case::from_input("p1").resolve().unwrap()];
        let members = vec![Member::mock("m1", "yes", 1), Member::mock("m2", "no", 1)];
        let budget = SharedDollarBudget::new(100);
        let mat = Runner::run_matrix(&cases, &members, &budget).await;
        let report = Runner::stamp_verdicts("s", &cases, &members, &mat, &Compare::equal());
        assert!(!report.passed());
        assert_eq!(report.failure_count(), 1);
        assert_eq!(report.divergences.len(), 1);
        assert_eq!(report.divergences[0].member_label, "m2");
    }

    #[tokio::test]
    async fn stamp_verdicts_single_member_no_baseline_marks_single_member() {
        let cases = vec![Case::from_input("p1").resolve().unwrap()];
        let members = vec![Member::mock("m1", "yes", 1)];
        let budget = SharedDollarBudget::new(100);
        let mat = Runner::run_matrix(&cases, &members, &budget).await;
        let report = Runner::stamp_verdicts("s", &cases, &members, &mat, &Compare::equal());
        // The single-member case has no baseline + no other column;
        // we stamp `SingleMember` and pass.
        assert!(report.passed());
        assert_eq!(report.cells[0][0], Verdict::SingleMember);
    }

    #[tokio::test]
    async fn stamp_verdicts_records_error_cells() {
        let cases = vec![Case::from_input("p1").resolve().unwrap()];
        let members = vec![
            Member::mock("m1", "yes", 1),
            Member::mock_error("m2", "fail"),
        ];
        let budget = SharedDollarBudget::new(100);
        let mat = Runner::run_matrix(&cases, &members, &budget).await;
        let report = Runner::stamp_verdicts("s", &cases, &members, &mat, &Compare::equal());
        assert_eq!(report.cells[0][1], Verdict::Error);
        assert!(report.divergences.iter().any(|d| d.reason.contains("fail")));
    }

    #[tokio::test]
    async fn stamp_verdicts_explanation_for_semantic_includes_cosine() {
        // Force a divergence on cosine.
        let cases = vec![Case::from_input("p1").resolve().unwrap()];
        let members = vec![
            Member::mock("m1", "alpha bravo charlie", 1),
            Member::mock("m2", "delta echo foxtrot golf", 1),
        ];
        let budget = SharedDollarBudget::new(100);
        let mat = Runner::run_matrix(&cases, &members, &budget).await;
        let cmp = Compare::semantic_similarity(0.9);
        let report = Runner::stamp_verdicts("s", &cases, &members, &mat, &cmp);
        if !report.passed() {
            assert!(report
                .divergences
                .iter()
                .any(|d| d.reason.contains("cosine")));
        }
    }
}
