# Stardust Slice 8 — Complete (FINAL v0.1 slice)

**Tag:** `v0.8.0-codegen`
**Also tagged:** `v0.1.0` (the v0.1 milestone)
**Date:** 2026-05-24

## What landed

### Three new crates

```
crates/sdust-codegen-cranelift/    ~1 200 lines
crates/sdust-codegen-wasm/         ~  600 lines
crates/sdust-codegen-llvm/         ~   50 lines (scaffold, feature-gated)
```

### Cranelift native backend (spec §24.6, A46)

- **JIT path** (`sdust run` default): SIR → cranelift IR → in-process
  fn-pointer, called via transmute. Runtime ABI bridge
  (`sdust_runtime::codegen_abi`) supplies twelve C-ABI fns.
- **AOT path** (`sdust build --target native`): cranelift `ObjectModule`
  → host-format `.o` → platform linker → executable. Per A52, linker
  discovery prefers `clang`/`gcc`/`cc`; if none found, `.o` is emitted
  and the user is prompted.
- **Type lowering** ([`abi`](crates/sdust-codegen-cranelift/src/abi.rs))
  + **layout** ([`layout`](crates/sdust-codegen-cranelift/src/layout.rs))
  cover primitives, refs, strings, aggregates (slice-8 sequential layout).
- **Runtime imports** ([`runtime_imports`]): 12 symbols
  (`stardust_runtime_log`, `_print`, `_panic`, `_arena_push`,
  `_arena_pop`, `_alloc`, `_budget_charge`, `_send`, `_ask`, `_spawn`,
  `_extern_call`, `_log_i64`).
- **Monomorphization** ([`mono`]): slice-8 MVP strips generic fns and
  lets concrete callers go through; full per-(fn, type-args) specialization
  is v0.2 (A49).

### Wasm backend (spec §24.7, A47)

- `sdust-codegen-wasm` emits core Wasm modules via `wasm-encoder
  0.250`. Targets: `wasm32-wasi` and `wasm32-web`.
- Memory: 16-page initial linear memory, exported as `memory`.
- Strings interned into `.data` section starting at offset 1024
  (first 1 KiB reserved for the eventual shadow stack).
- Capability imports: `(import "stardust" "log" (func (param i32 i32)))`.
- WASI bridge for `log`/`print` shapes is the same import name on both
  targets — a host-side adapter routes to `fd_write` for WASI.
- Component Model wrapper (`wit-component`) deferred to v0.2.

### LLVM scaffold (A46)

`sdust-codegen-llvm` exists as a feature-gated placeholder. Default
build returns `LlvmError::FeatureDisabled`; with `--features llvm`,
returns `LlvmError::NotYetImplemented`. The slice-8 build host had no
LLVM/llvm-config available, so Cranelift became the v0.1 native
backend. A future v0.2 build host can enable it.

### Runtime ABI bridge

- `sdust_runtime::codegen_abi` — twelve `extern "C" fn` symbols the
  Cranelift JIT links against.
- `sdust_runtime::arena` — real `bumpalo::Bump` per arena frame; pop
  drops all bytes; alloc charges bytes against the per-thread budget
  counter (A50).
- `sdust_runtime::extern_loader` — `libloading` registry; opens libc
  (`libc.so.6` / `libSystem.dylib` / `msvcrt.dll`); per-extern
  overrides via `star.toml [extern]` table (A53).
- `sdust_runtime::supervisor_orchestrator` — completes the slice-7
  deferral. On child failure, looks up the supervisor binding,
  consults `RestartTracker::may_restart()`, computes deterministic
  XorShift* jitter backoff, returns
  `RestartDecision::Restart{backoff}` / `Escalate` / `Drop`.

### `sdust build` CLI subcommand

```
sdust build [PATH] [--debug] [--release] [--target TARGET] [--out-dir DIR]
```

- `--target native` (default) → `target/<name>` (or `<name>.exe`)
- `--target wasm32-wasi` → `target/<name>.wasm`
- `--target wasm32-web` → `target/<name>.wasm`
- Writes intermediate `.o` to `--out-dir`.
- Exit codes: 0 = ok, 1 = frontend error, 2 = backend / unknown target.

### `sdust run` switches to JIT (A48)

`sdust run <file>` now tries Cranelift JIT first. On
`CodegenError::Unsupported(_)` (the slice-8 backend covers a narrow
SIR subset), it falls back transparently to the slice-7 runtime path
(`pipeline::run_file_with_runtime`). `--legacy-interp` still routes
to the slice-6 tree-walker directly.

### Diagnostics: MT8001..MT8010

| Code | Meaning |
|------|---------|
| MT8001 | divide by zero (compiled code) |
| MT8002 | out-of-bounds index |
| MT8003 | integer overflow (checked) |
| MT8004 | null deref |
| MT8005 | extern symbol unresolved |
| MT8006 | unreachable executed |
| MT8007 | codegen rejected SIR shape |
| MT8008 | native linker missing |
| MT8009 | emitted Wasm failed validation |
| MT8010 | monomorphization failed |

All have `sdust explain SD8xxx` entries.

### Conformance corpus

`tests/conformance/codegen/` ships 7 cases:
- `native_hello`, `native_arith` (JIT-compile success)
- `wasm_hello`, `wasm_empty`, `wasm_web_target_emits_valid_module`
- `examples_01_hello_compiles_native`, `examples_01_hello_compiles_wasm`

