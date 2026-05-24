# HIR

The HIR (high-level IR) is the first semantically-shaped view of a
Stardust program. It is name-resolved (slice 1: per-file scope), arena-
allocated, and indexed by stable `Idx<_>` ids.

It lives in [`crates/sdust-hir/`](../../crates/sdust-hir/).

## Storage shape

A whole package is a single `Package` struct with one arena per node
kind:

```rust
pub struct Package {
    pub items: Arena<Item>,
    pub fns: Arena<HirFn>,
    pub structs: Arena<HirStruct>,
    pub enums: Arena<HirEnum>,
    pub type_aliases: Arena<HirTypeAlias>,
    pub agents: Arena<HirAgent>,
    pub protocols: Arena<HirProtocol>,
    pub supervisors: Arena<HirSupervisor>,
    pub exprs: Arena<HirExpr>,
    pub pats: Arena<HirPat>,
    pub types: Arena<HirType>,
    pub blocks: Arena<HirBlock>,
    pub locals: Arena<HirLocal>,
    pub top_level: Vec<ItemId>,
}
```

Ids are typed `la-arena` indices:

```rust
pub type ItemId = Idx<Item>;
pub type FnId   = Idx<HirFn>;
pub type ExprId = Idx<HirExpr>;
// ... and so on for every arena
```

This shape is good for **cheap cloning, snapshot dumping, and
incremental analysis** in later slices. It is bad for **graph-shaped
mutations**; those are intentionally pushed into later IRs (AIR, SIR).

## Lowering entry

```rust
LoweringCtx::new()
    .lower_file(file: sdust_ast::File)
    -> (Package, Vec<Diagnostic>)
```

`LoweringCtx` owns the in-flight `Package` and the diagnostics vector.
It exposes per-arena `alloc_*` helpers and dispatches per-kind lowering
through submodules:

| Submodule | What it lowers |
|---|---|
| `lower/items.rs` | fn, struct, enum, type alias, impl, trait, use, mod, const, extern, export, macro |
| `lower/agents.rs` | agent, protocol, supervisor, on-handler, on_fail, child |
| `lower/exprs.rs` | every `HirExpr` variant |
| `lower/patterns.rs` | every `HirPat` variant |
| `lower/types.rs` | path, borrow, tuple, array, fn, `T!E` result sugar, `T!{A,B}` union |

## Desugarings performed

Slice 1 normalizes a small set of surface forms:

- `T!E` becomes `HirType::Result { ok: T, err: E }`. `T!{A,B}` becomes
  `HirType::Result { ok, err: HirType::Union([A, B]) }`. The original
  surface form is preserved on the CST so the formatter can re-emit it.
- Expression-body functions (`fn f() = expr`) lower to a block with a
  single tail expression.
- The compact agent forms (`on Msg(args) -> body`, agent state without
  an explicit `state` keyword) lower to the same `HirAgent` shape as
  the long form.

## Dumping

`dump::dump_package(&pkg) -> String` returns a stable S-expression
form, used by snapshot tests and the `sdust dump --hir` command.

## Resolve

The `resolve.rs` module is a stub in slice 1. Real cross-file name
resolution lands in slice 3.

## See also

- [Architecture](architecture.md)
- [Diagnostics](diagnostics.md)
