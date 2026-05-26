# Mighty

[![Status](https://img.shields.io/badge/status-pre--alpha-orange)](#status)
[![Spec](https://img.shields.io/badge/spec-v1.0--RC4-blueviolet)](docs/spec/v1.0-rc.md)
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![CI](https://img.shields.io/github/actions/workflow/status/hassard0/Mighty/ci.yml?branch=main)](https://github.com/hassard0/Mighty/actions/workflows/ci.yml)
[![Docs](https://img.shields.io/badge/docs-online-success)](https://hassard0.github.io/Mighty/)

**Mighty is an agent-first systems programming language.** It is
statically typed, ownership-based, and treats *agents*, *protocols*,
*capabilities*, *effects*, *arenas*, and *budgets* as first-class
concepts. The toolchain targets both native code (Cranelift JIT + AOT;
LLVM behind `--features llvm`) and WebAssembly (Component Model by
default; bare core modules via `--no-component`).

The compiler, runtime, formatter, package manager, doc generator, LSP
server, and stdlib are all in one Rust workspace and one `mty` binary.

> **Status:** pre-alpha. The v1.0 language spec is at **v1.0-RC4**
> (operator precedence promoted to normative §11.1.1; full
> 63-reserved-keyword set enumerated; effect-row grammar admits the
> multi-row-variable tail since v0.18). The toolchain is exercised by
> **1604 Rust tests** across 20 crates plus a second independent
> Python implementation at [`impl-py/`](impl-py/) (full pipeline:
> lex → parse → lower → typeck → borrow → wasm; **474 tests**,
> 23/23 examples typeck clean, 21/24 emit wasm) and a third
> source-only Go front-end at [`impl-go/`](impl-go/) (4848 LOC,
> cross-validation pending Go toolchain). All six CI jobs are
> required gates. **All KNOWN_ISSUES P1/P2 entries are closed.**
> **v1.0 freeze blockers are down to RFC comment windows**: blocker
> #1 (Python 2nd-impl) closed with HM closures + generic-constraints
> + full borrow + wasm codegen pipeline; blocker #3 (normative
> conformance suite) closed with the **153-case kit** + normative
> `docs/spec/conformance.md` (coverage stable at 63% direct / 99%
> any-harness; MT3012 DROP_IN_CONST_CONTEXT remains the only
> uncovered code, deferred pending HIR `CONST_DECL` lowering);
> blocker #2 (eight RFC comment-window closures) is
> infrastructure-ready (`docs/spec/rfcs/COMMENT_WINDOWS.md` tracks
> all 8 windows) and awaits the user-side Discussion-thread
> openings. **Every former Post-v1.0 roadmap item has now landed
> pre-v1.0.** **Earliest possible v1.0.0 tag: 2026-07-26** (the day
> after the longest 60-day windows close). See [Status](#status)
> below.

## Release timeline

- **v0.23.0** (this release): **Mighty can run a web game on
  localhost.** First-class browser host surface — `mty:web/canvas@0.1`
  + `mty:web/input@0.1` Component-Model WIT pair lowered through new
  `std.web.Canvas` + `std.web.Input` Mighty-side bindings (`crates/
  mty-stdlib/src/web/{canvas,input}.rs`, drift-guarded by
  `WIT_IMPORT_*` / `WIT_EXPORT_*` consts, 8 codegen tests + 13
  stdlib unit tests). New `mty serve [--port <n>] [--watch]`
  subcommand — hand-rolled HTTP/1.1 dev server (no `hyper`/`axum`
  dep) with RFC 6455 hand-rolled websocket hot-reload over
  `notify` file watches. New `mty new --template web-game <name>`
  scaffolds the 5-file agent + canvas + dom-shim project. New
  6th demo `06_canvas_game` where the Mighty agent drives the
  canvas directly via the new WIT surface (JS shim **-32%** LOC vs
  the v0.22 notetris demo). Headless-browser visual smoke at
  `tests/web-smoke/smoke-headless.mjs` (Playwright + 8x8 average-
  hash phash + per-demo golden lock-in, gated by `MTY_WEB_SMOKE=1`).
  `wasm32-web` framing-floor regression harness at `crates/
  mty-codegen-wasm/tests/embedded_core_module.rs` locks in the
  embedded-core-module invariant (5 tests, ±32 byte tolerance for
  wit-component drift). Conformance kit grows 147 → 153 cases.
  Rust test count **1554 → 1604** (+50). Three v0.24 language gaps
  documented for closure (`BuiltinId::CanvasOp(...)` lowering arm,
  `format!()` / interpolation, `export fn` reaching the embedded
  core module's export table).
- **v0.24-RC1** (next): close Track D's three language gaps (canvas
  op lowering + `format!()` interpolation + `export fn` reaching
  the embedded core module's export table); promote `mty serve
  --watch`'s in-browser hot-reload from manual-only to test-gated;
  v1.0 freeze gate prep — RFC monitoring + final spec polish ahead
  of the 2026-07-25 last-RFC-window close.
- **v1.0.0 GA**: when all 8 RFC comment windows close.
  **Earliest: 2026-07-26** (the day after RFC-002 / RFC-006's 60-day
  windows close). The integrator collects dispositions →
  `dev/history/notes/RFC_DISPOSITION_<RFC>.md`, builds
  `mty-conformance-kit-v1.0.0.tar.gz` from
  `scripts/build-conformance-kit.sh`, tags `v1.0.0`.

## Install

Pre-built `mty` binaries for Linux x86_64, macOS arm64, and Windows
x86_64 are attached to each tagged release on the
[GitHub Releases page](https://github.com/hassard0/Mighty/releases)
(produced automatically by `.github/workflows/release.yml` on `v*`
tag push, starting with v0.15.0; Intel macOS dropped from the matrix
in v0.18 after Apple's runner retirement).

To build the toolchain from source instead:

```bash
git clone https://github.com/hassard0/Mighty
cd Mighty
cargo install --path crates/mty-cli
```

This installs the `mty` binary. **MSRV: Rust 1.85.**

Fastest on-ramp to a browser-hosted Mighty program (v0.23):

```bash
mty new --template web-game asteroids
cd asteroids
mty serve --port 8000   # built-in dev server + file-watcher hot-reload
# open http://localhost:8000
```

`mty new --template web-game <name>` scaffolds a 5-file
agent + canvas + dom-shim project ready to build under
`--target wasm32-web`. `mty serve` hands the resulting Component +
`web/` static assets out over HTTP/1.1 with optional `--watch`-driven
websocket hot-reload.

## Hello, Mighty

```bash
mty new hello
cd hello
mty check src/main.mty
mty run   src/main.mty
# → hello, Mighty
```

`mty new` produces:

```mty
fn main() {
  log("hello, Mighty")
}
```

Then:

| Command | What it does |
|---|---|
| `mty check` | lex, parse, lower, type-check, borrow-check |
| `mty run`   | JIT via Cranelift (interpreter fallback on unsupported shapes) |
| `mty build` | native object + linker, or `--target wasm32-wasi` for Wasm |
| `mty fmt`   | canonical Wadler/Lindig formatter |
| `mty dump --sir` | inspect the mid-level IR |
| `mty explain MTxxxx` | one-paragraph explanation of any diagnostic code |

## Features

**Type system**

- Hindley–Milner inference with bidirectional checking
- Generics with monomorphization, trait dispatch, `dyn Trait` fat pointers
- `?` propagation for `Result`
- Ownership + move + borrow + affine + arena tracking
- Field-level borrow Places with NLL last-use deactivation
- Effect system + capabilities (`fs`, `net`, `time`, `rand`, `model`)
- Formal `Sendable` trait — cross-agent message-arg soundness

**Concurrency**

- Tokio-backed concurrent runtime with mailboxes, supervisors, deadline timers
- Multi-core scheduler — per-worker tokio runtimes + crossbeam-deque work-stealing + affinity hints
- **Per-message work-stealing (v0.22, Tier 5)** — promotes the v0.10
  affinity-hint scheduler to true per-worker crossbeam-deque queues
  with NUMA-locality steal ordering (own NUMA node → same socket →
  anywhere fallback via Linux `/sys` probe with flat-topology
  Windows/macOS fallback); `local → siblings → injector` phase
  reversal (v0.21 had `local → injector → siblings` which let
  pinned-burst workloads sit on the injector-race winner); new
  process-wide `worker.steals_total{src,dst}` OTel counter
  (cardinality-bounded at `(N+1) × N` entries); -61% on the "1000
  tasks pinned to worker 0" microbenchmark vs v0.21, -9.3% on the
  "1000 tasks via global injector" workload
- Cooperative mid-turn cancellation, deterministic-execution mode
- Per-handler memory budget + tick budget with auto-charge on alloc
- **Live agent introspection** — `mty inspect` CLI + opt-in
  `MTY_RUNTIME_CONTROL_SOCK` runtime control socket exposing
  mailbox depth, in-flight handler, budgets, and last-N messages
- **OpenTelemetry agent spans** — spawn / send / ask / handler /
  restart / budget-exhausted spans, plus `agent.event(name, &[(k, v)])`
  helper; lazy init from `MTY_OTLP_ENDPOINT`, cost-zero when disabled
- **Deterministic replay — byte-identical re-execution (v0.19 / v0.20,
  wire v2)** — `Recorder` + 8 typed `TraceEvent` variants wired into
  the Runtime hot path across 13 instrumentation sites (spawn / send
  / ask / handle / IO / clock / random / budget / exit); structural
  `ReplayPayload::Values` codec mirrors the 13 `Value` variants and
  is the live default for in-process send/ask payloads since v0.20
  (the v0.18 hot-path migration finishes; the `Opaque` arm is now a
  back-compat fallback that never fires for fresh recordings);
  `ReplayDriver` re-runs the program against the trace and diffs
  events byte-for-byte; `mty replay --byte-identical --program <src>`
  CLI; v1 traces decode transparently via `V1TraceFile` back-compat
  shim; opt-in via `MTY_RECORD_TRACE=/path/to/trace`
- **Distributed agents — cross-node send + ask (v0.19, Tier 4.1)** —
  `AgentAddr = node:type:pid` + framed CBOR-over-TLS transport;
  `Runtime::with_cluster(SharedRouter)` +
  `Runtime::send_addr(AgentAddr, …)` +
  `Runtime::ask_addr(AgentAddr, …)` consult the router; node-wide
  `CorrelationTable` demuxes inbound `Reply` / `Error` frames into
  oneshot receivers; peer-disconnect fan-out fails every in-flight
  ask to that node (`MT5032`); `[cluster]` / `[[cluster.peers]]` /
  `[cluster.tls]` manifest section
- **Cluster mTLS + CN-bound node identity (v0.20)** — opt-in via
  `ClusterMesh::from_config_mtls(cfg)` (zero-breakage for v0.18 /
  v0.19 callers); `cluster/tls.rs` builds rustls accept/connect
  configs and pins a custom post-handshake `verify_peer_identity`
  that rejects connections whose cert CN doesn't match the
  configured `node_id`; hand-rolled ~50-LOC TLV walker
  (`extract_cn_from_der`) pulls the CN without an extra dep
- **Cluster supervisor (v0.20, Tier 4.2)** — `ClusterSupervisor`
  with per-child state machine, three restart strategies
  (`OneForOne` / `RestForOne` / `OneForAll`) matching the local
  supervisor surface, per-child circuit breaker (sliding-window
  failure count → tripped open → half-open after cooldown → closed
  on next success); restart decisions emit on a bounded
  `SUPERVISOR_EVENT_CAPACITY = 256` channel (caller picks
  placement); mesh `notify_node_disconnect` hook marks affected
  children `:noproc`
- **Hot reload — `Resumable` + swap pipeline (v0.20/v0.21, Tier 1.5)** —
  `Resumable` trait (FNV-1a `SCHEMA_HASH` const + default
  ciborium-backed `to_snapshot`/`from_snapshot`); swap pipeline
  `reload::swap` (pause → drain → snapshot → schema check → restore
  → resume) via `ReloadGate`, mailbox preserved across the boundary;
  `mty reload <agent-type> --from new.wasm` CLI with `--dry-run` /
  `--deadline-ms` / `--sock` / `--json`; new diagnostic band
  `MT5060`–`MT5069`; **v0.21 completes the wasm-bytes path** via a
  `wasmparser`-driven loader (`__mty_agent_type` + `__mty_schema_hash`
  custom sections), `MigrateFrom<Old>` + `SchemaRegistry` BFS over
  `(old_hash, new_hash)` edges (V1 → V2 → V3 supported), the
  control-socket `op=reload` handler + `ReloadHook` registry, and
  a parking_lot condvar drain (no more 1 ms busy-poll)
- **Lossless live agent migration (v0.21, Tier 4.3 — RFC-006)** —
  `MigrationOrchestrator::migrate_agent(agent, target, deadline)`
  ships a running agent's snapshot + queued mailbox + continuation
  between cluster nodes via the new `WireFrame::MigrateSnapshot` /
  `MigrateAck` / `MigrateError` frames; abstracted over the runtime
  via `SnapshotSource` / `SnapshotSink` / mesh wire hooks (6 MB
  hard cap on snapshot payload); new `PlacementPolicy` trait + 3
  bundled policies (`StickyPolicy`, `LeastLoadedPolicy`,
  `StaticPolicy`) feed `RestartRequested` events with
  `placement_hint: Option<NodeId>`; new `[cluster.placement]`
  manifest block; OTel cluster metrics
  (migrations_started/completed/failed/rolled_back_total,
  migration_state_bytes_sum, placements_chosen_total{policy}); new
  `MT507x` diagnostic band (MT5071..MT5079)
- **Polonius-style borrows (v0.21, opt-in `polonius` feature)** —
  second-pass borrow checker layered on the v0.3-vintage NLL walker;
  datalog fact model (`Borrow(origin, place, mut)`, `Loan(origin,
  scope)`, `Subset(o1, o2, point)`, `Invalidates(origin, point)`)
  + 4 inference rules (transitive subset closure, loan-region
  intersection, mutual-borrow conflict, end-of-scope loan death) +
  fixpoint solver; default build (no feature) is byte-identical to
  v0.20 borrow-check semantics
- **Cap-name resolver — MT4060–MT4065 active (v0.21)** — 3-layer
  scope-frame resolver (current fn signature, enclosing impl/trait,
  module-level prelude) pinning `Fs` / `Net` / `Clock` / `Dom` /
  `Model` names against their cap family + narrowing surface; the
  six v0.20-uncovered MT4xxx codes now actively emit (MT4060
  Unbound / MT4061 FamilyMismatch / MT4062 NarrowingParamMismatch /
  MT4063 NarrowingInBodyButNotSignature / MT4064
  FamilySurfaceInconsistency / MT4065 NarrowingConstructorArgShape)

**Codegen**

- Cranelift JIT + AOT object emission (default)
- LLVM backend (`--features llvm`)
- WASI Preview 2 **default for `wasm32-wasi`** (explicit `--wasi=p1`
  opts back to v0.13/v0.14 behaviour); the preview1 adapter is
  **opt-in** via
  `Preview2Options::with_adapter(Some(AdapterEmbed::new(kind, bytes)))`
  rather than always-on, and the vendored bytes were dropped from
  the crate in v0.19 — callers download the matching wasmtime
  release's adapter when they need it. `std.fs.*` / `std.http.*` /
  `std.random.*` / `std.time.*` / `log()` all emit direct versioned
  P2 imports (`wasi:filesystem` / `wasi:http` / `wasi:random` /
  `wasi:clocks` / `wasi:cli/stdout` + `wasi:io/streams`); no
  surface still flows through the adapter on a default build
- **Real free-list `cabi_realloc` (v0.18)** — extracted from
  `emit.rs` into `cabi_realloc.rs`; segregated free-list with 8 size
  classes (8B → 1024B, powers of 2) + a large bump path, with
  per-class LIFO push/pop, ~190 emitted wasm instructions, 32-byte
  state region; 17 dedicated coverage tests
- Wasm Component Model (`wit-component`) emission with user-supplied
  WIT via `[wit]` in `mighty.toml`
- DWARF v4 debug info + Wasm source maps + `name` section; opt-in
  DWARF v5 + per-instruction line program via `MTY_DWARF5=1`
  (v0.20/v0.21) — parallel emission path with the v5
  `.debug_line_str` quintuple; **v0.21 plumbs cranelift's
  `MachSrcLoc` map through `Module::define_function`** so every
  machine instruction inherits its MtyIR statement source loc
  (`LowerCtx { fn_debug, capture_debug_info }` + per-stmt
  `b.set_srcloc(...)`); `.debug_loclists` per-local emitted from
  cranelift slot offsets; v5 binary-size delta flips from +3.2%
  (v0.20, conservative 2-entry table) to **-2.3% vs v4** (v0.21,
  dense `DW_LNS_advance_pc` + small-delta `DW_LNS_copy` opcodes
  beat the equivalent v4 stream once you cross ~8 rows per fn)

**Tooling**

- `mty lsp` — LSP 3.17 server (diagnostics, hover, completion, go-to-def, semantic tokens, rename, inlay hints, code actions, signature help, workspace folders)
- `mty pkg` — package manager: resolver, lockfile, GitHub-Releases-backed registry, `.tar.gz` bundles, signed sidecars
- **`mty pkg publish --sign` — real Sigstore keyless (v0.18, `sigstore-real` feature)** — Fulcio short-lived ECDSA-P256 cert + Rekor `hashedrekord` transparency-log entry, full Sigstore Bundle JSON embedded in the `.bundle` envelope; `cosign verify-blob` / `rekor-cli` consume the embedded Bundle directly
- `mty doc` — markdown / HTML doc generator with search index
- `mty fmt` — canonical formatter (idempotent under fuzz)
- Stdlib: `std.json`, `std.tls`, `std.http`, `std.fs`, `std.time`, `std.test`, `std.io` — backed by `rustls` / `hyper` / `serde_json` / `tokio`
- Diagnostics: `MT0001`–`MT8010`, each with `mty explain`
- **PGO + ThinLTO build profile (v0.22)** — `release-pgo` cargo
  profile (inherits `release` with `lto = "thin"` + `codegen-units = 1`
  + `panic = "abort"` + `debug = "line-tables-only"`); two-stage
  `scripts/build-pgo.{sh,ps1}` pipeline drives an instrumented
  build → `mty-bench-pgo` sweep across `examples/*.mty` →
  `llvm-profdata merge` → final build with `-Cprofile-use` +
  `-Clinker-plugin-lto` writing to `target/mty-pgo`; manual
  `.github/workflows/pgo-bench.yml` runs the pipeline on
  `workflow_dispatch` and writes a baseline-vs-PGO `mty check`
  wall-clock delta to the workflow summary (v0.22 ships measurement,
  not gating; v0.23's BOLT follow-up turns it into the default
  release artifact pipeline)
- **`std.web` (canvas + keyboard) + `mty serve` + headless visual
  smoke (v0.23)** — first-class browser host surface for Mighty.
  `std.web.Canvas::{clear, fill_rect, request_animation_frame}` +
  `std.web.Input::{poll_keydown, poll_keyup}` lower to a
  `mty:web/canvas@0.1` + `mty:web/input@0.1` Component-Model WIT
  pair (drift-guarded by `WIT_IMPORT_*` / `WIT_EXPORT_*` consts in
  `crates/mty-stdlib/src/web/{canvas,input}.rs`). New `mty serve
  [--port <n>] [--watch]` subcommand: hand-rolled HTTP/1.1 dev
  server (no `hyper`/`axum` dep) + RFC 6455 hand-rolled websocket
  hot-reload triggered by `notify` file-watcher events under `src/`
  + `web/`. New `mty new --template web-game <name>` scaffolds the
  5-file agent + canvas + dom-shim project. Headless-browser visual
  smoke at `tests/web-smoke/smoke-headless.mjs` (Playwright,
  8x8 average-hash perceptual hash, hamming-distance budget 12,
  per-demo `golden/<name>.phash` lock-in, manual `web-smoke.yml`
  workflow_dispatch CI job, gated by `MTY_WEB_SMOKE=1` locally so
  the heavy Playwright install is opt-in). A regression harness at
  `crates/mty-codegen-wasm/tests/embedded_core_module.rs` locks in
  the wasm32-web framing invariant (Component envelope embeds the
  core module at byte offset 189, ±32 byte tolerance for
  wit-component drift between releases). Three Mighty-source-side
  language gaps documented for v0.24 closure
  (`BuiltinId::CanvasOp(...)` lowering arm, `format!()` /
  interpolation, `export fn` reaching the embedded core module's
  export table).

**Self-hosting (full pipeline)**

- Lexer (full), parser (~1.9 KLOC subset), HIR lowering, minimal typeck, MtyIR lowering, **and Wasm core-module codegen** are all written in Mighty itself and exercised end-to-end against examples 01-05 plus arithmetic / option / pattern / string fixtures.
- 23 self-host driver codegen tests passing (0 ignored), supported by 9 IR + 7 typeck + 7 HIR + 13 parser + 4 lexer suites.
- v0.13 closed the front-end-through-back-end self-host chain for the slice-1 subset; v0.14 broadened the Wasm codegen with string pool emission, ADT bump-alloc layout, and pattern lowering, bringing example 03 through the bootstrap chain. v0.15 added variant-call lowering in `mty-ir::lower::exprs::resolve_callee` (Some/Ok/MyEnum.Variant lower to `Rvalue::AdtInit`), a SwitchInt cascade for dense integer matches, and a `for i in 0..n` desugar. v0.16 lowers `Rvalue::MethodCall` through the host `ir_method_resolve(name)` bridge (v0.15 emitted `unreachable`) and desugars `for x in custom_iter` at the selfhost-IR layer into the iter-protocol shape, so for-loops over user-defined iterators now emit real iteration code.

**Independent implementations**

- Rust reference compiler (this repo, `crates/mty-*`).
- Python 2nd-impl at [`impl-py/`](impl-py/) — **full pipeline**
  (lex → parse → lower → typeck → borrow → wasm) in pure Python,
  built from the v1.0-RC spec prose alone (**v0.22**, extends the
  v0.19 typeck-only impl with NLL-flavoured borrow checker covering
  MT3001–MT3005 and Core 1.0 wasm codegen emitting i32 arithmetic /
  control flow / calls / locals with deduplicated type table and
  structural validation). **474 tests** (+163 from v0.21), 23/23
  examples typeck clean, **21/24 examples emit wasm fn bodies**
  (the 3 zero-fn examples are agent-only files). Closes v1.0
  freeze blocker #1 + completes the v1.0-RC validation question
  (the Rust reference is no longer the only impl that exists —
  every spec-prose claim now has a 2nd impl that round-trips
  through codegen).
- Go 3rd-impl front-end at [`impl-go/`](impl-go/) — Go 1.22+ lexer + parser + CLI built from the v1.0-RC spec prose alone. 4848 LOC; cross-validation (`go test ./...`, example sweep) pending Go toolchain on the build host.

## Documentation

Live docs site: **<https://hassard0.github.io/Mighty/>**

- [Getting started](docs/getting-started.md)
- [Tour](docs/tour/README.md) — walk through the canonical examples
- [Language spec v1.0-RC4](docs/spec/v1.0-rc.md) (frozen for v1.0)
- [Normative conformance doc](docs/spec/conformance.md) (v0.19)
- [Spec v0.1 + amendment log](docs/spec/v0.1.md)
- [Reference](docs/reference/README.md) — CLI, manifest, registry, diagnostics
- [Internals](docs/internals/README.md) — compiler architecture, crate-by-crate
- [FAQ](docs/faq.md)
- [Contributing](docs/contributing.md)

## Project layout

The compiler is a Rust workspace of twenty crates:

| Crate | Responsibility |
|---|---|
| `mty-syntax` | lexer (logos), CST (rowan), parser |
| `mty-ast` | typed AST view over the CST |
| `mty-diagnostics` | diagnostic types, MT-coded labels, ariadne rendering |
| `mty-hir` | name-resolved HIR with arena storage; macro preprocessor hook |
| `mty-types` | resolved `Ty`, HM inference, bidirectional type checker, effects + capabilities, `Sendable` |
| `mty-borrow` | ownership / move / borrow / affine / arena analysis (field-level Places + NLL last-use) |
| `mty-sir` | mid-level IR + tree-walking interpreter |
| `mty-runtime` | concurrent tokio runtime: agents, mailboxes, supervisors, budgets |
| `mty-codegen-cranelift` | native backend — JIT + AOT object |
| `mty-codegen-wasm` | wasm32-wasi / wasm32-web core module + Component Model emitter |
| `mty-codegen-llvm` | LLVM backend (real lowering behind `--features llvm`) |
| `mty-debuginfo` | DWARF v4 builder + wasm source-map + `name` section |
| `mty-fmt` | canonical formatter (Wadler/Lindig pretty-printer) |
| `mty-driver` | compilation pipeline and `mighty.toml` manifest loader |
| `mty-pkg` | package manager: resolver, lockfile, fetchers, publisher |
| `mty-lsp` | LSP 3.17 server over stdio |
| `mty-doc` | doc generator (extract + render markdown/HTML) |
| `mty-stdlib` | real `std.json` / `tls` / `http` / `fs` / `time` / `test` |
| `mty-macros` | declarative-macro registry + expander + hygiene |
| `mty-cli` | the `mty` binary |

Adjacent trees: `examples/` (canonical programs), `demos/`
(end-to-end runnable apps with `smoke.sh`), `benches/` (criterion +
the `mty-bench` runner), `selfhost/` (the bootstrap compiler written
in Mighty), `tests/conformance/` (cross-crate behavioural specs),
`editor/vscode/` (VS Code extension).

## Roadmap

### To `v1.0`

The v1.0 spec is feature-complete at **v1.0-RC4** (`docs/spec/v1.0-rc.md`).
**Earliest possible v1.0.0 tag: 2026-07-26.** Remaining blockers:

1. ~~A second independent compiler implementation (RFC-007).~~
   **CLOSED v0.19, extended v0.22** — Python 2nd-impl through HM
   closures + generic-constraints + full borrow + wasm codegen;
   474 tests; 23/23 examples typeck clean; 21/24 emit wasm.
2. The eight RFC comment windows
   (RFC-001..006 + RFC-008 + RFC-009).
   **Infrastructure shipped v0.19** in
   [`docs/spec/rfcs/COMMENT_WINDOWS.md`](docs/spec/rfcs/COMMENT_WINDOWS.md);
   awaits user-side Discussion-thread openings. Earliest close
   2026-06-09 (RFC-005, 14 days); latest close 2026-07-25 (RFC-002
   / RFC-006, 60 days each).
3. ~~A published normative conformance suite.~~ **CLOSED v0.19,
   grown v0.20/v0.22/v0.23** —
   [`scripts/build-conformance-kit.sh`](scripts/build-conformance-kit.sh)
   packages **153 cases** / 24 categories + the normative
   [`docs/spec/conformance.md`](docs/spec/conformance.md) into a
   tarball attached to every tagged release. Coverage 63%
   direct / 99% any-harness (only MT3012 uncovered).

### Post-v1.0

The Post-v1.0 backlog is **empty** as of v0.22 — every former
post-v1.0 roadmap item (per-message work-stealing, PGO/ThinLTO,
Python 2nd-impl borrow + codegen) landed pre-v1.0. Beyond v1.0,
candidate slices are tracked in the `[Unreleased]` block of
[`CHANGELOG.md`](CHANGELOG.md) (v0.23 Track D language gaps —
`BuiltinId::CanvasOp(...)` lowering, `format!()` / string
interpolation, `export fn` reaching the embedded core module's
export table; `mty serve --watch` hot-reload promoted to test-
gated; BOLT post-link optimisation; multi-socket NUMA benchmark;
`mty conform` implementer-CLI shim; systematic v1.0-RC validation
sweep; MT3012 closure pending HIR `CONST_DECL` lowering).

### Landed pre-v1.0 (formerly post-v1.0)

- WASI Preview 2 + user-supplied WIT (v0.13 → v0.19) — `[wit]` section, default for `wasm32-wasi`, every `std.*` lowering goes through versioned P2 imports, the preview1 adapter is opt-in via `AdapterEmbed::new(kind, bytes)` and the vendored bytes were removed in v0.19.
- Effect-row polymorphism end-to-end (v0.13 → v0.19, RFC-008) — surface syntax (`!E` / `!{a | E}` / `effect a | E` / `!{| E1, E2}` / `effect a, b | E1, E2`); typeck `HirEffectRow::Open(Vec<HirRowVar>)`; HIR multi-row lowering complete in v0.19 (every `EFFECT_ROW_VAR` child read); MT4055–MT4059 all active emit.
- Set-of-scopes macro hygiene (RFC-009, v0.13 → v0.15).
- End-to-end self-hosting through Wasm codegen (v0.13 → v0.16) — 23 codegen driver tests, 0 ignored; examples 01-03 bootstrap through the self-host chain.
- Live agent introspection + OpenTelemetry (v0.16) — `mty inspect` CLI + `MTY_RUNTIME_CONTROL_SOCK`; OTel spans at every agent boundary + `agent.event(name, …)` helper; cost-zero when disabled.
- Deterministic replay — byte-identical re-execution (v0.17 → v0.19) — Recorder wired into the Runtime hot path across 13 instrumentation sites; wire format v2 + structural `ReplayPayload::Values` codec; `ReplayDriver` re-runs the program against the trace and diffs events byte-for-byte; `mty replay --byte-identical --program <src>`; v1 traces decode transparently.
- Independent second implementation (v0.17 → v0.19) — Python 2nd-impl through HM closures + generic-constraints; 311 tests; 23/23 examples typeck clean. **Closes v1.0 freeze blocker #1.**
- Real `cabi_realloc` free-list allocator (v0.18) — closes KNOWN_ISSUES #1. 8 size classes (8B → 1024B) + large bump path; ~190 wasm instructions; 17 dedicated tests.
- Real Sigstore keyless signing (v0.18, `sigstore-real` feature) — closes KNOWN_ISSUES #2. Fulcio short-lived ECDSA-P256 cert + Rekor `hashedrekord` entry; Sigstore Bundle JSON embedded for direct `cosign verify-blob` / `rekor-cli` consumption.
- Distributed agents — cross-node send + ask (v0.18 transport → v0.19 routing) — `AgentAddr = node:type:pid` + framed CBOR-over-TLS mesh; `Runtime::with_cluster(SharedRouter)` + `send_addr` / `ask_addr` consult the router; node-wide `CorrelationTable` demuxes inbound `Reply` / `Error`; peer-disconnect fan-out fails in-flight asks with `MT5032`.
- Normative conformance kit (v0.19 → v0.20) — `scripts/build-conformance-kit.sh` packages 140 cases / 24 categories + spec docs into a ~108 K versioned tarball; v0.20 backfilled the 4 placeholder categories (`deterministic_replay/` +5, `formatter_idempotence/` +5, `native_abi/` +4, `wasm_component/` +4) and wired the kit-build into `release.yml` so it auto-attaches alongside the binaries on every tagged release. **Closes v1.0 freeze blocker #3.**
- Hot reload — `Resumable` + swap pipeline (v0.20, Tier 1.5) — `Resumable` trait (FNV-1a `SCHEMA_HASH` const + default ciborium-backed `to_snapshot`/`from_snapshot`); swap pipeline `reload::swap` (pause → drain → snapshot → schema check → restore → resume) via `ReloadGate`; `mty reload <agent-type> --from new.wasm` CLI; `MT5060`–`MT5069` diagnostic band; state-only reload wired end-to-end (raw-wasm via `Program::with_swapped_agent` is v0.21).
- Cluster mTLS + Tier 4.2 supervisor (v0.20) — mTLS via opt-in `ClusterMesh::from_config_mtls(cfg)` with CN-bound `verify_peer_identity` (hand-rolled `extract_cn_from_der` TLV walker, no extra dep); `ClusterSupervisor` with three restart strategies (`OneForOne` / `RestForOne` / `OneForAll`) + per-child circuit breaker; restart decisions emit on a bounded event channel (caller picks placement; v0.21 lands `PlacementPolicy`); mesh `notify_node_disconnect` hook marks affected children `:noproc`.
- DWARF v5 + per-instruction line program (v0.20, opt-in) — `MTY_DWARF5=1` env-var toggle (parallel emission alongside the v4 default); new `crates/mty-debuginfo/src/dwarf5.rs` emits the v5 `.debug_info` + `.debug_line` + `.debug_str` + `.debug_line_str` + `.debug_abbrev` quintuple; v5 *capacity* for denser opcode-table line rows + cross-CU `.debug_line_str` string sharing is wired (the *enablement* of those wins waits on cranelift `MachSrcLoc` plumbing in v0.21).
- Strict-equality replay payloads (v0.20) — v0.18 hot-path migration of `Runtime::send` / `Runtime::ask` from `Opaque(format!("{:?}", args))` to `Values(...)`; the `ReplayDriver`'s strict structural equality arm is now the live replay semantic (the `Opaque ≈ Opaque` loose-equality arm becomes a back-compat fallback that never fires for fresh recordings; cluster routing still uses the byte envelope by transport contract).
- Hot reload completion + wasm-bytes swap + schema migrations (v0.21, Tier 1.5) — `wasm_loader` parses `__mty_agent_type` + `__mty_schema_hash` custom sections via `wasmparser`; `Program::with_swapped_agent` clones the per-agent slot map; `MigrateFrom<Old>` + `SchemaRegistry` BFS over `(old_hash, new_hash)` edges supports schema-evolution chains (V1 → V2 → V3); control-socket `op=reload` handler via `Request::Reload { agent_type, module_b64, deadline_ms }` + `ReloadHook` registry; parking_lot condvar drain replaces the 1 ms busy-poll.
- Lossless live agent migration (v0.21, Tier 4.3, RFC-006) — `MigrationOrchestrator::migrate_agent(agent, target, deadline)` ships an agent's snapshot + queued mailbox + continuation between cluster nodes; abstracted over the runtime via `SnapshotSource` / `SnapshotSink` / mesh wire hooks; 6 MB hard cap; `PlacementPolicy` trait + 3 bundled policies (`StickyPolicy` / `LeastLoadedPolicy` / `StaticPolicy`) feeding `RestartRequested` placement hints; `[cluster.placement]` manifest block; OTel cluster metrics; new `MT507x` diagnostic band.
- DWARF v5 MachSrcLoc plumbing (v0.21) — cranelift's per-instruction `MachSrcLoc` map flows through `Module::define_function`; v0.20's conservative 2-entry line table replaced with a dense per-statement line program; `.debug_loclists` per-local emitted from cranelift slot offsets; v5 binary-size flips from +3.2% to -2.3% vs v4 on the synthetic benchmark.
- Polonius-style borrows (v0.21, opt-in `polonius` feature) — datalog fact model + 4 inference rules + fixpoint solver layered on the v0.3-vintage NLL walker; default build (no feature) is byte-identical to v0.20 borrow-check semantics.
- Cap-name resolver — MT4060–MT4065 active emit (v0.21) — 3-layer scope-frame resolver (current fn signature, enclosing impl/trait, module-level prelude) pinning `Fs` / `Net` / `Clock` / `Dom` / `Model` names against their cap family + narrowing surface; closes the 6 v0.20-uncovered MT4xxx typeck codes.
- Per-message work-stealing — Tier 5 (v0.22) — promotes the v0.10 affinity-hint scheduler to true crossbeam-deque per-worker queues with NUMA-locality steal ordering (own NUMA → same socket → anywhere via Linux `/sys` probe, flat fallback on Windows/macOS); `local → siblings → injector` phase reversal; process-wide `worker.steals_total{src,dst}` OTel counter; -61% on the pinned-burst microbenchmark vs v0.21. Closes the last roadmap Tier.
- PGO + ThinLTO build profile (v0.22) — new `release-pgo` cargo profile + two-stage `scripts/build-pgo.{sh,ps1}` pipeline (instrumented build → `mty-bench-pgo` sweep over `examples/*.mty` → `llvm-profdata merge` → final build with `-Cprofile-use` + `-Clinker-plugin-lto`); manual `.github/workflows/pgo-bench.yml` writes baseline-vs-PGO `mty check` wall-clock delta to the workflow summary (measurement-only; v0.23 BOLT follow-up turns it into the default release artifact pipeline).
- Python 2nd-impl full pipeline (v0.22) — extends the v0.19 typeck-only impl with NLL-flavoured borrow checker (MT3001–MT3005, +865 LOC, +28 tests) + Core 1.0 wasm codegen (+954 LOC, +37 tests) emitting i32 arithmetic, control flow, calls, locals with deduplicated type table and structural validation; 96-case full-pipeline sweep; 21/24 examples emit wasm fn bodies; Python test count 311 → 474 (+163). Completes the v1.0-RC validation question — every spec-prose claim now has a 2nd impl that round-trips through codegen.
- Diagnostic-code coverage closure (v0.22) — activates 7 of the 8 v0.21-uncovered codes (MT0004 / MT0030 via `Parser::pre_lex_scan` + driver `DiagCode` preservation; MT2015 / MT2016 via `synth_match`; MT2018 via `synth_expr_inner` If branch; MT2019 via `items` custom function-body path; MT3015 via `mty-borrow::flow::walk_stmt` binding `let x: T;` as `Ownership::Uninit`). +7 conformance fixtures. Coverage 62 → 69 direct (56% → 63%), any-harness 93% → 99%, uncovered 8 → 1. MT3012 DROP_IN_CONST_CONTEXT explicitly deferred to v0.23 pending HIR `CONST_DECL` lowering.
- MtyIR `Stmt` real source-span carrier (v0.22) — every MtyIR `Stmt` + `Terminator` now carries a real `SourceSpan` field (default `SourceSpan::ZERO` for back-compat with manually-constructed programs); HIR spans propagate through `lower → MtyIR → cranelift SourceLoc → DWARF v5 line row`, replacing v0.21's synthetic-uniform per-statement byte-offset spread; `gdb step-line` against v0.22 binaries walks source lines byte-accurately.
- `std.web` (canvas + keyboard) + `mty serve` + headless visual smoke (v0.23) — first-class browser host surface for Mighty. `mty:web/canvas@0.1` + `mty:web/input@0.1` Component-Model WIT pair + `std.web.Canvas` / `std.web.Input` Mighty-side bindings drift-guarded by `WIT_IMPORT_*` / `WIT_EXPORT_*` consts; new `mty serve [--port <n>] [--watch]` subcommand (hand-rolled HTTP/1.1 + RFC 6455 hand-rolled websocket hot-reload over `notify` file watches); new `mty new --template web-game <name>` scaffold; `wasm32-web` embedded-core-module regression harness (the long-standing "header-only component" suspicion was wrong — the core module IS embedded at byte offset 189); 6th demo `06_canvas_game` (agent-driven canvas, JS shim -32% LOC vs v0.22 notetris); headless-browser visual smoke (Playwright + 8x8 phash + per-demo golden, gated by `MTY_WEB_SMOKE=1`). Three v0.24 language gaps documented: `BuiltinId::CanvasOp(...)` lowering arm in `mty-codegen-wasm/src/emit.rs`; `format!()` / interpolation; `export fn` reaching the embedded core module's export table. Conformance kit grows 147 → 153 cases.

For the full per-version history of what shipped on the road to v1.0,
see [`CHANGELOG.md`](CHANGELOG.md).

## Status

Mighty is **pre-alpha**. Internal milestones have been tagged through
**v0.23**. The v1.0 language spec is at v1.0-RC4 — see
`docs/spec/v1.0-rc.md`. There are **1604 Rust tests** across the
workspace (plus **474 Python tests** in the [`impl-py/`](impl-py/)
2nd-impl now covering the full pipeline lex → parse → lower → typeck
→ borrow → wasm, **153 normative conformance cases** at **63%
direct / 99% any-harness** coverage with only MT3012
DROP_IN_CONST_CONTEXT remaining uncovered (deferred pending HIR
`CONST_DECL` lowering), and **23 self-host driver**
codegen tests = **2254 combined**), 0 clippy warnings *under the
strict `pedantic` gate* (a required CI job, not advisory), and
**6/6 demos** pass `smoke.sh` (including the v0.23 canvas-driven
06_canvas_game demo). The cargo-fuzz harness covers four
targets (parser / typeck / fmt / codegen). **All KNOWN_ISSUES P1/P2
items are closed**; **every former Post-v1.0 roadmap item has now
landed pre-v1.0** (per-message work-stealing, PGO/ThinLTO, Python
2nd-impl full pipeline all shipped in v0.22); **v1.0 freeze
blockers are down to the RFC comment windows only** (infrastructure
shipped; awaits user-side Discussion thread openings; earliest
v1.0.0 tag 2026-07-26 — see [Release timeline](#release-timeline)
above and
[`docs/spec/rfcs/COMMENT_WINDOWS.md`](docs/spec/rfcs/COMMENT_WINDOWS.md)).

**Pre-built `mty` binaries** for Linux x86_64, macOS arm64, and
Windows x86_64 are produced automatically on every `v*` tag push
(see [Releases](https://github.com/hassard0/Mighty/releases)). Intel
macOS was dropped from the matrix in v0.18 after Apple's runner
retirement. Building from source is still supported. Treat the
language as unstable and please file issues for everything that
surprises you.

## Contributing

Issues are welcome. Pull requests must rebase on `main`, include
tests, and pass `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo test --workspace`. See [`CONTRIBUTING.md`](CONTRIBUTING.md)
for the short form and [`docs/contributing.md`](docs/contributing.md)
for the full workflow. The community standards live in
[`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).

## Community

- File bug reports, feature requests, and language questions in
  [GitHub Issues](https://github.com/hassard0/Mighty/issues).
- Open-ended design discussions go in
  [GitHub Discussions](https://github.com/hassard0/Mighty/discussions).
- There is no Discord / Slack / matrix room yet.

## License

Mighty is released under the [MIT license](LICENSE).

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this work shall be MIT-licensed as above,
without any additional terms or conditions.
