# Stardust

[![Status](https://img.shields.io/badge/status-v0.1-green)](https://github.com/hassard0/stardust/releases/tag/v0.1.0)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue)](#license)

Stardust is an agent-first systems programming language. It is statically
typed, ownership-based, and treats agents, protocols, capabilities, effects,
arenas, and budgets as first-class concepts. The toolchain targets both
native code (via Cranelift in v0.1; LLVM scaffolded for v0.2) and
WebAssembly (core modules in v0.1; full Component Model in v0.2).

**v0.1 is shipped.** Slice 8 — native (Cranelift) and Wasm codegen — is tagged
[`v0.8.0-codegen`](https://github.com/hassard0/stardust/releases/tag/v0.8.0-codegen),
and the v0.1 milestone itself is tagged
[`v0.1.0`](https://github.com/hassard0/stardust/releases/tag/v0.1.0).
See [`RELEASE-v0.1.md`](RELEASE-v0.1.md) for the headline numbers.

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

The compiler is a Rust workspace of fourteen crates:

| Crate | Responsibility |
|---|---|
| `sdust-syntax` | lexer (logos), CST (rowan), parser |
| `sdust-ast` | typed AST view over the CST |
| `sdust-diagnostics` | diagnostic types, SD-coded labels, ariadne rendering |
| `sdust-hir` | name-resolved HIR with arena storage |
| `sdust-types` | resolved Ty, HM inference, bidirectional type checker, effects + capabilities |
| `sdust-borrow` | ownership/move/borrow/affine/arena analysis |
| `sdust-sir` | mid-level IR + tree-walking interpreter (slice 6) |
| `sdust-runtime` | concurrent tokio runtime: agents, mailboxes, supervisors, budgets (slice 7) |
| `sdust-codegen-cranelift` | native backend — JIT + AOT object (slice 8) |
| `sdust-codegen-wasm` | wasm32-wasi / wasm32-web core-module emitter (slice 8) |
| `sdust-codegen-llvm` | LLVM backend scaffold (feature-gated; v0.2 work) |
| `sdust-fmt` | canonical formatter (Wadler/Lindig pretty-printer) |
| `sdust-driver` | compilation pipeline and `star.toml` manifest loader |
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
| 8 | native (Cranelift) and Wasm backends | **shipped (`v0.8.0-codegen` / `v0.1.0`)** |

### Post-v0.1 roadmap

| Slice | Scope | Status |
|---|---|---|
| 9 | LLVM backend (real lowering, currently scaffold) | planned |
| 10 | Full SIR coverage in native codegen (ADT, `?`, agent dispatch) | planned |
| 11 | Wasm Component Model (`wit-component`) + WASI bridge runtime | planned |
| 12 | LSP server | planned |
| 13 | Package manager + registry | planned |
| - | Multi-core scheduler, PGO/ThinLTO, distributed agents | future |

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
