# Architecture

This page sketches the compilation pipeline as it stands at slice 1.

## Pipeline

```
            source bytes (.sd)
                  |
                  v
        +-------------------+      mty-syntax::lex
        |       lexer       |      logos-based
        +-------------------+
                  |  Vec<LexedToken>
                  v
        +-------------------+      mty-syntax::parser
        |      parser       |      hand-rolled RD + Pratt
        +-------------------+
                  |  GreenNode (rowan)
                  v
        +-------------------+      mty-syntax::SyntaxNode
        |       CST         |      lossless concrete tree
        +-------------------+
                  |
                  +--------------+
                  |              v
                  |     +-----------------+      mty-ast
                  |     |  typed AST view |      cast wrappers
                  |     +-----------------+
                  |              |
                  v              v
        +-------------------+   +-------------------+
        |    formatter      |   |   HIR lowering    |   mty-hir
        |  (Wadler/Lindig)  |   |    (LoweringCtx)  |
        +-------------------+   +-------------------+
                  |                       |
                  v                       v
              fmt output            Package + Vec<Diagnostic>
```

`mty-driver` chains parser → AST cast → HIR lowering and surfaces the
combined diagnostics. `mty-cli` is the user-facing entry point.

## Future stages

The full spec pipeline (spec §24.1) adds:

```
        HIR -> type/effect/capability check
            -> ownership/borrow check
            -> AIR (agent IR)
            -> MtyIR (Mighty mid-level IR)
            -> optimization
            -> LLVM IR    (native release)
            -> Cranelift  (debug/JIT)
            -> Wasm component (web/edge/plugin)
```

None of these stages exist yet. They map to slices 3–8 of the
[roadmap](https://github.com/hassard0/Mighty#roadmap).

## Crate dependency graph

```
mty-cli
   |
   v
mty-driver
   |
   +--> mty-syntax
   +--> mty-ast --> mty-syntax
   +--> mty-hir --> mty-ast, mty-diagnostics
   +--> mty-fmt --> mty-syntax
   +--> mty-diagnostics
```

No cycles. `mty-syntax` is the foundation; everything else depends on
it directly or transitively.

## Errors

Every stage produces typed errors that bubble up as
[`Diagnostic`](https://github.com/hassard0/Mighty/blob/main/crates/mty-diagnostics/src/diagnostic.rs) values
with stable [SD codes](../reference/diagnostics.md). The CLI renders
them through ariadne; library callers can render or process them as
they wish.

## Determinism

The compiler is deterministic. Given the same source, the lexer and
parser produce the same `GreenNode`, the lowerer produces the same
HIR with the same arena indices, the formatter produces the same
bytes, and the snapshot tests round-trip every example identically
across runs.
