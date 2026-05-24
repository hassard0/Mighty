//! Stardust runtime MVP (spec §25 + §31.5).
//!
//! Tokio-backed concurrent executor for agents lowered to SIR.
//! Provides scheduling, mailboxes, supervisors, deadline timers,
//! budget enforcement, deterministic replay, and a minimal `std.http`
//! server surface.

pub mod agent;
pub mod budget;
pub mod deterministic;
pub mod error;
pub mod host_std;
pub mod http;
pub mod mailbox;
pub mod runtime;
pub mod scheduler;
pub mod supervisor;
pub mod telemetry;
pub mod timer;

pub use agent::{AgentDescriptor, AgentHandle, AgentId};
pub use budget::{Budget, BudgetBreach, BudgetTracker};
pub use error::{RuntimeError, RuntimeResult};
pub use mailbox::{Mailbox, MessageFrame, SendPolicy, SmallPayload};
pub use runtime::{RunOutcome, Runtime, RuntimeBuilder};
pub use supervisor::{ChildFailure, Strategy};
pub use telemetry::{TelemetryEvent, TelemetrySink};
