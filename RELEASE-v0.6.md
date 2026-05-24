# Stardust v0.6 — Release Notes

**Tag:** `v0.6.0`
**Date:** 2026-05-24
**Status:** SHIPPED — sixth milestone release. The runtime
distributes work across N OS threads with per-worker tokio runtimes
+ crossbeam-deque work-stealing, the first honest cross-language
benchmarks land alongside a new `sdust-bench` crate, the Stardust
source parser is itself ported to Stardust (~1930 LOC running through
a bootstrap host bridge), and three v0.5 loose ends close inline:
DOM SIR lowering reaches `emit_dom_call` end-to-end, the
SD6001-SD6006 macro codes merge into the central
`sdust_diagnostics` catalog, and per-call `FsCap` isolation gains a
contract test.

Stardust v0.1 walked the spec §31 ladder end-to-end. v0.2 lit up
every surface the v0.1 deferral list named. v0.3 hardened
soundness. v0.4 was dogfood + ecosystem. v0.5 was self-hosting
(lexer) + dogfood completion + LSP advanced. v0.6 is the
**multi-core + benchmarks + self-host-parser** milestone: the runtime
finally uses every core, perf has numbers, and the compiler parses
its own source.

## What you can do (new in v0.6)

```bash
# Multi-core scheduler — defaults to one worker per core
STARDUST_RUNTIME_THREADS=8 sdust run examples/07_agent_echo.sd
# (or just `sdust run ...` — N defaults to available_parallelism())

# Per-worker scheduler stats
cargo test -p sdust-runtime --test load_balance
# → 5 passed (worker_steal / cross_worker_send / affinity_sticky /
#   load_balance / deterministic_mode)

# First honest cross-language benchmarks
cargo bench -p sdust-bench --bench parse_throughput
cargo bench -p sdust-bench --bench agent_send_latency
# (CLI runner with 30 iters: `cargo run -p sdust-bench --bin sdust-bench-runner`)

# The Stardust parser is now itself written in Stardust
cargo test -p sdust-driver --test selfhost_parser
# → 13 passed (examples 01-05 bootstrap through the host bridge)

# DOM SIR lowering now reaches the wasm32-web import table
cargo test -p sdust-sir --test dom_lowering
# → 3 passed (`d.set_text("#id", "x")` lowers to BuiltinId::DomOp("set_text"))

# Central SD catalog: `sdust explain` resolves SD6001-SD6006 the
# same way it resolves every other code
sdust explain SD6001
# → SD6001: Unknown macro. ...
sdust explain SD6006
# → SD6006: Procedural macro execution is not supported yet. ...

# Per-call FsCap isolation contract is pinned
cargo test -p sdust-stdlib --test fs_capability_allowlist
# → 5 passed (including the new two_disjoint_caps_isolate_in_the_same_process)
```

Everything from v0.5 still works the same way. `RuntimeBuilder`'s
new default of `available_parallelism()` workers is overridable via
`STARDUST_RUNTIME_THREADS=N`; deterministic mode
(`.deterministic(seed)`) still pins a single worker for
reproducibility (A106). The `.threads(n)` API from slice-7 remains
as an alias of `.workers(n)`.

## The three swarm agents + integrator pass

v0.6 was built by 3 autonomous swarm agents working disjoint crate
boundaries plus an integrator pass that picked up three loose ends
the killed "loose ends" swarm agent left on the table:

| Agent | Crates / files | Commits |
|---|---|---|
| multi-core scheduler | `sdust-runtime` (scheduler, runtime, lib, Cargo), 7 new tests, conformance `mailbox_ordering/06+07`, `docs/internals/{scheduler,multi-core}.md`, spec A101..A106 | `ee5d83b`, `f071f12` |
| benchmarks | new `sdust-bench` workspace member, 6 categories × {Rust / Go / C++ comparators}, criterion harness + CLI runner, `docs/benchmarks/*.md`, CI workflow | `a678e41`, `3f6fb89`, `b303cee` |
| self-host parser | `selfhost/parser/parser.sd` (~1930 LOC), `crates/sdust-driver/tests/selfhost_parser.rs` (host bridge), `docs/internals/self-hosting.md`, `SELFHOST_PARSER_V0_6_NOTES.md` | `a9c89c8`, `1b41b22` |
| integrator easy wins | `sdust-diagnostics::codes` (SD6001-SD6006), `sdust-macros::diag` (re-exports), `sdust-sir::BuiltinId::DomOp` + lowering, `sdust-codegen-wasm::emit_call` (DomOp dispatch + dead-code removal), `sdust-codegen-cranelift::lower_call` (DomOp stub), 3 new sir tests + 1 new wasm-emit test + 1 new fs cap isolation test | `03c74fd`, `697cd79`, `76e962c` |

