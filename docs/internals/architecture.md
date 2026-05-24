# Architecture

This page sketches the compilation pipeline as it stands at slice 1.

## Pipeline

```
            source bytes (.sd)
                  |
                  v
        +-------------------+      sdust-syntax::lex
        |       lexer       |      logos-based
        +-------------------+
                  |  Vec<LexedToken>
                  v
        +-------------------+      sdust-syntax::parser
        |      parser       |      hand-rolled RD + Pratt
        +-------------------+
                  |  GreenNode (rowan)
                  v
        +-------------------+      sdust-syntax::SyntaxNode
        |       CST         |      lossless concrete tree
        +-------------------+
                  |
                  +--------------+
                  |              v
                  |     +-----------------+      sdust-ast
                  |     |  typed AST view |      cast wrappers
                  |     +-----------------+
                  |              |
                  v              v
        +-------------------+   +-------------------+
        |    formatter      |   |   HIR lowering    |   sdust-hir
        |  (Wadler/Lindig)  |   |    (LoweringCtx)  |
        +-------------------+   +-------------------+
                  |                       |
                  v                       v
              fmt output            Package + Vec<Diagnostic>
```

`sdust-driver` chains parser → AST cast → HIR lowering and surfaces the
combined diagnostics. `sdust-cli` is the user-facing entry point.

## Future stages

The full spec pipeline (spec §24.1) adds:

```
        HIR -> type/effect/capability check
            -> ownership/borrow check
            -> AIR (agent IR)
            -> SIR (Stardust mid-level IR)
            -> optimization
            -> LLVM IR    (native release)
            -> Cranelift  (debug/JIT)
            -> Wasm component (web/edge/plugin)
```

None of these stages exist yet. They map to slices 3–8 of the
[roadmap](../../README.md#roadmap).

## Crate dependency graph

```
sdust-cli
   |
   v
sdust-driver
   |
   +--> sdust-syntax
   +--> sdust-ast --> sdust-syntax
   +--> sdust-hir --> sdust-ast, sdust-diagnostics
   +--> sdust-fmt --> sdust-syntax
   +--> sdust-diagnostics
```

No cycles. `sdust-syntax` is the foundation; everything else depends on
it directly or transitively.

## Errors

Every stage produces typed errors that bubble up as
[`Diagnostic`](../../crates/sdust-diagnostics/src/diagnostic.rs) values
with stable [SD codes](../reference/diagnostics.md). The CLI renders
them through ariadne; library callers can render or process them as
they wish.

## Determinism

The compiler is deterministic. Given the same source, the lexer and
parser produce the same `GreenNode`, the lowerer produces the same
HIR with the same arena indices, the formatter produces the same
bytes, and the snapshot tests round-trip every example identically
across runs.
