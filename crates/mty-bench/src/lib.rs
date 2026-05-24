//! mty-bench: shared fixtures + helpers for the v0.6 honest-benchmarks
//! swarm.
//!
//! This crate is **publish = false** and exists solely to host:
//!
//! - The 10 KLOC synthetic Mighty source ([`fixtures::stardust_10kloc`])
//!   used by the parse_throughput benchmark.
//! - Cooked SIR programs ([`fixtures::echo_sir_program`]) used by the
//!   agent/mailbox latency + throughput benchmarks so the cost of
//!   parsing is excluded from the measured loop.
//! - Tiny HTTP echo helpers ([`http`]) used by the http_server benchmark.
//!
//! The benchmarks themselves live in `benches/*.rs` and use criterion's
//! custom harness (the workspace already has criterion 0.5 pinned).
//!
//! ## Honesty contract
//!
//! - Numbers reported by these benchmarks are recorded from a *real*
//!   run on the host machine into `docs/benchmarks/*.md`.
//! - Where a comparator language toolchain is unavailable on the host,
//!   the impl ships as code under `benches/<category>/<lang>/` and the
//!   doc note records the reference environment instead of fabricating
//!   numbers.

pub mod fixtures;
pub mod http;
pub mod metrics;

pub use fixtures::{echo_sir_program, stardust_10kloc, ten_kloc_lines};
