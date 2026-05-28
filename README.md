# Mighty

[![Status](https://img.shields.io/badge/status-pre--alpha-orange)](#status)
[![Spec](https://img.shields.io/badge/spec-v1.0--RC5-blueviolet)](docs/spec/v1.0-rc.md)
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![CI](https://img.shields.io/github/actions/workflow/status/hassard0/Mighty/ci.yml?branch=main)](https://github.com/hassard0/Mighty/actions/workflows/ci.yml)
[![Docs](https://img.shields.io/badge/docs-online-success)](https://hassard0.github.io/Mighty/)

**Mighty is a statically-typed, agent-first systems language.**
*Agents*, *protocols*, *capabilities*, *effects*, *arenas*, and
*budgets* are first-class. The toolchain targets native code
(Cranelift JIT + AOT; LLVM behind `--features llvm`) and WebAssembly
(Component Model by default).

Compiler, runtime, formatter, package manager, doc generator, LSP,
and stdlib all live in one Rust workspace and one `mty` binary.

## Why Mighty

Mighty is the first compiler-backed agent language with
**capability-typed tools** and **deterministic replay**. LLM-agent
frameworks reinvent these badly in Python/TS; Mighty bakes them into
the type system:

- Tainted LLM/MCP/HTTP data can't reach `fs.write`, `process.exec`,
  `sql.execute`, `net.request` without a sanitiser — **prompt
  injection is a compile error**.
- `@tool(cap: fs.read("./data/**"))` is **enforced by the runtime**,
  not the prompt. A misbehaving LLM cannot escape its capability set.
- Every agent run is byte-identically replayable from the recorded
  trace — regression-test LLM agents like any other code.
- Cross-node agent swarms, hot reload preserving conversation
  state, OpenTelemetry spans — all in stdlib.

## Install

Pre-built binaries (Linux x86_64, macOS arm64, Windows x86_64) on
the [Releases page](https://github.com/hassard0/Mighty/releases).
Or from source (MSRV: Rust 1.85):

```bash
git clone https://github.com/hassard0/Mighty && cd Mighty
cargo install --path crates/mty-cli
```

Scaffold a new project:

```bash
mty new --template web-game my-game     # canvas-driven web game
mty new my-cli                          # plain CLI binary
mty serve --watch                       # dev server + hot reload
```

## Hello, Mighty

```bash
mty new hello && cd hello
mty check src/main.mty
mty run   src/main.mty
# → hello, Mighty
```

| Command          | What it does                                                       |
|------------------|--------------------------------------------------------------------|
| `mty check`      | lex, parse, lower, type-check, borrow-check                        |
| `mty run`        | JIT via Cranelift (interpreter fallback on unsupported shapes)     |
| `mty build`      | native object + linker, or `--target wasm32-web` for the browser   |
| `mty serve`      | dev server: build + http + websocket reload on file change         |
| `mty fmt`        | canonical Wadler/Lindig formatter                                  |
| `mty inspect`    | live agent snapshot via the runtime control socket                 |
| `mty replay`     | re-run a recorded trace; `--byte-identical` strict mode            |
| `mty reload`     | swap an agent's wasm without losing its state                      |
| `mty test`       | run unit tests; `--eval` runs `*.eval.mty` suites against a panel  |
| `mty explain`    | one-paragraph explanation of any `MTxxxx` diagnostic code          |

## Features

**Type system**
- Hindley-Milner inference with bidirectional checking
- Generics with monomorphization, trait dispatch, `dyn Trait` fat pointers
- Ownership + move + borrow + affine + arena tracking with field-level
  Places and NLL last-use deactivation
- Effect system + capabilities (`fs`, `net`, `time`, `rand`, `model`)
- Effect-row polymorphism (Koka/Eff-style) with `!{a, b | E}` surface
- Polonius-style borrow check available via `--features polonius`
- Hygienic macros via set-of-scopes resolution

**Concurrency**
- Tokio-backed runtime: mailboxes, supervisors, deadlines, budgets
- Per-message work-stealing with NUMA-locality steal ordering
- Cluster mesh: cross-node `send`/`ask` over framed CBOR + mTLS
- Lossless live agent migration between nodes (RFC-006)
- Hot reload with `Resumable` trait + schema-hash migrations
- Deterministic replay end-to-end (byte-identical)
- Live introspection via `mty inspect` + OTel span integration

**LLM agent stdlib**
- `std.llm` — typed Anthropic / OpenAI / Gemini / Bedrock providers
  with streaming, tool use, structured outputs, `TokenBudget` short-circuit
- `@tool` decorator — `@tool(description, cap)` generates JSON
  schema for every provider; `cap:` enforced by the runtime
- `std.mcp` — server (stdio + http) auto-exposes annotated tools;
  client connects to other MCP servers
- `std.memory` — `VectorStore`, `Episodic`, `Working`;
  deterministic snapshots fold into the replay machinery
- `std.swarm` — votes consensus across providers under a shared
  dollar budget; `Majority` / `Plurality` / `Unanimous` / `Weighted`
- `std.eval` — typed `Suite` / `Case` / `Member` / `Compare`
  regression harness on top of byte-identical replay
- `std.observe` + `mty inspect --cost` — every LLM call's cost +
  latency auto-recorded in local SQLite
- `std.computer` + `@computer_use` — Anthropic Computer Use as a
  capability with typed sandbox bounds
- `mty replay --diff` — divergence reporter, points at the first
  divergent recorded turn across two traces

**Web** *(canvas + keyboard agents)*
- `std.web.Canvas` + `std.web.Input` WIT interfaces
- `wasm32-web` target emits a real Component-Model component
- Demo 06 — agent owns the canvas, JS shim is ~110 LOC

**Codegen**
- Cranelift JIT + AOT object emission (default)
- LLVM backend (`--features llvm`)
- WASI Preview 2 default for `wasm32-wasi`; `std.fs` / `std.http` /
  `std.random` / `std.time` / `log()` emit direct versioned P2 imports
- DWARF v5 with per-instruction line program (opt-in via `MTY_DWARF5=1`)
- PGO + ThinLTO via `release-pgo` profile + `scripts/build-pgo.sh`

**Tooling**
- `mty lsp` — LSP 3.17 (hover, completion, go-to-def, semantic tokens,
  rename, inlay hints, code actions, signature help)
- `mty pkg` — resolver, lockfile, GitHub-Releases-backed registry,
  signed bundles via real sigstore behind `sigstore-real` feature
- `mty doc` — markdown + HTML doc generator with search
- `format!()` with `{:<>^}` alignment, `{:.3}` precision, `{:#x}`
  alt-hex/bin/oct, named-arg passthrough

**Independent implementations**
- Rust reference compiler (`crates/mty-*`)
- Python 2nd-impl ([`impl-py/`](impl-py/)) — full pipeline through
  wasm codegen; 490 tests; 23/23 examples typeck clean, 21/24 emit wasm
- Go 3rd-impl front-end ([`impl-go/`](impl-go/)) — lexer + parser,
  cross-validation pending Go toolchain on the build host

## Documentation

Live docs site: <https://hassard0.github.io/Mighty/>

- [Getting started](docs/getting-started.md)
- [Tour](docs/tour/README.md) — walk through the canonical examples
- [Language spec v1.0-RC5](docs/spec/v1.0-rc.md) (frozen for v1.0)
- [Reference](docs/reference/README.md) — CLI, manifest, registry, diagnostics
- [Internals](docs/internals/README.md) — compiler architecture
- [Agent features roadmap](docs/internals/agent-features-roadmap.md)
- [FAQ](docs/faq.md)
- [Contributing](docs/contributing.md)

## Project layout

20-crate Rust workspace:

| Crate | Responsibility |
|-------|---------------|
| `mty-syntax` | lexer (logos), CST (rowan), parser |
| `mty-ast` | typed AST view over the CST |
| `mty-diagnostics` | `MTxxxx` codes, ariadne rendering |
| `mty-hir` | name-resolved HIR with arena storage |
| `mty-types` | HM inference, bidirectional check, effects + caps + `Sendable` |
| `mty-borrow` | ownership / move / borrow / affine / arena analysis (Polonius opt-in) |
| `mty-ir` | mid-level IR + tree-walking interpreter |
| `mty-runtime` | tokio runtime: agents, mailboxes, supervisors, cluster, replay, reload, introspect, telemetry |
| `mty-codegen-cranelift` | native backend (JIT + AOT) |
| `mty-codegen-wasm` | wasm core + Component Model + `mty:web/*` WIT |
| `mty-codegen-llvm` | LLVM backend (real lowering behind `--features llvm`) |
| `mty-debuginfo` | DWARF v4 + v5 builders |
| `mty-fmt` | canonical formatter |
| `mty-driver` | compilation pipeline + `mighty.toml` loader |
| `mty-pkg` | package manager (sigstore-real signing) |
| `mty-lsp` | LSP 3.17 server |
| `mty-doc` | doc generator |
| `mty-stdlib` | `std.{json,tls,http,fs,time,test,llm,mcp,memory,web,fmt}` |
| `mty-macros` | declarative macros + `format!` + `@tool` builtin attributes |
| `mty-cli` | the `mty` binary |

Adjacent trees: `examples/` (36 canonical programs), `demos/`
(9 runnable apps with `smoke.sh`), `benches/` + `bench/swe/`
(criterion + SWE-bench Verified harness), `selfhost/` (bootstrap
in Mighty), `tests/conformance/` + `tests/web-smoke/` (159-case
normative kit + headless-browser visual smoke).

## Roadmap

### To `v1.0`

The v1.0 spec is feature-complete at v1.0-RC5. **Proposed freeze
date: 2026-09-01** (earliest possible tag: **2026-07-26** — one
day after the longest 60-day RFC window closes).

Only one blocker remains:

- **8 RFC comment windows.** Opened 2026-05-26 on GitHub Discussions
  ([dashboard](docs/spec/rfcs/RFC_DASHBOARD.md)). RFC-005 closes
  earliest (2026-06-09, 14 days); RFC-002 + RFC-006 latest
  (2026-07-25, 60 days). Threads
  [#2–#9](https://github.com/hassard0/Mighty/discussions).

The other two former blockers — 2nd-impl through the pipeline, and
the published normative conformance suite — both closed in v0.19.

### Post-v1.0

Currently empty. Every former post-v1.0 roadmap item — lossless
live agent migration, Polonius borrows, distributed agents, hot
reload, DWARF v5, PGO/ThinLTO, work-stealing — has landed pre-v1.0.
The next reach is into v1.1+ territory: cluster placement policies,
multi-agent swarm consensus primitives, MCP federation, BOLT
post-link optimisation.

Per-version detail lives in `CHANGELOG.md` and
`dev/history/releases/RELEASE-v0.X.md`. This README tracks the
current state of the language, not its history.

## Status

Mighty is **pre-alpha**. Internal milestones tagged through v0.30.
The toolchain is exercised by **2502 Rust tests** across 20 crates
plus **490 Python 2nd-impl tests** plus **159 normative conformance
cases** plus **23 self-host driver codegen tests** — combined
**3174 tests, 0 failing**. All four LLM providers are full;
`std.swarm` votes consensus across them under a shared dollar
budget; `std.eval` regression-tests agents against provider panels
under byte-identical replay. All 9 demos pass `smoke.sh`; 3 web
demos opt-in to a headless-browser visual smoke; 2 agent demos
opt-in to a mock-LLM end-to-end smoke. All KNOWN_ISSUES P1 are closed; P2 holds one open
entry (#9 — demo 06 RAF-mid-frame phash flake; 4-of-5 success, no
required-gate impact). Six CI jobs are
required gates: `test` (cross-OS matrix), `test-minimal`, `msrv`,
`clippy-strict` (pedantic + `-D warnings`), `bench`, `security`
(`cargo audit --deny warnings`). Coverage at 63% direct / 99%
any-harness (only `MT3012 DROP_IN_CONST_CONTEXT` uncovered, pending
HIR `CONST_DECL` lowering).

**There is no released GA binary yet.** Pre-built tagged binaries
ship on every release; treat the language as unstable; please file
issues for everything that surprises you.

## Contributing

Issues welcome. PRs must rebase on `main`, include tests, and pass
`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
-- -D warnings`, and `cargo test --workspace`. See
[`CONTRIBUTING.md`](CONTRIBUTING.md) for the short form and
[`docs/contributing.md`](docs/contributing.md) for the full
workflow. Community standards live in
[`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).

## Community

- Bug reports, feature requests, and language questions in
  [GitHub Issues](https://github.com/hassard0/Mighty/issues).
- Open-ended design discussions in
  [GitHub Discussions](https://github.com/hassard0/Mighty/discussions).
- Active **v1.0 RFC comment windows** — feedback welcome on any of
  the 8 RFC threads.

## License

MIT — see [LICENSE](LICENSE).

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this work shall be MIT-licensed as above,
without any additional terms or conditions.
