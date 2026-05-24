# Mighty Slice 1 — Complete

**Tag:** `v0.1.0-phase1`
**HEAD:** `308feb4238e81b5d91665e6ee4fb2ad06ddbd6c2`
**Date:** 2026-05-24

## What landed

- **Lexer** (`mty-syntax`): logos-based with duration/size/typed-numeric literal support, 56 keywords, full punctuation.
- **CST** (`mty-syntax`): rowan-based lossless concrete syntax tree.
- **Parser** (`mty-syntax`): hand-rolled recursive descent with Pratt expression precedence. Full surface syntax: items, types, patterns, expressions, agents, protocols, supervisors, budgets, sandboxes, arenas, task scopes, extern blocks, macros, unsafe blocks.
- **AST view** (`mty-ast`): 50 typed accessor structs over the rowan CST.
- **Diagnostics** (`mty-diagnostics`): ariadne-rendered, span-tracked, SD-coded.
- **HIR** (`mty-hir`): name-resolved arena-allocated IR with desugarings (T!E -> Result, expression-body fns, compact agent/protocol forms).
- **Formatter** (`mty-fmt`): Wadler/Lindig pretty-printer combinators + identity-passthrough mode (real per-node formatting deferred to slice 2).
- **Driver** (`mty-driver`): compilation pipeline + mighty.toml manifest loader.
- **CLI** (`mty-cli`): `mty new | fmt | check | dump` user-facing commands.
- **Examples**: 20 canonical .sd programs that parse, fmt-roundtrip, and HIR-lower cleanly.
- **Conformance scaffold**: spec §37 directory layout with placeholders for slices 2-6.

## Stats

- 132 tests pass
- 32 commits
- 217 files
- ~7.0k lines of Rust

## Deferred to slice 2 — closed by v0.2.0-phase1-polish

The following slice-1 deferrals shipped in slice 2:

- Real per-node formatter (Wadler/Lindig)
- Lambda expressions
- `if let` patterns
- Generic args in expression position (turbofish `::[T]`)
- Keyword-tolerant method/field names (e.g. `dom.on(...)`)
- Keyword-tolerant effect names (e.g. `effect spawn`)
- Lexer support for decimal size suffixes (`1k`, `2M`)
- Sandbox body `run <expr>` keyword form

## Still deferred to slice 3+

- Type checker (slice 3)
- Borrow / ownership / affine checking (slice 3)
- Effect / capability checking (slice 3)
- Top-level `sandbox` items per spec §16.1 (slice 3)

## Known parser-spec divergences — resolved

The slice-1 `// Note:` divergence comments in examples 19 and 20 are
removed in slice 2. Example 18 keeps the fn-wrapper around the sandbox
(top-level sandbox items are slice 3).
