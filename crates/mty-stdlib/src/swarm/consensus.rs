//! Consensus strategies + the [`Consensus`] result shape.
//!
//! Four strategies, picked from the patterns that show up in real
//! multi-LLM workflows:
//!
//! - **Majority** — the most common cluster wins. Members outside
//!   that cluster become `dissents`. Ties break by *first*
//!   cluster-to-form (deterministic across runs).
//! - **Unanimous** — only return a verdict if *every* member agrees
//!   (all in one cluster). Otherwise return `None` for `majority`
//!   and surface all replies in `dissents`.
//! - **WeightedVote** — each member carries a weight; the cluster
//!   with the highest weight-sum wins. Useful when the panel mixes
//!   "expensive frontier model" + "cheap small model" — you want
//!   the frontier's vote to count more.
//! - **FirstAgreed** — as soon as two members land in the same
//!   cluster, return immediately. Drops pending members (so callers
//!   pay less, at the cost of less robustness). The swarm loop
//!   short-circuits dispatch when this strategy is set.

use crate::swarm::member::MemberReply;
use crate::swarm::vote::{cluster_replies, Cluster, SimilarityMode};

/// How the swarm decides which answer wins.
#[derive(Debug, Clone, Default)]
pub enum ConsensusStrategy {
    /// Cluster-with-most-members wins. Dissents are the rest.
    #[default]
    Majority,
    /// Only return if every member lands in one cluster.
    Unanimous,
    /// Weighted vote — `weights[i]` is the weight of `panel[i]` (same
    /// position). Missing weights default to `1`. Cluster with the
    /// highest *weight sum* wins.
    WeightedVote { weights: Vec<u32> },
    /// First two members to land in the same cluster return early.
    /// The swarm loop short-circuits the remaining members.
    FirstAgreed,
}

impl ConsensusStrategy {
    /// True if the strategy benefits from short-circuiting dispatch
    /// — only [`FirstAgreed`] today, but kept as a method so the
    /// swarm loop reads cleanly.
    pub fn supports_early_exit(&self) -> bool {
        matches!(self, ConsensusStrategy::FirstAgreed)
    }
}

/// The swarm's final result.
///
/// `majority` is `None` when no cluster won (Unanimous + disagreement,
/// or all panel members errored).
#[derive(Debug, Clone)]
pub struct Consensus {
    /// Winning cluster's representative body, or `None` if no
    /// consensus emerged.
    pub majority: Option<String>,
    /// Replies that landed *outside* the winning cluster. For
    /// `Unanimous` with no consensus, this is *every* reply.
    pub dissents: Vec<MemberReply>,
    /// All replies, in dispatch order. Useful for "show me the panel
    /// transcript" UI even when `majority` is `Some`.
    pub all_replies: Vec<MemberReply>,
    /// `true` if the shared budget tripped mid-flight and the swarm
    /// dropped one or more pending members. The consensus may still
    /// be `Some` if the members that did run agreed.
    pub budget_exhausted: bool,
    /// Strategy name (`"majority"`, `"unanimous"`, `"weighted"`,
    /// `"first_agreed"`) for observability. Kept as a string so the
    /// shape is loggable without a `Display` impl on the enum.
    pub strategy: &'static str,
}

impl Consensus {
    /// Convenience: number of dissenting members.
    pub fn dissent_count(&self) -> usize {
        self.dissents.len()
    }

    /// Convenience: true when a verdict landed.
    pub fn has_consensus(&self) -> bool {
        self.majority.is_some()
    }
}

