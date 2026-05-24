//! Supervisor engine (spec §15).

use crate::error::RuntimeError;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    OneForOne = 0,
    OneForAll = 1,
    RestForOne = 2,
    Escalate = 3,
}

#[derive(Debug, Clone)]
pub struct RestartPolicy {
    pub max_attempts: u32,
    pub window: Duration,
    pub backoff_min: Duration,
    pub backoff_max: Duration,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            window: Duration::from_secs(30),
            backoff_min: Duration::from_millis(0),
            backoff_max: Duration::from_millis(0),
        }
    }
}

#[derive(Debug)]
pub struct RestartTracker {
    policy: RestartPolicy,
    attempts: Vec<Instant>,
    rng_seed: u64,
}

impl RestartTracker {
    pub fn new(policy: RestartPolicy) -> Self {
        Self {
            policy,
            attempts: Vec::new(),
            rng_seed: 0xDEAD_BEEF,
        }
    }

    /// Returns Some(backoff) if a restart is allowed; None if the
    /// limit has been hit within the current window.
    pub fn may_restart(&mut self) -> Option<Duration> {
        let now = Instant::now();
        self.attempts
            .retain(|t| now.duration_since(*t) < self.policy.window);
        if (self.attempts.len() as u32) >= self.policy.max_attempts {
            return None;
        }
        self.attempts.push(now);
        Some(self.sample_backoff())
    }

    fn sample_backoff(&mut self) -> Duration {
        let lo = self.policy.backoff_min.as_nanos() as u64;
        let hi = self.policy.backoff_max.as_nanos() as u64;
        if hi <= lo {
            return Duration::from_nanos(lo);
        }
        // Tiny LCG for jitter; deterministic given seed.
        self.rng_seed = self
            .rng_seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let span = hi - lo;
        let pick = lo + (self.rng_seed % span);
        Duration::from_nanos(pick)
    }
}

#[derive(Debug, Clone)]
pub enum ChildFailure {
    Panic(String),
    Budget(String),
    Deadline,
}

#[derive(Debug)]
pub struct ChildFailureEvent {
    pub child: u64,
    pub failure: ChildFailure,
}

impl From<RuntimeError> for ChildFailure {
    fn from(e: RuntimeError) -> Self {
        match e {
            RuntimeError::BudgetExceeded(k) => ChildFailure::Budget(k),
            RuntimeError::DeadlineExceeded(_) => ChildFailure::Deadline,
            other => ChildFailure::Panic(other.to_string()),
        }
    }
}

#[derive(Debug, Default)]
pub struct SupervisorRegistry;

impl SupervisorRegistry {
    pub fn new() -> Self {
        Self
    }
}
