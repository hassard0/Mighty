//! Tier 1 agent introspection (v0.16, see
//! `docs/internals/agent-features-roadmap.md`).
//!
//! Computes a [`RuntimeSnapshot`] / [`AgentSnapshot`] from live
//! runtime state without going through the agent's per-turn handler
//! dispatch. The snapshot types are deliberately a separate,
//! serializable wire-stable surface — they are *NOT* references into
//! the runtime's internal agent/budget/mailbox types. See the
//! [INTROSPECT_V0_16_NOTES.md](../../../dev/history/notes/INTROSPECT_V0_16_NOTES.md)
//! note for the wire-version policy.
//!
//! ## Wire version
//!
//! The wire version is **1**. Future versions may *add* fields, but
//! must never rename or repurpose existing ones. CLI consumers gate on
//! `version >= 1`.
//!
//! ## Message-body capture
//!
//! The per-agent `last_messages` ring is empty by default — message
//! bodies can carry sensitive data, so capture is opt-in via the
//! `MTY_INSPECT_CAPTURE_BODIES=1` environment variable.

use crate::agent::{AgentDescriptor, AgentRegistry};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Wire-format version of the snapshot payload. Bumped only for
/// breaking changes; additive fields keep the same version.
pub const SNAPSHOT_WIRE_VERSION: u32 = 1;

/// Default ring-buffer size for captured messages. Small on purpose —
/// snapshots have to fit in a single control-socket frame.
pub const DEFAULT_RING_CAPACITY: usize = 8;

/// Per-agent snapshot. See module docs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSnapshot {
    /// Wire-format version. Always [`SNAPSHOT_WIRE_VERSION`] today.
    pub version: u32,
    /// Runtime-internal pid (the `AgentId.0` u64).
    pub agent_id: u64,
    /// Agent type as declared in source (e.g. `"echo::Worker"`).
    pub agent_type: String,
    /// Runtime-internal pid of the supervising agent, if any.
    pub supervisor_parent: Option<u64>,
    /// Best-effort mailbox depth (number of frames currently queued).
    pub mailbox_depth: usize,
    /// High-water mark for `mailbox_depth` since spawn.
    pub mailbox_high_water: usize,
    /// Name of the handler currently executing, if any.
    pub in_flight_handler: Option<String>,
    /// Wall time elapsed in the in-flight handler, in milliseconds.
    pub in_flight_elapsed_ms: Option<u64>,
    /// Budget consumption + limits.
    pub budget: BudgetSnapshot,
    /// Last N messages handled. Empty unless `MTY_INSPECT_CAPTURE_BODIES=1`.
    pub last_messages: Vec<String>,
}

/// Budget consumption snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetSnapshot {
    pub mem_used_bytes: u64,
    pub mem_limit_bytes: Option<u64>,
    pub ticks_used: u64,
    pub ticks_limit: Option<u64>,
    /// Remaining wall-budget in milliseconds, if a wall budget is set.
    pub deadline_ms: Option<u64>,
}

/// Whole-runtime snapshot. Returned by `{"op":"snapshot"}` on the
/// control socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    pub version: u32,
    pub agents: Vec<AgentSnapshot>,
    pub worker_count: usize,
    /// Unix milliseconds when the snapshot was taken.
    pub timestamp_ms: u64,
}

/// Tracks introspection-only per-agent state — mailbox high-water,
/// in-flight handler, last-N message ring. Held side-by-side with
/// the agent descriptor; the per-turn evaluator updates it directly.
#[derive(Debug)]
pub struct AgentIntrospectState {
    pub mailbox_depth: AtomicU64,
    pub mailbox_high_water: AtomicU64,
    pub in_flight_handler: Mutex<Option<InFlight>>,
    pub last_messages: Mutex<VecDeque<String>>,
    pub ring_capacity: usize,
}

#[derive(Debug, Clone)]
pub struct InFlight {
    pub handler: String,
    pub started: Instant,
}

impl Default for AgentIntrospectState {
    fn default() -> Self {
        Self::new(DEFAULT_RING_CAPACITY)
    }
}

impl AgentIntrospectState {
    pub fn new(ring_capacity: usize) -> Self {
        Self {
            mailbox_depth: AtomicU64::new(0),
            mailbox_high_water: AtomicU64::new(0),
            in_flight_handler: Mutex::new(None),
            last_messages: Mutex::new(VecDeque::with_capacity(ring_capacity)),
            ring_capacity,
        }
    }