All exercised by `crates/sdust-driver/tests/conformance_codegen.rs`.

## Spec interpretations (A46..A53)

| Amendment | Topic |
|-----------|-------|
| A46 | Cranelift-only native backend; LLVM scaffold gated on `feature = "llvm"` |
| A47 | Wasm Component Model deferred to v0.2; slice 8 ships core modules |
| A48 | `sdust run` defaults to JIT with interpreter fallback |
| A49 | Per-(fn, type-args) monomorphization; slice 8 strips generic fns |
| A50 | `bumpalo`-backed arenas with byte-charging |
| A51 | Codegen trap codes MT8001..MT8010 reserved |
| A52 | Native linker discovery: `clang` / `gcc` / `cc` preferred, skips MSYS `link.exe` shim |
| A53 | Extern resolution via `libloading` against host libc, overridable in `star.toml` |

## Stats

- **376 tests pass** (slice 7: 327 → slice 8: +49)
- 0 failing, 0 ignored
- `cargo clippy --workspace --all-targets -- -D warnings` clean
- 3 new crates; runtime crate gains 4 modules
- Workspace MSRV bumped to 1.85 (transitive `indexmap 2.14`
  requires edition2024)
- 8 new spec amendments (A46..A53)
- 10 new SD8xxx diagnostic codes

## Examples that compile end-to-end

| Example | `sdust run` (JIT or fallback) | `sdust build --target native` | `sdust build --target wasm32-wasi` |
|---|:-:|:-:|:-:|
| 01_hello | ok (JIT) | ok (`.o`; needs linker for exe) | ok |
| 02_struct_enum | ok (fallback) | unsupported (variants) | unsupported |
| 03_generic_fn | ok (fallback) | ok | ok |
| 04_result_propagation | ok (fallback) | unsupported (`?`) | unsupported |
| 05_match_expr | ok (fallback) | ok | ok |
| 06_for_while_loop | ok (fallback) | unsupported | unsupported |
| 07_agent_echo | ok (fallback) | unsupported | unsupported |
| 08_agent_state | ok (fallback) | unsupported | unsupported |
| 11_budget_block | ok (fallback) | unsupported | unsupported |
| 19_backend_service | ok (fallback) | unsupported | unsupported |

The "unsupported" cells fall through to the interpreter via
`CodegenError::Unsupported` — programs still run correctly under
`sdust run`. Full SIR coverage in the native backend (Result, ADT
construct/destructure, ?-propagation, agent spawn/send/ask) is v0.2
work.

## Still deferred (v0.2 / post-v0.1)

- LSP server
- Package manager + registry
- LLVM backend (scaffold present; enable with `--features llvm`)
- Wasm Component Model + `wit-component` integration
- Full SIR coverage in native codegen (ADT/Result/?/agent dispatch)
- Per-(fn, type-args) shared-generic monomorphization
- PGO / ThinLTO
- Multi-core work-stealing scheduler
- Cross-machine distributed agents
- Procedural macros
- DWARF / Wasm source maps (beyond function-level)
- Strict OTLP wire format for telemetry
- Field-level borrow tracking
- Effect-row polymorphism

## Files of note

- `crates/sdust-codegen-cranelift/src/lib.rs` — backend entry points
- `crates/sdust-codegen-cranelift/src/lower.rs` — SIR → cranelift IR
- `crates/sdust-codegen-cranelift/src/jit.rs` — JIT driver
- `crates/sdust-codegen-cranelift/src/object.rs` — AOT object writer + linker
- `crates/sdust-codegen-cranelift/src/abi.rs` — call-conv + type lowering
- `crates/sdust-codegen-cranelift/src/layout.rs` — ADT layout
- `crates/sdust-codegen-cranelift/src/mono.rs` — monomorphization
- `crates/sdust-codegen-cranelift/src/runtime_imports.rs` — runtime ABI sigs
- `crates/sdust-codegen-wasm/src/emit.rs` — Wasm module emitter
- `crates/sdust-codegen-wasm/src/target.rs` — `WasmTarget` enum
- `crates/sdust-codegen-llvm/src/lib.rs` — LLVM scaffold
- `crates/sdust-runtime/src/codegen_abi.rs` — runtime ABI bridge
- `crates/sdust-runtime/src/arena.rs` — bumpalo arena stack
- `crates/sdust-runtime/src/extern_loader.rs` — libloading registry
- `crates/sdust-runtime/src/supervisor_orchestrator.rs` — restart engine
- `crates/sdust-driver/src/build.rs` — `sdust build` pipeline
- `crates/sdust-cli/src/cmd/build.rs` — CLI subcommand
- `crates/sdust-cli/src/cmd/run.rs` — JIT-first run path
- `docs/internals/codegen-cranelift.md` — Cranelift backend internals
- `docs/internals/codegen-wasm.md` — Wasm backend internals
- `docs/internals/codegen-llvm.md` — LLVM scaffold notes
- `docs/reference/cli/sdust-build.md` — build CLI reference
- `docs/spec/v0.1-amendments.md` — A46..A53
- `tests/conformance/codegen/*` — 4 case dirs
- `crates/sdust-driver/tests/conformance_codegen.rs` — 7-case driver

## End of v0.1

Slice 8 closes Stardust v0.1. The language is feature-complete per
spec §31. The pre-v0.1 roadmap (`README.md`'s slice table) is now
fully shipped. v0.2 work begins with the LLVM backend, full SIR
coverage in codegen, and the Component Model wrapper.
