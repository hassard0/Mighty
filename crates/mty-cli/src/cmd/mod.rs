pub mod build;
pub mod check;
pub mod doc;
pub mod dump;
pub mod explain;
pub mod fmt;
pub mod inspect;
pub mod lsp;
pub mod new;
pub mod pkg;
// v0.17 Tier 1.4 — `mty replay <trace>` (deterministic replay).
pub mod replay;
// v0.20 Tier 1.5 — `mty reload <agent-type> --from new.wasm`
// (state-preserving hot reload). See docs/internals/hot-reload.md.
pub mod reload;
pub mod run;
// v0.23 Track C — `mty serve [--port N] [--watch]` (built-in dev
// server + websocket-driven reload for the web-game template).
pub mod serve;
