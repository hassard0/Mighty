# Stardust Documentation

Stardust is a statically typed, ownership-based, agent-first systems
language that compiles to native code and to WebAssembly components. These
docs cover the language as it stands at the **slice 1** milestone
(`v0.1.0-phase1`): lexer, parser, formatter, HIR, CLI, and twenty
canonical example programs.

The type checker, borrow checker, codegen, and runtime are not yet
implemented. Where a section describes a feature that is specified but not
yet enforced, it is marked **(spec only)**.

## Learn the language

- [Getting started](getting-started.md) — install, scaffold a package,
  run `sdust check`.
- [Tour of Stardust](tour/README.md) — work through the canonical examples
  one chapter at a time.
- [Language specification v0.1](spec/v0.1.md) — the normative reference.

## Use the tools

- [Reference](reference/README.md)
  - [CLI](reference/cli/sdust.md) — `sdust new`, `fmt`, `check`, `dump`.
  - [Manifest format](reference/manifest.md) — the `star.toml` schema.
  - [Diagnostic codes](reference/diagnostics.md) — the `SDxxxx` registry.

## Hack on the compiler

- [Internals](internals/README.md) — pipeline overview, per-crate notes.
- [Contributing](contributing.md) — workflow, tests, style.
- [FAQ](faq.md)

## Status snapshot

| Component | State |
|---|---|
| Lexer, parser, CST | shipped |
| Typed AST view | shipped |
| HIR + lowering | shipped |
| Diagnostics engine | shipped |
| Formatter — combinators | shipped |
| Formatter — per-node rules | identity-passthrough (slice 2) |
| Type checker | not started |
| Borrow / move / affine checker | not started |
| Effect / capability checker | not started |
| Codegen (LLVM, Cranelift, Wasm) | not started |
| Runtime (scheduler, mailboxes, supervisors) | not started |

See [SLICE1.md](../SLICE1.md) for the exact shipping scope and
[../README.md](../README.md) for the slice roadmap.
