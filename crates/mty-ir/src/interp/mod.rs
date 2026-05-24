//! SIR interpreter (slice 6).
//!
//! Single-threaded, deterministic walker. Executes a SIR `Program`
//! starting from a fn named `main` (slice 6 takes no args). Output is
//! routed through a [`Host`] trait so tests can capture stdout/stderr
//! and inspect effect calls.

pub mod host;
pub mod run;
pub mod value;

pub use host::*;
pub use run::*;
pub use value::*;
