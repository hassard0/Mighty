//! Wire format for deterministic-replay traces (v0.17, Tier 1.4 in
//! `docs/internals/agent-features-roadmap.md`).
//!
//! ## Wire-version policy
//!
//! [`TRACE_WIRE_VERSION`] is bumped only for breaking changes; additive
//! variant fields use serde defaults so old readers still decode new
//! writers (and vice-versa for tail-only events). The replayer refuses
//! traces with `version > TRACE_WIRE_VERSION` to keep the contract
//! one-way-stable.
//!
//! ## Format choice
//!
//! Postcard is preferred for its compact varint encoding and
//! no-std-friendly story. When the optional `replay-postcard` feature
//! is not enabled (or in tests that want a human-readable diff), the
//! `bincode2` JSON fallback is gated behind `to_json` / `from_json` in
//! [`super::mod`]. The wire types themselves are codec-agnostic — every
//! field implements `Serialize` + `Deserialize`.
//!
//! ## Privacy
//!
//! [`TraceEvent::MessageSent`] / [`TraceEvent::IoRead`] capture raw
//! bytes. Recording is opt-in via the `MTY_RECORD_TRACE` environment
//! variable (see [`super::recorder`]). Documented in
//! `docs/reference/cli/mty-replay.md`.

use serde::{Deserialize, Serialize};

/// Current wire-format version. Bumped only for breaking changes.
pub const TRACE_WIRE_VERSION: u32 = 1;

/// Magic bytes prefix for postcard-encoded trace files. Lets `mty
/// replay` reject random binaries before attempting full decode.
pub const TRACE_MAGIC: &[u8; 8] = b"MTYTRACE";

/// Top-level container serialized to disk.
///
/// Layout on disk (postcard mode):
/// ```text
/// [8 bytes MAGIC] [postcard-encoded TraceFile]
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraceFile {
    /// Wire-format version. See [`TRACE_WIRE_VERSION`].
    pub version: u32,
    /// Unix milliseconds when the recording started.
    pub created_at_ms: u64,
    /// Seed used to derive every deterministic-rand stream in the
    /// recorded run. Replay re-seeds from this so RNG draws line up.
    pub runtime_seed: u64,
    /// Number of worker threads in the recorded run. Replay normally
    /// re-runs with `workers(1)` for determinism, but the field is
    /// preserved for diagnostics.
    pub worker_count: u32,
    /// Ordered event log. Append-only during recording; iterated in
    /// order during replay.
    pub events: Vec<TraceEvent>,
}

/// One captured runtime event. Variants are append-only — never
/// rename, repurpose, or reorder.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TraceEvent {
    /// A new agent was spawned.
    Spawn {
        agent_id: u64,
        agent_type: String,
        /// Optional supervisor parent (the spawning agent), if any.
        #[serde(default)]
        supervisor: Option<u64>,
    },
    /// A message was placed on the target's mailbox.
    MessageSent {
        from: u64,
        to: u64,
        /// Protocol message name (e.g. `"Ping"`).
        msg: String,
        /// Opaque serialized payload bytes. May be empty.
        #[serde(default)]
        payload: Vec<u8>,
    },
    /// A message was dispatched to its handler.
    MessageHandled {
        agent: u64,
        /// Sequence number within this agent's handled stream. Used by
        /// the replayer to detect skipped messages.
        msg_idx: u64,
        msg: String,
        /// Wall-clock microseconds the handler took. Replay uses this
        /// to advance the logical clock.
        elapsed_us: u64,
    },
    /// External IO read (file / network / stdin). Bytes are exactly
    /// what the runtime returned to user code.
    IoRead {
        agent: u64,
        /// Logical source label (`"file:/etc/foo"`, `"net:1.2.3.4"`).
        source: String,
        bytes: Vec<u8>,
    },
    /// `std.time.now_ms` (or equivalent) read.
    ClockRead { agent: u64, value_ms: u64 },
    /// `std.random.fill` (or equivalent) read.
    RandomRead { agent: u64, bytes: Vec<u8> },
    /// Agent's budget tripped during the run. Carries the human-
    /// readable reason for replayer/debugger display.
    BudgetExhausted { agent: u64, reason: String },
    /// Agent exited normally (terminated). Recorded so the replayer
    /// can step through full lifecycles.
    Exit {
        agent: u64,
        /// Free-form reason string (`"normal"`, `"trap:MT5020"`).
        reason: String,
    },
}

impl TraceEvent {
    /// Stable short name for human-readable summary output.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Spawn { .. } => "spawn",
            Self::MessageSent { .. } => "message_sent",
            Self::MessageHandled { .. } => "message_handled",
            Self::IoRead { .. } => "io_read",
            Self::ClockRead { .. } => "clock_read",
            Self::RandomRead { .. } => "random_read",
            Self::BudgetExhausted { .. } => "budget_exhausted",
            Self::Exit { .. } => "exit",
        }
    }

    /// Agent that owns the event, when meaningful. Used by the
    /// summary command + replayer for grouping. `MessageSent` returns
    /// the recipient (the agent whose mailbox grew).
    pub fn agent(&self) -> Option<u64> {
        match self {
            Self::Spawn { agent_id, .. } => Some(*agent_id),
            Self::MessageSent { to, .. } => Some(*to),
            Self::MessageHandled { agent, .. }
            | Self::IoRead { agent, .. }
            | Self::ClockRead { agent, .. }
            | Self::RandomRead { agent, .. }
            | Self::BudgetExhausted { agent, .. }
            | Self::Exit { agent, .. } => Some(*agent),
        }
    }
}

