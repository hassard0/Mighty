# v0.27 Track B — Opaque-ADT handler scope + wasm32-web handle lowering

Forcing function: v0.26 demo 07 (Track E) had to construct `VectorStore.local(...)`,
`Episodic.in_memory(...)`, `Working.new()`, `AnthropicClient.from_env()` in `main()`
and pass them as ctor args to the Researcher agent — because constructing them
INSIDE an `on Ask()` handler tripped MT2021 (strict handler scope, v0.3 A65). The
rule was too strict: std.* opaque ADTs are effect-bearing but that's already
tracked by the `!{...}` clause.

## Two related fixes

### Fix 1: MT2021 relaxation for std.* opaque ADTs

**Before**: any unresolved name in a strict scope (agent body / handler body /
supervisor body / cap-narrow body) hard-errors with MT2021 (`crate::diag::
unresolved_value_strict`). `VectorStore`, `Episodic`, `Working`, `AnthropicClient`
weren't in the prelude as opaque ADTs, so a body of

```mty
on Ask(question) {
  let working = std.memory.Working.new()   // MT2021: Working unresolved
  // ...
}
```

would fail.

**After**: `crate::prelude` registers seven std.* opaque ADTs and marks each
`AdtId` as `handler_safe` in the new `DefMap::handler_safe_adts` set:

| Module        | Name              |
|---------------|-------------------|
| std.memory    | `VectorStore`     |
| std.memory    | `Episodic`        |
| std.memory    | `Working`         |
| std.memory    | `Snapshot`        |
| std.llm       | `AnthropicClient` |
| std.llm       | `OpenAIClient`    |
| std.llm       | `GeminiClient`    |
| std.llm       | `BedrockClient`   |
| std.llm       | `Message`         |

`synth_path` in `mty_types::check` checks `defs.is_handler_safe_name(name)`
before firing MT2021 in strict scopes. Handler-safe names take the
permissive-fresh-var path (same as `tolerance_open`), so the rest of the
handler body still types as before.

**Back-compat**: USER-defined opaque ADTs (declared without effect annotations)
still hit MT2021 in strict scope. The relaxation is keyed strictly off the
prelude registration set.

### Fix 2: Opaque ADT agent fields lower to wasm32-web

**Before**: declaring `agent X { client: AnthropicClient }` would route the
field through the agent-state layout in `crates/mty-codegen-wasm/src/emit.rs`.
`agent_field_layout` already gives every non-64-bit type a 4-byte slot (the
`_ => 4` branch), and reads/writes already lower as `I32Load`/`I32Store`. But
**no test pinned the shape** — and the resource-table side that the JS shim
needs to consult to recover the underlying Rust value didn't exist.

**After**:

1. `IrTy::Adt(_, _)` field reads stay i32 (matches existing `field_size_bytes`).
   The i32 IS the handle — a stable opaque integer that indexes into a
   host-side resource table the JS shim manages.
2. `field_size_bytes` and `field_align_bytes` keep the `_ => 4` fallback;
   no Rust changes needed (the existing code already handles this shape
   correctly — the v0.26 emitter for `IrTy::Adt`-typed fields wasn't broken,
   just untested).
3. `web_lower.rs` gains `OPAQUE_HANDLE_TABLE_MODULE` + a small accessor for
   the resource-table import name that JS shims look up.
4. New test in `crates/mty-codegen-wasm/tests/agent_handle_fields.rs` pins:
   * `agent_with_llm_handle_field_compiles_to_web` — module with an opaque-ADT
     field compiles cleanly (no `WasmError::Unsupported`).
   * `agent_handle_field_loads_as_i32` — read of the field emits `I32Load`
     against the agent's region.
   * `agent_handle_field_persists_across_callbacks` — handle stored in
     callback A is returned identically by callback B (linear memory survives,
     so the i32 is stable across the boundary).

## Resource-table convention

The JS shim that owns the WASM instance maintains a single resource table
per agent (or globally, per the program). When a `BuiltinId::Extern("Type.ctor")`
returns a "handle value" through `cabi`, the shim:

1. Stashes the underlying Rust value in its resource map at index `H`.
2. Returns `H` to wasm — that's what gets stored in the agent's region.
3. When wasm calls a method on the handle (`vector.search(...)`), it passes
   the i32 `H` through the canonical-ABI lowering. The shim looks up `H`,
   re-injects the Rust value, dispatches the method, and returns the result.

In v0.27 the resource table itself is a JS-side concept — wasm just sees i32s.
Future v0.28 work: WIT-resources for proper component-side typing.

## Files touched

EXTEND:
- `crates/mty-types/src/check.rs` (MT2021 carve-out)
- `crates/mty-types/src/prelude.rs` (std.* opaque ADT registrations + `handler_safe_adts` population)
- `crates/mty-types/src/defs.rs` (new `handler_safe_adts: HashSet<AdtId>` + `is_handler_safe_name` helper)
- `crates/mty-codegen-wasm/src/web_lower.rs` (resource-table constants)

NEW:
- `crates/mty-types/tests/opaque_adt_handler_scope.rs` — 5 tests for Fix 1
- `crates/mty-codegen-wasm/tests/agent_handle_fields.rs` — 3 tests for Fix 2
- `examples/28_agent_with_llm_field.mty` — example exercising the relaxation
- `dev/history/notes/OPAQUE_ADT_WASM_V0_27_NOTES.md` — this file

## Pre-flight gate

All required gates pass: `cargo build --workspace`, focused test suites,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`,
and `mty check examples/28_agent_with_llm_field.mty`.
