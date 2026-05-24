# Stardust

An agent-first systems programming language. Statically typed, ownership-based, with first-class agents, protocols, capabilities, effects, arenas, and budgets. Compiles to native (LLVM) and WebAssembly components.

See `C:\Users\ihass\Downloads\stardust_language_spec_v0_1.md` for the language specification.

## Status

Pre-alpha. Currently building Phase 1 (lexer, parser, formatter, HIR) per `docs/superpowers/specs/2026-05-23-phase1-parser-fmt-hir-design.md`.

## Build

```
cargo build --workspace
cargo test --workspace
```

## CLI

```
sdust new <name>      # scaffold a new package
sdust fmt [paths...]  # canonical formatter
sdust check [path]    # parse + HIR-lower; emit diagnostics
sdust dump --ast      # AST dump
sdust dump --cst      # CST debug dump
sdust dump --hir      # HIR S-expression dump
```

## License

Dual-licensed under Apache-2.0 and MIT.