/// Aggregate summary returned by [`TraceFile::summary`] — used by the
/// CLI for the default "no flags" mode.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraceSummary {
    pub version: u32,
    pub created_at_ms: u64,
    pub runtime_seed: u64,
    pub worker_count: u32,
    pub event_count: usize,
    pub agent_count: usize,
    pub spawn_count: usize,
    pub message_sent_count: usize,
    pub message_handled_count: usize,
    pub io_read_count: usize,
    pub clock_read_count: usize,
    pub random_read_count: usize,
    pub budget_exhausted_count: usize,
    pub exit_count: usize,
    /// Total elapsed microseconds across recorded handler dispatches.
    pub total_handler_elapsed_us: u64,
}

impl TraceFile {
    /// Build a fresh, empty trace anchored to the given seed + clock.
    pub fn new(runtime_seed: u64, created_at_ms: u64, worker_count: u32) -> Self {
        Self {
            version: TRACE_WIRE_VERSION,
            created_at_ms,
            runtime_seed,
            worker_count,
            events: Vec::new(),
        }
    }

    /// Compute an aggregate summary in one pass. Used by `mty replay`
    /// in its default validate-and-summarize mode.
    pub fn summary(&self) -> TraceSummary {
        use std::collections::BTreeSet;
        let mut spawn = 0;
        let mut sent = 0;
        let mut handled = 0;
        let mut io = 0;
        let mut clock = 0;
        let mut random = 0;
        let mut budget = 0;
        let mut exit = 0;
        let mut elapsed_us = 0_u64;
        let mut agents: BTreeSet<u64> = BTreeSet::new();
        for e in &self.events {
            if let Some(id) = e.agent() {
                agents.insert(id);
            }
            match e {
                TraceEvent::Spawn { .. } => spawn += 1,
                TraceEvent::MessageSent { .. } => sent += 1,
                TraceEvent::MessageHandled { elapsed_us: us, .. } => {
                    handled += 1;
                    elapsed_us = elapsed_us.saturating_add(*us);
                }
                TraceEvent::IoRead { .. } => io += 1,
                TraceEvent::ClockRead { .. } => clock += 1,
                TraceEvent::RandomRead { .. } => random += 1,
                TraceEvent::BudgetExhausted { .. } => budget += 1,
                TraceEvent::Exit { .. } => exit += 1,
            }
        }
        TraceSummary {
            version: self.version,
            created_at_ms: self.created_at_ms,
            runtime_seed: self.runtime_seed,
            worker_count: self.worker_count,
            event_count: self.events.len(),
            agent_count: agents.len(),
            spawn_count: spawn,
            message_sent_count: sent,
            message_handled_count: handled,
            io_read_count: io,
            clock_read_count: clock,
            random_read_count: random,
            budget_exhausted_count: budget,
            exit_count: exit,
            total_handler_elapsed_us: elapsed_us,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_version_is_one() {
        assert_eq!(TRACE_WIRE_VERSION, 1);
    }

    #[test]
    fn magic_is_eight_bytes() {
        assert_eq!(TRACE_MAGIC.len(), 8);
    }

    #[test]
    fn empty_summary() {
        let t = TraceFile::new(42, 1_000, 4);
        let s = t.summary();
        assert_eq!(s.event_count, 0);
        assert_eq!(s.agent_count, 0);
        assert_eq!(s.runtime_seed, 42);
        assert_eq!(s.worker_count, 4);
    }

    #[test]
    fn summary_groups_by_agent_and_kind() {
        let mut t = TraceFile::new(0, 0, 1);
        t.events.push(TraceEvent::Spawn {
            agent_id: 1,
            agent_type: "Echo".into(),
            supervisor: None,
        });
        t.events.push(TraceEvent::Spawn {
            agent_id: 2,
            agent_type: "Echo".into(),
            supervisor: Some(1),
        });
        t.events.push(TraceEvent::MessageSent {
            from: 1,
            to: 2,
            msg: "Ping".into(),
            payload: vec![],
        });
        t.events.push(TraceEvent::MessageHandled {
            agent: 2,
            msg_idx: 0,
            msg: "Ping".into(),
            elapsed_us: 250,
        });
        t.events.push(TraceEvent::Exit {
            agent: 2,
            reason: "normal".into(),
        });
        let s = t.summary();
        assert_eq!(s.event_count, 5);
        assert_eq!(s.agent_count, 2);
        assert_eq!(s.spawn_count, 2);
        assert_eq!(s.message_sent_count, 1);
        assert_eq!(s.message_handled_count, 1);
        assert_eq!(s.exit_count, 1);
        assert_eq!(s.total_handler_elapsed_us, 250);
    }

    #[test]
    fn event_kind_matches_variant() {
        let e = TraceEvent::ClockRead {
            agent: 7,
            value_ms: 12,
        };
        assert_eq!(e.kind(), "clock_read");
        assert_eq!(e.agent(), Some(7));
    }

    #[test]
    fn message_sent_agent_is_recipient() {
        let e = TraceEvent::MessageSent {
            from: 1,
            to: 99,
            msg: "X".into(),
            payload: vec![],
        };
        // We want grouping by mailbox-owner.
        assert_eq!(e.agent(), Some(99));
    }
}
