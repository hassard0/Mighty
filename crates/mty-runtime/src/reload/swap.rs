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
use crate::reload::condvar_drain::DrainSignal;
use crate::reload::resumable::{
    schema_check, ResumableError, SchemaCheck, SchemaRegistry, SnapshotCodec,
};
use crate::reload::wasm_loader::{load_agent_module, LoadedAgentModule, WasmLoadError};
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

// ---------------------------------------------------------------------
// Per-agent program slot (v0.21)
// ---------------------------------------------------------------------

/// Per-agent code-module record. Tracks the agent's installed wasm
/// bytes + the metadata pulled out of the module's custom sections.
#[derive(Debug, Clone)]
pub struct AgentSlot {
    pub agent_type: String,
    pub wasm: Vec<u8>,
    pub schema_hash: u64,
}

/// Process-wide registry mapping agent type name to its installed
/// wasm bytes + metadata. The reload pipeline calls
/// [`Program::with_swapped_agent`] to produce a clone of the program
/// with one slot replaced — the live `Arc<Program>` swap is atomic
/// from the caller's perspective.
///
/// `Program` is intentionally introduced inside the reload subsystem
/// rather than mty-ir: the v0.21 slice ships the registry shape +
/// per-agent reload semantics without changing the interpreter's
/// data model. A future v0.22 will move the slot map into mty-ir's
/// `ir::Program` once the per-agent module surface is wired through
/// the interpreter's dispatch path.
#[derive(Debug, Clone, Default)]
pub struct Program {
    slots: Vec<AgentSlot>,
}

impl Program {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_agent(mut self, slot: AgentSlot) -> Self {
        self.install(slot);
        self
    }

    fn install(&mut self, slot: AgentSlot) {
        if let Some(existing) = self
            .slots
            .iter_mut()
            .find(|s| s.agent_type == slot.agent_type)
        {
            *existing = slot;
        } else {
            self.slots.push(slot);
            self.slots.sort_by(|a, b| a.agent_type.cmp(&b.agent_type));
        }
    }

    pub fn get(&self, agent_type: &str) -> Option<&AgentSlot> {
        self.slots.iter().find(|s| s.agent_type == agent_type)
    }

    pub fn agent_count(&self) -> usize {
        self.slots.len()
    }

    /// Return a clone of this program with the named agent's wasm
    /// slot replaced. If the agent type wasn't previously installed
    /// the slot is appended (so a fresh agent reload bootstraps
    /// cleanly).
    pub fn with_swapped_agent(&self, agent_type: &str, new_wasm: Vec<u8>) -> ReloadResult<Self> {
        let loaded = load_agent_module(&new_wasm)?;
        if loaded.agent_type != agent_type {
            return Err(ReloadError::AgentTypeMismatch {
                requested: agent_type.to_string(),
                embedded: loaded.agent_type,
            });
        }
        let mut next = self.clone();
        next.install(AgentSlot {
            agent_type: loaded.agent_type,
            wasm: loaded.wasm,
            schema_hash: loaded.schema_hash,
        });
        Ok(next)
    }

    /// Variant of [`Self::with_swapped_agent`] that takes a pre-loaded
    /// module record — used by the swap pipeline so the loader only
    /// runs once per reload.
    pub fn with_swapped_agent_preloaded(&self, loaded: LoadedAgentModule) -> Self {
        let mut next = self.clone();
        next.install(AgentSlot {
            agent_type: loaded.agent_type,
            wasm: loaded.wasm,
            schema_hash: loaded.schema_hash,
        });
        next
    }
}

