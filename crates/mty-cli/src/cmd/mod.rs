// v0.33 T5 — `mty agent`: structured JSON-over-stdio protocol for
// LLM-agent consumption of every other mty subcommand. See
// `docs/internals/agent-mode-protocol.md`.
pub mod agent;
pub mod build;
pub mod check;
// v0.32 Track A — `mty dap`: Debug Adapter Protocol server over stdio.
// Drives breakpoint-aware execution of `main` (or a replay trace)
// from a VS Code DAP client or a JetBrains debug configuration. See
// `docs/reference/cli/mty-dap.md`.
pub mod dap;
pub mod doc;
pub mod dump;
pub mod explain;
// v0.33 T7 — `mty find <query>`: capability-tagged stdlib search.
// Walks `crates/mty-stdlib/src` and builds an index of public items so
// agents and humans can discover the right API via natural-language
// queries (e.g. `mty find "write files"` → `std.fs.write`). See
// `docs/reference/find.md` for the query DSL + ranking spec.
pub mod find;
pub mod fmt;
// v0.34 T4 — `mty hooks install`: install the project's pre-push hook
// (`.git-hooks/pre-push`) into `.git/hooks/pre-push`. See cmd/hooks.rs.
pub mod hooks;
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
// v0.30 Track E — `mty test [--eval]`: discover *.test.mty and
// *.eval.mty files; run them through the v0.2 std.test runner +
// the v0.28-onwards std.eval suite driver. See cmd/test.rs.
pub mod test;
