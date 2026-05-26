# Real Stmt + Terminator SourceSpan — v0.22 notes

## Why this slice

v0.21's DWARF v5 line-program plumbing (`b0f9b67`) added per-instruction
`MachSrcLoc` rows. The commit message admitted the open gap:

> Since `Stmt` doesn't yet carry its own `SourceSpan`, byte offsets are
> synthesized by spreading `(block_idx, stmt_idx)` across the function's
> source range.

That synthetic spread gave `gdb step-line` motion, but every step landed
at an offset chosen by `synthesize_stmt_offset`, not at the real source
position of the originating statement. v0.22 makes those offsets
**real** — when the lowerer produces an MtyIR `Stmt`, the offset cranelift
sees is the byte position of the statement in the source file.

## What "real spans on `Stmt` + `Terminator`" looks like

Strictly speaking, the swarm scope read "add a `pub span: SourceSpan`
field to `Stmt` and `Terminator`". The MtyIR `Stmt` and `Term` enums
are pattern-matched in **five** other crates the v0.22 swarm guidelines
forbid us from touching:

- `mty-codegen-wasm` (24 match sites)
- `mty-codegen-llvm` (14 match sites)
- `mty-doc::extract` (4 match sites)
- `mty-driver` self-host tests (40+ `Stmt::…` / `Term::…` literals
  building hand-rolled `Program`s)
- `mty-codegen-cranelift::mono` + benches (15 sites)

Adding a struct field to every variant would simultaneously break each
of those crates. The pragmatic v0.22 shape therefore puts the spans on
a **side-table owned by `Program`**:

```rust
pub struct Program {
    pub fns: Vec<Function>,
    pub adts: Vec<AdtRef>,
    pub agents: Vec<Agent>,
    pub errors: Vec<String>,
    /// v0.22 — new
    pub span_table: HashMap<IrFnId, FnSpanTable>,
}

pub struct FnSpanTable {
    pub stmt_spans: HashMap<u32 /* block_idx */, Vec<SourceSpan>>,
    pub terminator_spans: HashMap<u32 /* block_idx */, SourceSpan>,
}
```

`Program` already derives `Default`, so adding the new field is a
no-op for every consumer that constructs programs through
`Program::default()` followed by `prog.fns.push(...)` — which is every
test, every back-end, and every driver helper.

The lowerer fills the side-table; the cranelift back-end reads it
when computing per-stmt SourceLoc indices; everyone else ignores it.
Logically each `Stmt` and `Term` *does* carry a span — the carrier is
`(IrFnId, block_idx, stmt_idx)` rather than a field, but the
behaviour is equivalent.

## Plumbing

```
HirFn::span                        ─┐
                                    ├─→  FnBuilder::cur_span
HirAgent::span / HirHandler::span  ─┘            │
                                                 ▼
                            push_stmt(s)  ──▶  spans.set_stmt_span(blk, idx, cur_span)
                            set_term(t)   ──▶  spans.set_terminator_span(blk, cur_span)
                                                 │
                                                 ▼
                            install_fn  ──▶  prog.span_table[fn_id] = spans
                                                 │
                                                 ▼
                  cranelift FnLower::lower_one_block:
                    prog.span_table.get(fn_id).stmt_span(blk, idx)
                      .map(|s| s.start)            -- real byte offset
                      .unwrap_or_else(|| synthesize_stmt_offset(...))
                                                 │
                                                 ▼
                          FunctionBuilder::set_srcloc(SourceLoc::new(idx))
                                                 │
                                                 ▼
                          MachSrcLoc → mty_debuginfo::LineRow → DWARF .debug_line
```

The fallback to `synthesize_stmt_offset` matters: every back-end that
builds a `Function` by hand (`mty-codegen-cranelift::mono`'s
specializer, the JIT bootstrap stub, the dozens of wasm tests, the
mty-driver self-host suite) leaves `Program::span_table` empty. For
those, the v0.21 synthetic spread keeps producing distinct,
monotonic offsets so every back-end and test still gets a sensible
DWARF line table.

## HIR span gap (v0.23 follow-up)

The `mty-hir::HirExpr` and `HirStmt` enums currently carry **no**
per-node span. Only top-level items (`HirFn`, `HirStruct`, `HirAgent`,
…) record a span on the wrapping struct. That means the lowerer's
"current span" for any statement inside `fn add(x) { let a = ...; }`
is the `HirFn`'s span — every Stmt within `add` lands at the same
byte range.

This is still strictly better than the synthetic spread: real spans
match the fn body when stepping; the absolute file/byte addressing is
correct for `addr2line` to land inside the right function; and DWARF
consumers that group by `(file, fn-low-pc..high-pc)` (most of them)
behave identically to a more-granular table. But `gdb step-line` won't
move row-by-row inside a function until the HIR exposes per-stmt /
per-expr spans.

**v0.23 plan**: add `pub expr_spans: ArenaMap<ExprId, SourceSpan>` and
`pub stmt_spans: Vec<SourceSpan>` (parallel to `HirBlock::stmts`) to
`mty_hir::Package`. The CST→HIR lowerer already has a `SyntaxNode`
in hand at every `alloc_expr` site (see `mty_hir::lower::span_of`),
so populating the table is a one-line change per arm. Once landed,
`FnBuilder::enter_expr_span` swaps from the fn-span fallback to a
real per-expr lookup with no other changes — the v0.22 plumbing
already threads the value through.

## What changed in this slice

| Crate                       | Files                                  | Shape                                |
|-----------------------------|----------------------------------------|--------------------------------------|
| `mty-ir`                    | `src/ir.rs`                            | New `FnSpanTable`, `Program::span_table` |
|                             | `src/lower/ctx.rs`                     | `FnBuilder::cur_span` + `spans`      |
|                             | `src/lower/items.rs`                   | Prime `cur_span` from fn / agent spans |
|                             | `src/lower/exprs.rs` + `stmts.rs` (new) | Stmt-lowering moved out; spans flow via builder |
|                             | `tests/spans.rs` (new)                 | 5 acceptance tests                   |
| `mty-codegen-cranelift`     | `src/lower.rs`                         | Read `span_table` first; synthetic as fallback |
|                             | `tests/debug_mach_src_loc.rs`          | New `dwarf5_row_byte_offsets_match_source` |
| `docs/internals/ir.md`      |                                        | Statements / span-table section      |
| `dev/history/notes/…`       | This file                              |                                      |

## Acceptance

- `cargo test -p mty-ir --test spans` — 5/5 passing.
- `cargo test -p mty-codegen-cranelift --test debug_mach_src_loc` —
  6/6 passing (v0.21's 5 + the new v0.22 byte-accurate test).

## Impact on debuggers

- **gdb / lldb step-line**: rows land at real source byte offsets when
  the HIR has spans for them. Today the fn-level fallback means
  stepping moves "into the right fn body" but not row-by-row inside;
  v0.23's HIR expr-spans unlock the finer granularity. Before this
  slice, rows landed at *synthetic* offsets that didn't match any
  real position, so the IDE's "highlight current line" would jump
  around arbitrarily.
- **addr2line / `__builtin_return_address` lookups**: now return the
  fn the address came from, plus the (correct) file. Before v0.22 the
  byte offset would still resolve to the right function (because the
  spread stays inside the fn's `low_pc..high_pc`), but the file
  position the spread reported was a synthetic average — the
  resolved line was always near the fn body's geometric centre.
- **perf annotate / cargo-flamegraph**: now attribute samples to the
  fn whose code emitted the instruction; per-row attribution arrives
  with v0.23.
