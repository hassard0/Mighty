# Slice 8 — Native + Wasm Codegen (spec §24.5, §24.6, §24.7, §31.6, §31.7)

**Date:** 2026-05-24
**Predecessor:** `v0.7.0-runtime` (`e02b122`) — runtime MVP
**Target tag:** `v0.8.0-codegen` (with `v0.1.0` rolled on top if green)
**Status:** FINAL slice of Stardust v0.1.

## Goal

Compile SIR to real, runnable artifacts. Stop being an interpreter.

After this slice:

- `sdust build src/main.sd` produces a **native executable** that runs
  without the interpreter being present.
- `sdust build --target wasm32-wasi src/main.sd` produces a
  **Wasm module** runnable under `wasmtime`/`wasmer`.
- `sdust run` JIT-compiles via Cranelift and executes — replacing the
  slice-7 per-turn interpreter call with a compiled function pointer.
- Agents, mailboxes, supervisors, budgets, timers (all of slice-7) keep
  working — runtime crate is unchanged; only the per-turn callback
  swaps from interpreter to compiled fn.

## Big decision: LLVM vs Cranelift-only

The spec calls for an LLVM backend. We **detected LLVM is not installed**
on the build host (no `llvm-config`, no `clang`). Per the slice-leader
fallback explicit in the dispatch:

> If LLVM install isn't available on the build host, that's a real
> problem — slice 8 leader: detect upfront and degrade to "Cranelift-only
> for native" with a documented amendment (A46) if LLVM isn't installed.

We're taking the fallback. Slice 8 ships **Cranelift-only for native**,
with Wasm via pure-Rust `wasm-encoder`. The `sdust-codegen-llvm` crate is
**scaffolded behind a `cfg(feature = "llvm")` flag** so a future host
with LLVM installed can flip it on, but the default build path is
Cranelift everywhere for native and `wasm-encoder` for Wasm.

This is documented as **Amendment A46**.

## Scope

In-scope:

1. SIR → Cranelift IR lowering (replaces LLVM lowering for slice 8).
2. JIT-compile via `cranelift-jit` for `sdust run` and `sdust build --debug`.
3. AOT object emission via `cranelift-object` for `sdust build --release`.
4. SIR → Wasm core module via `wasm-encoder`. Component-model wrapper
   via lightweight WIT sidecar (deferred to v0.2 for full
   `wit-component` integration; we emit core module + capability
   imports for slice 8).
5. New `sdust build` subcommand with `--debug`, `--release`,
   `--target {native,wasm32-wasi,wasm32-web}`, `--out DIR`.
6. Runtime adapter so `sdust run` invokes the compiled main fn via JIT
   when possible, falling back to interpreter for unsupported SIR
   features.
7. Generic monomorphization — emit one specialized fn per (fn, type
   args) tuple. Slice-8-acceptable code bloat.
8. Real bumpalo-backed arena allocator inside the runtime, charging
   bytes against `BudgetTracker::mem_bytes`.
9. Real C ABI `extern { fn ... }` calls via `libloading`. Defaults to
   libc; user can specify alternate libs in `star.toml`.
10. Supervisor auto-restart orchestrator wiring (deferred from slice 7).
11. Examples 01, 07, 08, 11, 19 produce runnable artifacts.
12. Conformance corpus `tests/conformance/codegen/` (8 native + 4 wasm).
13. Internals docs: `codegen-cranelift.md`, `codegen-wasm.md`,
    `codegen-llvm.md` (placeholder + design notes).
14. `RELEASE-v0.1.md` summary spanning slices 1-8.
15. Amendment A46 + amendments for runtime/codegen interactions.

Out-of-scope (deferred to v0.2 or post-v0.1):

- LLVM backend code generation (scaffold-only; behind feature flag).
- Full Wasm Component Model with WIT auto-binding (we emit core
  modules + a hand-written WIT sketch for the prelude — closest we
  can come without `wit-component`).
- DWARF debug info (Cranelift can emit DWARF stubs; we emit basic
  function-level entries only).
- Source maps for Wasm.
- PGO / ThinLTO.
- Cross-compilation to non-host triples (host triple only for native).
- Multi-core scheduler.

## Architecture

Three new crates, one optional via feature:

