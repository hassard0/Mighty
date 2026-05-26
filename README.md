# Mighty

[![Status](https://img.shields.io/badge/status-pre--alpha-orange)](#status)
[![Spec](https://img.shields.io/badge/spec-v1.0--RC3-blueviolet)](docs/spec/v1.0-rc.md)
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

> **Status:** pre-alpha. The v1.0 language spec is at **v1.0-RC3**
> (operator precedence promoted to normative §11.1.1; full
> 63-reserved-keyword set enumerated). The toolchain is exercised by
> 1217 Rust tests across 20 crates plus a second independent Python
> front-end at [`impl-py/`](impl-py/) (139 tests) and a third
> source-only Go front-end at [`impl-go/`](impl-go/) (4848 LOC,
> cross-validation pending Go toolchain). All six CI jobs are
> required gates. **End-to-end self-hosting** is complete for the
> slice-1 subset (lexer → parser → HIR → typeck → MtyIR → wasm
> codegen, all in Mighty); the codegen now lowers `Rvalue::MethodCall`
> through a host-bridged dispatch and desugars `for x in custom_iter`
> into the iter-protocol shape (23 live driver tests, was 17),
> and examples 01-03 all bootstrap through the self-host chain. A
> `1.0` GA tag still awaits the completion of the 2nd-impl through
> type-check, the 3rd-impl cross-validation, eight RFC comment-window
> closures (RFC-001..006 plus the v0.13 RFC-008 + RFC-009 drafts),
> and the normative conformance suite (currently 92 cases / 16
> categories). See [Status](#status) below.

## Install

Pre-built `mty` binaries for Linux x86_64, macOS x86_64 + arm64,
and Windows x86_64 are attached to each tagged release on the
[GitHub Releases page](https://github.com/hassard0/Mighty/releases)
(produced automatically by `.github/workflows/release.yml` on
`v*` tag push, starting with v0.15.0).

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

**Codegen**

- Cranelift JIT + AOT object emission (default)
- LLVM backend (`--features llvm`)
- WASI Preview 2 with embedded preview1-adapter (vendored upstream
  wasmtime v32 adapter; **default for `wasm32-wasi`** — explicit
  `--wasi=p1` opts back to v0.13/v0.14 behaviour). Direct P2 imports
  for `std.random.bytes` + `std.time.*` (v0.15) **and** for the full
  `std.fs.*` (`open` / `read_file` / `write_file` / `stat` / `close`)
  + `std.http.*` (`get` / `post` / `send` / `incoming_request_consume`)
  surfaces (v0.16); only the `log()` shim still flows through the
  adapter (deferred to v0.17)
- Wasm Component Model (`wit-component`) emission with user-supplied
  WIT via `[wit]` in `mighty.toml`
- DWARF v4 debug info + Wasm source maps + `name` section

**Tooling**

- `mty lsp` — LSP 3.17 server (diagnostics, hover, completion, go-to-def, semantic tokens, rename, inlay hints, code actions, signature help, workspace folders)
- `mty pkg` — package manager: resolver, lockfile, GitHub-Releases-backed registry, `.tar.gz` bundles, signed sidecars
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
- Python 2nd-impl front-end at [`impl-py/`](impl-py/) — pure-Python lexer + parser built from the v1.0-RC2 spec prose alone. 139 tests, 22/22 examples lex+parse.
- Go 3rd-impl front-end at [`impl-go/`](impl-go/) — Go 1.22+ lexer + parser + CLI built from the v1.0-RC3 spec prose alone. 4848 LOC; cross-validation (`go test ./...`, example sweep) pending Go toolchain on the build host.

## Documentation

Live docs site: **<https://hassard0.github.io/Mighty/>**

- [Getting started](docs/getting-started.md)
- [Tour](docs/tour/README.md) — walk through the canonical examples
- [Language spec v1.0-RC3](docs/spec/v1.0-rc.md) (frozen for v1.0)
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

The v1.0 spec is feature-complete at v1.0-RC2 (`docs/spec/v1.0-rc.md`).
**Proposed freeze date: 2026-09-01.** Blockers:

1. A second independent compiler implementation (RFC-007).
2. The six RFC 30-day comment windows (RFC-001 .. RFC-006).
3. A published normative conformance suite.

### Post-v1.0

- Lossless live agent migration; per-message work-stealing.
- Polonius-style borrows; real cap-name resolution wiring.
- DWARF v5 + per-instruction line program.
- Distributed agents; PGO / ThinLTO.

### Landed pre-v1.0 (formerly post-v1.0)

- WASI Preview 2 + user-supplied WIT — v0.13 (`--wasi=p2`, `--world`, `[wit]` section). v0.14 embeds the upstream wasmtime preview1→preview2 adapter and ships direct P2 imports for `std.random` / `std.time`. v0.15 wires `P2DirectImport` into `emit.rs` dispatch and **flips the toolchain default to P2 for `wasm32-wasi`** (explicit `--wasi=p1` opts back). v0.16 takes nine more lowerings direct (full `std.fs.*` + `std.http.*`); only the `log()` shim still routes through the embedded adapter.
- Effect-row polymorphism end-to-end — v0.13 (RFC-008, `mty-types::effects::row`). v0.14 ships 19 row-polymorphic stdlib signatures; v0.15 wires call-site dispatch (`BuiltinMethod.row_sig` + `walk_expr_effects`, MT4050 on closed-row rejection) and lands the surface syntax (`!E` / `!{a | E}` / `effect a | E`). **v0.16 wires the surface syntax through typed AST → `HirEffectRow` → `UserRowPolyIndex` typeck**, with five new diagnostic codes (MT4055–MT4059, MT4057 actively emits); user-authored row variables now typecheck and `examples/22_effect_row.mty` is live in the example sweep.
- Set-of-scopes macro hygiene — v0.13 (RFC-009); v0.14 wires HIR macro resolution to `expand_scoped_to_source`; v0.15 removes the deprecated `mty_macros::expand` / `expand_to_source` API.
- End-to-end self-hosting through Wasm codegen — v0.13; v0.14 broadens with string pool + ADT layout + pattern lowering so example 03 passes; v0.15 adds variant-call lowering, SwitchInt cascade, and for-range desugar; **v0.16 lowers `Rvalue::MethodCall` through the host bridge and desugars `for x in custom_iter` into the iter-protocol shape (23 codegen driver tests, 0 ignored).**
- Live agent introspection + OpenTelemetry — **v0.16.** `mty inspect` CLI + opt-in `MTY_RUNTIME_CONTROL_SOCK` runtime control socket exposing agent snapshots (mailbox depth, in-flight handler, budgets, last-N messages); OTel spans at every agent boundary plus `agent.event(name, &[(k, v)])` helper, lazy init from `MTY_OTLP_ENDPOINT` (cost-zero when disabled). Tiers 1.1–1.3 of `docs/internals/agent-features-roadmap.md`.

For the full per-version history of what shipped on the road to v1.0,
see [`CHANGELOG.md`](CHANGELOG.md).

## Status

Mighty is **pre-alpha**. Internal milestones have been tagged through
v0.16. The v1.0 language spec is at v1.0-RC3 — see
`docs/spec/v1.0-rc.md`. There are 1217 Rust tests across the workspace
(plus 139 Python tests in the [`impl-py/`](impl-py/) 2nd-impl, 92
normative conformance cases, and 23 self-host driver codegen tests
= **1471 combined**), 0 clippy warnings *under the strict `pedantic`
gate* (a required CI job, not advisory), and **4/4 demos** pass
`smoke.sh`. The cargo-fuzz harness covers four targets
(parser / typeck / fmt / codegen), and the normative conformance
corpus stands at **92 cases across 16 categories** (2 ignored:
long-standing `capability_checking/03_narrow_to_ro` and
`supervisor_restart/02_escalate`). v0.16 lands **production
observability** — `mty inspect` + opt-in runtime control socket plus
OpenTelemetry agent spans (Tiers 1.1–1.3 of the
[agent-features roadmap](docs/internals/agent-features-roadmap.md))
— wires the **RFC-008 effect-row surface syntax** through typed AST
+ HIR + typeck so user-authored row variables typecheck end-to-end
(MT4055–MT4059), closes the WASI P2 `std.fs` + `std.http` direct
lowering (only `log()` still adapter-routed), and lowers self-host
`Rvalue::MethodCall` + desugars `for x in custom_iter` (23 driver
codegen tests, was 17). The remaining agent-features tiers
(replay / hot-reload / distributed) and the WASI P2 `log()` finish
are tracked for v0.17.

**Pre-built `mty` binaries** for Linux x86_64, macOS x86_64 + arm64,
and Windows x86_64 are now produced automatically on every `v*` tag
push (see [Releases](https://github.com/hassard0/Mighty/releases)).
Building from source is still supported. Treat the language as
unstable and please file issues for everything that surprises you.

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
