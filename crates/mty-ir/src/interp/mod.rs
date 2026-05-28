//! SIR interpreter (slice 6).
//!
//! Single-threaded, deterministic walker. Executes a SIR `Program`
//! starting from a fn named `main` (slice 6 takes no args). Output is
//! routed through a [`Host`] trait so tests can capture stdout/stderr
//! and inspect effect calls.

pub mod breakpoints;
pub mod debug;
pub mod host;
pub mod run;
pub mod value;

pub use breakpoints::*;
pub use debug::{BreakReason, DebugFrame, DebugLocal, DebugSession, DebugStop};
pub use host::*;
pub use run::*;
pub use value::*;
