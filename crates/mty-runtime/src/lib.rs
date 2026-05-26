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

// v0.16 OpenTelemetry agent-span layer (roadmap Tier 1.2 / 1.3).
// Additive — the slice-7 `TelemetryEvent` / `TelemetrySink` path keeps
// working unchanged. See `crates/mty-runtime/src/telemetry/mod.rs`.
pub use telemetry::{
    agent_event, init_from_env as init_telemetry_from_env, record_budget_exhausted, record_restart,
    shutdown as shutdown_telemetry, span_ask, span_handler, span_send, span_spawn, HandlerGuard,
    SpanContext, SpawnGuard,
};
