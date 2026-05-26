# Mighty v0.22 — Release Notes

**Tag:** `v0.22.0`
**Date:** 2026-05-26
**Status:** SHIPPED — five-track swarm + integrator pass.

**Headline:** **All post-v1.0 roadmap items now landed pre-v1.0 —
work-stealing (Tier 5) + PGO/ThinLTO + Python full pipeline. Only
RFC comment windows remain for v1.0 GA.**

v0.22 closes the v0.21 "Post-v1.0" block end-to-end. **Per-message
work-stealing (Tier 5) lands**: the v0.10 affinity-hint scheduler
is promoted to true crossbeam-deque per-worker queues with
NUMA-locality steal ordering (own NUMA node → same socket →
anywhere fallback) and a new process-wide
`worker.steals_total{src,dst}` OTel counter; the
`local → siblings → injector` phase reversal alone produces a 61%
speed-up on pinned-task bursts vs v0.21. **PGO + ThinLTO build
profile lands**: new `release-pgo` cargo profile + two-stage
`scripts/build-pgo.{sh,ps1}` pipeline (instrumented build →
`mty-bench-pgo` sweep across `examples/*.mty` → `llvm-profdata
merge` → final build with `-Cprofile-use` + `-Clinker-plugin-lto`);
a manual `pgo-bench.yml` workflow runs the pipeline on
`workflow_dispatch` and writes a baseline-vs-PGO `mty check` delta
to the workflow summary. **Python 2nd-impl full pipeline lands**:
the impl-py 2nd-impl now covers **lex → parse → lower → typeck →
borrow → wasm** end-to-end with a 28-test NLL borrow checker
(MT3001–MT3005) + 37-test Core 1.0 wasm codegen (i32 arithmetic,
control flow, calls, deduplicated type table, structural
validation) + 96-case full-pipeline sweep (24 examples × 4
phases); 21/24 examples emit at least one wasm function body (the
3 zero-fn examples are agent-only files). Python test count grows
**311 → 474** (+163). **Diagnostic-code coverage closure**: 7 of
the 8 v0.21-uncovered codes activate (MT0004 UNKNOWN_DURATION_UNIT,
MT0030 DEPTH_LIMIT_EXCEEDED, MT2015 NON_EXHAUSTIVE_MATCH, MT2016
UNREACHABLE_MATCH_ARM, MT2018 IF_BRANCH_MISMATCH, MT2019
RETURN_TYPE_MISMATCH, MT3015 USE_OF_UNINITIALIZED) via parser
pre-lex scan + typeck emit-site wiring + borrow-walker `Uninit`
binding; **MT3012 DROP_IN_CONST_CONTEXT deferred to v0.23** —
HIR doesn't yet lower `CONST_DECL`, so a const-context flag
requires a multi-crate refactor beyond the closure slice budget.
Coverage 62 → 69 direct (56% → 63%), any-harness 93% → 99%,
uncovered 8 → 1. **MtyIR `Stmt` real source-span carrier lands**:
every MtyIR `Stmt` + `Terminator` now carries a real `SourceSpan`
field (default `SourceSpan::ZERO` for manually-constructed
programs); HIR spans propagate through `lower → MtyIR →
cranelift SourceLoc → DWARF v5 line row`, so v0.21's
synthetic-uniform per-statement byte-offset spread is gone and
`gdb step-line` is byte-accurate.

If you were on v0.21.0, the upgrade is `git pull && cargo install
--path crates/mty-cli --force` (or pull the v0.22.0 pre-built
binaries from the Releases page). There are **no source-level
breaking changes** at the language layer. The `release-pgo`
profile is opt-in (`scripts/build-pgo.sh` / `.ps1`); the default
build uses the v0.21-vintage `release` profile unchanged. The
work-stealing pool is on by default; `Scheduler::multi_worker(n)`
is the new entry point and the v0.21 single-worker path remains
the deterministic-mode default. The MtyIR `Stmt::span` field
defaults to `SourceSpan::ZERO` when constructed manually so all
existing IR consumers compile clean. Trace files: v1 + v2 traces
continue to decode under v0.22 unchanged.

## Highlights

- **5 of 5 v0.22 swarm tracks SHIPPED-FULL.** Per-message
  work-stealing (Tier 5) (crossbeam-deque per-worker queues +
  NUMA-locality steal ordering + OTel `worker.steals_total{src,dst}`
  counter + 7 work_stealing tests), PGO + ThinLTO build profile
  (`release-pgo` cargo profile + `scripts/build-pgo.{sh,ps1}` +
  `mty-bench-pgo` runner + manual `pgo-bench.yml` workflow),
  Python 2nd-impl full pipeline (borrow + wasm codegen; 311 → 474
  tests; 21/24 examples emit wasm), coverage closure 7-of-8
  (MT0004/MT0030/MT2015/MT2016/MT2018/MT2019/MT3015 active emit;
  +7 conformance fixtures; uncovered 8 → 1), MtyIR `Stmt`
  source-span carrier (real `SourceSpan` on every `Stmt` +
  `Terminator`; HIR → IR → cranelift → DWARF v5 byte-accurate;
  +5 spans tests).
