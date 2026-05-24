# Stardust

[![Status](https://img.shields.io/badge/status-pre--alpha-orange)](https://github.com/hassard0/stardust)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue)](#license)

Stardust is an agent-first systems programming language. It is statically
typed, ownership-based, and treats agents, protocols, capabilities, effects,
arenas, and budgets as first-class concepts. The toolchain targets both
native code (via LLVM) and the WebAssembly Component Model.

The language is at the **pre-alpha** stage. Slice 5 — effects,
capabilities, traits, `dyn`, derives, top-level sandboxes, and strict
protocol checks — is tagged
[`v0.5.0-effects`](https://github.com/hassard0/stardust/releases/tag/v0.5.0-effects).
Slice 5 adds bottom-up effect inference with public-fn discipline,
narrowable capability typing (Net/Fs/Clock/Dom/Model), real trait
coherence + dispatch with `SD4020 method_ambiguous` /
`SD4021 method_not_found` / `SD4022 trait_coherence_violation`,
conservative `dyn Trait` object safety (`SD4023`), `#[derive(Copy,
Hash, Eq)]`, top-level `sandbox` items, and strict protocol-handler
arity / coverage checks (SD4030/32/33). Codegen and runtime are not
yet implemented.

## Install

A versioned release is not yet published. Build from source:

```bash
git clone https://github.com/hassard0/stardust
cd stardust
cargo install --path crates/sdust-cli
```

This installs the `sdust` binary. The minimum supported Rust version is
1.82.

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

`sdust check` lexes, parses, lowers, and type-checks the source,
reporting any diagnostics. `sdust explain SDxxxx` prints a paragraph
describing any diagnostic code emitted.

## Documentation

- [Getting started](docs/getting-started.md)
- [Tour](docs/tour/README.md) — walk through the twenty canonical examples
- [Language specification v0.1](docs/spec/v0.1.md)
- [Reference](docs/reference/README.md) — CLI, manifest, diagnostics
- [Internals](docs/internals/README.md) — compiler architecture
- [FAQ](docs/faq.md)
- [Contributing](docs/contributing.md)

## Project layout

The compiler is a Rust workspace of nine crates:

| Crate | Responsibility |
|---|---|
| `sdust-syntax` | lexer (logos), CST (rowan), parser |
| `sdust-ast` | typed AST view over the CST |
| `sdust-diagnostics` | diagnostic types, SD-coded labels, ariadne rendering |
| `sdust-hir` | name-resolved HIR with arena storage |
| `sdust-types` | resolved Ty, HM inference, bidirectional type checker |
| `sdust-borrow` | ownership/move/borrow/affine/arena analysis (slice 4) |
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
| 6 | SIR and interpreter | next |
| 7 | runtime MVP (scheduler, mailboxes, supervisors) | planned |
| 8 | native (LLVM) and Wasm backends | planned |

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
