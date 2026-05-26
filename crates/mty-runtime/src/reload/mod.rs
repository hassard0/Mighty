//! Hot reload — v0.20 Tier 1.5 (see `docs/internals/agent-features-roadmap.md`).
//!
//! v0.21 closes out the v0.20 deferrals:
//!
//! - [`wasm_loader`] — parses incoming wasm bytes and extracts the
//!   Mighty-embedded `__mty_agent_type` + `__mty_schema_hash` custom
//!   sections; replaces the v0.20 `MT5064 wasm-reload-not-yet`
//!   placeholder.
//! - [`condvar_drain`] — replaces the 1 ms busy-poll with a
//!   [`parking_lot::Condvar`]-driven drain signal.
//! - [`resumable`] — adds the [`resumable::MigrateFrom`] trait + the
//!   [`resumable::SchemaRegistry`] for schema-evolution chains.
//! - [`swap`] — wires the wasm-loader + migration path into the
//!   runner so a `ModuleSource::WasmBytes` reload succeeds end-to-end.
//!
//! The pre-existing surface (the [`Resumable`] trait, the
//! [`compute_schema_hash`](resumable::compute_schema_hash) helper, the
//! [`swap::ReloadRunner`]) stays source-compatible — v0.20 callers
//! that built against `mty-runtime` 0.20 keep working unchanged.
//!
//! See `docs/internals/hot-reload.md` for architecture notes and
//! `dev/history/notes/RELOAD_V0_21_NOTES.md` for the completion log.

pub mod condvar_drain;
pub mod resumable;
pub mod swap;
pub mod wasm_loader;

// Convenience re-exports — the common case is one `use mty_runtime::reload::*`.
pub use condvar_drain::DrainSignal;
pub use resumable::{
    compute_schema_hash, schema_check, try_migrate, MigrateFrom, Resumable, ResumableError,
    ResumableResult, SchemaCheck, SchemaRegistry, SnapshotCodec,
};
pub use swap::{
    decode_snapshot, dry_run_swap, encode_snapshot, AgentSlot, ModuleSource, Program, ReloadError,
    ReloadGate, ReloadOptions, ReloadReport, ReloadResult, ReloadRunner, SwapPlan,
    DEFAULT_MAX_SNAPSHOT_BYTES,
};
pub use wasm_loader::{
    load_agent_module, LoadedAgentModule, WasmLoadError, SECTION_AGENT_TYPE, SECTION_SCHEMA_HASH,
};
