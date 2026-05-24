# Stardust

[![Status](https://img.shields.io/badge/status-pre--alpha-orange)](https://github.com/hassard0/stardust)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue)](#license)

Stardust is an agent-first systems programming language. It is statically
typed, ownership-based, and treats agents, protocols, capabilities, effects,
arenas, and budgets as first-class concepts. The toolchain targets both
native code (via LLVM) and the WebAssembly Component Model.

The language is at the **pre-alpha** stage. Slice 2 —
formatter completion and surface-syntax polish — is tagged
[`v0.2.0-phase1-polish`](https://github.com/hassard0/stardust/releases/tag/v0.2.0-phase1-polish).
Lambdas, `if let`, turbofish, decimal size suffixes, keyword-tolerant
method/field/effect names, `run <expr>` in sandbox bodies, the real
per-node formatter, and `sdust explain` all ship in slice 2. The type
checker, ownership checking, codegen, and runtime are not yet
implemented.

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

`sdust check` parses and lowers the source to HIR, reporting any
diagnostics. As of slice 2, `check` does not yet type-check; it
verifies that the program is syntactically valid and lowers cleanly.
`sdust explain SDxxxx` prints a paragraph describing any diagnostic
code emitted.

## Documentation

- [Getting started](docs/getting-started.md)
- [Tour](docs/tour/README.md) — walk through the twenty canonical examples
- [Language specification v0.1](docs/spec/v0.1.md)
- [Reference](docs/reference/README.md) — CLI, manifest, diagnostics
- [Internals](docs/internals/README.md) — compiler architecture
- [FAQ](docs/faq.md)
- [Contributing](docs/contributing.md)

## Project layout

The compiler is a Rust workspace of seven crates:

| Crate | Responsibility |
|---|---|
| `sdust-syntax` | lexer (logos), CST (rowan), parser |
| `sdust-ast` | typed AST view over the CST |
| `sdust-diagnostics` | diagnostic types, SD-coded labels, ariadne rendering |
| `sdust-hir` | name-resolved HIR with arena storage |
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
| 3 | type checker, generics MVP | next |
| 4 | ownership and borrow checker MVP | planned |
| 5 | effects and capabilities | planned |
| 6 | SIR and interpreter | planned |
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
