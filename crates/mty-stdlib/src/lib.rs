//! `mty-stdlib` — real implementations of Mighty's `std.*` modules.
//!
//! v0.2 ships **Strategy A** (see `docs/internals/stdlib.md`): the
//! prelude in `mty-types::prelude` registers `std.*` modules as opaque,
//! and effect-calls of the shape `std.<module>.<method>(args)` lower to
//! `EffectOp::GenericCall` in SIR. The runtime's `host_std::StdHost`
//! routes these calls into this crate's free functions, which carry the
//! real semantics (parsing JSON via `serde_json`, opening TLS sockets via
//! `rustls`, serving HTTP via `hyper`, etc.).
//!
//! v0.3 will migrate to **Strategy B**: real `.mty` source files shipped
//! as a bundled package and resolved by `mty-pkg`.
//!
//! ## Module layout
//!
//! - [`json`]  — `Json` value type + `parse` / `encode` / `encode_pretty`
//! - [`tls`]   — async client `connect` + server `acceptor_from_pem`
//! - [`http`]  — async client `get`/`post` + server `serve`
//! - [`fs`]    — sync filesystem ops gated by an `Fs` cap value
//! - [`time`]  — monotonic clock + `sleep`
//! - [`log`]   — `log()` / `print()` host fallback + v0.17 direct-import constants
//! - [`test`]  — Mighty-native test discovery + reporter
//! - [`host`]  — single entry point invoked from `mty-runtime`'s
//!   `host_std` to dispatch `std.*` generic calls.

pub mod fs;
pub mod host;
pub mod http;
pub mod http_server;
pub mod json;
pub mod log;
pub mod random;
pub mod test;
pub mod time;
pub mod tls;
