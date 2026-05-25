# v0.8 Loose-End Closure Notes

Agent: parallel swarm worker (loose-ends). Scope: close 4 of 5 remaining
v0.5 loose ends (deferring set-of-scopes hygiene to post-1.0).

## Task 1 — Proc-macro sandboxed execution

### Interpretation decisions

- The sandboxed proc-macro interpreter runs on a sub-`mty_ir::Interp` via
  `run_fn_with_resource_budget`. Effects are blocked by routing through
  a `ProcMacroHost` that surfaces any `effect_call` as `MT6005`
  (proc_macro_impure_at_runtime) by raising a trap inside the host.
  The host stores a flag; after the run returns we map the trap back to
  the `ProcMacroResult::Impure` arm.
- Wall-clock timeout (100 ms) uses `std::thread::spawn` + a watcher
  `JoinHandle` channel because `mty_ir::interp::run` is sync; we run the
  interpreter on a dedicated thread and cancel via the cooperative step
  budget the thread exposes. We pre-set `step_budget = 100_000` so even a
  CPU-bound runaway is bounded; the wall timer is the upper guard.
- Memory cap (16 MB) maps to `mem_budget = 16 * 1024 * 1024` passed to
  the interpreter. The existing `MT5009` trap is mapped at the boundary
  to `MT6004` (proc_macro_too_deep).
- Step cap (100,000) is the interpreter step budget.
- Token-stream marshaling: a proc-macro body of shape
  `proc macro name(input: TokenStream) -> TokenStream { … }` runs as a
  synthetic SIR program where `input` is supplied as `Value::Str` of the
  call-site token text. The body's return value is treated as the
  rewritten source text. This is a deliberate simplification — full
  TokenStream marshalling is post-1.0.
- New diagnostic codes:
    * MT6007 `proc_macro_impure_at_runtime` — runtime effect leak
      (separate from MT6005's static detection).
    * MT6008 `proc_macro_resource_exceeded` — generic resource-bound
      breach (wall / mem / steps).

### Files

- `crates/mty-macros/src/proc.rs` — replaced `Unsupported` with a real
  `execute()` path.
- `crates/mty-macros/Cargo.toml` — added dev-dep on `mty-ir` (test only).
- `crates/mty-hir/src/lower/macros.rs` — call-site expansion path now
  attempts execution; on failure raises MT6005/MT6007/MT6008 as
  appropriate.
- `crates/mty-diagnostics/src/codes.rs` — registered MT6007 and MT6008.

### Tests added

- `crates/mty-macros/tests/proc_macro_exec_simple.rs`
- `crates/mty-macros/tests/proc_macro_exec_timeout.rs`
- `crates/mty-macros/tests/proc_macro_exec_impure.rs`
- `crates/mty-macros/tests/proc_macro_exec_mem.rs`

## Task 2 — Real per-agent HTTP routing

### Interpretation decisions

- The runtime gains a thin `http_server.rs` module that exposes
  `install_agent_dispatcher(rt, handle)`. Internally it composes a
  closure that takes an incoming `Request`, wraps it as
  `Value::Str(json)`, posts an `ask` to the agent's mailbox, blocks on
  the reply via tokio's runtime handle, and translates the reply
  `Value::Str` body into a `Response`.
- The bridge is registered with `mty-stdlib::http_server::install_agent_dispatch`
  so the existing `std.http.serve` dispatcher path stays the gate.
- Demo 01 keeps the same handlers; we wire `main()` so that under the
  `mty run` driver the smoke output is unchanged but the agent now also
  serves the same handler over a real socket if invoked under
  `std.http.serve`. The smoke script remains text-based.

### Files

- `crates/mty-runtime/src/http_server.rs` (NEW) — agent-dispatcher
  bridge.
- `crates/mty-runtime/src/lib.rs` — re-export.
- `crates/mty-runtime/tests/http_serve_agent_dispatch.rs` — full
  roundtrip.

## Task 3 — LSP cross-file workspace resolve map

### Interpretation decisions

- New `workspace.rs` module holds a `WorkspaceModel` keyed by workspace
  folder root. Each folder walks for `.mty` files (capped by an
  in-memory MAX_FILES bound to prevent runaway) and produces a map of
  `path → DocAnalysis`.
- Cross-file rename: when the target name is a top-level public symbol
  in any file's `def_map`, the rename harvests occurrences in every
  file in the same workspace folder and emits a multi-file
  `WorkspaceEdit`.
- Refreshes happen on `didChangeWatchedFiles` (create/delete/rename) and
  on `didOpen` for files inside known folders.

### Files

- `crates/mty-lsp/src/workspace.rs` (NEW).
- `crates/mty-lsp/src/lib.rs` — module export.
- `crates/mty-lsp/src/server.rs` — Backend gains `workspaces`, hooks
  workspace-folder events.
- `crates/mty-lsp/src/rename.rs` — cross-file branch added.
- `crates/mty-lsp/tests/workspace_resolve.rs` — multi-file rename.
- `docs/internals/lsp.md` — new capability section.
- `editor/vscode/*` — version bump + capability note.

## Task 4 — WIT canonical-ABI return-area for get-text/query

### Interpretation decisions

- `get-text` and `query` now declare return type `string` /
  `option<string>` in the generated WIT. The core-wasm import signature
  changes from `(ptr,len) -> i32` to `(ptr,len, ret_area_ptr) -> ()`
  for `get-text` (string) and `(ptr,len, ret_area_ptr) -> ()` for
  `query` (option<string> uses 1 disc byte + ptr + len, totalling
  variant-tag + 8 bytes).
- The caller-allocated return area lives in linear memory at a fixed
  reserved offset (`8208` = `8192 + 16` to leave room for the JS shim's
  legacy write area). The lifter reads:
    * For `string`: `(ptr: i32, len: i32)` from the return area, then
      decodes the bytes into a `Value::Str`.
    * For `option<string>`: `disc: u8` at byte 0, then `(ptr, len)` at
      offset 4 if `disc == 1`, returning `Value::Enum{variant: 0|1, …}`.
- The Mighty source-level surface stays the same: `dom.get_text(id)`
  returns `String`; `dom.query(sel)` returns `Option<String>`.

### Files

- `crates/mty-codegen-wasm/src/wit.rs` — return-type signatures.
- `crates/mty-codegen-wasm/src/emit.rs` — import signatures + post-call
  lift instructions.
- `crates/mty-codegen-wasm/src/component.rs` — keep canonical-ABI
  encoding aligned (no change beyond comment update).
- `demos/02_counter_web/web/dom-shim.js` — JS write-into-return-area.
- `crates/mty-codegen-wasm/tests/canonical_abi_return.rs` — unit test.