/// Resolve a set of replies through `strategy` into a [`Consensus`].
///
/// `weights` is consulted only for `WeightedVote` — for other
/// strategies it's ignored. `budget_exhausted` is plumbed through to
/// the final [`Consensus`] verbatim; this function doesn't poll the
/// budget itself (the swarm-loop owns that).
pub fn resolve(
    replies: Vec<MemberReply>,
    strategy: &ConsensusStrategy,
    budget_exhausted: bool,
) -> Consensus {
    // For zero replies (e.g. budget was exhausted *before* any
    // member ran) every strategy returns `majority: None`.
    if replies.is_empty() {
        return Consensus {
            majority: None,
            dissents: vec![],
            all_replies: vec![],
            budget_exhausted,
            strategy: strategy_name(strategy),
        };
    }

    let bodies: Vec<String> = replies.iter().map(|r| r.body.clone()).collect();
    // Mode + threshold heuristic — short bodies use Exact (yes/no),
    // longer use TokenSet (free-form).
    let mode = if bodies.iter().all(|b| b.len() <= 24) {
        SimilarityMode::Exact
    } else {
        SimilarityMode::TokenSet
    };
    let clusters = cluster_replies(&bodies, mode, 0.6);

    match strategy {
        ConsensusStrategy::Majority => resolve_majority(replies, clusters, budget_exhausted),
        ConsensusStrategy::Unanimous => resolve_unanimous(replies, clusters, budget_exhausted),
        ConsensusStrategy::WeightedVote { weights } => {
            resolve_weighted(replies, clusters, weights, budget_exhausted)
        }
        ConsensusStrategy::FirstAgreed => resolve_first_agreed(replies, clusters, budget_exhausted),
    }
}

fn strategy_name(s: &ConsensusStrategy) -> &'static str {
    match s {
        ConsensusStrategy::Majority => "majority",
        ConsensusStrategy::Unanimous => "unanimous",
        ConsensusStrategy::WeightedVote { .. } => "weighted",
        ConsensusStrategy::FirstAgreed => "first_agreed",
    }
}

fn resolve_majority(
    replies: Vec<MemberReply>,
    clusters: Vec<Cluster>,
    budget_exhausted: bool,
) -> Consensus {
    // Pick the cluster with the most members; tie-break by which
    // cluster formed first (lowest min index).
    let winner_idx = clusters
        .iter()
        .enumerate()
        .max_by_key(|(_, c)| (c.size(), std::cmp::Reverse(c.indices.iter().min().copied())))
        .map(|(i, _)| i);

    let Some(winner_idx) = winner_idx else {
        return Consensus {
            majority: None,
            dissents: replies.clone(),
            all_replies: replies,
            budget_exhausted,
            strategy: "majority",
        };
    };
    let winner = &clusters[winner_idx];

    let mut dissents = Vec::new();
    for (i, r) in replies.iter().enumerate() {
        if !winner.indices.contains(&i) {
            dissents.push(r.clone());
        }
    }

    Consensus {
        majority: Some(winner.representative.clone()),
        dissents,
        all_replies: replies,
        budget_exhausted,
        strategy: "majority",
    }
}

fn resolve_unanimous(
    replies: Vec<MemberReply>,
    clusters: Vec<Cluster>,
    budget_exhausted: bool,
) -> Consensus {
    if clusters.len() == 1 {
        // Every member agreed.
        let winner = &clusters[0];
        Consensus {
            majority: Some(winner.representative.clone()),
            dissents: vec![],
            all_replies: replies,
            budget_exhausted,
            strategy: "unanimous",
        }
    } else {
        // Disagreement — surface every reply as a "dissent" so the
        // caller can see the full split.
        Consensus {
            majority: None,
            dissents: replies.clone(),
            all_replies: replies,
            budget_exhausted,
            strategy: "unanimous",
        }
    }
}

fn resolve_weighted(
    replies: Vec<MemberReply>,
    clusters: Vec<Cluster>,
    weights: &[u32],
    budget_exhausted: bool,
) -> Consensus {
    // weight_sum_per_cluster[i] = sum of weights[idx] for idx in
    // clusters[i].indices. Members missing a weight entry default
    // to 1.
    let cluster_weights: Vec<u32> = clusters
        .iter()
        .map(|c| {
            c.indices
                .iter()
                .map(|i| weights.get(*i).copied().unwrap_or(1))
                .sum::<u32>()
        })
        .collect();
    // Pick the cluster with the highest weight sum; tie-break by
    // member count, then by first-formed.
    let winner_idx = cluster_weights
        .iter()
        .enumerate()
        .max_by_key(|(i, w)| {
            (
                **w,
                clusters[*i].size(),
                std::cmp::Reverse(clusters[*i].indices.iter().min().copied()),
            )
        })
        .map(|(i, _)| i);

    let Some(winner_idx) = winner_idx else {
        return Consensus {
            majority: None,
            dissents: replies.clone(),
            all_replies: replies,
            budget_exhausted,
            strategy: "weighted",
        };
    };
    let winner = &clusters[winner_idx];

    let mut dissents = Vec::new();
    for (i, r) in replies.iter().enumerate() {
        if !winner.indices.contains(&i) {
            dissents.push(r.clone());
        }
    }
    Consensus {
        majority: Some(winner.representative.clone()),
        dissents,
        all_replies: replies,
        budget_exhausted,
        strategy: "weighted",
    }
}