/// Where the swap pipeline finds the replacement code module.
///
/// v0.21 wires both variants:
///
/// - [`ModuleSource::SameProgram`] — state-only restart (round-trips
///   the snapshot but keeps the existing wasm slot).
/// - [`ModuleSource::WasmBytes`] — fresh wasm bytes. The pipeline
///   parses the module via [`crate::reload::wasm_loader::load_agent_module`],
///   cross-checks the embedded agent type + schema hash, then swaps
///   the per-agent program slot via [`Program::with_swapped_agent`].
pub enum ModuleSource<'a> {
    /// Use the runtime's currently-loaded program. State is preserved
    /// through `Resumable`; the code shape is unchanged.
    SameProgram,
    /// Opaque wasm-module bytes. The loader extracts the embedded
    /// `__mty_agent_type` + `__mty_schema_hash` custom sections; the
    /// swap pipeline cross-checks them against the plan + the
    /// snapshot before swapping the program slot.
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
         but the new module expects hash {new:#018x} (no migration registered)"
    )]
    IncompatibleSchema { old: u64, new: u64 },

    #[error("agent drain deadline of {0:?} exceeded before handler returned")]
    DrainDeadline(Duration),

    #[error("snapshot encode/decode failed: {0}")]
    Snapshot(#[from] ResumableError),

    #[error("wasm module load failed: {0}")]
    WasmLoad(#[from] WasmLoadError),

    #[error(
        "wasm module's embedded agent type ({embedded}) doesn't match the \
         caller-supplied agent type ({requested})"
    )]
    AgentTypeMismatch { requested: String, embedded: String },

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
            ReloadError::WasmLoad(_) => "MT5064",
            ReloadError::AgentTypeMismatch { .. } => "MT5065",
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
    /// Optional condvar-driven drain signal. When supplied, the
    /// pipeline waits on the condvar rather than busy-polling
    /// `gate.is_busy()`. Production callers should supply this; the
    /// v0.20-shape tests still work without it (legacy path).
    pub drain_signal: Option<DrainSignal>,
    /// Optional schema-migration registry. When supplied, mismatched
    /// hashes succeed if a chain is registered; otherwise the
    /// pipeline falls back to bit-equality (v0.20 behaviour).
    pub schema_registry: Option<Arc<SchemaRegistry>>,
    /// Optional program-slot registry. When supplied along with
    /// [`ModuleSource::WasmBytes`], the pipeline swaps the slot via
    /// [`Program::with_swapped_agent_preloaded`] and stores the new
    /// program back into the same cell.
    pub program: Option<Arc<Mutex<Program>>>,
}

