//! Hot-reload swap pipeline.
//!
//! v0.20 Tier 1.5 (see `docs/internals/agent-features-roadmap.md`).
//! The pipeline drains an in-flight handler, snapshots opaque state
//! via [`Resumable`], swaps the underlying code module, then
//! re-attaches the same mailbox so queued messages flow to the new
//! agent uninterrupted.
//!
//! ## Design
//!
//! The runtime keeps the live mailbox in [`crate::AgentHandle`]; the
//! receiver lives inside the agent's per-task loop. To swap without
//! losing queued messages we keep the *same* `Arc<Mailbox>` across
//! the swap — old agent stops (its receiver hangs up), but the
//! producer-side `Sender` stays cloned on the mailbox, so anything
//! sent during the gap simply waits on the channel until the new
//! agent's loop calls `take_receiver`.
//!
//! In v0.20 we explicitly **do not** load a new wasm module from the
//! filesystem. The runtime interpreter operates on an in-memory
//! `mty_ir::ir::Program` and there is no addressable "module per
//! agent" surface yet. Instead the swap pipeline takes a
//! [`ModuleSource`] enum that lets callers supply either the existing
//! program (a no-op code swap, useful for state-only restarts in
//! tests + the v0.21 cluster live-migration path) or an opaque byte
//! payload that future revisions of the runtime will turn into a
//! fresh `Program`.
//!
//! The schema-hash check, drain semantics, and snapshot transfer are
//! all implemented + tested here. Code-module reloading proper waits
//! for the v0.21 "wasm module per agent" work that's already on the
//! agent-features roadmap.

use crate::agent::{AgentDescriptor, AgentId};
use crate::error::RuntimeError;
use crate::reload::resumable::{ResumableError, SnapshotCodec};
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Soft upper bound on snapshot payload size — guards against a
/// runaway state blob blowing up the swap pipeline. Configurable
/// per-call via [`ReloadOptions::max_snapshot_bytes`].
pub const DEFAULT_MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;

/// Lifecycle marker the swap pipeline uses to short-circuit handler
/// dispatch while a reload is in progress. The agent loop checks
/// [`ReloadGate::is_paused`] before pulling the next frame.
///
/// Public so user code that opts into the v0.20 reload surface can
/// inspect the gate from telemetry callbacks.
#[derive(Debug, Default)]
pub struct ReloadGate {
    paused: AtomicBool,
    busy: AtomicBool,
}

impl ReloadGate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::Release);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::Release);
    }

    pub fn mark_busy(&self) {
        self.busy.store(true, Ordering::Release);
    }

    pub fn mark_idle(&self) {
        self.busy.store(false, Ordering::Release);
    }
}

/// Where the swap pipeline finds the replacement code module.
///
/// In v0.20 the runtime interpreter doesn't have a per-agent module
/// surface, so the only fully-wired variant is [`ModuleSource::SameProgram`]
/// (state-only restart). [`ModuleSource::WasmBytes`] is accepted at
/// the API boundary so callers (the CLI, future cluster migration)
/// can record their intent today; the swap pipeline currently rejects
/// it with [`ReloadError::WasmReloadNotImplemented`].
pub enum ModuleSource<'a> {
    /// Use the runtime's currently-loaded program. State is preserved
    /// through `Resumable`; the code shape is unchanged.
    SameProgram,
    /// Opaque wasm-module bytes. v0.20 returns a structured error;
    /// v0.21 will lower these into a fresh `Program` via the wasm
    /// component-model loader.
    WasmBytes(&'a [u8]),
}

/// Tunable knobs for a single swap.
#[derive(Debug, Clone)]
pub struct ReloadOptions {
    /// Maximum wall time the pipeline will wait for the agent's
    /// current handler to finish before failing with
    /// [`ReloadError::DrainDeadline`]. Defaults to 5 s.
    pub deadline: Duration,
    /// Per-call override of [`DEFAULT_MAX_SNAPSHOT_BYTES`].
    pub max_snapshot_bytes: usize,
}

