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
pub mod delay_timers;
pub mod deterministic;
pub mod error;
pub mod extern_loader;
pub mod host_std;
pub mod http;
pub mod http_server;
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
pub use error::{RuntimeError, RuntimeResult};
pub use mailbox::{try_recv_many, Mailbox, MessageFrame, SendPolicy, SmallPayload};
pub use runtime::{RunOutcome, Runtime, RuntimeBuilder};
pub use scheduler::{Affinity, LoadMonitor, Scheduler, WorkerStatsSnapshot};
pub use slab_pool::{SlabPool, DEFAULT_INLINE_BYTES, DEFAULT_POOL_SIZE};
pub use supervisor::{ChildFailure, Strategy};
pub use telemetry::{TelemetryEvent, TelemetrySink};