impl<'a, T: Serialize + DeserializeOwned> ReloadRunner<'a, T> {
    /// Convenience for the v0.20-shape call sites — they don't pass
    /// the new fields, so we default them to `None`.
    pub fn new(
        plan: SwapPlan<'a>,
        desc: Arc<AgentDescriptor>,
        state: Arc<Mutex<T>>,
        gate: Arc<ReloadGate>,
    ) -> ReloadRunner<'a, T> {
        ReloadRunner {
            plan,
            desc,
            state,
            gate,
            drain_signal: None,
            schema_registry: None,
            program: None,
        }
    }

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
        // descriptor — catches the common bug of passing the wrong
        // descriptor in a multi-agent runtime.
        if !agent_type.is_empty() && self.desc.name != agent_type {
            return Err(ReloadError::Internal(format!(
                "plan.agent_type ({}) doesn't match desc.name ({})",
                agent_type, self.desc.name
            )));
        }

        // (0) pre-load the wasm bytes (if any) so we catch loader
        // failures *before* draining the agent. Failing here means
        // the agent stays running with the gate untouched.
        let preloaded: Option<LoadedAgentModule> = match module {
            ModuleSource::SameProgram => None,
            ModuleSource::WasmBytes(bytes) => {
                let loaded = load_agent_module(bytes)?;
                if loaded.agent_type != agent_type {
                    return Err(ReloadError::AgentTypeMismatch {
                        requested: agent_type.clone(),
                        embedded: loaded.agent_type,
                    });
                }
                if loaded.schema_hash != new_schema_hash {
                    return Err(ReloadError::Internal(format!(
                        "embedded schema hash {:#018x} doesn't match plan.new_schema_hash {:#018x}",
                        loaded.schema_hash, new_schema_hash
                    )));
                }
                Some(loaded)
            }
        };

        // (1) schema-hash compatibility. v0.21: if hashes differ,
        // consult the SchemaRegistry for a registered migration
        // chain. The default registry path falls back to v0.20
        // bit-equality so legacy tests stay green.
        let registry_owned: Arc<SchemaRegistry>;
        let registry: &SchemaRegistry = match self.schema_registry.as_ref() {
            Some(r) => r.as_ref(),
            None => {
                registry_owned = Arc::new(SchemaRegistry::new());
                registry_owned.as_ref()
            }
        };
        let migration = match schema_check(registry, old_schema_hash, new_schema_hash) {
            SchemaCheck::Direct => None,
            SchemaCheck::Migrate(chain) => Some(chain),
            SchemaCheck::Incompatible => {
                return Err(ReloadError::IncompatibleSchema {
                    old: old_schema_hash,
                    new: new_schema_hash,
                });
            }
        };

        // (2) drain the in-flight handler. v0.21: prefer the condvar
        // drain when supplied. The legacy busy-poll is kept as a
        // fallback so call sites that haven't migrated yet still work.
        let drain_started = Instant::now();
        let drain_elapsed = if let Some(signal) = self.drain_signal.as_ref() {
            match signal.wait_until_idle(options.deadline) {
                Ok(elapsed) => elapsed,
                Err(_) => return Err(ReloadError::DrainDeadline(options.deadline)),
            }
        } else {
            loop {
                if !self.gate.is_busy() {
                    break;
                }
                if drain_started.elapsed() >= options.deadline {
                    return Err(ReloadError::DrainDeadline(options.deadline));
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            drain_started.elapsed()
        };

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

        // (5) source the new code. v0.21 wires both variants.
        if let Some(loaded) = preloaded {
            // The caller must provide the `program: Some(...)` field
            // for the swap to be visible to subsequent reloads; if
            // they don't, we still accept the wasm bytes (the agent
            // type + hash check has already validated them) but
            // silently no-op the program update.
            if let Some(slot_cell) = self.program.as_ref() {
                let mut prog = slot_cell.lock();
                *prog = prog.with_swapped_agent_preloaded(loaded);
            }
        }

        // (6) decode the snapshot back into the (still typed) state
        // cell. With a migration chain registered, we re-encode the
        // snapshot through the chain first so the typed `T` decoder
        // sees the new-shape bytes.
        let final_bytes = match migration {
            None => snapshot,
            Some(chain) => {
                SchemaRegistry::apply_chain(&chain, &snapshot).map_err(ReloadError::Snapshot)?
            }
        };
        let restored: T = decode_snapshot(&final_bytes)?;
        *self.state.lock() = restored;

        // (7) resume. The mailbox is preserved end-to-end because we
        // never touched `desc.mailbox`.
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

    // -----------------------------------------------------------------
    // v0.21 Program slot tests
    // -----------------------------------------------------------------

    fn synth_module(agent_type: &str, schema_hash: u64) -> Vec<u8> {
        let mut module = wasm_encoder::Module::new();
        module.section(&wasm_encoder::CustomSection {
            name: std::borrow::Cow::Borrowed(crate::reload::wasm_loader::SECTION_AGENT_TYPE),
            data: std::borrow::Cow::Borrowed(agent_type.as_bytes()),
        });
        let hash_bytes = schema_hash.to_le_bytes();
        module.section(&wasm_encoder::CustomSection {
            name: std::borrow::Cow::Borrowed(crate::reload::wasm_loader::SECTION_SCHEMA_HASH),
            data: std::borrow::Cow::Borrowed(&hash_bytes),
        });
        module.finish()
    }

    #[test]
    fn program_swap_installs_new_slot() {
        let initial = Program::new();
        assert_eq!(initial.agent_count(), 0);
        let wasm = synth_module("Echo", 0xDEAD);
        let next = initial.with_swapped_agent("Echo", wasm.clone()).unwrap();
        assert_eq!(next.agent_count(), 1);
        let slot = next.get("Echo").expect("slot present");
        assert_eq!(slot.schema_hash, 0xDEAD);
        assert_eq!(slot.wasm, wasm);
        // Original was not mutated (clone-shaped surface).
        assert_eq!(initial.agent_count(), 0);
    }

    #[test]
    fn program_swap_replaces_existing_slot() {
        let prog = Program::new();
        let prog = prog
            .with_swapped_agent("Echo", synth_module("Echo", 1))
            .unwrap();
        let prog = prog
            .with_swapped_agent("Echo", synth_module("Echo", 2))
            .unwrap();
        assert_eq!(prog.agent_count(), 1);
        assert_eq!(prog.get("Echo").unwrap().schema_hash, 2);
    }

    #[test]
    fn program_swap_rejects_agent_type_mismatch() {
        let prog = Program::new();
        let wasm = synth_module("Other", 0);
        let err = prog.with_swapped_agent("Echo", wasm).unwrap_err();
        match err {
            ReloadError::AgentTypeMismatch {
                requested,
                embedded,
            } => {
                assert_eq!(requested, "Echo");
                assert_eq!(embedded, "Other");
            }
            other => panic!("expected AgentTypeMismatch, got {other:?}"),
        }
    }

    #[test]
    fn program_swap_rejects_malformed_wasm() {
        let prog = Program::new();
        let err = prog
            .with_swapped_agent("Echo", b"not-wasm".to_vec())
            .unwrap_err();
        assert!(matches!(err, ReloadError::WasmLoad(_)));
        assert_eq!(err.diag_code(), "MT5064");
    }

    #[test]
    fn program_with_swapped_agent_keeps_other_slots() {
        let prog = Program::new().with_agent(AgentSlot {
            agent_type: "Other".into(),
            wasm: vec![1, 2, 3],
            schema_hash: 99,
        });
        let next = prog
            .with_swapped_agent("Echo", synth_module("Echo", 7))
            .unwrap();
        assert_eq!(next.agent_count(), 2);
        assert_eq!(next.get("Other").unwrap().schema_hash, 99);
        assert_eq!(next.get("Echo").unwrap().schema_hash, 7);
    }
}
