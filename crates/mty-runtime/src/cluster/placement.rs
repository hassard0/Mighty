//! Placement policy for cluster-supervised agents (Tier 4.3, v0.21).
//!
//! v0.20's cluster supervisor knew how to react to a peer disconnect
//! ("every child on the dead node is now `:noproc`") but had no opinion
//! on *where* the restart should land — it always emitted a
//! [`crate::cluster::supervisor::SupervisorEvent::RestartRequested`] for
//! the same `AgentAddr` (i.e. the node that just went down). v0.21
//! ships [`PlacementPolicy`] so the supervisor can ask "given the
//! cluster's current shape, where should this child run next?"
//!
//! ### The three default policies
//!
//! - [`StickyPolicy`] — restart on the same node when it's still
//!   reachable, otherwise fall back to the least-loaded node. The
//!   default. Closest match to v0.20 semantics; preserves agent /
//!   node affinity (which matters when the agent has built a local
//!   cache, holds a host-side resource, etc.).
//! - [`LeastLoadedPolicy`] — always pick the node with the smallest
//!   `child_count_per_node`. Best for stateless workers where you
//!   want even spread.
//! - [`StaticPolicy`] — always return a single configured node.
//!   Useful for tests and for "send every restart to the spare" setups.
//!
//! ### Why a trait, not an enum
//!
//! User code can implement [`PlacementPolicy`] for their own routing
//! (e.g. consistent hashing keyed on `agent_id`, GPU-aware placement,
//! tag-based affinity). The trait is `Send + Sync` so it can live behind
//! the same `Arc` shape the supervisor uses for its other hooks.

use crate::cluster::address::NodeId;
use crate::cluster::supervisor::ChildSpec;
use std::collections::HashMap;

/// Snapshot of the cluster shape the policy needs to make a decision.
///
/// The supervisor builds one of these per restart event from the
/// current peer map + child registry. The `available_nodes` list is
/// *just* the nodes that are reachable right now — a policy can rely
/// on every entry being a live target.
#[derive(Debug, Clone, Default)]
pub struct PlacementContext {
    /// Reachable nodes (this node + every connected peer).
    pub available_nodes: Vec<NodeId>,
    /// The node the child used to live on, if any. `None` for a fresh
    /// spawn that's never been placed; `Some` for a restart after
    /// crash / disconnect / migration.
    pub current_node: Option<NodeId>,
    /// Per-node child counts. Read-only snapshot; the supervisor
    /// updates the underlying counters as restart events are emitted.
    pub child_count_per_node: HashMap<NodeId, usize>,
}

impl PlacementContext {
    /// Convenience constructor for tests.
    pub fn new(available: impl IntoIterator<Item = NodeId>) -> Self {
        let nodes: Vec<NodeId> = available.into_iter().collect();
        Self {
            available_nodes: nodes,
            current_node: None,
            child_count_per_node: HashMap::new(),
        }
    }

    /// Return a copy with `current_node` set.
    pub fn with_current_node(mut self, node: NodeId) -> Self {
        self.current_node = Some(node);
        self
    }

    /// Replace the per-node child-count map.
    pub fn with_child_counts(mut self, counts: HashMap<NodeId, usize>) -> Self {
        self.child_count_per_node = counts;
        self
    }

    /// Helper: pick the entry with the smallest count, breaking ties
    /// by lexicographic node id so the choice is deterministic across
    /// processes and test runs.
    pub(crate) fn least_loaded(&self) -> Option<NodeId> {
        let mut best: Option<(NodeId, usize)> = None;
        for node in &self.available_nodes {
            let count = self
                .child_count_per_node
                .get(node)
                .copied()
                .unwrap_or(0usize);
            best = match best {
                None => Some((node.clone(), count)),
                Some((_, c)) if count < c => Some((node.clone(), count)),
                Some((ref b, c)) if count == c && node.as_str() < b.as_str() => {
                    Some((node.clone(), count))
                }
                other => other,
            };
        }
        best.map(|(n, _)| n)
    }
}

/// Policy plug-in surface. Given a child spec + cluster snapshot,
/// return the node id the supervisor should aim the restart at.
///
/// The policy is allowed to return `current_node` if it would prefer
/// to keep the child where it was. Callers MUST be prepared for the
/// returned node to currently be unreachable (e.g. a sticky policy
/// that doesn't downgrade when the source disappears); in practice
/// the bundled policies all check `available_nodes` first.
pub trait PlacementPolicy: Send + Sync + 'static {
    fn place(&self, child: &ChildSpec, ctx: &PlacementContext) -> NodeId;

    /// Human-readable name used in telemetry and the docs. Defaults to
    /// the policy's type name in `Debug` form for ergonomics.
    fn name(&self) -> &'static str {
        "custom"
    }
}