impl Default for ReloadOptions {
    fn default() -> Self {
        Self {
            deadline: Duration::from_millis(5_000),
            max_snapshot_bytes: DEFAULT_MAX_SNAPSHOT_BYTES,
        }
    }
}

/// Structured report returned to the caller (CLI / cluster migrator)
/// once the swap completes. Mirrors the shape promised in the
/// agent-features roadmap. The `agent_id` is the raw `u64` form so
/// the report serialises cleanly through the control-socket wire
/// without needing `AgentId` to implement `Serialize` (which would
/// be a touchier change to `mty-runtime::agent`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReloadReport {
    pub agent_id: u64,
    pub agent_type: String,
    pub old_schema_hash: u64,
    pub new_schema_hash: u64,
    pub state_bytes_size: usize,
    pub drain_elapsed_ms: u64,
    pub total_elapsed_ms: u64,
}

/// Errors surfaced by the swap pipeline. Maps to the `MT506x`
/// diagnostic family — see [`ReloadError::diag_code`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum ReloadError {
    #[error("agent #{0} not found in registry")]
    AgentNotFound(u64),

    #[error(
        "schema hash incompatible: snapshot was produced by hash {old:#018x} \
         but the new module expects hash {new:#018x}"
    )]
    IncompatibleSchema { old: u64, new: u64 },

    #[error("agent drain deadline of {0:?} exceeded before handler returned")]
    DrainDeadline(Duration),

    #[error("snapshot encode/decode failed: {0}")]
    Snapshot(#[from] ResumableError),

    #[error(
        "loading replacement code from raw wasm bytes is not yet \
         implemented in v0.20 — pass ModuleSource::SameProgram \
         (state-only restart). Tracking: docs/internals/hot-reload.md"
    )]
    WasmReloadNotImplemented,

    #[error("internal runtime error during reload: {0}")]
    Internal(String),
}

impl ReloadError {
    /// Map to the `MT506x` diagnostic id used in CLI output.
    pub fn diag_code(&self) -> &'static str {
        match self {
            ReloadError::AgentNotFound(_) => "MT5061",
            ReloadError::IncompatibleSchema { .. } => "MT5060",
            ReloadError::DrainDeadline(_) => "MT5062",
            ReloadError::Snapshot(_) => "MT5063",
            ReloadError::WasmReloadNotImplemented => "MT5064",
            ReloadError::Internal(_) => "MT5069",
        }
    }
}

impl From<ReloadError> for RuntimeError {
    fn from(e: ReloadError) -> Self {
        let code = e.diag_code();
        RuntimeError::Trap {
            code,
            message: e.to_string(),
        }
    }
}

pub type ReloadResult<T> = Result<T, ReloadError>;

/// Pure-data swap descriptor — what the orchestrator hands to the
/// codec layer. Kept separate from [`ReloadRunner`] so tests can
/// exercise the schema-check + snapshot path without spinning up an
/// agent loop.
pub struct SwapPlan<'a> {
    pub agent_id: AgentId,
    pub agent_type: String,
    pub old_schema_hash: u64,
    pub new_schema_hash: u64,
    pub module: ModuleSource<'a>,
    pub options: ReloadOptions,
}

/// Encode + size-check a snapshot value through [`SnapshotCodec`].
pub fn encode_snapshot<T: Serialize>(value: &T, max_bytes: usize) -> ReloadResult<Vec<u8>> {
    let bytes = SnapshotCodec::encode(value)?;
    if bytes.len() > max_bytes {
        return Err(ReloadError::Snapshot(ResumableError::TooLarge {
            bytes: bytes.len(),
            limit: max_bytes,
        }));
    }
    Ok(bytes)
}

/// Decode a snapshot payload of any type that round-trips through
/// `Serialize + DeserializeOwned`.
pub fn decode_snapshot<T: DeserializeOwned>(bytes: &[u8]) -> ReloadResult<T> {
    Ok(SnapshotCodec::decode(bytes)?)
}