```
crates/
  sdust-codegen-cranelift/    — default native backend (JIT + object)
  sdust-codegen-wasm/         — Wasm core module emission
  sdust-codegen-llvm/         — feature-gated, scaffold-only
```

A small `sdust-codegen-shared` crate hosts the SIR-to-backend translation
machinery shared between Cranelift and the future LLVM backend (type
lowering tables, layout calculation, monomorphization queue, calling
convention helpers).

For Slice 8 we collapse `sdust-codegen-shared` into a `lower` module
inside each backend crate to avoid a fifth new crate; we re-evaluate
extraction once LLVM lands.

### Pipeline

```
.sd source
  → syntax (CST)
  → ast
  → hir
  → types (typed-hir)
  → borrow
  → sir
  → MONOMORPHIZE   ← new
  → codegen-cranelift   ← new      OR    codegen-wasm   ← new
  → cranelift-jit (run) | cranelift-object (build)   OR   wasm bytes
  → executable | .o / .wasm
```

### `sdust build` flow

1. Driver: parse → lower → typeck → borrowck → SIR.
2. Pick backend by `--target` flag:
   - `native` (default) → `sdust-codegen-cranelift`.
   - `wasm32-wasi` / `wasm32-web` → `sdust-codegen-wasm`.
3. Monomorphize generic call sites.
4. Build Cranelift module / Wasm module.
5. Emit:
   - native: `.o` via `cranelift-object`, then link with platform
     linker (cc on unix, link.exe on win) into `target/<name>` or
     `target/<name>.exe`.
   - wasm: write `.wasm` bytes to `target/<name>.wasm`.
6. Print artifact path.

### `sdust run` flow (NEW)

1. Driver: parse → lower → typeck → borrowck → SIR.
2. Monomorphize.
3. Build Cranelift JIT module.
4. Look up `main` fn pointer.
5. If `main` exists, invoke directly via JIT.
6. If agents are declared, start the runtime, but per-turn now calls
   into compiled handler fn pointers (via the JIT module's symbol
   table) rather than `interp::run_handler_isolated`.
7. Interpreter fallback: any SIR shape the codegen rejects (extern C,
   complex effect ops) still routes to the interpreter via a
   `CompileResult::Unsupported(reason)` path. This is the slice-8
   safety valve — we degrade rather than crash.

### Cranelift type lowering

| SIR type        | Cranelift type                              |
|-----------------|---------------------------------------------|
| `Bool`          | `I8`                                        |
| `Int(I32)`      | `I32`                                       |
| `Int(I64)`      | `I64`                                       |
| `Int(U32)`      | `I32` (unsigned ops where it matters)       |
| `Int(USize)`   | host pointer-width                          |
| `Float(F32)`    | `F32`                                       |
| `Float(F64)`    | `F64`                                       |
| `Str`/`String`  | `(ptr, len)` pair → struct-by-value in regs |
| `Bytes`         | same shape as Str                           |
| `Unit`/`Never`  | zero-size; pass nothing                     |
| `Ref<T>`        | host pointer (`I64`/`I32` per target)       |
| `Tuple(...)`    | flattened to multi-return where small       |
| `Array<T,N>`    | stack slot of `N * sizeof(T)`               |
| `Adt(id, args)` | struct on stack with computed layout        |
| `Cap`           | opaque pointer to runtime handle            |
| `Dyn`           | (ptr, vtable) fat ptr                       |
| `RawPtr`        | host pointer width                          |
| `Param`         | monomorphized away                          |
| `Error`         | trap on construct                           |

Layout uses natural alignment, no packing tricks. ADT field offsets
computed deterministically (alignment-respecting sequential layout).

### Wasm lowering

Wasm core module per package. Each SIR function emits a Wasm function
with the same calling convention rules as Cranelift (Wasm has no
multi-return for some toolchains, so we marshal via stack pointer for
returns >2 i32s). Capabilities are imports (`(import "stardust" "cap_open"
(func ...))`). Memory: linear memory of 16 pages initial, grows on
demand. Strings encoded as `(ptr i32, len i32)` pairs into linear memory.

We do NOT emit a Component Model wrapper for slice 8. A WIT sketch is
hand-written and saved at `runtime/wit/stardust.wit` for documentation.