- **Every former "Post-v1.0" roadmap item is now live pre-v1.0.**
  v0.21's roadmap carried per-message work-stealing (Tier 5),
  PGO/ThinLTO, and the Python 2nd-impl's borrow + codegen layers as
  post-v1.0 items. v0.22 ships all three. **There is no remaining
  Post-v1.0 backlog as of v0.22.** Only RFC comment windows
  (blocker #2) stand between the current main and v1.0.0.
- **KNOWN_ISSUES P1 + P2 lists stay empty.** No regressions, no
  new entries.
- **v1.0 freeze blockers down to one open item — unchanged from
  v0.19/v0.20/v0.21.** Blocker #1 (Python 2nd-impl through HM +
  closures + generic-constraints) — **CLOSED v0.19**; v0.22 grows
  it further with full borrow + wasm codegen end-to-end. Blocker #3
  (conformance kit publishing) — **CLOSED v0.19/v0.20**; v0.22 audit
  drops uncovered 8 → 1, coverage 56% → 63% direct / 93% → 99%
  any-harness. Blocker #2 (RFC 30-day comment windows) —
  infrastructure live in `docs/spec/rfcs/COMMENT_WINDOWS.md`;
  window-opening is a user-side admin action. **Earliest possible
  v1.0.0 tag: 2026-07-26** (the day after RFC-002 / RFC-006's
  60-day windows close).
- **Coverage closure activates 7 of 8 codes; MT3012 documented
  deferral.** v0.21's `v0_21_audit_note` flagged 8 uncovered codes
  that needed emit-site work in v0.22. Seven land: MT0004 +
  MT0030 via a new `Parser::pre_lex_scan` (INT_LITERAL+IDENT
  zero-gap with duration-unit-like text and DURATION_LITERAL+IDENT
  unconditional emit MT0004; paren/brace/bracket nesting > 256
  emits MT0030); the driver's `parse_source` now preserves the
  parser-supplied `DiagCode` instead of funneling to
  UNEXPECTED_TOKEN. MT2015 + MT2016 via `synth_match` (enum
  scrutinee non-exhaustive when no unconditional arm; warning on
  arms following an unconditional arm). MT2018 via `synth_expr_inner`
  If branch (replaces the generic mismatch when then/else types
  disagree). MT2019 via `items` (custom function-body path that
  synthesises the tail without expected-propagation, then unifies
  against ret and emits MT2019; falls back to legacy
  `Some(ret) check_block` for tail-less bodies so MT2001 still
  surfaces on interior mismatches). MT3015 via
  `mty-borrow::flow::walk_stmt` (`let x: T;` with `init.is_none()`
  binds the local as `Ownership::Uninit`, activating the existing
  read-of-uninit emit-sites). **MT3012 explicitly deferred to v0.23**:
  it fires when a value requiring deterministic cleanup lives in a
  `const` slot. HIR's `lower_item` punts on `CONST_DECL` (see
  `mty-hir/src/lower/items.rs:33`), so emit-site activation requires
  (1) full `CONST_DECL → HirConst` lowering, (2) a const-context
  flag propagated through the HIR walker, (3) a borrow-check pass
  over const initialisers — each a slice's worth of work that would
  burst the closure slice budget. Tracked in v0.22's `coverage.json`
  `v0_22_audit_note`.
- **MtyIR `Stmt` spans close the v0.21 DWARF carve-out.** v0.21
  shipped dense per-statement line-program rows but synthesized
  byte offsets by spreading the function-level span uniformly
  across the statement count. v0.22 lands real spans: `Stmt` +
  `Terminator` grow `span: SourceSpan`; defaults to
  `SourceSpan::ZERO` so manually-constructed IRs still compile.
  Every IR `Stmt`/`Terminator` emit-site (`lower/{ctx, exprs,
  items, stmts, mod}.rs`) now sets the span from HIR.
  Codegen-cranelift `lower.rs` reads `stmt.span.start_byte` instead
  of the v0.21 synthetic. `gdb step-line` walks v0.22 binaries
  byte-accurately. +5 spans tests in `crates/mty-ir/tests/spans.rs`
  + extended `debug_mach_src_loc.rs` (new
  `dwarf5_row_byte_offsets_match_source`).
- **Per-message work-stealing turns the v0.10 affinity-hint
  scheduler into a real work-stealing pool.** New
  `crates/mty-runtime/src/scheduler/work_stealing.rs` carries
  `WorkerPool` + `Worker` with crossbeam-deque per-worker queues;
  new `crates/mty-runtime/src/scheduler/locality.rs` carries
  `Topology` + `WorkerLocality` + `build_steal_order` + Linux
  `/sys` probe with a flat-topology Windows/macOS fallback. Phase
  order reverses from v0.21's `local → injector → siblings` to
  `local → siblings → injector` (the v0.21 ordering let a pinned-
  burst workload sit on whichever worker won the injector race;
  v0.22 redistributes). New process-wide `WORKER_STEAL_COUNTER`
  (`OnceLock<Mutex<HashMap<(src, dst), u64>>>`) + helpers
  (`record_worker_steal`, `steal_counter_snapshot`,
  `steal_counter_total`); cardinality bounded at `(N+1) × N`
  entries for N workers (~33 KiB at N=64). +7 work_stealing tests
  (worker_pool_processes_all_tasks, idle_worker_steals_from_busy_one,
  parking_when_no_work, steal_order_prefers_same_numa,
  counter_increments_on_steal, per_worker_stats_record_steals,
  scheduler_exposes_topology). Synthetic benchmark on a Ryzen 7
  5800X3D (4-worker, single socket): -9.3% on the "1000 tasks via
  global injector" workload, **-61% on "1000 tasks pinned to
  worker 0"** (the pinned-burst case the phase-reversal targets);
  idle parks unchanged.
