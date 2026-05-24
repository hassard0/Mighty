# Mighty Slice 6 — Complete

**Tag:** `v0.6.0-sir`
**Date:** 2026-05-24

## What landed

### Mighty mid-level IR (spec §24.4)

- New crate `mty-sir` with `Program`, `Function`, `Block`, `Stmt`,
  `Rvalue`, `Term`, `Place`, `Projection`, `Const`, `FnRef`, `BuiltinId`,
  `EffectOp`, `SirTy`.
- Basic-block form (no SSA / phi). Locals are MIR-style with `_0` as
  the return slot and `_1..=N` as parameters. `LocalDecl` carries
  `name`, `ty`, `mutable`, `source: LocalSource`.
- Capability values carry `(family, constraint)` from `mty-types`.
- Arena push/pop sentinels, effect-invoke statements, agent
  spawn/send/ask rvalues, `?` propagation via `TryReturnErr`, async
  suspension reserved by `Term::Suspend`.

### HIR → MtyIR lowering (`crates/mty-sir/src/lower/`)

- Total + best-effort: never panics, even on partially-type-checked
  input.
- Fn shells allocated up front so call-site resolution can refer to
  callees in any order.
- Pattern matching for `match` and `if let` (wildcard, binding,
  literal, tuple, enum, struct, range, ref).
- `?` operator: switch-variant + `TryReturnErr` synthesis.
- Agents: synthesized state ADT + ctor fn + per-handler fn; sends/asks
  become `Rvalue::Send`/`Rvalue::Ask`.
- Effect calls: receivers resolving to a `Module` lower to
  `Stmt::EffectInvoke`.
- Method-call disambiguation: `local.method(args)` (which the parser
  emits as `Call{Path([local,method])}`) is re-routed to `MethodCall`.

### Slice-6 interpreter (`crates/mty-sir/src/interp/`)

- Tree-walking, single-threaded, deterministic.
- `Value` enum covers primitives + tuples + arrays + structs + enums +
  refs + agents + caps.
- `Host` trait + `RealHost` (stdout/stderr) + `BufferHost` (captures
  output, effect log, extern log for tests).
- Step budget (default 1 000 000) and `RunResult` discriminated union.
- Builtins: `log`, `print`, `panic`, `spawn`, `move`, `fetch`, `null`,
  `raw_ptr`, `valid`, + `extern:<name>` fallthrough to the host.
- Built-in methods (permissive): `len`, `to_str`, `as_str`,
  `is_empty`, `unwrap`/`unwrap_or`, `ok`/`ok_or`, `ro`/`rw`/`path`/
  `host`, `get`/`query`, `contains`/`starts_with`/`ends_with`, etc.

### `mty run <file>`

- New CLI subcommand wiring parse → typeck → borrowck → MtyIR-lower → interp.
- Exit codes: `0` success, `1` compile error / trap / `Err`, `2` no
  `main`, `3` budget exceeded.
- `mty dump --sir <file>` joins the existing `--ast --cst --hir`
  flags; emits a MIR-style text rendering.

### Diagnostics MT5001..MT5050 (slice 6 runtime)

- MT5001 runtime_panic
- MT5002 use_after_drop
- MT5003 division_by_zero
- MT5004 integer_overflow (debug)
- MT5005 unreachable_match
- MT5006 unhandled_error_result
- MT5007 arena_escape_runtime
- MT5008 uncallable_builtin
- MT5009 budget_exceeded
- MT5010 sandbox_violation (placeholder)
- MT5020 agent_handler_missing
- MT5021 send_to_dead_agent
- MT5050 extern_fn_unimpl

All have `mty explain SD5xxx` entries.

## All 20 examples lower cleanly

```
mty dump --sir examples/01_hello.sd  → ok
... (all 20)
```

`mty run examples/01_hello.sd` prints `hello, Mighty` and exits 0.
Examples with a runnable `main()` execute end-to-end; the rest succeed
at MtyIR-lowering. The synchronous-agent path (A32) handles 07/08
shallowly; full agent execution arrives in slice 7.

## Conformance corpus

