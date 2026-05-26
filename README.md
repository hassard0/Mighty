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
> 977 Rust tests across 20 crates plus a second independent Python
> front-end at [`impl-py/`](impl-py/) (135 tests) and a third
> source-only Go front-end at [`impl-go/`](impl-go/) (4848 LOC,
> cross-validation pending Go toolchain). All six CI jobs are
> required gates. A `1.0` GA tag still awaits the completion of the
> 2nd-impl through type-check, the 3rd-impl cross-validation, six
> RFC comment-window closures, and the normative conformance suite
> (currently 89 cases / 16 categories). See [Status](#status) below.

## Install

A versioned release is not yet published. Build the toolchain from source:

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

**Codegen**

- Cranelift JIT + AOT object emission (default)
- LLVM backend (`--features llvm`)
- Wasm core module + Component Model (`wit-component`) emission
- DWARF v4 debug info + Wasm source maps + `name` section

**Tooling**

- `mty lsp` — LSP 3.17 server (diagnostics, hover, completion, go-to-def, semantic tokens, rename, inlay hints, code actions, signature help, workspace folders)
- `mty pkg` — package manager: resolver, lockfile, GitHub-Releases-backed registry, `.tar.gz` bundles, signed sidecars
- `mty doc` — markdown / HTML doc generator with search index
- `mty fmt` — canonical formatter (idempotent under fuzz)
- Stdlib: `std.json`, `std.tls`, `std.http`, `std.fs`, `std.time`, `std.test`, `std.io` — backed by `rustls` / `hyper` / `serde_json` / `tokio`
- Diagnostics: `MT0001`–`MT8010`, each with `mty explain`

**Self-hosting (in progress)**

- Lexer (full), parser (~1.9 KLOC subset), HIR lowering, minimal typeck, and MtyIR lowering are all written in Mighty itself and exercised against examples 01-05.
- 40 self-host tests passing.

**Independent implementations**

- Rust reference compiler (this repo, `crates/mty-*`).
- Python 2nd-impl front-end at [`impl-py/`](impl-py/) — pure-Python lexer + parser built from the v1.0-RC2 spec prose alone. 135 tests, 20/20 examples lex+parse.
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
- WASI Preview 2 + user-supplied WIT; DWARF v5 + per-instruction line program.
- Effect-row polymorphism.
- Distributed agents; PGO / ThinLTO.

For the full per-version history of what shipped on the road to v1.0,
see [`CHANGELOG.md`](CHANGELOG.md).

## Status

Mighty is **pre-alpha**. Internal milestones have been tagged through
v0.12. The v1.0 language spec is at v1.0-RC3 — see
`docs/spec/v1.0-rc.md`. There are 977 Rust tests across the workspace
(plus 135 Python tests in the [`impl-py/`](impl-py/) 2nd-impl, 89
normative conformance cases, and 40 self-host tests = **1241
combined**), 0 clippy warnings *under the strict `pedantic` gate*
(a required CI job, not advisory), and **4/4 demos** pass `smoke.sh`.
The cargo-fuzz harness covers four targets (parser / typeck / fmt /
codegen), and the normative conformance corpus stands at **89 cases
across 16 categories** (3 ignored: 2 carried over from v0.11, 1 new
red-shirt deferred to v0.13).

**There is no released binary yet.** Build from source, treat the
language as unstable, and please file issues for everything that
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
