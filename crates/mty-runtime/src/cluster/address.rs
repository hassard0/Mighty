//! `node:type:pid` agent addresses.
//!
//! Pre-v0.18 the runtime addressed agents as `type:pid` — the
//! `AgentHandle` carried a name + an `AgentId(u64)` and that was enough
//! because every agent lived in the same process. Tier 4.1 introduces
//! *distributed* agents: a `send` / `ask` may have to cross a node
//! boundary, so the address has to identify which node owns the target.
//!
//! The address gets a third axis: `node`. The local-only constructor
//! [`AgentAddr::local`] stamps the current process's [`NodeId`] (read
//! from `MTY_NODE_ID` at first call and cached); [`AgentAddr::remote`]
//! takes an explicit `node` for cross-process work.
//!
//! ### Design notes
//!
//! - The runtime's existing `AgentId(u64)` and `AgentHandle` are not
//!   modified — we ADD this address shape next to them. Wiring is the
//!   `ClusterRouter` trait in `cluster/mod.rs`, not a refactor of
//!   `AgentHandle`. This keeps the OFF-LIMITS files intact.
//! - We don't use `smol_str` (not in the workspace) — `NodeId` wraps a
//!   plain `String`. Node IDs are short (16-32 bytes) and rarely
//!   re-allocated; the extra word is not worth a new dep.
//! - `AgentAddr` implements `Eq + Hash` so it can key `DashMap`s in the
//!   mesh (e.g. pending-ask correlations).

use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::OnceLock;

/// A node identifier. Free-form short string; the wire protocol uses
/// it verbatim, so two peers MUST agree on the same string for them to
/// route to each other.
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct NodeId(String);

impl NodeId {
    /// Construct from a borrowed or owned string.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Borrow the underlying string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for NodeId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for NodeId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// `node:type:pid` — the distributed-system address of an agent.
///
/// `node` is the [`NodeId`] of the owning process.
/// `agent_type` is the agent's declared name (matches
/// `AgentHandle.name`).
/// `agent_id` is the per-node `AgentId(u64)` — unique within `node`
/// but not necessarily globally.
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct AgentAddr {
    pub node: NodeId,
    pub agent_type: String,
    pub agent_id: u64,
}

impl AgentAddr {
    /// Address of an agent on the *current* node.
    pub fn local(agent_type: impl Into<String>, id: u64) -> Self {
        Self {
            node: current_node_id().clone(),
            agent_type: agent_type.into(),
            agent_id: id,
        }
    }

    /// Address of an agent on a remote node.
    pub fn remote(node: impl Into<NodeId>, agent_type: impl Into<String>, id: u64) -> Self {
        Self {
            node: node.into(),
            agent_type: agent_type.into(),
            agent_id: id,
        }
    }

    /// True iff this address points at the current node.
    pub fn is_local(&self) -> bool {
        &self.node == current_node_id()
    }
}

impl fmt::Display for AgentAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.node, self.agent_type, self.agent_id)
    }
}

/// The current process's node id. Read once from `MTY_NODE_ID` at the
/// first call site, then cached for the lifetime of the process.
/// Default: `"local"`.
pub fn current_node_id() -> &'static NodeId {
    static CACHED: OnceLock<NodeId> = OnceLock::new();
    CACHED.get_or_init(|| {
        std::env::var("MTY_NODE_ID")
            .ok()
            .filter(|s| !s.is_empty())
            .map(NodeId::new)
            .unwrap_or_else(|| NodeId::new("local"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_uses_current_node_id() {
        let a = AgentAddr::local("Greeter", 7);
        assert_eq!(&a.node, current_node_id());
        assert!(a.is_local());
    }

    #[test]
    fn remote_carries_supplied_node() {
        let a = AgentAddr::remote("node-b", "Greeter", 7);
        assert_eq!(a.node.as_str(), "node-b");
    }

    #[test]
    fn display_uses_colon_format() {
        let a = AgentAddr::remote("node-b", "Greeter", 42);
        assert_eq!(format!("{a}"), "node-b:Greeter:42");
    }

    #[test]
    fn equality_is_struct_eq() {
        let a = AgentAddr::remote("n", "A", 1);
        let b = AgentAddr::remote("n", "A", 1);
        let c = AgentAddr::remote("n", "A", 2);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