`tests/conformance/runtime/` ships **6** initial cases driven by
`crates/mty-driver/tests/conformance_runtime.rs`:

1. `hello` — `log("hello")`
2. `arithmetic` — `(1 + 2 * 3).to_str()` → 7
3. `if_chain` — int comparison branches
4. `let_block` — multi-binding addition
5. `string_concat` — Str `+` operator
6. `panic_exits` — `panic("boom")` traps

## Spec interpretation calls (recorded as amendments)

- **A31** — Arena runtime enforcement deferred to slice 7
- **A32** — Slice-6 agent dispatch is synchronous (no mailbox queue)
- **A33** — Effect calls dispatched via Host trait
- **A34** — Budgets + sandboxes are metadata-only in slice 6
- **A35** — Slice-6 interpreter is single-thread deterministic

## Stats

- **290 tests pass** (slice 5: 274 → slice 6: +16)
- 13 new SD5xxx diagnostic codes
- 6 runtime conformance cases
- New crate `mty-sir` (~2 400 lines of Rust)
- `mty-driver` + `mty-cli` extended with MtyIR-lower / run / dump-sir
  surfaces

## Still deferred (slice 7 unless noted)

- ~~Concurrent scheduler + work stealing~~ — **shipped in slice 7 (`v0.7.0-runtime`)**
- ~~Real mailbox slabs~~ — **shipped in slice 7**
- ~~Supervisor restart policies~~ — **strategies + restart-tracker shipped in slice 7;**
  **automatic restart orchestrator lands with slice 8 codegen**
- ~~Budget / sandbox enforcement~~ — **shipped in slice 7 (BudgetTracker + allowlists)**
- Real `extern { fn ... }` calls — slice 8
- Real effect-system syscalls — slice 8 (slice 7 wires the host trait + sandbox checks)
- Real arena allocator — slice 8 (slice 7 ships approximate `mem_bytes` per A37)
- Field-level borrow tracking — slice 8 (slice-4 still local-granular)
- LLVM / Cranelift codegen — slice 8
- Wasm component-model codegen — slice 8
- Monomorphization of generics — slice 8
- DCE / inlining / escape analysis — post-v0.1
- True NLL / Polonius — post-v0.1
- Effect-row polymorphism — post-v0.1
- Full Drop impl execution at scope exit — post-v0.1

## Files of note

- `crates/mty-sir/src/sir.rs` — MtyIR data types
- `crates/mty-sir/src/dump.rs` — text rendering
- `crates/mty-sir/src/lower/mod.rs` — lowering entry
- `crates/mty-sir/src/lower/items.rs` — fn/struct/enum/agent
- `crates/mty-sir/src/lower/exprs.rs` — expression lowering
- `crates/mty-sir/src/lower/pats.rs` — pattern matching
- `crates/mty-sir/src/lower/ty.rs` — type translation
- `crates/mty-sir/src/lower/ctx.rs` — `LowerCtx`, `FnBuilder`
- `crates/mty-sir/src/interp/value.rs` — `Value`, `Frame`,
  `Reference`, `AgentHandle`
- `crates/mty-sir/src/interp/host.rs` — `Host`, `RealHost`,
  `BufferHost`
- `crates/mty-sir/src/interp/run.rs` — step loop + eval + builtins
- `crates/mty-cli/src/cmd/run.rs` — `mty run` subcommand
- `crates/mty-cli/src/cmd/dump.rs` — `--sir` flag wiring
- `crates/mty-driver/src/pipeline.rs` — `lower_to_sir`, `run_file`
- `crates/mty-driver/tests/sir_lower_examples.rs` — 20-example smoke
- `crates/mty-driver/tests/interp_runnable.rs` — interp acceptance
- `crates/mty-driver/tests/conformance_runtime.rs` — corpus driver
- `tests/conformance/runtime/*` — 6 runtime cases
- `crates/mty-diagnostics/src/codes.rs` — MT5001..MT5050 + explain
- `docs/internals/sir.md`, `docs/internals/interpreter.md` — new
- `docs/reference/cli/mty-run.md` — new
- `docs/getting-started.md` — extended with `mty run`
- `docs/spec/v0.1-amendments.md` — A31..A35