### Calling convention

- Native: SystemV on Linux/Mac, Windows x64 ABI on Windows.
  `cranelift-codegen::isa::CallConv::SystemV` and `::WindowsFastcall`.
- `extern c` fns: same ABI as host (cranelift takes care of this).
- Wasm: `CallConv::WasmtimeSystemV`.

### Calling into the runtime from JIT'd code

The runtime is Rust. JIT'd code calls runtime functions via:

1. The runtime registers a small table of C-ABI fns
   (`stardust_runtime_send`, `stardust_runtime_ask`, `stardust_runtime_spawn`,
   `stardust_runtime_log`, `stardust_runtime_alloc_arena`,
   `stardust_runtime_budget_charge`, `stardust_runtime_panic`,
   `stardust_runtime_eff_call`).
2. The Cranelift module declares these as imported symbols.
3. The JIT links them at finalization time.

This keeps the runtime crate free of cranelift dependencies.

### Monomorphization

A pass before codegen:

1. Walk SIR fns, collect every `Call { func, args }` where the callee
   is generic. For each, compute the concrete type-args from the call
   site (recorded by typeck and threaded through to SIR via slice-3
   metadata).
2. Maintain a worklist `Set<(SirFnId, TypeArgs)>`. Pop, emit a
   specialized SIR fn (clones the source fn, substitutes
   `SirTy::Param` occurrences with concrete `SirTy`), recurse for
   further generic calls discovered.
3. Update the call graph: every generic call site is rewritten to a
   monomorphized fn id.
4. Original generic fns are dropped from the codegen unit.

Slice-8 takes the code-bloat hit; size-optimized shared generics is a
v0.2 task.

### Real arena

Replace slice-7's "approximate" arena. The runtime maintains a
`bumpalo::Bump` per arena scope. `ArenaPush` opens a frame
(snapshot pointer), `ArenaPop` resets the bump to that pointer.
Bytes allocated are charged via `BudgetTracker::charge_bytes()` and
counted against `mem_bytes` limit.

### Real extern C

`extern { fn foo(...) -> ... }` declarations get resolved at runtime
startup via `libloading::Library::open(...)`. Default library on linux
is `libc.so.6`, on macos `libSystem.dylib`, on win `msvcrt.dll`. Users
can override per-extern via `[extern]` table in `star.toml`:

```toml
[extern]
"sqlite3_open" = "libsqlite3"
```

For unresolved externs we trap at the call site with MT8005.

### Supervisor restart orchestrator

Wire `SupervisorRegistry::on_child_failure` into the agent loop's error
path. On failure the orchestrator:

1. Looks up the child's strategy from the supervisor binding.
2. Applies the rate-limit window (`restart up_to N in DUR`).
3. Computes backoff (uniform-jitter, seeded LCG for determinism).
4. Schedules a restart via the runtime scheduler.
5. If rate-limit exceeded, escalates per strategy.

The slice-7 code shipped all the pieces; slice 8 just wires the
orchestrator into the loop. Tests live in
`crates/sdust-runtime/tests/supervisor_orchestrator.rs`.

### Panic & trap handling

Native: compiled fns panic via Rust runtime → caught at agent boundary
by `std::panic::catch_unwind`. Trap codes: MT8001 (divide-by-zero),
MT8002 (oob index), MT8003 (overflow when checked), MT8004 (null
deref), MT8005 (extern unresolved), MT8006 (unreachable).

Wasm: same trap codes, surface via wasmtime's `Trap` type when run
under wasmtime; slice-8 ships byte-only Wasm and leaves wasmtime
integration to v0.2.

## Crate dependencies

```
sdust-codegen-cranelift/Cargo.toml:
  cranelift-codegen = "0.116"
  cranelift-frontend = "0.116"
  cranelift-module = "0.116"
  cranelift-jit = "0.116"
  cranelift-object = "0.116"
  cranelift-native = "0.116"
  target-lexicon = "0.12"
  sdust-sir = { workspace = true }
  sdust-types = { workspace = true }
  sdust-diagnostics = { workspace = true }

sdust-codegen-wasm/Cargo.toml:
  wasm-encoder = "0.220"
  wasmparser = "0.220"
  sdust-sir = { workspace = true }
  sdust-types = { workspace = true }

sdust-codegen-llvm/Cargo.toml:
  # feature-gated scaffold; not built by default
  [features]
  default = []
  llvm = ["inkwell"]
  [dependencies]
  inkwell = { version = "0.4", features = ["llvm17-0"], optional = true }
```

