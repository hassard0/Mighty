//! `std.observe` — cost + latency observability for `std.llm`.
//!
//! v0.30 Track D. Production agent development is mostly
//! "why is this slow / expensive"; Mighty owns that workflow with
//! a tight, opinionated loop:
//!
//! 1. Every `Member.ask(...)` / `LlmProvider::complete(...)` records
//!    a typed [`LlmObservation`] when `MTY_OBSERVE=1` (or when
//!    `mty inspect --cost --record` flipped the persistent flag).
//! 2. Observations land in a local SQLite at
//!    `~/.mty/observations.sqlite` (override via `MTY_OBSERVE_DB`).
//! 3. `mty inspect --cost` reads the same DB back as an ASCII table:
//!    total $$, per-provider/per-model breakdown, p50/p95/p99 latency,
//!    top-N most expensive calls.
//!
//! ## Why integer cents
//!
//! `cost_cents` is `i64` (not `f64`) so summing 1M+ observations
//! doesn't drift. Sub-cent fractions land in the integer arithmetic
//! that the [`pricing`] table already uses (rate is cents-per-million,
//! multiply first / divide last).
//!
//! ## Storage
//!
//! Gated behind the `observe-sqlite` feature (default-on; already
//! present via the `memory-sqlite` rusqlite dep). When the feature is
//! off, [`record_if_enabled`] is a no-op and [`query`] returns
//! "feature disabled".
//!
//! ## OTel (Phase 2)
//!
//! `MTY_OBSERVE_OTEL=http://otel-collector:4318` redirects records
//! into an OTLP/HTTP span exporter. The v0.30 ship is a documented
//! stub + one round-trip test — the SQLite path is the must-ship.
//! See `docs/internals/observability.md` for the schema + roadmap.
//!
//! ## Manual instrumentation
//!
//! [`span`] returns a [`SpanGuard`] that records latency-only on
//! drop, into a sibling `spans` table. Use for non-LLM hot paths
//! (tool dispatch, vector lookups, etc.) where you want the same
//! `mty inspect` aggregations applied.

pub mod observation;
pub mod otel;
pub mod pricing;
pub mod query;
pub mod storage;

pub use observation::{LlmObservation, ToolCallObservation};
pub use pricing::{cost_cents_for, load_pricing_overrides, PricingTable};
pub use query::{
    aggregate_by, percentiles, summarize, AggregateRow, CostSummary, GroupBy, LatencyPercentiles,
    Window,
};
pub use storage::{
    is_recording_enabled, record_if_enabled, record_now, span, with_storage, ObservationStore,
    SpanGuard, StorageError,
};