    /// Note an incoming message (called on enqueue).
    pub fn note_enqueue(&self) {
        let d = self.mailbox_depth.fetch_add(1, Ordering::Relaxed) + 1;
        let mut hw = self.mailbox_high_water.load(Ordering::Relaxed);
        while d > hw {
            match self.mailbox_high_water.compare_exchange_weak(
                hw,
                d,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => hw = actual,
            }
        }
    }

    /// Note a message removed from the mailbox (called on dequeue).
    pub fn note_dequeue(&self) {
        // Saturating: depth can't go below 0.
        let prev = self.mailbox_depth.load(Ordering::Relaxed);
        if prev > 0 {
            let _ = self.mailbox_depth.compare_exchange(
                prev,
                prev - 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }
    }

    /// Note that a handler started. Records a body string in the ring
    /// when [`capture_bodies_enabled`] returns true.
    pub fn note_handler_start(&self, handler: &str, body: Option<String>) {
        *self.in_flight_handler.lock() = Some(InFlight {
            handler: handler.to_string(),
            started: Instant::now(),
        });
        if let Some(body) = body {
            let mut ring = self.last_messages.lock();
            if ring.len() == self.ring_capacity {
                ring.pop_front();
            }
            ring.push_back(body);
        }
    }

    pub fn note_handler_end(&self) {
        *self.in_flight_handler.lock() = None;
    }
}

/// `true` when the runtime should capture message bodies in each
/// agent's introspect ring. Off by default — bodies can carry user
/// data. Reads the env var on every call (so tests can flip it).
pub fn capture_bodies_enabled() -> bool {
    static CACHED: AtomicBool = AtomicBool::new(false);
    static INIT: AtomicBool = AtomicBool::new(false);
    // Fast path: read cached value if we've initialized.
    if INIT.load(Ordering::Acquire) {
        return CACHED.load(Ordering::Relaxed);
    }
    let v = std::env::var("MTY_INSPECT_CAPTURE_BODIES")
        .map(|s| !s.is_empty() && s != "0" && s != "false")
        .unwrap_or(false);
    CACHED.store(v, Ordering::Relaxed);
    INIT.store(true, Ordering::Release);
    v
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Compute a snapshot for a single agent. Reads atomics + the
/// introspect-state ring; never invokes a handler.
pub fn snapshot_agent(
    desc: &AgentDescriptor,
    introspect: Option<&AgentIntrospectState>,
) -> AgentSnapshot {
    let budget = desc.budget.budget().clone();
    let mem_used = desc.budget.mem_used();
    let cpu_ns = desc.budget.cpu_ns_used();
    let deadline_ms = budget
        .wall
        .map(|d| d.as_millis() as u64)
        .map(|wall_ms| wall_ms.saturating_sub(desc.budget.elapsed_ms()));

    // Live mailbox depth: read from the bounded channel itself rather
    // than maintaining a parallel counter. Senders/receivers don't have
    // to learn anything new.
    let live_depth = desc.mailbox.introspect().channel_used;

    let (mailbox_depth, mailbox_high_water, in_flight_handler, in_flight_elapsed_ms, last_messages) =
        if let Some(intr) = introspect {
            // Bump high-water to live depth before reading. Cheap
            // CAS-loop on a u64 atomic.
            let mut hw = intr.mailbox_high_water.load(Ordering::Relaxed);
            while (live_depth as u64) > hw {
                match intr.mailbox_high_water.compare_exchange_weak(
                    hw,
                    live_depth as u64,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(actual) => hw = actual,
                }
            }
            let hw_out = intr.mailbox_high_water.load(Ordering::Relaxed) as usize;
            let (h, elapsed) = intr
                .in_flight_handler
                .lock()
                .as_ref()
                .map(|inf| {
                    (
                        Some(inf.handler.clone()),
                        Some(inf.started.elapsed().as_millis() as u64),
                    )
                })
                .unwrap_or((None, None));
            let ring: Vec<String> = intr.last_messages.lock().iter().cloned().collect();
            (live_depth, hw_out, h, elapsed, ring)
        } else {
            (live_depth, live_depth, None, None, Vec::new())
        };

    AgentSnapshot {
        version: SNAPSHOT_WIRE_VERSION,
        agent_id: desc.id.0,
        agent_type: desc.name.clone(),
        supervisor_parent: desc.supervisor.map(|p| p.0),
        mailbox_depth,
        mailbox_high_water,
        in_flight_handler,
        in_flight_elapsed_ms,
        budget: BudgetSnapshot {
            mem_used_bytes: mem_used,
            mem_limit_bytes: budget.mem_bytes,
            ticks_used: cpu_ns,
            ticks_limit: budget.cpu.map(|d| d.as_nanos() as u64),
            deadline_ms,
        },
        last_messages,
    }
}

/// Compute a whole-runtime snapshot from the registry + an optional
/// introspect map. Snapshot creation never blocks an agent.
pub fn snapshot_runtime(
    registry: &AgentRegistry,
    introspect: &IntrospectMap,
    worker_count: usize,
) -> RuntimeSnapshot {
    let mut agents: Vec<AgentSnapshot> = registry
        .iter()
        .into_iter()
        .map(|desc| {
            let intr = introspect.get(desc.id.0);
            snapshot_agent(&desc, intr.as_deref())
        })
        .collect();
    agents.sort_by_key(|a| a.agent_id);
    RuntimeSnapshot {
        version: SNAPSHOT_WIRE_VERSION,
        agents,
        worker_count,
        timestamp_ms: now_unix_ms(),
    }
}

/// Concurrent map of `agent_id -> AgentIntrospectState`. Wrapped here
/// so the runtime doesn't have to import dashmap directly.
#[derive(Debug, Default)]
pub struct IntrospectMap {
    inner: dashmap::DashMap<u64, Arc<AgentIntrospectState>>,
}

impl IntrospectMap {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn insert(&self, id: u64, state: Arc<AgentIntrospectState>) {
        self.inner.insert(id, state);
    }
    pub fn get(&self, id: u64) -> Option<Arc<AgentIntrospectState>> {
        self.inner.get(&id).map(|r| r.clone())
    }
    pub fn remove(&self, id: u64) {
        self.inner.remove(&id);
    }
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

/// One-line list entry for the `{"op":"list"}` socket op.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentListEntry {
    pub agent_id: u64,
    pub agent_type: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_version_is_one() {
        assert_eq!(SNAPSHOT_WIRE_VERSION, 1);
    }

    #[test]
    fn high_water_tracks_max_depth() {
        let st = AgentIntrospectState::new(4);
        for _ in 0..5 {
            st.note_enqueue();
        }
        for _ in 0..3 {
            st.note_dequeue();
        }
        assert_eq!(st.mailbox_depth.load(Ordering::Relaxed), 2);
        assert_eq!(st.mailbox_high_water.load(Ordering::Relaxed), 5);
    }

    #[test]
    fn ring_buffer_caps_at_capacity() {
        let st = AgentIntrospectState::new(3);
        for i in 0..6 {
            st.note_handler_start("h", Some(format!("m{i}")));
            st.note_handler_end();
        }
        let ring = st.last_messages.lock();
        assert_eq!(ring.len(), 3);
        assert_eq!(ring[0], "m3");
        assert_eq!(ring[2], "m5");
    }

    #[test]
    fn snapshot_serdes_round_trip() {
        let snap = AgentSnapshot {
            version: 1,
            agent_id: 42,
            agent_type: "Echo".into(),
            supervisor_parent: Some(1),
            mailbox_depth: 3,
            mailbox_high_water: 7,
            in_flight_handler: Some("Ping".into()),
            in_flight_elapsed_ms: Some(12),
            budget: BudgetSnapshot {
                mem_used_bytes: 1024,
                mem_limit_bytes: Some(8192),
                ticks_used: 5_000_000,
                ticks_limit: Some(100_000_000),
                deadline_ms: Some(80),
            },
            last_messages: vec!["Ping".into()],
        };
        let s = serde_json::to_string(&snap).unwrap();
        let back: AgentSnapshot = serde_json::from_str(&s).unwrap();
        assert_eq!(back.agent_id, snap.agent_id);
        assert_eq!(back.agent_type, snap.agent_type);
        assert_eq!(back.mailbox_high_water, 7);
    }
}