A fourth "v0.5 loose ends" agent was killed before committing
anything; the integrator picked up the three items that fit inside
a 10-file-edit budget (DOM SIR lowering, central SD catalog,
per-call FsCap test). The other v0.5 loose ends roll into v0.7
(proc-macro sandboxed execution, real per-agent HTTP routing,
set-of-scopes hygiene, LSP workspace resolve map).

## Headline numbers

- **885 tests pass** (0 failures, 2 ignored — network / pending) —
  was 839 in v0.5
- **+46 tests** added in v0.6 (23 scheduler, 8 bench, 13 self-host
  parser, 3 dom_lowering, 1 dom_imports wasm-emit smoke, 1 fs cap
  isolation)
- **0 clippy warnings** with `-D warnings`
- **`cargo fmt --check` clean**
- **21 crates** in the workspace (+1 from v0.5: `sdust-bench`)
- **11 commits** since `v0.4.0` → **8 commits** since `v0.5.0` (+1
  prep commit `ef031d2` before tag)
- **9,000 insertions / 183 deletions** across 99 files
- **20/20 examples compile to native objects** (unchanged from v0.5)
- **20/20 examples compile to wasm32-web Components** (unchanged from
  v0.5)
- **All conformance categories pass**, including the 2 new
  `mailbox_ordering/06_multicore_fifo` + `07_multicore_throughput_smoke`
  cases from the scheduler swarm (v0.5 `control_flow/01..05` cases
  remain green)
- **3/3 dogfood demo smoke scripts pass** (search_api,
  counter_web @ 1494 bytes, extract_tool)
- **Self-host lexer: 4/4 pass** (unchanged from v0.5 full byte-for-byte
  diff)
- **Self-host parser: 13/13 pass** (NEW; covers examples 01-05)
- **9 new spec amendments** (A101..A109)
- **0 new SD codes** this slice (SD6001-SD6006 relocated, not new)
- **MSRV unchanged**

## Correctness assertions newly enforced

| Property | v0.5 | v0.6 |
|---|---|---|
| Default worker count | 1 (single tokio runtime) | `available_parallelism()` per-worker tokio + crossbeam-deque work-stealing (A106) |
| Driver runtime vs worker runtimes | shared (deadlock-prone) | separated (A105) |
| Agent affinity | n/a | `AffinityHint::Sticky` + `Sticky(worker_id)` (A102) |
| Lightweight migration | n/a | routing-table retargeting on next spawn (A103) |
| Per-worker telemetry | n/a | `Scheduler::stats()` -> `WorkerStats { queue_depth, executed, stolen, parks }` (A104) |
| Per-token-stream perf number | absent | parse / send / mailbox / HTTP / native-compile / wasm-size shipped via `sdust-bench` |
| Stardust parser written in Stardust | only lexer | parser at ~1930 LOC, 13/13 bootstrap tests pass |
| `d.set_text(...)` on a `Dom` cap | typeck only — never reaches `emit_dom_call` | full SIR lowering + wasm `stardust:web/dom` call (A108) |
| `sdust explain SD6001` | resolved via `sdust_macros::diag` (separate catalog) | single-sourced on `sdust_diagnostics::codes` (A107) |
| `FsCap` isolation between two caps in one process | implicit | contract pinned by test (A109) |

## Closed deferrals from v0.5

The v0.5 deferral list named 47 carry-over items. v0.6 closes:

**Scheduler / runtime**:
- **Multi-core scheduler** — shipped (per-worker tokio runtimes +
  crossbeam-deque work-stealing + affinity hints + lightweight
  migration + per-worker stats)

**Benchmarks**:
- **First honest perf data** — shipped (`sdust-bench` crate +
  6 categories with Rust/Go/C++ comparators + criterion harness)

**Self-host**:
- **Self-host parser subset** — shipped (~1930 LOC, 13/13 bootstrap
  tests, examples 01-05 covered)

**Integrator easy wins** (closed inline):
- **DOM SIR lowering** (A108) — closes v0.5 deferral #6
  (`emit_dom_call` `#[allow(dead_code)]`)
- **Central SD6001-SD6006 catalog** (A107) — closes v0.5 deferral #8
- **Per-call FsCap isolation contract** (A109) — closes v0.5
  deferral #7 modulo per-call materialisation from sandbox manifest
  (the API shape was already correct; the test is the contract)

**Carried forward to v0.7** (too invasive for v0.6 integration
scope):
- Proc-macro sandboxed execution (SD6006-gated)
- Real per-agent HTTP routing via `install_agent_dispatch` wiring
- Set-of-scopes macro hygiene (replaces v0.5 mangling pass)
- LSP workspace resolve map for cross-file rename / go-to-def
- Canonical-ABI return-area bridge so `get-text` / `query` return
  `string` / `option<string>` instead of `u32` handles
- Per-call FsCap materialisation from sandbox manifest at the SIR
  lower (A109 ships the contract; this lifts the cap into each
  call site)