/// Default policy: restart on the same node if it's still reachable,
/// otherwise hop to the least-loaded reachable node.
#[derive(Debug, Default, Clone, Copy)]
pub struct StickyPolicy;

impl PlacementPolicy for StickyPolicy {
    fn place(&self, _child: &ChildSpec, ctx: &PlacementContext) -> NodeId {
        if let Some(current) = ctx.current_node.as_ref() {
            if ctx.available_nodes.iter().any(|n| n == current) {
                return current.clone();
            }
        }
        // Fall back to least-loaded. If `available_nodes` is empty we
        // return the current node unchanged so the caller surfaces the
        // empty-cluster condition (which is genuinely fatal —
        // `available_nodes.is_empty()` means there's nowhere to place
        // anything, including the original).
        ctx.least_loaded()
            .or_else(|| ctx.current_node.clone())
            .unwrap_or_else(|| NodeId::new("local"))
    }

    fn name(&self) -> &'static str {
        "sticky"
    }
}

/// Always pick the reachable node with the smallest child count.
#[derive(Debug, Default, Clone, Copy)]
pub struct LeastLoadedPolicy;

impl PlacementPolicy for LeastLoadedPolicy {
    fn place(&self, _child: &ChildSpec, ctx: &PlacementContext) -> NodeId {
        ctx.least_loaded()
            .or_else(|| ctx.current_node.clone())
            .unwrap_or_else(|| NodeId::new("local"))
    }

    fn name(&self) -> &'static str {
        "least-loaded"
    }
}

/// Always place on a configured node. Useful for tests + "send every
/// restart to the spare" deployment shapes.
#[derive(Debug, Clone)]
pub struct StaticPolicy(pub NodeId);

impl StaticPolicy {
    pub fn new(node: impl Into<NodeId>) -> Self {
        Self(node.into())
    }
}

impl PlacementPolicy for StaticPolicy {
    fn place(&self, _child: &ChildSpec, _ctx: &PlacementContext) -> NodeId {
        self.0.clone()
    }

    fn name(&self) -> &'static str {
        "static"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::address::AgentAddr;
    use crate::cluster::supervisor::RestartPolicy;

    fn spec(node: &str, ty: &str, id: u64) -> ChildSpec {
        ChildSpec {
            addr: AgentAddr::remote(node, ty, id),
            restart: RestartPolicy::Permanent,
            max_restarts: 5,
            window_ms: 30_000,
        }
    }

    #[test]
    fn sticky_keeps_current_when_reachable() {
        let ctx = PlacementContext::new([NodeId::new("a"), NodeId::new("b")])
            .with_current_node(NodeId::new("a"));
        let p = StickyPolicy;
        assert_eq!(p.place(&spec("a", "X", 1), &ctx).as_str(), "a");
    }

    #[test]
    fn sticky_falls_back_when_current_unreachable() {
        // Only "b" is reachable; current was "a" — should move to "b".
        let mut counts = HashMap::new();
        counts.insert(NodeId::new("b"), 3);
        let ctx = PlacementContext::new([NodeId::new("b")])
            .with_current_node(NodeId::new("a"))
            .with_child_counts(counts);
        let p = StickyPolicy;
        assert_eq!(p.place(&spec("a", "X", 1), &ctx).as_str(), "b");
    }

    #[test]
    fn least_loaded_picks_smallest_count() {
        let mut counts = HashMap::new();
        counts.insert(NodeId::new("a"), 5);
        counts.insert(NodeId::new("b"), 1);
        counts.insert(NodeId::new("c"), 3);
        let ctx = PlacementContext::new([NodeId::new("a"), NodeId::new("b"), NodeId::new("c")])
            .with_child_counts(counts);
        let p = LeastLoadedPolicy;
        assert_eq!(p.place(&spec("a", "X", 1), &ctx).as_str(), "b");
    }

    #[test]
    fn least_loaded_breaks_ties_lexicographically() {
        let ctx = PlacementContext::new([NodeId::new("c"), NodeId::new("a"), NodeId::new("b")]);
        // All counts default to 0; lex-min is "a".
        let p = LeastLoadedPolicy;
        assert_eq!(p.place(&spec("z", "X", 1), &ctx).as_str(), "a");
    }

    #[test]
    fn static_always_returns_configured_node() {
        let p = StaticPolicy::new("hardcoded");
        let ctx = PlacementContext::new([NodeId::new("a"), NodeId::new("b")]);
        assert_eq!(p.place(&spec("a", "X", 1), &ctx).as_str(), "hardcoded");
    }

    #[test]
    fn names_match_manifest_keys() {
        assert_eq!(StickyPolicy.name(), "sticky");
        assert_eq!(LeastLoadedPolicy.name(), "least-loaded");
        assert_eq!(StaticPolicy::new("x").name(), "static");
    }
}
