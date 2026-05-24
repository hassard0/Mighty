# Stardust

[![Status](https://img.shields.io/badge/status-v0.3-green)](https://github.com/hassard0/stardust/releases/tag/v0.3.0)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue)](#license)

Stardust is an agent-first systems programming language. It is statically
typed, ownership-based, and treats agents, protocols, capabilities, effects,
arenas, and budgets as first-class concepts. The toolchain targets both
native code (Cranelift JIT + AOT; LLVM behind `--features llvm`) and
WebAssembly (Component Model by default; bare core modules via
`--no-component`).

**v0.3 is shipped.** The v0.3 milestone tag
[`v0.3.0`](https://github.com/hassard0/stardust/releases/tag/v0.3.0)
hardens soundness across the borrow checker (NLL last-use + field-level
Places), the type checker (scope-aware tolerance + Sendable trait), and
the runtime (cooperative mid-turn cancellation + OTLP telemetry +
slab-pool mailboxes), and closes the v0.2 cleanup backlog (stdlib host
install, 6/20→20/20 wasm Components, 5→2 ignored conformance cases).
See [`RELEASE-v0.3.md`](RELEASE-v0.3.md) for the headline numbers and
[`SLICE_V0_3.md`](SLICE_V0_3.md) for the shipped/deferred detail. The
v0.2 milestone remains tagged
[`v0.2.0`](https://github.com/hassard0/stardust/releases/tag/v0.2.0);
the v0.1 milestone at
[`v0.1.0`](https://github.com/hassard0/stardust/releases/tag/v0.1.0).

### v0.3 highlights

- **NLL last-use + field-level Places** — the borrow checker now
  deactivates borrows at their last use and tracks disjoint fields
  separately (A54/A55/A56)
- **Scope-aware strict tolerance** — agent/handler/supervisor bodies
  hard-error unresolved names with SD2021 (A65); permissive scopes
  keep the slice-3 fresh-var fallback
- **Sendable trait** — formal cross-agent message-arg contract (Copy
  ∨ owned-Sized-no-refs ∨ `derive(Sendable)`); SD3011 at every
  `!Msg(...)` / `?Msg(...)` site (A65.b)
- **Cooperative mid-turn cancellation** — per-turn deadlines now
  interrupt blocking handlers (A70); closes A41
- **OTLP wire-format telemetry** — `STARDUST_OTLP_ENDPOINT` routes
  spans/metrics to any collector via tonic-gRPC (A71); closes A38
- **Slab-pool mailbox frames** — per-mailbox `SlabPool` reuses
  pre-allocated `MessageFrame` slots (A72); closes A40
- **Stdlib really runs under `sdust run`** — driver wired to
  `sdust_stdlib::host::dispatch` via CLI bridge
- **20/20 wasm Components** (was 14/20 in v0.2)
- **623 tests pass** (+73 over v0.2), 0 clippy warnings

### v0.2 highlights

- **`sdust lsp`** — LSP 3.17 server (diagnostics, hover, completion,
  go-to-def) plus a VS Code extension scaffold
- **`sdust pkg`** — package manager (resolver + lockfile + path/git
  fetchers + publisher); CLI `add` / `remove` / `update` / `fetch` /
  `list` / `publish`
- **`sdust doc`** — doc generator producing markdown or HTML with an
  item index, per-item pages, back-links, and a search index
- **20/20 native + 20/20 wasm core-module compilation** across the
  example corpus (Cranelift + wasm backend now cover ADT,
  `?`-propagation, agent handlers, monomorphization)
- **Real stdlib** (`std.json`, `std.tls`, `std.http`, `std.fs`,
  `std.time`, `std.test`) backed by `rustls`, `hyper`, `serde_json`,
  `tokio`
- **DWARF v4 debug info** (Cranelift) + wasm `name` section +
  source-map v3 sidecar
- **Wasm Component Model output by default** (`wit-component`); use
  `--no-component` for a bare core module

```bash
# Compile and JIT-run
sdust run examples/01_hello.sd
# → hello, Stardust

# Build a native executable (linker-permitting)
sdust build examples/01_hello.sd
# wrote target/01_hello

# Build a WebAssembly module
sdust build --target wasm32-wasi examples/01_hello.sd
# wrote target/01_hello.wasm
```

The CLI ships `sdust new`, `sdust check`, `sdust fmt`, `sdust dump`,
`sdust run`, `sdust build`, and `sdust explain`. Runtime diagnostics
range from `SD0001` (parse errors) through `SD8010` (codegen traps);
`sdust explain SDxxxx` prints a paragraph describing each.

## Install

A versioned release is not yet published. Build from source:

```bash
git clone https://github.com/hassard0/stardust
cd stardust
cargo install --path crates/sdust-cli
```

This installs the `sdust` binary. The minimum supported Rust version is
1.85 (slice 8 bumped from 1.82 because the cranelift dependency chain
pulls in `indexmap 2.14`, which requires edition2024).

## Hello, Stardust

```bash
sdust new hello
cd hello
sdust check src/main.sd
```

`sdust new` produces:

```sd
fn main() {
  log("hello, Stardust")
}
```

`sdust check` lexes, parses, lowers, type-checks, and borrow-checks
the source, reporting any diagnostics. `sdust run src/main.sd`
executes the program under the slice-6 interpreter. `sdust explain
SDxxxx` prints a paragraph describing any diagnostic code emitted.

## Documentation

- [Getting started](docs/getting-started.md)
- [Tour](docs/tour/README.md) — walk through the twenty canonical examples
- [Language specification v0.1](docs/spec/v0.1.md)
- [Reference](docs/reference/README.md) — CLI, manifest, diagnostics
- [Internals](docs/internals/README.md) — compiler architecture
- [FAQ](docs/faq.md)
- [Contributing](docs/contributing.md)

## Project layout

The compiler is a Rust workspace of nineteen crates:

| Crate | Responsibility |
|---|---|
| `sdust-syntax` | lexer (logos), CST (rowan), parser |
| `sdust-ast` | typed AST view over the CST |
| `sdust-diagnostics` | diagnostic types, SD-coded labels, ariadne rendering |
| `sdust-hir` | name-resolved HIR with arena storage |
| `sdust-types` | resolved Ty, HM inference, bidirectional type checker, effects + capabilities; v0.3 scope-strict + Sendable |
| `sdust-borrow` | ownership/move/borrow/affine/arena analysis; v0.3 field-level Places + NLL last-use |
| `sdust-sir` | mid-level IR + tree-walking interpreter (slice 6) |
| `sdust-runtime` | concurrent tokio runtime: agents, mailboxes, supervisors, budgets (slice 7); v0.3 mid-turn cancel + OTLP + slab pool |
| `sdust-codegen-cranelift` | native backend — JIT + AOT object (slice 8 + v0.2 completion) |
| `sdust-codegen-wasm` | wasm32-wasi / wasm32-web core module + Component Model emitter |
| `sdust-codegen-llvm` | LLVM backend (real lowering behind `--features llvm`; v0.2) |
| `sdust-debuginfo` | DWARF v4 builder + wasm source-map + `name` section (v0.2) |
| `sdust-fmt` | canonical formatter (Wadler/Lindig pretty-printer) |
| `sdust-driver` | compilation pipeline and `star.toml` manifest loader |
| `sdust-pkg` | package manager: resolver, lockfile, fetchers, publish (v0.2) |
| `sdust-lsp` | LSP 3.17 server over stdio (v0.2) |
| `sdust-doc` | doc generator (extract + render markdown/HTML) (v0.2) |
| `sdust-stdlib` | real `std.json` / `tls` / `http` / `fs` / `time` / `test` (v0.2) |
| `sdust-cli` | the `sdust` binary |

## Roadmap

The full plan is in `stardust_language_spec_v0_1.md` §31. The slices
implemented or planned:

| Slice | Scope | Status |
|---|---|---|
| 1 | parser, formatter, HIR, CLI, examples | shipped (`v0.1.0-phase1`) |
| 2 | per-node formatter, lambdas, if-let, turbofish, polish | shipped (`v0.2.0-phase1-polish`) |
| 3 | type checker, generics MVP, `?` propagation | shipped (`v0.3.0-typeck`) |
| 4 | ownership / borrow / affine / arena + slice-3 hardening | shipped (`v0.4.0-borrowck`) |
| 5 | effects, capabilities, traits, `dyn`, derives, strict protocols | shipped (`v0.5.0-effects`) |
| 6 | SIR and interpreter | shipped (`v0.6.0-sir`) |
| 7 | runtime MVP (scheduler, mailboxes, supervisors) | shipped (`v0.7.0-runtime`) |
| 8 | native (Cranelift) and Wasm backends | shipped (`v0.8.0-codegen` / `v0.1.0`) |
| **v0.2** | LSP + pkg + doc + full codegen + stdlib + DWARF + Wasm CM | **shipped (`v0.2.0`)** |
| **v0.3** | Soundness hardening: NLL last-use + field Places, scope-strict + Sendable, mid-turn cancel + OTLP + slab mailboxes, v0.2 cleanup (stdlib install, 20/20 wasm-CM, 5→2 ignored) | **shipped (`v0.3.0`)** |

### Post-v0.3 roadmap

| Slice | Scope | Status |
|---|---|---|
| v0.4 | Polonius-style borrows, real cap-name resolution wiring, SIR-side cancellation polling, WASI Preview 2 + user WIT, DWARF v5 + per-instr line program, backtracking pkg resolver | planned |
| - | `dyn` dispatch + closure capture in compiled code, real `loop { break }` lowering, `escalate` supervisor action | planned |
| - | Multi-core scheduler, PGO/ThinLTO, distributed agents, procedural macros, effect-row polymorphism | future |

## License

Stardust is dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this work shall be dual-licensed as above,
without any additional terms or conditions.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) and
[docs/contributing.md](docs/contributing.md).
