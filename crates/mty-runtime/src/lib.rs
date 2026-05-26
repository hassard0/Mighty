//! Mighty runtime MVP (spec §25 + §31.5).
//!
//! Tokio-backed concurrent executor for agents lowered to SIR.
//! Provides scheduling, mailboxes, supervisors, deadline timers,
//! budget enforcement, deterministic replay, and a minimal `std.http`
//! server surface.

pub mod agent;
pub mod arena;
pub mod budget;
pub mod cancel;
// v0.18 Tier 4.1 — distributed agents (single-cluster mesh). The
// module ships the transport layer (`AgentAddr`, framed CBOR/TLS
// wire, reconnecting peers, mesh) so the runtime can opt-in to
// distribution via the `ClusterRouter` trait. See
// `docs/internals/cluster.md` for the architecture.
pub mod cluster;
pub mod codegen_abi;
pub mod control_socket;
pub mod delay_timers;
pub mod deterministic;
pub mod error;
pub mod extern_loader;
pub mod host_std;
pub mod http;
pub mod http_server;
pub mod introspect;
pub mod mailbox;
#[cfg(feature = "otlp")]
pub mod otlp;
// v0.17 Tier 1.4 — deterministic replay (record + step a binary trace).
// v0.18: the recorder is wired into the Runtime hot path; opt-in via
// `MTY_RECORD_TRACE=<path>`. See `replay/mod.rs` for the surface;
// CLI: `mty replay <trace>`.
pub mod replay;
// v0.20 Tier 1.5 — hot reload + Resumable trait. Additive: no
// existing path consumes this module unless an agent opts in.
// See `docs/internals/hot-reload.md`.
pub mod reload;
pub mod runtime;
pub mod scheduler;
pub mod slab_pool;
pub mod supervisor;
pub mod supervisor_orchestrator;
pub mod telemetry;
pub mod timer;

pub use agent::{AgentDescriptor, AgentHandle, AgentId};
pub use budget::{Budget, BudgetBreach, BudgetTracker};
pub use cancel::{CancelReason, CancellationToken};
// v0.18 Tier 4.1 — cluster surface re-exports.
pub use cluster::{
    current_node_id, AgentAddr, ClusterConfig, ClusterMesh, ClusterRouter, MeshError, NodeId,
    PeerEntry, SharedRouter, TlsConfig, WireFrame, WIRE_VERSION,
};
pub use control_socket::{
    sock_path_from_env, spawn_control_socket, spawn_control_socket_at, ControlContext,
    ControlSocketHandle, CONTROL_SOCK_ENV,
};
pub use error::{RuntimeError, RuntimeResult};
pub use introspect::{
    snapshot_agent, snapshot_runtime, AgentIntrospectState, AgentListEntry, AgentSnapshot,
    BudgetSnapshot, IntrospectMap, RuntimeSnapshot, SNAPSHOT_WIRE_VERSION,
};
pub use mailbox::{try_recv_many, Mailbox, MessageFrame, SendPolicy, SmallPayload};
pub use runtime::{RunOutcome, Runtime, RuntimeBuilder};
pub use scheduler::{Affinity, LoadMonitor, Scheduler, WorkerStatsSnapshot};
pub use slab_pool::{SlabPool, DEFAULT_INLINE_BYTES, DEFAULT_POOL_SIZE};
pub use supervisor::{ChildFailure, Strategy};
pub use telemetry::{TelemetryEvent, TelemetrySink};

// v0.18 Tier 1.4 — process-wide recorder accessors for the hot path.
// `global_recorder` returns the installed handle (if any) so callers
// can opt out of the `with_recorder` callback shape when they need to
// hold the `Arc` (e.g. across an async await point). `with_recorder`
// + `recording_enabled` are the standard fire-and-forget hooks.
pub use replay::{global_recorder, recording_enabled, with_recorder};

// v0.16 OpenTelemetry agent-span layer (roadmap Tier 1.2 / 1.3).
// Additive — the slice-7 `TelemetryEvent` / `TelemetrySink` path keeps
// working unchanged. See `crates/mty-runtime/src/telemetry/mod.rs`.
pub use telemetry::{
    agent_event, init_from_env as init_telemetry_from_env, record_budget_exhausted, record_restart,
    shutdown as shutdown_telemetry, span_ask, span_handler, span_send, span_spawn, HandlerGuard,
    SpanContext, SpawnGuard,
};