fn resolve_first_agreed(
    replies: Vec<MemberReply>,
    clusters: Vec<Cluster>,
    budget_exhausted: bool,
) -> Consensus {
    // For FirstAgreed the swarm loop already short-circuited once
    // two members agreed — by the time we reach `resolve` the
    // `replies` slice only carries the members that actually ran.
    // The first cluster of size >= 2 wins; if no cluster reached
    // size 2, we have no consensus.
    let winner = clusters.iter().find(|c| c.size() >= 2);
    if let Some(winner) = winner {
        let mut dissents = Vec::new();
        for (i, r) in replies.iter().enumerate() {
            if !winner.indices.contains(&i) {
                dissents.push(r.clone());
            }
        }
        Consensus {
            majority: Some(winner.representative.clone()),
            dissents,
            all_replies: replies,
            budget_exhausted,
            strategy: "first_agreed",
        }
    } else {
        Consensus {
            majority: None,
            dissents: replies.clone(),
            all_replies: replies,
            budget_exhausted,
            strategy: "first_agreed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(name: &str, body: &str) -> MemberReply {
        MemberReply {
            member: name.into(),
            body: body.into(),
            tokens_used: 1,
            cost_cents: 1,
            tool_uses: Vec::new(),
        }
    }

    #[test]
    fn majority_picks_largest_cluster() {
        let rs = vec![reply("a", "yes"), reply("b", "yes"), reply("c", "no")];
        let c = resolve(rs, &ConsensusStrategy::Majority, false);
        assert_eq!(c.majority.as_deref(), Some("yes"));
        assert_eq!(c.dissent_count(), 1);
        assert_eq!(c.strategy, "majority");
    }

    #[test]
    fn unanimous_yields_no_consensus_on_split() {
        let rs = vec![reply("a", "yes"), reply("b", "no")];
        let c = resolve(rs, &ConsensusStrategy::Unanimous, false);
        assert!(c.majority.is_none());
        assert_eq!(c.dissent_count(), 2);
    }

    #[test]
    fn unanimous_when_all_agree() {
        let rs = vec![reply("a", "yes"), reply("b", "yes"), reply("c", "yes")];
        let c = resolve(rs, &ConsensusStrategy::Unanimous, false);
        assert_eq!(c.majority.as_deref(), Some("yes"));
        assert!(c.dissents.is_empty());
    }

    #[test]
    fn weighted_picks_highest_weighted_cluster() {
        // Three members, two agree on "yes" with weight 1+1, one
        // says "no" with weight 5. Weighted picks "no".
        let rs = vec![reply("a", "yes"), reply("b", "yes"), reply("c", "no")];
        let c = resolve(
            rs,
            &ConsensusStrategy::WeightedVote {
                weights: vec![1, 1, 5],
            },
            false,
        );
        assert_eq!(c.majority.as_deref(), Some("no"));
    }

    #[test]
    fn first_agreed_needs_a_cluster_of_two() {
        let rs = vec![reply("a", "yes"), reply("b", "yes")];
        let c = resolve(rs, &ConsensusStrategy::FirstAgreed, false);
        assert_eq!(c.majority.as_deref(), Some("yes"));
    }

    #[test]
    fn first_agreed_no_consensus_when_all_disagree() {
        let rs = vec![reply("a", "x"), reply("b", "y")];
        let c = resolve(rs, &ConsensusStrategy::FirstAgreed, false);
        assert!(c.majority.is_none());
    }

    #[test]
    fn empty_replies_returns_no_consensus_for_every_strategy() {
        for s in [
            ConsensusStrategy::Majority,
            ConsensusStrategy::Unanimous,
            ConsensusStrategy::FirstAgreed,
            ConsensusStrategy::WeightedVote { weights: vec![] },
        ] {
            let c = resolve(vec![], &s, false);
            assert!(c.majority.is_none());
            assert!(c.dissents.is_empty());
        }
    }
}
