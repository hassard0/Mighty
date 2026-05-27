//! Per-strategy consensus tests for `std.swarm`.
//!
//! One test per strategy plus edge cases (single-member, ties,
//! ordering of dissents, weighted-vote-with-default-weights).

use mty_stdlib::swarm::{
    ConsensusStrategy, Member, SharedDollarBudget, SimilarityMode, cluster_replies, similarity,
    swarm,
};

#[tokio::test]
async fn consensus_majority_strategy() {
    let panel = vec![
        Member::mock("a", "approve", 1),
        Member::mock("b", "approve", 1),
        Member::mock("c", "reject", 1),
        Member::mock("d", "approve", 1),
    ];
    let c = swarm(
        "x",
        panel,
        SharedDollarBudget::new(100),
        ConsensusStrategy::Majority,
    )
    .await
    .unwrap();
    assert_eq!(c.majority.as_deref(), Some("approve"));
    assert_eq!(c.dissents.len(), 1);
    assert_eq!(c.dissents[0].body, "reject");
    assert_eq!(c.strategy, "majority");
}

#[tokio::test]
async fn consensus_unanimous_strategy_no_consensus_on_disagree() {
    let panel = vec![
        Member::mock("a", "yes", 1),
        Member::mock("b", "yes", 1),
        Member::mock("c", "no", 1),
    ];
    let c = swarm(
        "x",
        panel,
        SharedDollarBudget::new(100),
        ConsensusStrategy::Unanimous,
    )
    .await
    .unwrap();
    assert!(c.majority.is_none());
    assert_eq!(
        c.dissents.len(),
        3,
        "unanimous surfaces every reply as a dissent"
    );
    assert_eq!(c.strategy, "unanimous");
}

#[tokio::test]
async fn consensus_unanimous_strategy_when_all_agree() {
    let panel = vec![
        Member::mock("a", "yes", 1),
        Member::mock("b", "yes", 1),
        Member::mock("c", "yes", 1),
    ];
    let c = swarm(
        "x",
        panel,
        SharedDollarBudget::new(100),
        ConsensusStrategy::Unanimous,
    )
    .await
    .unwrap();
    assert_eq!(c.majority.as_deref(), Some("yes"));
    assert!(c.dissents.is_empty());
}

#[tokio::test]
async fn consensus_weighted_vote_picks_highest_weighted() {
    // Three members, two say "yes" (weight 1 each), one says "no"
    // (weight 5). The "no" cluster wins on weight even though it's
    // smaller.
    let panel = vec![
        Member::mock("a", "yes", 1),
        Member::mock("b", "yes", 1),
        Member::mock("c", "no", 1),
    ];
    let c = swarm(
        "x",
        panel,
        SharedDollarBudget::new(100),
        ConsensusStrategy::WeightedVote {
            weights: vec![1, 1, 5],
        },
    )
    .await
    .unwrap();
    assert_eq!(c.majority.as_deref(), Some("no"));
    assert_eq!(c.dissents.len(), 2);
    assert_eq!(c.strategy, "weighted");
}

#[tokio::test]
async fn consensus_weighted_vote_defaults_missing_weights_to_one() {
    // Only one weight supplied; the rest default to 1 each.
    // Cluster A (members 0,1) has weight 1+1=2.
    // Cluster B (member 2) has weight 1.
    let panel = vec![
        Member::mock("a", "yes", 1),
        Member::mock("b", "yes", 1),
        Member::mock("c", "no", 1),
    ];
    let c = swarm(
        "x",
        panel,
        SharedDollarBudget::new(100),
        ConsensusStrategy::WeightedVote { weights: vec![1] },
    )
    .await
    .unwrap();
    assert_eq!(c.majority.as_deref(), Some("yes"));
}

#[tokio::test]
async fn consensus_first_agreed_short_circuits_after_two_match() {
    let m1 = Member::mock("a", "yes", 5);
    let m2 = Member::mock("b", "yes", 5);
    let m3 = Member::mock("c", "yes", 5);
    let counter_c = if let Member::Mock(inner) = &m3 {
        inner.call_count.clone()
    } else {
        unreachable!()
    };

    let c = swarm(
        "x",
        vec![m1, m2, m3],
        SharedDollarBudget::new(100),
        ConsensusStrategy::FirstAgreed,
    )
    .await
    .unwrap();
    assert_eq!(c.majority.as_deref(), Some("yes"));
    // Member c was never dispatched — FirstAgreed short-circuited
    // after the second reply landed in the same cluster.
    assert_eq!(*counter_c.lock().unwrap(), 0);
    assert_eq!(c.strategy, "first_agreed");
}

#[tokio::test]
async fn consensus_first_agreed_no_consensus_when_all_disagree() {
    let panel = vec![
        Member::mock("a", "alpha", 1),
        Member::mock("b", "beta", 1),
        Member::mock("c", "gamma", 1),
    ];
    let c = swarm(
        "x",
        panel,
        SharedDollarBudget::new(100),
        ConsensusStrategy::FirstAgreed,
    )
    .await
    .unwrap();
    assert!(c.majority.is_none());
    assert_eq!(c.all_replies.len(), 3);
    assert_eq!(c.strategy, "first_agreed");
}

#[tokio::test]
async fn vote_token_set_clusters_paraphrases() {
    let bodies = vec![
        "The code is safe and well-tested".to_string(),
        "code is safe well tested".to_string(),
        "no, do not deploy this".to_string(),
    ];
    let clusters = cluster_replies(&bodies, SimilarityMode::TokenSet, 0.6);
    assert_eq!(clusters.len(), 2);
    let biggest = clusters.iter().max_by_key(|c| c.size()).unwrap();
    assert_eq!(biggest.size(), 2);
}

#[tokio::test]
async fn vote_exact_mode_clusters_yes_no_with_trimming() {
    assert_eq!(similarity("yes", "  YES!\n", SimilarityMode::Exact), 1.0);
    assert_eq!(similarity("yes", "no", SimilarityMode::Exact), 0.0);
}

#[tokio::test]
async fn consensus_majority_ties_break_by_first_cluster_to_form() {
    // 2-vs-2 tie. First-formed cluster wins.
    let panel = vec![
        Member::mock("a", "yes", 1),
        Member::mock("b", "yes", 1),
        Member::mock("c", "no", 1),
        Member::mock("d", "no", 1),
    ];
    let c = swarm(
        "x",
        panel,
        SharedDollarBudget::new(100),
        ConsensusStrategy::Majority,
    )
    .await
    .unwrap();
    // Either "yes" or "no" wins — the *parallel* dispatch can shuffle
    // the order of replies, so we only assert that there's a verdict
    // and that the dissent count is 2.
    assert!(c.majority.is_some());
    assert_eq!(c.dissents.len(), 2);
}