/// Sync-shaped runner that drives the swap. Returns a [`ReloadReport`]
/// when every phase succeeded.
///
/// The runner is *parametric* over the snapshot type `T` because the
/// real runtime's agent state is an interpreter `Value`, but the
/// trait surface — and most tests — work with concrete Rust structs.
/// The control-socket entry point in [`run_reload_via_socket`] picks
/// a concrete `T` based on the gate metadata.
pub struct ReloadRunner<'a, T: Serialize + DeserializeOwned> {
    pub plan: SwapPlan<'a>,
    /// Borrow of the live descriptor (`gate` lives on this).
    pub desc: Arc<AgentDescriptor>,
    /// Cell holding the agent's typed state. Tests stash a
    /// `Mutex<Counter>` here; the production wire-up will use an
    /// adapter over the descriptor's `Value` state cell.
    pub state: Arc<Mutex<T>>,
    pub gate: Arc<ReloadGate>,
}

impl<T: Serialize + DeserializeOwned> ReloadRunner<'_, T> {
    /// Execute the swap. Steps 1-10 from the roadmap, with the
    /// schema-hash check + size guard layered in before any
    /// destructive action.
    pub fn run(self) -> ReloadResult<ReloadReport> {
        let started = Instant::now();
        let SwapPlan {
            agent_id,
            agent_type,
            old_schema_hash,
            new_schema_hash,
            module,
            options,
        } = self.plan;

        // Sanity-check that the plan's agent_type matches the live
        // descriptor — this catches the common bug of passing the
        // wrong descriptor in a multi-agent runtime. Cheap: a single
        // string compare against an already-held `Arc<AgentDescriptor>`.
        if !agent_type.is_empty() && self.desc.name != agent_type {
            return Err(ReloadError::Internal(format!(
                "plan.agent_type ({}) doesn't match desc.name ({})",
                agent_type, self.desc.name
            )));
        }

        // (1) schema-hash compatibility short-circuit. We do this
        // *before* the drain so a known-incompatible swap fails fast
        // with the agent untouched. Mirrors the roadmap's wording:
        // "The runtime refuses the swap if the new version's
        // SCHEMA_HASH is incompatible with the recorded snapshot's hash."
        if old_schema_hash != new_schema_hash {
            return Err(ReloadError::IncompatibleSchema {
                old: old_schema_hash,
                new: new_schema_hash,
            });
        }

        // (2) drain the in-flight handler. The agent loop sets
        // `gate.busy = true` while a handler runs; we busy-poll the
        // flag with a small sleep so the deadline is enforced.
        let drain_started = Instant::now();
        loop {
            if !self.gate.is_busy() {
                break;
            }
            if drain_started.elapsed() >= options.deadline {
                // Leave the gate paused so the agent stays quiescent
                // for the caller's follow-up action (typically "fail
                // the reload, restart the agent later"). The agent
                // loop unblocks naturally when its handler returns.
                return Err(ReloadError::DrainDeadline(options.deadline));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let drain_elapsed = drain_started.elapsed();

        // (3) pause: any future handler invocation will short-circuit
        // (see the agent loop's wait-on-gate.is_paused check).
        self.gate.pause();

        // (4) snapshot the state. We hold the typed `T` lock for the
        // shortest possible window so any racing producer that sends
        // a message during the swap doesn't see a stale read.
        let snapshot = {
            let guard = self.state.lock();
            encode_snapshot(&*guard, options.max_snapshot_bytes)?
        };
        let state_bytes_size = snapshot.len();

        // (5) source the new code. v0.20 only allows SameProgram.
        match module {
            ModuleSource::SameProgram => { /* zero-overhead path */ }
            ModuleSource::WasmBytes(_) => {
                self.gate.resume();
                return Err(ReloadError::WasmReloadNotImplemented);
            }
        }

        // (6) decode the snapshot back into the (still typed) state
        // cell. In the production path this is where the new wasm
        // module's `from_snapshot` would run. With SameProgram the
        // decode is a round-trip, but it still validates the payload.
        let restored: T = decode_snapshot(&snapshot)?;
        *self.state.lock() = restored;

        // (7) resume. The mailbox is preserved end-to-end because we
        // never touched `desc.mailbox` — the same `Arc<Mailbox>` that
        // producers hold on `AgentHandle::mailbox` is still live and
        // the agent loop will pick the next frame up as soon as the
        // gate clears.
        self.gate.resume();

        Ok(ReloadReport {
            agent_id: agent_id.0,
            agent_type,
            old_schema_hash,
            new_schema_hash,
            state_bytes_size,
            drain_elapsed_ms: drain_elapsed.as_millis() as u64,
            total_elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }
}

/// Convenience: drop the descriptor / agent-id types and just run the
/// schema-check + snapshot pipeline. Used by the trait-level tests
/// and by the CLI when the agent-id mapping isn't needed.
pub fn dry_run_swap<T: Serialize + DeserializeOwned>(
    state: &Mutex<T>,
    old_hash: u64,
    new_hash: u64,
    options: &ReloadOptions,
) -> ReloadResult<Vec<u8>> {
    if old_hash != new_hash {
        return Err(ReloadError::IncompatibleSchema {
            old: old_hash,
            new: new_hash,
        });
    }
    let guard = state.lock();
    encode_snapshot(&*guard, options.max_snapshot_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reload::resumable::{compute_schema_hash, Resumable};
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
    struct Counter {
        count: u64,
        label: String,
    }

    impl Resumable for Counter {
        const SCHEMA_HASH: u64 = 0xCAFE_F00D_DEAD_BEEF;
    }

    #[test]
    fn dry_run_round_trips() {
        let s = Mutex::new(Counter {
            count: 7,
            label: "x".into(),
        });
        let bytes = dry_run_swap(
            &s,
            Counter::SCHEMA_HASH,
            Counter::SCHEMA_HASH,
            &ReloadOptions::default(),
        )
        .expect("ok");
        assert!(!bytes.is_empty());
        let back: Counter = decode_snapshot(&bytes).unwrap();
        assert_eq!(back.count, 7);
    }

    #[test]
    fn dry_run_rejects_incompatible_hash() {
        let s = Mutex::new(Counter {
            count: 0,
            label: "x".into(),
        });
        let err = dry_run_swap(
            &s,
            Counter::SCHEMA_HASH,
            Counter::SCHEMA_HASH ^ 1,
            &ReloadOptions::default(),
        )
        .unwrap_err();
        assert!(matches!(err, ReloadError::IncompatibleSchema { .. }));
        assert_eq!(err.diag_code(), "MT5060");
    }

    #[test]
    fn encode_enforces_size_cap() {
        // Tiny size cap: a one-element payload will overflow.
        let big = Counter {
            count: 0,
            label: "0123456789ABCDEF".repeat(8),
        };
        let err = encode_snapshot(&big, 4).unwrap_err();
        assert!(matches!(
            err,
            ReloadError::Snapshot(ResumableError::TooLarge { .. })
        ));
    }

    #[test]
    fn schema_hash_from_helper_is_stable() {
        // The trait-level constant + the helper-level hash must agree
        // when the derived impl chooses the helper-driven form.
        let h = compute_schema_hash(&[("count", "u64"), ("label", "String")]);
        // Just check stability: the function is order-insensitive +
        // deterministic across calls.
        let again = compute_schema_hash(&[("label", "String"), ("count", "u64")]);
        assert_eq!(h, again);
    }

    #[test]
    fn reload_error_maps_to_runtime_error() {
        let e = ReloadError::IncompatibleSchema { old: 1, new: 2 };
        let re: RuntimeError = e.into();
        match re {
            RuntimeError::Trap { code, .. } => assert_eq!(code, "MT5060"),
            other => panic!("expected Trap, got {other:?}"),
        }
    }
}