- Labelled `break 'outer` / iterator trait surface
- `format!`-style variadic macro args
- Receiver-chain LSP completion + method-call receiver typing
- Self-host HIR / typeck (the next ladder rung after the parser)
- Lossless live migration (today: routing-table retargeting)
- Per-message work-stealing (today: per-agent activation stealing)

## Spec amendments (9 new)

```
A101 — v0.6 multi-worker scheduler (scheduler swarm)
A102 — Agent affinity hints (scheduler swarm)
A103 — Lightweight migration via routing-table retargeting (scheduler swarm)
A104 — Per-worker scheduler telemetry (scheduler swarm)
A105 — Scheduler driver runtime separation (scheduler swarm)
A106 — Default worker count = available_parallelism (scheduler swarm)
A107 — Central diagnostic catalog for SD6001-SD6006 (integrator)
A108 — BuiltinId::DomOp(name) SIR variant (integrator)
A109 — Per-call FsCap isolation contract (integrator)
```

All committed to `docs/spec/v0.1-amendments.md`.

## Diagnostic codes

No new SD codes in v0.6. The SD6001-SD6006 macro band (introduced
in v0.4 + v0.5) relocates from `sdust_macros::diag` to
`sdust_diagnostics::codes` (A107); `sdust_macros::diag` keeps the
historical bare-`u16` constants but re-exports their values from
the central catalog so existing call-sites in `sdust-hir` compile
unchanged. `sdust explain SDxxxx` is now single-sourced.

## Benchmarks

v0.6 ships honest perf data for six categories. See `BENCHMARKS_V0_6_NOTES.md`
and `docs/benchmarks/*.md` for methodology + per-category numbers + the
v0.7+ optimisation backlog (3-5 items per category).

The comparators ship as code:

| Category | Stardust impl | Comparators |
|---|---|---|
| parse_throughput | `crates/sdust-bench/benches/parse_throughput/stardust.rs` | Rust hand-written lexer + `logos` |
| agent_send_latency | crates/sdust-bench/benches/agent_send_latency/stardust.rs | `tokio::sync::mpsc`, Go `chan`, C++ asio coro |
| mailbox_throughput | crates/sdust-bench/benches/mailbox_throughput/stardust.rs | same as above (1 producer + 1 consumer) |
| http_server_throughput | crates/sdust-bench/benches/http_server_throughput/stardust.rs | `hyper`, Go `net/http`, `cpp-httplib` |
| compile_to_native | crates/sdust-bench/benches/compile_to_native/stardust.rs | `rustc`, `go`, `clang` |
| wasm_size | crates/sdust-bench/benches/wasm_size/stardust.rs | `wasm32-rust`, TinyGo, Emscripten |

The v0.6 build host (Windows 11, Rust 1.95.0, Python 3.11) doesn't
have Go / g++ / TinyGo / Emscripten installed, so the comparator
numbers are marked `(pending — Reference env)` in each category doc.
The Stardust numbers themselves are real (recorded by criterion).

## Toolchain

- **MSRV unchanged**
- **One new workspace crate** (`sdust-bench`) — 21 crates total
- `sdust-runtime` gains `crossbeam-deque` for work-stealing; the
  scheduler is a complete rewrite from the slice-7 single-threaded
  design (A101..A106)
- `sdust-macros` picks up a path-dep on `sdust-diagnostics` so the
  SD6001-SD6006 codes can re-export from the central catalog (A107)

## Known issues

1. **Scheduler migration is non-lossless.** v0.6 retargets the
   routing table on next spawn; existing in-flight loops are not
   disturbed. Lossless live migration is v0.7.
2. **Benchmark comparators are code-only on this build host.** Go /
   g++ / TinyGo / Emscripten not installed; the "Reference env"
   column in `docs/benchmarks/*.md` is marked pending.
3. **Self-host parser is a subset.** The deferred productions
   (send sugar `!Msg(args)`, deadline `@duration`, HTML literals,
   `agent`/`protocol`/`supervisor` blocks, `unsafe` blocks, etc.)
   wait on v0.7.
4. **DOM `get-text` / `query` still return `u32` in WIT.** The
   canonical-ABI return-area bridge is v0.7 work. A108 lowers the
   *calls* end-to-end; the return-types are still the v0.5
   integration shim.
5. **`install_agent_dispatch` not yet wired at runtime startup.**
   v0.5 shipped the http-serve infrastructure; the default
   dispatcher is still echo. Per-agent routing is v0.7.
6. **Carried from v0.5**: 2 conformance cases still ignored, OTLP
   transport gRPC-only, LLVM backend untested on this build host,
   slice-7 supervisor/cap-narrow scopes strict-but-open, proc macros
   SD6006-gated.

## What's next

v0.7 is the **self-host HIR + typeck + borrow-checker** + **per-message
work-stealing** + **proc-macro sandbox** milestone. See
[`SLICE_V0_6.md`](SLICE_V0_6.md) for the full v0.7 deferral list.
