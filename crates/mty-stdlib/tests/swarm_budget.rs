//! Shared-budget enforcement tests for `std.swarm`.
//!
//! Two invariants the budget tier guarantees:
//!
//! 1. The shared pool counts *every* member's cost — so a 3-member
//!    panel with `cost=10` each consumes 30 cents from a single
//!    50-cent cap.
//! 2. When the pool trips mid-flight, pending members are dropped
//!    and the consensus surfaces with `budget_exhausted: true`. The
//!    members that already ran still resolve through the strategy.

use mty_stdlib::swarm::{ConsensusStrategy, Member, SharedDollarBudget, swarm};

#[tokio::test]
async fn swarm_under_budget_completes_all_members() {
    let panel = vec![
        Member::mock("claude", "yes", 5),
        Member::mock("gpt", "yes", 5),
        Member::mock("gemini", "yes", 5),
    ];
    let budget = SharedDollarBudget::new(50);
    let c = swarm("x", panel, budget.clone(), ConsensusStrategy::Unanimous)
        .await
        .expect("swarm ok");
    assert_eq!(c.all_replies.len(), 3);
    assert!(!c.budget_exhausted);
    assert_eq!(budget.consumed_cents(), 15);
}

#[tokio::test]
async fn swarm_per_member_cost_decrements_shared_budget() {
    let panel = vec![Member::mock("a", "yes", 7), Member::mock("b", "yes", 11)];
    let budget = SharedDollarBudget::new(100);
    let _ = swarm("x", panel, budget.clone(), ConsensusStrategy::Majority)
        .await
        .unwrap();
    assert_eq!(budget.consumed_cents(), 18);
}

#[tokio::test]
async fn swarm_budget_exhausted_drops_remaining_members_in_first_agreed() {
    // FirstAgreed dispatches sequentially. When the budget trips
    // mid-flight, the remaining members are dropped. We size the
    // budget so the first two members fit but the third would
    // overshoot.
    let m1 = Member::mock("a", "yes", 15);
    let m2 = Member::mock("b", "yes", 15);
    let m3 = Member::mock("c", "no", 15);
    // Snapshot the call counters for assertion later.
    let counter_a = if let Member::Mock(inner) = &m1 {
        inner.call_count.clone()
    } else {
        unreachable!()
    };
    let counter_c = if let Member::Mock(inner) = &m3 {
        inner.call_count.clone()
    } else {
        unreachable!()
    };

    let budget = SharedDollarBudget::new(40);
    let c = swarm(
        "x",
        vec![m1, m2, m3],
        budget.clone(),
        ConsensusStrategy::FirstAgreed,
    )
    .await
    .expect("swarm ok");
    // First two members agreed; FirstAgreed exited after the second
    // reply. The third was never dispatched.
    assert!(c.majority.is_some());
    assert_eq!(c.all_replies.len(), 2);
    assert_eq!(*counter_a.lock().unwrap(), 1);
    assert_eq!(*counter_c.lock().unwrap(), 0);
}

#[tokio::test]
async fn swarm_budget_exhausted_surfaces_flag_when_partial() {
    // Sequential strategy (FirstAgreed) with the budget set too low
    // for all three members. After member 1 + 2 land in the same
    // cluster, the swarm short-circuits cleanly — budget_exhausted
    // false since FirstAgreed reached consensus first. To force the
    // flag, use disagreeing members so FirstAgreed keeps dispatching
    // until the budget trips.
    let panel = vec![
        Member::mock("a", "alpha", 15),
        Member::mock("b", "beta", 15),
        Member::mock("c", "gamma", 15),
        Member::mock("d", "delta", 15),
    ];
    let budget = SharedDollarBudget::new(35);
    let c = swarm("x", panel, budget.clone(), ConsensusStrategy::FirstAgreed)
        .await
        .expect("swarm ok");
    // The first three replies overshoot the budget — first reply
    // charges 15 (consumed=15), second 15 (consumed=30), third 15
    // (consumed=45 > 35). On the next iteration, `is_exhausted()`
    // returns true and the swarm drops the rest with
    // `budget_exhausted: true`.
    assert!(c.budget_exhausted, "expected budget_exhausted flag");
    // No two members agreed, so majority is None.
    assert!(c.majority.is_none());
}

#[tokio::test]
async fn swarm_pre_exhausted_budget_returns_swarm_error() {
    let panel = vec![Member::mock("a", "yes", 5), Member::mock("b", "yes", 5)];
    let budget = SharedDollarBudget::new(10);
    // Manually consume the budget before dispatch.
    let _ = budget.try_charge(100);

    let r = swarm("x", panel, budget, ConsensusStrategy::Majority).await;
    // Every member's `ask` short-circuits with BudgetExhausted, so
    // we fall into AllMembersFailed.
    assert!(r.is_err());
}

#[tokio::test]
async fn swarm_majority_strategy_charges_every_member_in_parallel() {
    // Majority dispatches *in parallel* — every member runs, so
    // the shared budget reflects the sum of all per-call costs.
    let panel = vec![
        Member::mock("a", "yes", 3),
        Member::mock("b", "yes", 4),
        Member::mock("c", "no", 5),
        Member::mock("d", "yes", 6),
    ];
    let budget = SharedDollarBudget::new(100);
    let _ = swarm("x", panel, budget.clone(), ConsensusStrategy::Majority)
        .await
        .unwrap();
    assert_eq!(budget.consumed_cents(), 3 + 4 + 5 + 6);
}
