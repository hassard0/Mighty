# Stardust Slice 1 — Complete

**Tag:** `v0.1.0-phase1`
**HEAD:** `308feb4238e81b5d91665e6ee4fb2ad06ddbd6c2`
**Date:** 2026-05-24

## What landed

- **Lexer** (`sdust-syntax`): logos-based with duration/size/typed-numeric literal support, 56 keywords, full punctuation.
- **CST** (`sdust-syntax`): rowan-based lossless concrete syntax tree.
- **Parser** (`sdust-syntax`): hand-rolled recursive descent with Pratt expression precedence. Full surface syntax: items, types, patterns, expressions, agents, protocols, supervisors, budgets, sandboxes, arenas, task scopes, extern blocks, macros, unsafe blocks.
- **AST view** (`sdust-ast`): 50 typed accessor structs over the rowan CST.
- **Diagnostics** (`sdust-diagnostics`): ariadne-rendered, span-tracked, SD-coded.
- **HIR** (`sdust-hir`): name-resolved arena-allocated IR with desugarings (T!E -> Result, expression-body fns, compact agent/protocol forms).
- **Formatter** (`sdust-fmt`): Wadler/Lindig pretty-printer combinators + identity-passthrough mode (real per-node formatting deferred to slice 2).
- **Driver** (`sdust-driver`): compilation pipeline + star.toml manifest loader.
- **CLI** (`sdust-cli`): `sdust new | fmt | check | dump` user-facing commands.
- **Examples**: 20 canonical .sd programs that parse, fmt-roundtrip, and HIR-lower cleanly.
- **Conformance scaffold**: spec §37 directory layout with placeholders for slices 2-6.

## Stats

- 132 tests pass
- 32 commits
- 217 files
- ~7.0k lines of Rust

## Deferred to slice 2

- Real per-node formatter (currently identity-passthrough)
- Type checker
- Borrow / ownership / affine checking
- Effect / capability checking
- Lambda expressions (LAMBDA_EXPR kind exists, no parser production)
- `if let` patterns
- Generic args in expression position (turbofish)
- Keyword-tolerant method/field names (e.g. `.on(...)`)
- Lexer support for `1k`/`1m` size suffixes
- Sandbox body `run <expr>` keyword form

## Known parser-spec divergences

Examples 19 and 20 contain inline `// Note:` comments documenting where they diverged from the source spec (`stardust_language_spec_v0_1.md` §34, §35) to fit the slice-1 grammar. All divergences are slice-2 work.
