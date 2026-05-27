//! End-to-end tests for `std.swarm` — multi-LLM consensus primitive.
//!
//! v0.27 Track D. Tests use the `Member::mock` constructor (canned
//! reply + per-call cost) so the panel runs deterministically without
//! touching any real provider. Every test exercises the full
//! `swarm()` entry point + the per-strategy resolution path.

use mty_stdlib::swarm::{ConsensusStrategy, Member, SharedDollarBudget, SwarmError, swarm};

#[tokio::test]
async fn swarm_with_three_members_all_agree_yields_unanimous() {
    let panel = vec![
        Member::mock("claude", "yes", 5),
        Member::mock("gpt", "yes", 4),
        Member::mock("gemini", "yes", 3),
    ];
    let budget = SharedDollarBudget::new(100);
    let consensus = swarm(
        "Is this code safe?",
        panel,
        budget,
        ConsensusStrategy::Unanimous,
    )
    .await
    .expect("swarm ok");
    assert_eq!(consensus.majority.as_deref(), Some("yes"));
    assert!(consensus.dissents.is_empty());
    assert_eq!(consensus.all_replies.len(), 3);
    assert!(!consensus.budget_exhausted);
    assert_eq!(consensus.strategy, "unanimous");
}

#[tokio::test]
async fn swarm_with_split_panel_yields_majority() {
    let panel = vec![
        Member::mock("claude", "yes", 5),
        Member::mock("gpt", "yes", 4),
        Member::mock("gemini", "no", 3),
    ];
    let consensus = swarm(
        "Is this code safe?",
        panel,
        SharedDollarBudget::new(100),
        ConsensusStrategy::Majority,
    )
    .await
    .expect("swarm ok");
    assert_eq!(consensus.majority.as_deref(), Some("yes"));
    assert_eq!(consensus.dissents.len(), 1);
    assert_eq!(consensus.dissents[0].member, "gemini");
    assert_eq!(consensus.dissents[0].body, "no");
}

#[tokio::test]
async fn swarm_with_single_member_short_circuits() {
    let panel = vec![Member::mock("only", "the answer", 7)];
    let consensus = swarm(
        "x",
        panel,
        SharedDollarBudget::new(100),
        ConsensusStrategy::Majority,
    )
    .await
    .expect("swarm ok");
    assert_eq!(consensus.majority.as_deref(), Some("the answer"));
    assert!(consensus.dissents.is_empty());
    assert_eq!(consensus.all_replies.len(), 1);
}

#[tokio::test]
async fn empty_panel_errors_with_empty_panel() {
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
async fn swarm_collects_per_member_cost_into_replies() {
    let panel = vec![
        Member::mock("claude", "yes", 7),
        Member::mock("gpt", "yes", 11),
    ];
    let budget = SharedDollarBudget::new(100);
    let consensus = swarm("x", panel, budget.clone(), ConsensusStrategy::Majority)
        .await
        .expect("swarm ok");
    assert_eq!(consensus.all_replies.len(), 2);
    let total: u64 = consensus.all_replies.iter().map(|r| r.cost_cents).sum();
    assert_eq!(total, 18);
    assert_eq!(budget.consumed_cents(), 18);
}

#[tokio::test]
async fn swarm_continues_when_a_single_member_errors() {
    let panel = vec![
        Member::mock("claude", "yes", 5),
        Member::mock_error("broken", "kaboom"),
        Member::mock("gemini", "yes", 5),
    ];
    let consensus = swarm(
        "x",
        panel,
        SharedDollarBudget::new(100),
        ConsensusStrategy::Majority,
    )
    .await
    .expect("swarm ok");
    // The two healthy members agreed; the broken member dropped out.
    assert_eq!(consensus.majority.as_deref(), Some("yes"));
    assert_eq!(consensus.all_replies.len(), 2);
}

#[tokio::test]
async fn swarm_with_all_members_failing_returns_swarm_error() {
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
    match r {
        Err(SwarmError::AllMembersFailed(first)) => {
            // The first error surfaces; order isn't guaranteed across
            // parallel dispatch, but it must contain *one* member's
            // body.
            assert!(first.contains("boom"));
        }
        other => panic!("expected AllMembersFailed, got {other:?}"),
    }
}