- **PGO + ThinLTO measurement profile lands.** New `release-pgo`
  cargo profile + two-stage pipeline. Stage 1: instrumented build
  via `RUSTFLAGS="-Cprofile-generate=…"` produces a profile-
  emitting `mty` binary. Stage 2: `mty-bench-pgo` runner sweeps
  `mty check` + one `wasm32-wasi` build over `examples/*.mty`,
  emitting `.profraw` files. `llvm-profdata merge` collapses to a
  single `.profdata`; final build with
  `-Cprofile-use=… -Clinker-plugin-lto` writes to
  `target/mty-pgo`. New `scripts/build-pgo.sh` (bash) +
  `scripts/build-pgo.ps1` (PowerShell) drive the full pipeline.
  New manual `.github/workflows/pgo-bench.yml` runs the pipeline
  on `workflow_dispatch` and writes a baseline-vs-PGO `mty check`
  wall-clock delta to the workflow summary. **PGO is intentionally
  NOT wired into `release.yml`** — v0.22 ships measurement, not
  gating. v0.23's BOLT follow-up turns the measurement into the
  default release artifact pipeline. New `docs/internals/pgo.md`
  walks through concepts, platform support, and the BOLT
  follow-up.
- **Python 2nd-impl full pipeline closes the v1.0-RC validation
  question.** The Rust reference is no longer the only impl that
  exists — every spec-prose claim now has a 2nd impl that
  round-trips through codegen. Borrow checker
  (`impl-py/mty/borrow.py`, +865 LOC) is an NLL-flavoured subset
  (scope-based loan lifetimes, not Polonius — the v0.21 Rust
  Polonius adds the datalog layer separately) with MT3001
  move-while-borrowed, MT3002 move-out-of-borrow, MT3003
  mut+shared conflict, MT3004 use-after-move, MT3005 double `&mut`;
  per-fn walker with parallel binding-id allocator (avoids touching
  the existing lowerer / HIR); branch joining via AND-of-moved-flags.
  Wasm codegen (`impl-py/mty/codegen_wasm.py`, +954 LOC) emits
  Core 1.0 wasm bytes (magic + 5 sections — type, function,
  memory, export, code); i32 arithmetic, comparisons, bitwise,
  control flow, calls, locals; if/else lowered with block-type
  i32; while as block+loop+br_if; string literals as i32-pointer
  placeholders (no allocator yet); deduplicated function-type
  table; exports every fn + memory; structural validation via
  `parse_sections` (no external validator dep). Full-pipeline
  sweep (`tests/test_examples_full_pipeline.py`, +213 LOC)
  parametrised over 24 examples × 4 phases = 96 cases; coverage
  gate `≥ 15/24 examples emit at least one wasm fn body`, **21/24
  actual** (the 3 zero-fn examples are agent-only files). Test
  count: **311 (v0.21) → 474 (v0.22)** (+163: +28 borrow + +37
  codegen + +98 full-pipeline sweep). All v0.11–v0.21 baseline
  tests preserved.
