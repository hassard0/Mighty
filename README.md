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
> **1433 Rust tests** across 20 crates plus a second independent
> Python implementation at [`impl-py/`](impl-py/) (front-end + HIR +
> typeck through HM closures + generic-constraints, **311 tests**,
> 23/23 examples typeck clean) and a third source-only Go front-end
> at [`impl-go/`](impl-go/) (4848 LOC, cross-validation pending Go
> toolchain). All six CI jobs are required gates. **All KNOWN_ISSUES
> P1/P2 entries are closed.** **v1.0 freeze blockers are down to RFC
> comment windows**: blocker #1 (Python 2nd-impl typeck) closed with
> HM closures + generic-constraints; blocker #3 (normative conformance
> suite) closed with the 140-case kit + normative
> `docs/spec/conformance.md`; blocker #2 (eight RFC comment-window
> closures) is infrastructure-ready
> (`docs/spec/rfcs/COMMENT_WINDOWS.md` tracks all 8 windows) and
> awaits the user-side Discussion-thread openings.
> **Earliest possible v1.0.0 tag: 2026-07-26** (the day after the
> longest 60-day windows close). See [Status](#status) below.

## Release timeline

- **v0.20.0** (this release): the full former post-v1.0 roadmap is
  now live pre-v1.0. Hot reload (Tier 1.5) with `Resumable` + swap
  pipeline + `mty reload` CLI; cluster mTLS + CN-bound identity +
  Tier 4.2 supervisor with 3 restart strategies + circuit breaker;
  DWARF v5 opt-in (`MTY_DWARF5=1`); strict-equality replay payloads
  (v0.18 hot-path migrated from Opaque to Values); conformance kit
  grows 122 → 140 cases and now auto-publishes alongside binaries.
- **v0.21-RC1** (next): wasm hot reload + schema-evolution migration
  hooks + control-socket reload handler; cluster `PlacementPolicy`
  + Tier 4.3 lossless live migration + supervisor OpenTelemetry
  metrics; DWARF v5 per-instruction line program enablement
  (cranelift `MachSrcLoc`); strict-equality recording-on benchmark +
  per-payload size cap; conformance per-backend link-and-run
  assertions + diagnostic-code gap closure.
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
- **Hot reload — `Resumable` + swap pipeline (v0.20, Tier 1.5)** —
  `Resumable` trait (FNV-1a `SCHEMA_HASH` const + default
  ciborium-backed `to_snapshot`/`from_snapshot`); swap pipeline
  `reload::swap` (pause → drain → snapshot → schema check → restore
  → resume) via `ReloadGate`, mailbox preserved across the boundary;
  `mty reload <agent-type> --from new.wasm` CLI with `--dry-run` /
  `--deadline-ms` / `--sock` / `--json`; new diagnostic band
  `MT5060`–`MT5069`; `ModuleSource::SameProgram` wired end-to-end
  (raw-wasm rejected with `MT5064` until v0.21)

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
  (v0.20) — parallel emission path with the v5 `.debug_line_str`
  quintuple; v5 capacity for denser opcode-table rows + cross-CU
  string sharing is wired (the wins are dormant until cranelift
  `MachSrcLoc` plumbing lands per-instruction rows in v0.21)

**Tooling**

- `mty lsp` — LSP 3.17 server (diagnostics, hover, completion, go-to-def, semantic tokens, rename, inlay hints, code actions, signature help, workspace folders)
- `mty pkg` — package manager: resolver, lockfile, GitHub-Releases-backed registry, `.tar.gz` bundles, signed sidecars
- **`mty pkg publish --sign` — real Sigstore keyless (v0.18, `sigstore-real` feature)** — Fulcio short-lived ECDSA-P256 cert + Rekor `hashedrekord` transparency-log entry, full Sigstore Bundle JSON embedded in the `.bundle` envelope; `cosign verify-blob` / `rekor-cli` consume the embedded Bundle directly
- `mty doc` — markdown / HTML doc generator with search index
- `mty fmt` — canonical formatter (idempotent under fuzz)
- Stdlib: `std.json`, `std.tls`, `std.http`, `std.fs`, `std.time`, `std.test`, `std.io` — backed by `rustls` / `hyper` / `serde_json` / `tokio`
- Diagnostics: `MT0001`–`MT8010`, each with `mty explain`

**Self-hosting (full pipeline)**

- Lexer (full), parser (~1.9 KLOC subset), HIR lowering, minimal typeck, MtyIR lowering, **and Wasm core-module codegen** are all written in Mighty itself and exercised end-to-end against examples 01-05 plus arithmetic / option / pattern / string fixtures.
- 23 self-host driver codegen tests passing (0 ignored), supported by 9 IR + 7 typeck + 7 HIR + 13 parser + 4 lexer suites.
- v0.13 closed the front-end-through-back-end self-host chain for the slice-1 subset; v0.14 broadened the Wasm codegen with string pool emission, ADT bump-alloc layout, and pattern lowering, bringing example 03 through the bootstrap chain. v0.15 added variant-call lowering in `mty-ir::lower::exprs::resolve_callee` (Some/Ok/MyEnum.Variant lower to `Rvalue::AdtInit`), a SwitchInt cascade for dense integer matches, and a `for i in 0..n` desugar. v0.16 lowers `Rvalue::MethodCall` through the host `ir_method_resolve(name)` bridge (v0.15 emitted `unreachable`) and desugars `for x in custom_iter` at the selfhost-IR layer into the iter-protocol shape, so for-loops over user-defined iterators now emit real iteration code.

**Independent implementations**

- Rust reference compiler (this repo, `crates/mty-*`).
- Python 2nd-impl at [`impl-py/`](impl-py/) — pure-Python front-end + HIR + lowering + Hindley-Milner typeck with **HM closure inference + generics-with-constraints** (v0.19), built from the v1.0-RC spec prose alone. **311 tests**, 23/23 examples typeck clean. Closes v1.0 freeze blocker #1; borrow + codegen layers stay post-v1.0.
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
   **CLOSED v0.19** — Python 2nd-impl through HM closures +
   generic-constraints; 311 tests; 23/23 examples typeck clean.
2. The eight RFC comment windows
   (RFC-001..006 + RFC-008 + RFC-009).
   **Infrastructure shipped v0.19** in
   [`docs/spec/rfcs/COMMENT_WINDOWS.md`](docs/spec/rfcs/COMMENT_WINDOWS.md);
   awaits user-side Discussion-thread openings. Earliest close
   2026-06-09 (RFC-005, 14 days); latest close 2026-07-25 (RFC-002
   / RFC-006, 60 days each).
3. ~~A published normative conformance suite.~~ **CLOSED v0.19** —
   [`scripts/build-conformance-kit.sh`](scripts/build-conformance-kit.sh)
   packages 122 cases / 24 categories + the new normative
   [`docs/spec/conformance.md`](docs/spec/conformance.md) into a
   ~92 K tarball attached to every tagged release.

### Post-v1.0

- Lossless live agent migration (Tier 4.3); per-message work-stealing
  (Tier 5); cluster `PlacementPolicy` for cross-node fail-over.
- Polonius-style borrows; real cap-name resolution wiring.
- PGO / ThinLTO.
- Python 2nd-impl borrow + codegen layers
  (out-of-scope for v1.0; the v1.0 freeze ships through HM typeck
  only).

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

For the full per-version history of what shipped on the road to v1.0,
see [`CHANGELOG.md`](CHANGELOG.md).

## Status

Mighty is **pre-alpha**. Internal milestones have been tagged through
**v0.20**. The v1.0 language spec is at v1.0-RC4 — see
`docs/spec/v1.0-rc.md`. There are **1433 Rust tests** across the
workspace (plus **311 Python tests** in the [`impl-py/`](impl-py/)
2nd-impl, **140 normative conformance cases**, and **23 self-host
driver** codegen tests = **1907 combined**), 0 clippy warnings *under
the strict `pedantic` gate* (a required CI job, not advisory), and
**4/4 demos** pass `smoke.sh`. The cargo-fuzz harness covers four
targets (parser / typeck / fmt / codegen). **All KNOWN_ISSUES P1/P2
items are closed**; v1.0 freeze blockers are down to the RFC
comment windows (infrastructure shipped; awaits user-side Discussion
thread openings; earliest v1.0.0 tag 2026-07-26 — see
[Release timeline](#release-timeline) above and
[`docs/spec/rfcs/COMMENT_WINDOWS.md`](docs/spec/rfcs/COMMENT_WINDOWS.md)).
v0.20's swarm landed the entire former post-v1.0 roadmap pre-v1.0:
hot reload with `Resumable` + the swap pipeline + the `mty reload`
CLI (Tier 1.5), cluster mTLS with CN-bound node identity + the
Tier 4.2 supervisor (3 restart strategies + per-child circuit
breaker), DWARF v5 + per-instruction line program (opt-in via
`MTY_DWARF5=1`), the strict-equality replay payload migration
(v0.18 hot-path now emits `Values` by default), and the conformance
corpus expansion from 122 → 140 cases (kit ~108 K, now auto-
attached to every tagged release).

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
