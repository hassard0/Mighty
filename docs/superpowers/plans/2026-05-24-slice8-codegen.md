# Slice 8 — Implementation Plan: Cranelift + Wasm Codegen

**Spec:** `docs/superpowers/specs/2026-05-24-slice8-codegen-design.md`
**Target tag:** `v0.8.0-codegen` then `v0.1.0`

## Phase 0 — Bootstrap (5 tasks)

1. **T01 — Detect LLVM** (DONE in design phase: not installed)
   - Confirms A46 fallback decision; LLVM crate is scaffold-only.

2. **T02 — Workspace plumbing**
   - Add `crates/sdust-codegen-cranelift`, `crates/sdust-codegen-wasm`,
     `crates/sdust-codegen-llvm` to `Cargo.toml` workspace members.
   - Workspace deps additions: `cranelift-codegen`, `cranelift-frontend`,
     `cranelift-module`, `cranelift-jit`, `cranelift-object`,
     `cranelift-native`, `target-lexicon`, `wasm-encoder`,
     `wasmparser`, `bumpalo`, `libloading`.
   - Verify `cargo build --workspace` after empty crate scaffolds.

3. **T03 — Crate skeletons**
   - Create `lib.rs` + minimal `mod backend` for each codegen crate.
   - Stub `pub fn compile(_: &Program) -> Result<Artifact, CodegenError>`
     returning `Err(NotYetImplemented)` to confirm wiring.

4. **T04 — Diagnostic codes MT8001..MT8010**
   - Reserve in `sdust-diagnostics/src/codes.rs`.
   - Wire `explain` for each.

5. **T05 — Amendment A46..A53 entries**
   - Add to `docs/spec/v0.1-amendments.md` after A45 with short prose
     each.

## Phase 1 — Cranelift Native Backend (12 tasks)

6. **T06 — Cranelift module abstraction**
   - `cl::CodegenCtx` wrapping `cranelift_module::Module`.
   - Functions to declare/import runtime fns.

7. **T07 — Type lowering**
   - `lower_ty(&SirTy) -> Vec<types::Type>` for the param/return ABI.
   - `layout_of(&SirTy, &AdtCatalog) -> Layout { size, align, fields }`.

8. **T08 — Function emission skeleton**
   - For each SIR fn: declare signature; build `FunctionBuilder`.
   - Walk blocks: emit terminators (`Goto`, `If`, `Return`).

9. **T09 — Statement lowering — Assign / Const / BinOp**
   - Const lit → `iconst`/`fconst`/`stack_store` for strings.
   - BinOp arithmetic / comparison.
   - Place writes to local stack slots.

10. **T10 — Place / Projection lowering**
    - Local → stack slot or SSA value.
    - Field / TupleIndex → offset compute.
    - Deref → load.

11. **T11 — Call lowering**
    - `Call { Builtin(Log) }` → call into runtime via imported sym.
    - `Call { User }` → direct call by `FuncId`.

12. **T12 — String constants & literal pool**
    - Emit strings as data symbols (`DataDescription` in cranelift).
    - Load (ptr, len) pair at use site.

13. **T13 — Monomorphization pass**
    - Walk SIR; collect generic call sites; specialize per (FnId, args).
    - Rewrite call graph to specialized ids; drop generic originals.

14. **T14 — JIT driver**
    - `JITBuilder::new(default_libcall_names()).build() -> JITModule`.
    - Register runtime symbol table.
    - Finalize, get fn ptr for `main`, call via `transmute<fn()>`.

15. **T15 — AOT driver (object)**
    - `ObjectBuilder` with host triple.
    - Write `.o` to temp; invoke linker via `cc`/`gcc`/`link.exe`.
    - Produce final exe at `target/<name>{,.exe}`.

16. **T16 — Runtime extern table (C ABI)**
    - `runtime/src/codegen_abi.rs`: `extern "C"` fns the JIT calls.
    - `stardust_runtime_log`, `stardust_runtime_panic`,
      `stardust_runtime_eff_call_generic`, etc.

17. **T17 — Cranelift unit tests**
    - 8-12 unit tests inside the crate.

## Phase 2 — Wasm Backend (8 tasks)

18. **T18 — Wasm module scaffold**
    - Use `wasm-encoder::{Module, FunctionSection, TypeSection,
      ExportSection, CodeSection}`.
    - Emit empty module → validate via `wasmparser`.