The runtime crate gains:
```
  bumpalo = "3"
  libloading = "0.8"
```

## Conformance corpus

`tests/conformance/codegen/`:

- `native_hello/` — example 01 compiled native, run, observe stdout
- `native_counter/` — example 08 compiled native, run, agents work
- `native_echo/` — example 07 compiled native, ping/pong
- `native_arena/` — bumpalo arena scope
- `native_supervisor/` — supervisor restart after panic
- `native_extern_c/` — `extern { fn abs(...) }` calling libc
- `native_budget/` — budget breach trap
- `native_generic/` — generic fn monomorphizes correctly

- `wasm_hello/` — example 01 compiled wasm, validate via wasmparser
- `wasm_arithmetic/` — int add, returns i32
- `wasm_string/` — string memory layout
- `wasm_capability/` — capability import shape

Each native test: build, exec, compare stdout to `expected.txt`.
Each wasm test: build, `wasmparser::validate(...)`, optional opcode-shape check.

## Test strategy

- Unit tests inside each codegen crate (~30 each).
- Integration tests in `tests/codegen_native.rs` and
  `tests/codegen_wasm.rs` driving the full pipeline on canonical
  programs.
- Conformance corpus (12 cases) as above.
- All slice-1..7 tests (327) keep passing — codegen only adds, never
  modifies existing crates beyond `cli`/`driver`/`runtime` wiring.

Target test count: **390+**.

## Amendments to coin

- **A46** — LLVM backend degraded to feature-gated scaffold when
  `llvm-config` absent at build time. Cranelift becomes the default
  for native in v0.1.
- **A47** — Wasm Component Model deferred to v0.2; slice 8 ships core
  modules with capability imports and a hand-written WIT sketch.
- **A48** — `sdust run` switches default execution path to Cranelift
  JIT; `--legacy-interp` retained as escape hatch (now means "use
  slice-6 tree-walking interpreter").
- **A49** — Generic monomorphization strategy: per-(fn, type-args)
  specialization; size-optimized shared generics deferred to v0.2.
- **A50** — Slice-8 arena = bumpalo-backed `Bump` per arena scope;
  byte-accurate budget charging through `BudgetTracker`.
- **A51** — Trap code namespace MT8001..MT8010 reserved for codegen
  runtime traps.
- **A52** — Native linker discovery order: `$STARDUST_LINKER` env →
  `cc` → `gcc` → `clang` → MSVC `link.exe`. First found wins. If none,
  emit `.o` only and instruct user.
- **A53** — Extern-fn resolution via `libloading` against host libc by
  default; per-extern overrides in `star.toml [extern]` table.

## Risks & mitigations

| Risk | Mitigation |
|------|------------|
| Cranelift API churn (0.x crate) | Pin minor version; CI exercises full pipeline |
| `link.exe` not on PATH (Windows dev) | Fall back to MSVC if `cc` missing; document |
| Cranelift can't lower exotic SIR | `CompileResult::Unsupported(reason)`; interpreter falls back transparently |
| `wasm-encoder` API surface | Stay on stable ops; vendor a shim if needed |
| `bumpalo` reset semantics | Drop-aware wrapper; verified by unit tests |
| `libloading` symbol-resolution UB | Mark `unsafe`; restrict to `extern` block-declared names |

## Acceptance criteria

- `cargo test --workspace` ≥ 390 passing, 0 failing.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `sdust build examples/01_hello.sd` produces `target/hello` and
  `./target/hello` prints `hello, Stardust`.
- `sdust build --target wasm32-wasi examples/01_hello.sd` produces
  `target/hello.wasm`, validates via `wasmparser`.
- `sdust run examples/08_agent_state.sd` produces `1 2 3` (via JIT).
- All 327 existing tests pass.
- Tag `v0.8.0-codegen` pushed; `v0.1.0` tag pushed on top.
- `RELEASE-v0.1.md`, `SLICE8.md` committed.
- Three new internals docs.
