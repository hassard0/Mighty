# RELOAD_V0_21_NOTES.md

Tier 1.5 v0.21 closes out the four v0.20 deferrals. Design log.

## v0.20 → v0.21 completeness summary

| Deferral | v0.20 state | v0.21 fix |
|----------|-------------|-----------|
| Wasm-byte swapping | `MT5064 wasm-reload-not-yet` placeholder | `wasm_loader::load_agent_module` parses `__mty_agent_type` + `__mty_schema_hash` custom sections via `wasmparser`; `Program::with_swapped_agent` clones the per-agent slot map; `ReloadRunner::run` wires the loader into the swap pipeline |
| Schema migrations | `schema_compatible_with` is bit-equality | New `MigrateFrom<Old>` trait + `SchemaRegistry` BFS over `(old_hash, new_hash)` edges; `schema_check` returns `Direct`/`Migrate(chain)`/`Incompatible` |
| Control-socket op=reload | `unknown_op` reply | New `Request::Reload { agent_type, module_b64, deadline_ms }` + `ReloadHook` trait + process-global `reload_hooks()` registry |
| Condvar drain | 1 ms `thread::sleep` busy-poll | `condvar_drain::DrainSignal` (parking_lot `Condvar` over `Mutex<DrainState>`); legacy busy-poll retained as fallback for v0.20-shape callers |

## Owned-files map

| File | Purpose |
|------|---------|
| `crates/mty-runtime/src/reload/wasm_loader.rs` (new) | wasmparser-driven loader |
| `crates/mty-runtime/src/reload/condvar_drain.rs` (new) | condvar drain signal |
| `crates/mty-runtime/src/reload/swap.rs` (extended) | `Program`/`AgentSlot`, `WasmLoad`/`AgentTypeMismatch` errors, condvar+migration paths in `ReloadRunner::run` |
| `crates/mty-runtime/src/reload/resumable.rs` (extended) | `MigrateFrom`, `try_migrate`, `SchemaRegistry`, `SchemaCheck`, `schema_check` |
| `crates/mty-runtime/src/reload/mod.rs` (extended) | re-exports for v0.21 surface |
| `crates/mty-runtime/src/control_socket.rs` (extended) | `Request::Reload`, `Response::Reload`, `ReloadHook` trait, `SimpleReloadHook<T>`, `ReloadHookMap`, `reload_hooks()` global, base64 decoder, error-code-carrying `Response::Error` |
| `crates/mty-runtime/tests/reload.rs` (updated) | v0.20 baseline test 6 retargeted to assert `WasmLoad(MissingSection)` (MT5064 stays the diag code) |
| `crates/mty-runtime/tests/reload_wasm.rs` (new) | 6 tests: success path, missing-section reject, embedded-hash-vs-plan mismatch, agent-type mismatch, condvar drain timing, program-slot visibility |
| `crates/mty-runtime/tests/reload_migration.rs` (new) | 8 tests: V1→V2, defaulted field, V1→V2→V3 chain, migration failure surface, no-chain MT5060, identity direct path, multi-edge registry, pipeline-driven migration |
| `crates/mty-cli/src/cmd/reload.rs` (extended) | error-reply distinguishment (`{"error":...,"code":"MT506x"}` now exits 1 with the diag code in stderr) |
| `docs/internals/hot-reload.md` (extended) | wasm-byte loading section, schema-migration section, condvar-drain note, control-socket protocol, diag-code table |

## Design decisions

### Why `Program` lives in the reload subsystem (not `mty-ir`)

The scope rules for v0.21 declared `mty-ir` off-limits, but more
importantly: the v0.21 slice ships the registry shape + per-agent
reload semantics without changing the interpreter's data model. The
interpreter still owns dispatch — the wasm bytes are stored for
metadata extraction + cross-version inspection (the cluster mesh
hashes the bytes for routing) but not executed during a swap. A
future v0.22 will move the slot map into `mty_ir::ir::Program` once
the per-agent module surface is wired through dispatch.

### Why `reload_hooks()` is a process-global

`ControlContext` is constructed by a struct-literal in `runtime.rs`,
which is off-limits to v0.21. Adding a new field to the struct would
require updating that literal. Instead we store the
`ReloadHookMap` in a `OnceLock<ReloadHookMap>` static and consult
it from `handle_reload`. This keeps the struct shape source-compatible
with v0.20 callers + with the off-limits `runtime.rs`. The trade-off
is the usual one for process-globals — tests must `clear()` the
registry to avoid bleed-over. The control-socket unit tests do this
explicitly.

### Why the loader's diag-code family extends past MT5064

The v0.20 placeholder used a single `MT5064 wasm-reload-not-yet`
code. v0.21 splits this into four narrower codes (`MT5066` magic,
`MT5067` parse, `MT5068` missing-section, `MT506A` malformed) +
keeps `MT5064` as the family-level diag the swap pipeline returns to
the CLI (so existing CLI matchers on `MT5064` still fire). The
loader's narrower codes surface in the error message text but the
top-level `ReloadError::WasmLoad` always reports `MT5064` from
`diag_code()`. Two layers of granularity = a clean upgrade path.

### Why we keep the v0.20 busy-poll as a fallback

`ReloadRunner` is `pub` with named fields. v0.20 call sites (and the
existing `tests/reload.rs` integration tests) build literal
`ReloadRunner { plan, desc, state, gate }` records. Adding new
fields with `None` defaults would break the literal — so the new
fields are `Option<...>` and the runner's `run()` method picks the
condvar path when `Some(signal)`, falling back to the busy-poll
otherwise. The integration test for the condvar path lives in
`reload_wasm.rs::reload_with_wasm_bytes_uses_condvar_drain` and
verifies the wake-up takes <500 ms for a 30 ms handler (vs the
busy-poll's 1 ms granularity).

### Test fixture: synthetic wasm modules

Integration tests synthesize minimal wasm modules via `wasm-encoder`
(workspace dep, added to `mty-runtime`'s dev-dependencies). The
emitted module has no functions/exports — the runtime doesn't
execute the wasm during a v0.21 swap (the interpreter still owns
dispatch); it only inspects the embedded custom sections. This
keeps the test scope clean of the off-limits codegen crate.

## Acceptance status

- `cargo build -p mty-runtime` — clean
- `cargo build -p mty-cli` — clean
- `cargo test -p mty-runtime --test reload` (v0.20 baseline) — 9 / 9 pass
- `cargo test -p mty-runtime --test reload_wasm` (v0.21 new) — 6 / 6 pass
- `cargo test -p mty-runtime --test reload_migration` (v0.21 new) — 8 / 8 pass
- `cargo test -p mty-runtime --lib` (unit suite) — 179 / 179 pass
- `cargo clippy -p mty-runtime --lib --no-deps -- -D warnings` — clean
- `cargo clippy -p mty-cli --all-targets --no-deps -- -D warnings` — clean
- `cargo fmt` — owned files pass `rustfmt --edition 2021 --check`

(Workspace-wide clippy + tests show a `manual_inspect` warning in
`crates/mty-runtime/src/cluster/migration.rs` and unrelated mty-types
fmt drift; both are other agents' in-flight WIP and out of scope.)