19. **T19 — Wasm type lowering**
    - Mirror Cranelift's `lower_ty`, mapped to `ValType::{I32, I64,
      F32, F64}`.
    - Aggregates: encode via stack pointer + linear memory.

20. **T20 — Wasm function bodies**
    - Per SIR fn, build a `Function` body via `wasm-encoder::Function`.
    - Lower stmts: const, binop, local set/get, return.

21. **T21 — Wasm memory + data**
    - `MemorySection` with 16-page initial / unbounded max.
    - `DataSection` for string constants.
    - Bump-allocator helper fn emitted inline.

22. **T22 — Wasm capability imports**
    - `(import "stardust" "log" (func (param i32 i32)))`.
    - One import per capability surface present in the program.

23. **T23 — `wasm32-wasi` adapter**
    - Emit `(import "wasi_snapshot_preview1" "fd_write" ...)`.
    - Route `log` calls to WASI `fd_write` with stderr fd.

24. **T24 — Wasm artifact writer**
    - Write `.wasm` bytes to `target/<name>.wasm`.
    - Print artifact path.

25. **T25 — Wasm unit + validate tests**
    - Validate every emitted module via `wasmparser::validate`.

## Phase 3 — Driver / CLI Wiring (5 tasks)

26. **T26 — `sdust build` subcommand**
    - clap subcommand with `--debug`, `--release`,
      `--target {native,wasm32-wasi,wasm32-web}`, `--out PATH`,
      `--profile {core}`.
    - Dispatches to codegen-cranelift or codegen-wasm.
    - Writes artifact to `target/`.

27. **T27 — `sdust run` switches to JIT**
    - Default path: compile via cranelift-jit, find `main`, invoke.
    - Fallback to interpreter on `CompileResult::Unsupported`.
    - `--legacy-interp` flag keeps slice-6 tree-walker.

28. **T28 — Manifest plumbing**
    - `star.toml [extern]` table → propagated through pipeline.
    - Build-mode defaults (`build.default-mode = "debug"` etc.).

29. **T29 — Driver compile entry**
    - `pipeline::compile_native(prog, out_dir, mode) -> ArtifactPath`.
    - `pipeline::compile_wasm(prog, out_dir, target) -> ArtifactPath`.

30. **T30 — Linker discovery**
    - `find_linker()` per A52 order.
    - Emit `.o` and instructive error if none found.

## Phase 4 — Runtime Integration (5 tasks)

31. **T31 — Bumpalo arenas**
    - `runtime/src/arena.rs`: real `Bump`-backed arena.
    - Hook into `ArenaPush`/`ArenaPop` via runtime extern table.
    - Charge bytes against `BudgetTracker`.

32. **T32 — Real `extern` resolution**
    - `runtime/src/extern_loader.rs`: open libc via `libloading`.
    - Symbol cache; lookup by name; trap MT8005 on miss.

33. **T33 — JIT-runtime bridge**
    - Runtime hosts a `CodegenBridge` that JIT'd code calls via the
      extern table.
    - Routes back into `Mailbox::send`, `Mailbox::ask`,
      `Scheduler::spawn`, `BudgetTracker::charge`, `Telemetry::emit`.

34. **T34 — Supervisor auto-restart orchestrator**
    - Wire `SupervisorRegistry::on_child_failure` into agent loop.
    - Rate-limit, backoff, restart per strategy.
    - 4 new unit tests.

35. **T35 — Runtime smoke tests**
    - End-to-end: build example 08 → run → assert state mutation.

## Phase 5 — Conformance + Examples (5 tasks)

36. **T36 — Conformance: native (8 cases)**
    - `tests/conformance/codegen/native_*/input.sd` + `expected.txt`.
    - Runner in `crates/sdust-driver/tests/conformance_codegen_native.rs`.

37. **T37 — Conformance: wasm (4 cases)**
    - `tests/conformance/codegen/wasm_*/input.sd` + expected validation.
    - Runner in `crates/sdust-driver/tests/conformance_codegen_wasm.rs`.

38. **T38 — Examples sweep**
    - Verify 01, 07, 08, 11, 19 build to native binary.
    - Verify 01, 02, 03, 04 build to wasm (subset; agents excluded
      from wasm slice-8 — out of scope).

39. **T39 — Update tour ch.16 (codegen)**
    - `docs/tour/16_codegen.md` walking through build+run for a real
      binary.

40. **T40 — Update `getting-started.md`**
    - Add `sdust build` flow section; show artifact paths.

## Phase 6 — Docs + Release (8 tasks)

41. **T41 — `docs/internals/codegen-cranelift.md`**
    - Architecture, type lowering table, monomorphization, JIT vs AOT.

42. **T42 — `docs/internals/codegen-wasm.md`**
    - Module shape, memory model, capability imports, WASI bridge.

43. **T43 — `docs/internals/codegen-llvm.md`**
    - Scaffold notes, why feature-gated, future-work outline.

44. **T44 — `docs/reference/cli/sdust-build.md`**
    - Full flag reference, examples, exit codes.

45. **T45 — Diagnostics ref update**
    - Add MT8001..MT8010 to `docs/reference/diagnostics.md`.

46. **T46 — README update**
    - Roadmap shows slice 8 shipped; v0.1 = complete; add `sdust build`
      to quickstart.

47. **T47 — `RELEASE-v0.1.md`**
    - Summary of slices 1-8: scope, stats, achievements, what v0.2
      will bring.

48. **T48 — `SLICE8.md`**
    - Same shape as SLICE1..7.md: what landed, amendments, stats,
      deferrals.

## Phase 7 — Ship (3 tasks)

49. **T49 — Final test pass + clippy**
    - `cargo test --workspace` ≥ 390 / 0 fail.
    - `cargo clippy --workspace --all-targets -- -D warnings` clean.
    - `cargo fmt --check`.

50. **T50 — Tag + push**
    - Commit: "Slice 8: docs — codegen + RELEASE-v0.1 + SLICE8.md".
    - Tag `v0.8.0-codegen`; tag `v0.1.0`.
    - Push main + tags.

## Execution

Tasks T01..T05 sequential (workspace state changes).
Tasks T06..T17, T18..T25, T31..T34 partially parallelizable.
Tasks T36..T48 mostly parallel after backends green.
T49..T50 strictly final.

Given the 8-hour subagent budget and that the user is asleep, the
slice-leader executes inline rather than dispatching parallel subagent
swarms; a single coherent driver thread is faster for this size of
work than orchestrating workers across crates that share a `Cargo.lock`.
