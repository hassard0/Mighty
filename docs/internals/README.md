# Compiler internals

The Stardust compiler is a Rust workspace of seven crates. This section
describes the pipeline and the responsibilities of each crate.

If you are looking to use the compiler, see the
[reference](../reference/README.md). If you are looking to learn the
language, see the [tour](../tour/README.md).

## Pages

- [Architecture](architecture.md) — pipeline diagram and crate map.
- [Lexer](lexer.md) — the logos-based tokenizer.
- [Parser](parser.md) — recursive descent + Pratt over a rowan builder.
- [CST and AST](cst-ast.md) — the lossless tree and its typed view.
- [HIR](hir.md) — name-resolved IR with arena storage.
- [Diagnostics](diagnostics.md) — types, codes, rendering.
- [Formatter](formatter.md) — Wadler/Lindig pretty-printer.

## Where to find things

| Concern | Crate | Entry point |
|---|---|---|
| Token kinds, lexer regex | `sdust-syntax` | `src/syntax_kind.rs`, `src/lexer.rs` |
| Parser productions | `sdust-syntax` | `src/parser/` |
| AST accessors | `sdust-ast` | `src/generated.rs` |
| Diagnostic types | `sdust-diagnostics` | `src/diagnostic.rs`, `src/codes.rs` |
| HIR nodes and ids | `sdust-hir` | `src/nodes.rs`, `src/ids.rs` |
| HIR lowering | `sdust-hir` | `src/lower/` |
| Formatter combinators | `sdust-fmt` | `src/doc.rs`, `src/printer.rs` |
| Per-node format rules | `sdust-fmt` | `src/fmt/` |
| Compilation pipeline | `sdust-driver` | `src/pipeline.rs` |
| Manifest loader | `sdust-driver` | `src/manifest.rs` |
| CLI dispatch | `sdust-cli` | `src/main.rs`, `src/cmd/` |

## Slice 1 stats

From [SLICE1.md](../../SLICE1.md):

- 132 tests pass.
- 32 commits, 217 files, ~7.0k lines of Rust.
- 20 canonical example programs that parse, fmt-roundtrip, and HIR-lower
  cleanly.
- Conformance suite scaffolded per spec §37.
