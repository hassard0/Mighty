//! Supervisor auto-restart orchestrator (slice 8, completes the
//! slice-7 deferral).
//!
//! Slice 7 shipped the pieces (`RestartTracker`, `Strategy`,
//! `ChildFailure`) but didn't wire them into the agent loop's error
//! path. Slice 8 closes the loop: when an agent panics, the
//! orchestrator looks up its supervisor, applies the strategy,
//! consults the rate-limit window, and either restarts the agent
//! (with backoff) or escalates.

use crate::supervisor::{ChildFailure, RestartPolicy, RestartTracker, Strategy};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Outcome of asking the orchestrator what to do on child failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartDecision {
    /// Restart the failed child after the indicated backoff.
    Restart { backoff: Duration },
    /// Rate limit exceeded — escalate to the supervisor's parent
    /// according to its strategy.
    Escalate,
    /// Drop the failure silently (e.g. strategy is `Escalate` and we're
    /// at the root).
    Drop,
}

#[derive(Debug, Clone)]
pub struct SupervisorBinding {
    pub strategy: Strategy,
    pub restart_limit: u32,
    pub restart_window: Duration,
    pub backoff_min: Duration,
    pub backoff_max: Duration,
    pub children: Vec<u32>,
}

impl Default for SupervisorBinding {
    fn default() -> Self {
        Self {
            strategy: Strategy::OneForOne,
            restart_limit: 5,
            restart_window: Duration::from_secs(60),
            backoff_min: Duration::from_millis(100),
            backoff_max: Duration::from_secs(5),
            children: Vec::new(),
        }
    }
}

#[derive(Default)]
pub struct SupervisorRegistry {
    /// supervisor id → binding
    bindings: HashMap<u32, SupervisorBinding>,
    /// agent id → supervisor id (parent)
    parents: HashMap<u32, u32>,
    /// per-supervisor restart tracker
    trackers: HashMap<u32, RestartTracker>,
    /// next jitter seed (XorShift state) for deterministic backoff
    rng_state: u64,
}

impl SupervisorRegistry {
    pub fn new() -> Self {
        Self {
            rng_state: 0x9E37_79B9_7F4A_7C15,
            ..Default::default()
        }
    }

    pub fn add_supervisor(&mut self, id: u32, binding: SupervisorBinding) {
        for &c in &binding.children {
            self.parents.insert(c, id);
        }
        self.trackers.insert(
            id,
            RestartTracker::new(RestartPolicy {
                max_attempts: binding.restart_limit,
                window: binding.restart_window,
                backoff_min: binding.backoff_min,
                backoff_max: binding.backoff_max,
            }),
        );
        self.bindings.insert(id, binding);
    }

    pub fn parent_of(&self, agent: u32) -> Option<u32> {
        self.parents.get(&agent).copied()
    }

    /// Look at a failure and decide what to do. Mutates the per-sup
    /// tracker (records the failure timestamp).
    pub fn on_child_failure(&mut self, child: u32, _failure: ChildFailure) -> RestartDecision {
        let Some(sup_id) = self.parents.get(&child).copied() else {
            return RestartDecision::Drop;
        };
        let binding = match self.bindings.get(&sup_id) {
            Some(b) => b.clone(),
            None => return RestartDecision::Drop,
        };
        let tracker = self.trackers.entry(sup_id).or_insert_with(|| {
            RestartTracker::new(RestartPolicy {
                max_attempts: binding.restart_limit,
                window: binding.restart_window,
                backoff_min: binding.backoff_min,
                backoff_max: binding.backoff_max,
            })
        });
        let backoff_opt = tracker.may_restart();
        let Some(_existing_backoff) = backoff_opt else {
            return RestartDecision::Escalate;
        };
        match binding.strategy {
            Strategy::OneForOne | Strategy::OneForAll | Strategy::RestForOne => {
                let backoff = self.next_backoff(&binding);
                RestartDecision::Restart { backoff }
            }
            Strategy::Escalate => RestartDecision::Escalate,
        }
    }

    fn next_backoff(&mut self, binding: &SupervisorBinding) -> Duration {
        // XorShift* for deterministic jitter.
        let mut x = self.rng_state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng_state = x;
        let r = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        let span = binding.backoff_max.as_nanos() as u64 - binding.backoff_min.as_nanos() as u64;
        if span == 0 {
            return binding.backoff_min;
        }
        let jitter = r % span;
        binding.backoff_min + Duration::from_nanos(jitter)
    }
}

pub type SharedRegistry = Arc<Mutex<SupervisorRegistry>>;

pub fn shared() -> SharedRegistry {
    Arc::new(Mutex::new(SupervisorRegistry::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_without_parent_drops() {
        let mut r = SupervisorRegistry::new();
        let d = r.on_child_failure(99, ChildFailure::Panic("x".into()));
        assert_eq!(d, RestartDecision::Drop);
    }

    #[test]
    fn one_for_one_restarts_first_time() {
        let mut r = SupervisorRegistry::new();
        r.add_supervisor(
            1,
            SupervisorBinding {
                strategy: Strategy::OneForOne,
                children: vec![10],
                ..Default::default()
            },
        );
        let d = r.on_child_failure(10, ChildFailure::Panic("x".into()));
        assert!(matches!(d, RestartDecision::Restart { .. }));
    }

    #[test]
    fn rate_limit_eventually_escalates() {
        let mut r = SupervisorRegistry::new();
        r.add_supervisor(
            1,
            SupervisorBinding {
                strategy: Strategy::OneForOne,
                restart_limit: 2,
                restart_window: Duration::from_secs(60),
                children: vec![10],
                ..Default::default()
            },
        );
        // Three failures in the window; the third must escalate.
        let _ = r.on_child_failure(10, ChildFailure::Panic("a".into()));
        let _ = r.on_child_failure(10, ChildFailure::Panic("b".into()));
        let d = r.on_child_failure(10, ChildFailure::Panic("c".into()));
        assert_eq!(d, RestartDecision::Escalate);
    }

    #[test]
    fn explicit_escalate_strategy_escalates() {
        let mut r = SupervisorRegistry::new();
        r.add_supervisor(
            1,
            SupervisorBinding {
                strategy: Strategy::Escalate,
                children: vec![10],
                ..Default::default()
            },
        );
        let d = r.on_child_failure(10, ChildFailure::Panic("x".into()));
        assert_eq!(d, RestartDecision::Escalate);
    }
}