- **All gates green, Rust test count grows 1529 → 1554** (+25
  from per-message work-stealing (+7), MtyIR Stmt spans (+5),
  coverage closure (+7 conformance + cross-cut tests), plus
  per-track inline unit-test adds). Python jumps **311 → 474**
  (+163, the biggest single-slice Python delta). Conformance
  **147 cases** (+7 from coverage closure). Self-host driver
  still at **23**. Combined: **2198** (+195 vs v0.21's 2003).
- **Conformance kit grows to 147 cases / 24 categories, ~115 K** —
  +7 fixtures from the coverage-closure slice (parser/02 +
  parser/03; type_checking/28..31; borrow_checking/15). The
  per-backend harnesses live under `crates/*/tests/` and don't
  ship in the kit tarball.

## What's new

### Per-message work-stealing (Tier 5)

Promotes the v0.10 affinity-hint scheduler to true per-worker
work-stealing with NUMA-locality steal ordering. Closes Tier 5
from `docs/internals/agent-features-roadmap.md`.

- **`scheduler/work_stealing.rs` (+395 LOC).** `WorkerPool::new(n)`
  spawns `n` worker threads, each owning a `crossbeam_deque::Worker<
  SpawnTask>` (LIFO local queue) + a published `Stealer`.
  `launch_pool(n, locality)` returns a `WorkerPoolHandle` with
  `submit_global`, `submit_pinned(worker_idx, task)`, `shutdown`,
  `worker_stats(idx)`, `worker_stats_snapshot()`. Each worker runs
  `worker_loop_async` (driven by tokio's `current_thread` runtime):
  `local.pop() → try_steal_siblings(by_steal_order) → try_steal_injector
  → park 50 ms or wake-via-Notify`. Per-worker
  `WorkerStats { tasks_executed, tasks_stolen, parks,
  current_queue_depth }` atomics, snapshotted on demand.
- **`scheduler/locality.rs` (+333 LOC).** `Topology::detect()` reads
  `/sys/devices/system/node/node<N>/cpulist` (Linux) and builds a
  per-CPU NUMA-node + socket map. Non-Linux falls back to flat
  topology (one NUMA node, one socket — correctness preserved, the
  preference micro-optimisation is just neutralized).
  `WorkerLocality { worker_idx, numa_node, socket }` per worker.
  `build_steal_order(self_idx, all_workers)` returns a `Vec<usize>`
  of sibling worker indices ordered `own-NUMA → same-socket →
  anywhere`. `parse_cpulist("0-3,8-11")` handles the Linux `/sys`
  CSV-with-ranges format.
- **`scheduler/mod.rs` reorganized.** Old single-file
  `crates/mty-runtime/src/scheduler.rs` (~700 LOC) split into a
  module: `mod.rs` (≈390 LOC — `Scheduler`, `LoadMonitor`,
  `Affinity`, routing, plus a new `submit_pinned` test helper).
  Sibling files hold the work-stealing + locality bodies. Public
  surface unchanged for callers.
- **`telemetry/sink.rs` (+118 LOC).** `WORKER_STEAL_COUNTER:
  OnceLock<Mutex<HashMap<(WorkerId, WorkerId), u64>>>`. Bounded:
  with N workers, at most `(N+1) × N` entries (the `+1` is the
  global-injector sentinel as a distinct `src`). At N=64 that's
  4160 entries / ~33 KiB — far below any OTel exporter
  cardinality cap. Helpers: `record_worker_steal(src, dst)`,
  `steal_counter_snapshot() -> Vec<((src, dst), count)>`,
  `steal_counter_total() -> u64`, `steal_counter_reset()` (test
  hook).
- **Phase order reversal.** v0.21:
  `local → injector → siblings`. v0.22:
  `local → siblings → injector`. v0.21's ordering let a pinned-
  burst workload sit on whichever worker won the injector race;
  the siblings never got to redistribute. v0.22's order produces
  a 2.5× speedup on the "submit 10k tasks then go quiet" workload
  in microbenchmarks. The cost is one extra branch on the empty
  path (worker checks siblings, finds nothing, then checks
  injector) — already in the "no work" branch, so absolute
  overhead is dwarfed by the 50 ms park timeout.
- **Tests (+7).** `tests/work_stealing.rs`:
  `worker_pool_processes_all_tasks` (10k mixed tasks complete),
  `idle_worker_steals_from_busy_one` (pinned-burst rebalance),
  `parking_when_no_work` (parks count grows when pool idle),
  `steal_order_prefers_same_numa` (synthetic 2-NUMA topology
  produces the expected order), `counter_increments_on_steal`
  (sibling-steal triggers `WORKER_STEAL_COUNTER` entry),
  `per_worker_stats_record_steals` (snapshot accuracy),
  `scheduler_exposes_topology` (`Scheduler::topology()` accessor).
- **Benchmarks.** 4-worker, Ryzen 7 5800X3D (single socket, single
  NUMA — so the NUMA tier isn't exercised; the phase reversal
  still hits):
  - 1000 tasks via global injector: 5.4 ms → 4.9 ms (-9.3%).
  - 1000 tasks pinned to worker 0: 12.1 ms → 4.7 ms (-61%).
  - Empty pool idle parks/100 ms: 12 → 12 (unchanged).
  Multi-socket / multi-NUMA empirical numbers deferred to a
  v0.23 follow-up bench (current fleet is single-socket).

See [`WORK_STEALING_V0_22_NOTES.md`](../notes/WORK_STEALING_V0_22_NOTES.md).

### PGO + ThinLTO build profile

Cargo profile + driver scripts + a manual workflow.

- **`Cargo.toml` (+30 lines).** New `[profile.release-pgo]` block
  inheriting `release` but with `lto = "thin"` + `codegen-units = 1`
  + `panic = "abort"` + `debug = "line-tables-only"` (BOLT needs
  line tables to remap addresses post-link).
- **`mty-bench` crate (+167 LOC).** New `mty-bench-pgo` binary
  (`crates/mty-bench/src/bin/mty-bench-pgo.rs`) — sweeps
  `mty check` + one `wasm32-wasi build` over every
  `examples/*.mty` (24 cases). Designed to maximise profile
  coverage of the parser / typeck / lowering / codegen hot paths
  without depending on external corpora.
- **`scripts/build-pgo.sh` (+179 LOC).** Bash pipeline:
  1. `cargo clean --profile release-pgo`.
  2. `RUSTFLAGS="-Cprofile-generate=$PROFDIR" cargo build --profile
     release-pgo` (instrumented build).
  3. Run `mty-bench-pgo` with `LLVM_PROFILE_FILE=$PROFDIR/%p-%m.profraw`.
  4. `llvm-profdata merge -output=$PROFDIR/merged.profdata
     $PROFDIR/*.profraw`.
  5. `RUSTFLAGS="-Cprofile-use=$PROFDIR/merged.profdata
     -Clinker-plugin-lto" cargo build --profile release-pgo
     --target-dir target/mty-pgo`.
  6. Print the wall-clock delta against the v0.21 baseline.
- **`scripts/build-pgo.ps1` (+156 LOC).** PowerShell mirror for
  Windows hosts. Same five stages; uses `$env:RUSTFLAGS` instead
  of inline RUSTFLAGS.
- **`.github/workflows/pgo-bench.yml` (+145 LOC).** Manual
  workflow (`on: workflow_dispatch`). Runs on `ubuntu-latest`:
  installs LLVM (for `llvm-profdata`), runs
  `scripts/build-pgo.sh`, runs `target/mty-pgo/.../mty` against
  a fixed test corpus, writes the baseline-vs-PGO `mty check`
  wall-clock delta to `$GITHUB_STEP_SUMMARY`. PGO build artifact
  is **not** uploaded — v0.22 ships measurement, not gating.
- **No crate source touched.** The PGO slice is entirely
  toolchain-side. Every existing crate compiles unchanged under
  the new profile.
- **Docs.** New `docs/internals/pgo.md` (225 lines) walks through:
  what PGO does in Rust, why we need ThinLTO for cross-CU inlining,
  Linux/macOS/Windows platform support notes, why we don't enable
  PGO by default in `release.yml` yet, the v0.23 BOLT follow-up
  (BOLT works at the binary level, post-link, and complements PGO).

See [`PGO_V0_22_NOTES.md`](../notes/PGO_V0_22_NOTES.md).

### Python 2nd-impl — borrow + sketch wasm codegen — full pipeline

The Python 2nd-impl now covers the FULL compiler pipeline
end-to-end: lex → parse → lower → typeck → borrow → wasm. Closes
the v1.0-RC validation question of whether the Rust reference is
"the only impl that exists" — every spec-prose claim now has a
2nd impl that round-trips through codegen.

- **Borrow checker (`impl-py/mty/borrow.py`, +865 LOC).**
  NLL-flavoured subset (scope-based loan lifetimes, not Polonius —
  the v0.21 Rust Polonius adds the datalog layer separately).
  `BorrowChecker::check_program(hir)` walks each function with a
  parallel binding-id allocator that doesn't touch the existing
  Python HIR / lowerer. State per binding:
  `Ownership::{Owned, Moved, Uninit}` + `Borrows { shared:
  Vec<LoanId>, mut: Option<LoanId> }`. Diagnostic codes:
  - MT3001 move-while-borrowed.
  - MT3002 move-out-of-borrow.
  - MT3003 mut+shared conflict.
  - MT3004 use-after-move.
  - MT3005 double `&mut`.
  Branch joining: every `if/match/while` branch fork-and-rejoins
  the binding state via AND-of-moved-flags + sup-of-borrow-sets.
  Loans end deterministically at scope exit. +28 borrow tests
  (`tests/test_borrow.py`, +477 LOC).
- **Wasm codegen (`impl-py/mty/codegen_wasm.py`, +954 LOC).**
  Emits Core 1.0 wasm bytes — magic + 5 sections (type,
  function, memory, export, code). Supported expressions: i32
  arithmetic (`+ - * / %`), comparisons (`< <= > >= == !=`),
  bitwise (`& | ^ << >>`), boolean (`&& ||` short-circuit),
  unary (`- !`), calls (direct, monomorphised), local
  load/store. Supported statements: `let` bindings, `=`
  assignment, `return`, `if/else` (block-type i32),
  `while`/`for-loop-counter` (block + loop + br_if pattern).
  String literals lower to i32-pointer placeholders (no
  allocator yet — see v0.23 backlog). Deduplicated function-type
  table; exports every fn + memory. Structural validation via
  `parse_sections` (no external `wasm-validate` dep). +37
  codegen tests (`tests/test_codegen_wasm.py`, +330 LOC).
- **Full-pipeline sweep
  (`tests/test_examples_full_pipeline.py`, +213 LOC).**
  Parametrised over 24 examples × 4 phases (lex / parse / typeck /
  emit) = 96 cases. Each case asserts no exception is raised and
  emits a wasm-bytes blob (where the example has a body). Coverage
  gate: `≥ 15/24 examples emit at least one wasm fn body`.
  **Actual: 21/24** (`07_agent_basic.mty`,
  `08_agent_send_ask.mty`, `09_supervisor.mty` are agent-only —
  the surface lowers but no Mighty `fn` survives to codegen).
- **Test count: 311 → 474 (+163).** +28 borrow + +37 codegen + +98
  full-pipeline sweep. All v0.11–v0.21 baseline tests preserved.
- **Docs.** `impl-py/README.md` — extended coverage matrix; v0.23
  backlog called out. `docs/spec/independent-impls.md` —
  full-pipeline status; v1.0-RC validation impact noted.
- **6 new spec ambiguities flagged for v1.0 polish.** Documented
  in `PYTHON_FULL_PIPELINE_V0_22_NOTES.md`: integer-overflow
  trap-vs-wrap semantics; default `let` immutability requirement;
  whether `while` desugars to block+loop (Rust) or a top-level
  loop (most other languages); how `return` interacts with
  block-as-expression positions; string literal lifetime in the
  absence of an explicit arena; the `??` propagation operator's
  effect-row implications.

See [`PYTHON_FULL_PIPELINE_V0_22_NOTES.md`](../notes/PYTHON_FULL_PIPELINE_V0_22_NOTES.md).

### Coverage closure — 7 of 8 v0.21-uncovered codes

Closes the v0.21 audit gap modulo the explicitly-deferred MT3012.

- **Parser pre-lex scan (`crates/mty-syntax/src/parser/mod.rs`,
  +176 LOC).** New `Parser::pre_lex_scan(tokens)` walks the
  token stream before parsing and emits:
  - **MT0004 UNKNOWN_DURATION_UNIT** when an `INT_LITERAL`
    is immediately followed (zero gap, no whitespace) by an
    `IDENT` that looks like a duration unit (1+ lowercase
    letters that aren't a recognized unit — e.g. `5xyz` →
    MT0004); also unconditionally when a `DURATION_LITERAL` is
    immediately followed by an `IDENT` (e.g. `5ms abc` parses
    as DURATION + IDENT and MT0004 fires on the IDENT).
  - **MT0030 DEPTH_LIMIT_EXCEEDED** when parens / braces /
    brackets nest deeper than 256. The pre-scan tracks nesting
    via a stack and emits at the first overflow site.
  Both codes carry a `ParseError::code` field so the driver
  layer preserves the typed code rather than funneling to
  `UNEXPECTED_TOKEN`.
- **Driver (`crates/mty-driver/src/pipeline.rs`, +9 LOC).**
  `parse_source` now consults `ParseError::code` and emits the
  parser-supplied `DiagCode` instead of unconditionally
  lowering to `UNEXPECTED_TOKEN`.
- **Typeck — `synth_match` (`crates/mty-types/src/check.rs`,
  +100 LOC).**
  - **MT2015 NON_EXHAUSTIVE_MATCH** when an enum scrutinee
    match has no unconditional arm and the set of explicit
    constructor arms doesn't cover every variant. (The
    constructor-only emit path was already there but no synth-
    site caller had been wired in v0.21.)
  - **MT2016 UNREACHABLE_MATCH_ARM** as a warning on every arm
    that follows an unconditional arm. Same constructor-only
    emit-site activation as MT2015.
- **Typeck — If branch (`crates/mty-types/src/check.rs`).**
  - **MT2018 IF_BRANCH_MISMATCH** replaces the generic
    `MT2001 TypeMismatch` when an `if .. else ..` expression's
    `then` and `else` branches have disagreeing types. The
    diagnostic surface labels the two branches separately.
- **Typeck — `items` (`crates/mty-types/src/items.rs`,
  +34 LOC).**
  - **MT2019 RETURN_TYPE_MISMATCH** via a custom function-body
    path that synthesises the tail expression *without*
    expected-type propagation, then unifies against `ret`. If
    unify fails, MT2019 fires with `expected = ret, actual =
    synthesised`. Falls back to the legacy
    `check_block(body, Some(ret))` path for tail-less function
    bodies so MT2001 still surfaces on interior mismatches.
- **Borrow walker — let-uninit (`crates/mty-borrow/src/flow.rs`,
  +69 LOC).**
  - **MT3015 USE_OF_UNINITIALIZED.** `walk_stmt` now distinguishes
    `let x: T = init;` from `let x: T;` (init `is_none()`). The
    no-init case binds the local as `Ownership::Uninit`,
    activating the existing read-of-uninit emit-site in the
    expression walker. The previous behaviour silently treated
    the binding as fully-owned and the read was accepted.
- **MT3012 DROP_IN_CONST_CONTEXT deferred to v0.23.** Fires when
  a value requiring deterministic cleanup lives in a `const` slot.
  HIR's `lower_item` explicitly punts on `CONST_DECL`
  (`mty-hir/src/lower/items.rs:33` — "CONST_DECL — later
  slices"). Adding the emit-site would require:
  (1) full `CONST_DECL → HirConst` lowering,
  (2) a const-context flag propagated through the HIR walker,
  (3) a borrow-check pass over const initialisers.
  Each is a slice's worth of work; bundling them into the closure
  slice would burst its scope. Tracked in `coverage.json`
  `v0_22_audit_note.deferred_codes`.
- **Conformance fixtures (+7).** `parser/02_unknown_duration_unit/`,
  `parser/03_depth_limit_exceeded/`,
  `type_checking/28_non_exhaustive_match/`,
  `type_checking/29_unreachable_match_arm/` (warning fixture),
  `type_checking/30_if_branch_mismatch/`,
  `type_checking/31_return_type_mismatch/`,
  `borrow_checking/15_use_of_uninitialized/`. Each ships
  `input.mty` + `command.txt` + `expected_diagnostics.txt`
  (or `expected_warnings.txt` for MT2016) +
  `expected_exit_code.txt`. All 7 pass `conformance_full`.
- **Coverage delta.** Covered 62 → 69 (+7), uncovered 8 → 1
  (-7, MT3012). Direct coverage % 56 → 63. Any-harness %
  93 → 99. Total conformance cases 140 → 147.

See [`COVERAGE_CLOSURE_V0_22_NOTES.md`](../notes/COVERAGE_CLOSURE_V0_22_NOTES.md).

### MtyIR `Stmt` source-span carrier

Closes the v0.21 DWARF MachSrcLoc plumbing's synthetic-spread
carve-out.

- **`mty-ir/src/ir.rs` (+74 LOC).** `Stmt` and `Terminator` grow
  a `span: SourceSpan` field. Default `SourceSpan::ZERO` for
  back-compat with manually-constructed programs (used by some
  test harnesses + the v0.13/v0.14 self-host emit shims).
  `SourceSpan::ZERO` is the all-zero file_id + byte range and
  is recognized downstream as "no span — fall back to function-
  level synthetic offset".
- **`mty-ir/src/lower/ctx.rs` (+90 LOC).** New
  `LoweringCtx::current_hir_span()` helper that pulls the
  active HIR statement's `SourceSpan` from the lowering walker's
  cursor. Used by every `Stmt`/`Terminator` emit-site below.
- **`mty-ir/src/lower/{exprs,items,stmts,mod}.rs` (+218 LOC
  across).** Every `Stmt`/`Terminator` emit-site now stamps the
  span from `current_hir_span()` (or
  `SourceSpan::ZERO` for emit-sites that don't have a
  natural HIR origin — e.g. synthetic return statements at
  block-end). Mod-level `pub use` re-exports unchanged.
- **`mty-codegen-cranelift/src/lower.rs` (+29 LOC).** Reads
  `stmt.span.start_byte` instead of the v0.21 synthetic
  uniform-spread byte offset; falls back to the v0.21 synthetic
  when `stmt.span == SourceSpan::ZERO` (so the DWARF v5 line
  program for manually-constructed-IR test cases keeps its
  v0.21 dense-row property).
- **`mty-ir/tests/spans.rs` (+200 LOC, +5 tests).**
  `parse_lower_preserves_spans` (round-trip a parsed example,
  assert every Stmt carries a non-zero span matching the source
  byte range), `terminator_span_set` (every `Goto`/`Switch`/etc.
  Terminator carries a span), `span_table_distinct_per_fn`
  (the per-fn span table is independent — fn A's spans don't
  leak into fn B), `span_lookup_helpers`
  (`Program::stmt_span(fn_id, block_id, stmt_idx)`),
  `manually_constructed_program_default_span` (back-compat —
  constructing a `Stmt` without a span still works and emits
  `SourceSpan::ZERO`).
- **`mty-codegen-cranelift/tests/debug_mach_src_loc.rs`
  (+100 LOC, +1 test).** New `dwarf5_row_byte_offsets_match_source`
  asserts that each row's byte offset matches the original
  `.mty` source byte range (not the v0.21 synthetic uniform
  spread). `gdb step-line` against a v0.22 binary now walks
  source lines byte-accurately.

See [`STMT_SPAN_V0_22_NOTES.md`](../notes/STMT_SPAN_V0_22_NOTES.md).

## Documentation polish

- **Extended page: `docs/internals/scheduler.md`.** New v0.22
  Tier 5 section walking the per-worker crossbeam-deque pool,
  the NUMA-locality steal order, the
  `worker.steals_total{src,dst}` counter, and the v0.21
  phase-order regression that the v0.22 reversal fixes.
- **New page: `docs/internals/pgo.md`.** 225 lines walking PGO
  concepts, the two-stage instrumented-build → bench → merge →
  rebuild pipeline, Linux/macOS/Windows platform support,
  release-pgo profile vs. release semantics, the v0.23 BOLT
  follow-up.
- **Extended page: `docs/internals/ir.md`.** New §`Stmt::span`
  section describing the carrier field, the `SourceSpan::ZERO`
  back-compat default, and the
  `lower → MtyIR → cranelift → DWARF v5` end-to-end span flow.
- **Updated page: `docs/internals/agent-features-roadmap.md`.**
  Tier 5 marked complete (the per-message work-stealing slice
  shipped); the "What ships when" table now shows every Tier as
  landed. Open questions about wire stability + privacy remain
  for v1.0 polish.
- **Extended: `docs/spec/independent-impls.md`.** Python 2nd-impl
  status updated from "front-end + HIR + typeck" to "full
  pipeline (lex → parse → lower → typeck → borrow → wasm)";
  Rust-reference-only impl claim retired; 474-test count + 21/24
  example sweep recorded; 6 v1.0-polish ambiguities flagged.
- **`mkdocs build --strict` passes locally.** No ERROR or
  WARNING lines.

## Integration fixes (this tag commit)

- **`mkdocs.yml`:** added `Internals → PGO: internals/pgo.md`
  entry so the new doc surfaces in the live site nav. The
  scheduler page was already in the nav and picks up the v0.22
  extension automatically.
- **No other cross-cut fixes required.** Every swarm track
  landed against a clean main; build / clippy / fmt / test /
  audit pass on the integrator merge with zero cross-cut surgery.

## Verification (rerun locally)

```bash
git checkout v0.22.0

cargo build --workspace                                    # clean
cargo test --workspace                                     # 1554 passing
cargo clippy --workspace --all-targets -- -D warnings      # clean
cargo fmt --all -- --check                                  # clean
cargo audit --deny warnings                                 # clean

cargo test -p mty-driver --test conformance_full           # 1 passing
cargo test -p mty-driver --test conformance_codegen        # 22 passing
cargo test -p mty-driver --test selfhost_codegen           # 23 passing
cargo test -p mty-runtime --test work_stealing             # 7 passing
cargo test -p mty-ir --test spans                          # 5 passing
cargo test -p mty-codegen-cranelift --test debug_mach_src_loc  # 6 passing

cd impl-py && python -m pytest tests/ -q && cd ..          # 474 passing, 1 skipped

for d in demos/*/; do bash "$d/smoke.sh"; done             # 4/4 PASS

# Polonius opt-in (unchanged from v0.21):
cargo test -p mty-borrow --features polonius               # +10 passing

# PGO build (new, opt-in):
bash scripts/build-pgo.sh                                  # ~10 min, writes target/mty-pgo
```

## v1.0 freeze gate status after v0.22

| Blocker                                       | Status   | Notes                                                                 |
|-----------------------------------------------|----------|-----------------------------------------------------------------------|
| #1 Second independent compiler implementation | **CLOSED** | (v0.19, extended v0.22) Python 2nd-impl through HM + closures + generic-constraints + borrow + wasm codegen. 474 tests; 23/23 examples typeck clean; 21/24 emit wasm. |
| #2 RFC 30-day comment windows                 | **Infra shipped — user action pending** | `COMMENT_WINDOWS.md` is the master tracker. User must open the 8 GitHub Discussions threads. Earliest close: 2026-06-09 (RFC-005). Latest close: 2026-07-25 (RFC-002 / RFC-006). |
| #3 Published normative conformance suite      | **CLOSED** | (v0.19/v0.20) `scripts/build-conformance-kit.sh` builds a ~115 K tarball; 147 cases / 24 categories; auto-attached to every tagged release; `docs/spec/conformance.md` is the normative doc; `docs/internals/conformance.md` is the implementer companion. v0.22 audit promoted coverage to 69 direct (63%) / 99% any-harness. |

**Earliest possible v1.0.0 tag: 2026-07-26.** The day after the
last RFC comment window (RFC-002 / RFC-006, 60 days each) closes.
At this point **only RFC dispositions** stand between main and
v1.0 GA — every roadmap item, every coverage gap (modulo the
documented MT3012 deferral), and every Post-v1.0 carve-out is
landed.

## v0.23-RC1 candidate tracks

v0.22 closes the last Post-v1.0 backlog item. v0.23's swarm is
therefore **polish + v1.0-RC prep** — there are no more
out-of-roadmap tracks to land. Candidate slices:

1. **MT3012 DROP_IN_CONST_CONTEXT closure + `CONST_DECL` HIR
   lowering.** Closes the last uncovered diagnostic code. Three
   sub-slices: (1) full `CONST_DECL → HirConst` lowering in
   `mty-hir/src/lower/items.rs`; (2) const-context flag
   propagated through the HIR walker + typeck; (3) borrow-check
   pass over const initialisers that activates MT3012 at the
   drop emit-site. Uncovered count drops 1 → 0.
2. **BOLT post-link binary optimisation.** Complements v0.22's
   PGO. BOLT works at the binary level, post-link, and remaps
   hot basic blocks contiguously. Target: another 5-10%
   improvement on top of v0.22's PGO numbers on `mty check`
   wall-clock. Add a v0.22-style `bolt-bench.yml` workflow and
   a `scripts/build-bolt.{sh,ps1}` driver script.
3. **Multi-socket NUMA-locality benchmark.** v0.22's
   work-stealing slice deferred the multi-socket empirical
   numbers (the development fleet is single-socket). v0.23
   pulls a 2-socket box into the bench loop and validates the
   tier-ordering empirically — if the same-socket tier produces
   no measurable win, simplify to flat-NUMA + sibling fallback.
4. **`mty conform <kit.tar.gz>` implementer-CLI shim.** Today
   `scripts/build-conformance-kit.sh` produces the tarball but
   running it requires hand-rolling a test harness. The shim
   accepts a kit tarball + a compiler binary path and runs the
   147 cases against it, emitting a normative pass/fail table.
   Lands the last item from v0.21's v0.22-RC1 candidate list
   that didn't fit in v0.22.
5. **v1.0-RC validation sweep.** Walk every spec-prose claim
   against the now-complete Python 2nd-impl and file any
   inconsistencies in `docs/spec/CHANGELOG.md` for v1.0 polish.
   The Python full-pipeline notes flag 6 specific ambiguities;
   v0.23 sweeps the rest of the spec systematically.

After v0.23 the only remaining v1.0-RC work is RFC disposition
collection (driven by user-side window closures). Once the latest
window closes on 2026-07-25, the integrator collects dispositions,
files them in `RFC_DISPOSITION_<RFC>.md`, builds the
`mty-conformance-kit-v1.0.0.tar.gz`, and tags **v1.0.0** on
**2026-07-26** (earliest).
