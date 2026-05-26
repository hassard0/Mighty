//! Hot reload — v0.20 Tier 1.5 (see `docs/internals/agent-features-roadmap.md`).
//!
//! The reload module is purely additive — it doesn't touch any
//! existing runtime hot path. Two submodules:
//!
//! - [`resumable`] — the `Resumable` trait + the
//!   [`compute_schema_hash`](resumable::compute_schema_hash) helper.
//!   Public so user agents can implement (or, in v0.21, derive) the
//!   trait for their state shapes.
//! - [`swap`] — the orchestrator. Drains the in-flight handler,
//!   snapshots the state, validates schema compatibility, then
//!   reapplies the snapshot. The mailbox is preserved end-to-end —
//!   producers continue sending into the same `Arc<Mailbox>` and the
//!   gate ensures handlers don't dispatch during the swap.
//!
//! See `docs/internals/hot-reload.md` for architecture notes and
//! `dev/history/notes/HOT_RELOAD_V0_20_NOTES.md` for the design log.

pub mod resumable;
pub mod swap;

// Convenience re-exports — the common case is one `use mty_runtime::reload::*`.
pub use resumable::{
    compute_schema_hash, Resumable, ResumableError, ResumableResult, SnapshotCodec,
};
pub use swap::{
    decode_snapshot, dry_run_swap, encode_snapshot, ModuleSource, ReloadError, ReloadGate,
    ReloadOptions, ReloadReport, ReloadResult, ReloadRunner, SwapPlan, DEFAULT_MAX_SNAPSHOT_BYTES,
};
