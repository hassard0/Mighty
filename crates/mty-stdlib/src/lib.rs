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
//! - [`fmt`]   — v0.24: runtime contract for `format!` conversion methods
//! - [`string`] — v0.25 Track E: owned UTF-8 `String` (host-side Rust impl)
//! - [`vec`]   — v0.25 Track E: generic `Vec[T]` (host-side Rust impl)
//! - [`llm`]   — v0.26 Track A: typed LLM provider abstraction
//!   (Anthropic Messages full impl + OpenAI/Gemini/Bedrock skeletons,
//!   streaming, tools, typed budgets). See `docs/reference/stdlib/llm.md`.
//! - [`mcp`]   — v0.26 Track B: Model Context Protocol server + client +
//!   `@tool` registry + capability-enforced sandbox. See
//!   `docs/reference/stdlib/mcp.md`.
//! - [`swarm`] — v0.27 Track D: multi-LLM consensus primitive. One
//!   prompt → N panel members → consensus (Majority/Unanimous/
//!   WeightedVote/FirstAgreed) with shared dollar budget. See
//!   `docs/reference/stdlib/swarm.md`.
//! - [`rag`]   — v0.33 Track T2: RAG-as-stdlib. `Index` + `Doc` +
//!   `Chunker` + `Retriever` + `Reranker` + `Rag` pipeline. Built on
//!   `std.memory.VectorStore` (v0.26 Track C) + `std.swarm.Member`
//!   (v0.27 Track D). See `docs/internals/rag.md`.
//! - [`eval`]  — v0.28 Track G: replay-driven LLM eval harness.
//!   Suite + Case + Member + Compare; runs a recorded trace (or raw
//!   prompt) against multiple model variants and stamps a verdict
//!   per (case, member) cell. See `docs/internals/std-eval.md`.
//! - [`test`]  — Mighty-native test discovery + reporter
//! - [`host`]  — single entry point invoked from `mty-runtime`'s
//!   `host_std` to dispatch `std.*` generic calls.

pub mod computer;
pub mod env;
pub mod eval;
pub mod fmt;
pub mod fs;
pub mod host;
pub mod http;
pub mod http_server;
pub mod json;
pub mod llm;
pub mod log;
pub mod mcp;
pub mod memory;
pub mod observe;
pub mod rag;
pub mod random;
pub mod string;
pub mod swarm;
pub mod test;
pub mod time;
pub mod tls;
pub mod vec;
pub mod web;
