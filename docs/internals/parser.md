# Parser

The parser is hand-rolled recursive descent with Pratt expression
precedence. It builds a [rowan](https://docs.rs/rowan) `GreenNode`
incrementally and produces a `ParseResult` containing the tree and any
errors.

It lives in [`crates/sdust-syntax/src/parser/`](../../crates/sdust-syntax/src/parser/).

## Module layout

| File | Productions |
|---|---|
| `mod.rs` | `Parser` struct, cursor primitives, file/expr/type/pattern entry points |
| `items.rs` | top-level items (fn, struct, enum, type alias, impl, trait, const, use, mod, package) |
| `agents.rs` | `agent`, `protocol`, `supervisor`, `on Msg`, `on_fail` |
| `concurrency.rs` | `arena`, `task scope`, `budget`, `sandbox`, `spawn`, `detach`, `join` |
| `exprs.rs` | expression Pratt with `!Msg`, `?Msg`, `@dur`, `?` postfix |
| `extern_.rs` | `extern c`, `extern js`, `export c fn`, `export fn` |
| `macros.rs` | `macro` declaration |
| `paths.rs` | path expressions and path types, generic args |
| `patterns.rs` | match patterns (literal, ident, wildcard, tuple, struct, enum, range, ref, binding) |
| `recovery.rs` | error recovery helpers |
| `stmts.rs` | `let` and `expr;` statements |
| `types.rs` | type expressions, `T!E` and `T!{A,B}` sugar |
| `unsafe_.rs` | `unsafe { ... }`, `unsafe fn`, `requires` clauses |

## Cursor primitives

```rust
peek(&self) -> SyntaxKind
peek_n(&self, n: usize) -> SyntaxKind
at(&self, kind) -> bool
at_set(&self, set: &[SyntaxKind]) -> bool
bump_any(&mut self)
bump(&mut self, expected: SyntaxKind)
eat(&mut self, kind) -> bool
expect(&mut self, kind) -> bool
skip_trivia(&mut self)
```

All productions are written against these. Nodes are opened with
`start_node` / `start_node_at(checkpoint, ...)` and closed with
`finish_node`.

## Pratt expressions

`exprs::expr` is a Pratt parser with a fixed precedence table covering:

- assignment and compound assignment (`=`, `+=`, ...);
- ranges (`..`, `..=`);
- short-circuit logical (`||`, `&&`);
- equality and comparison;
- bitwise (`|`, `^`, `&`, `<<`, `>>`);
- arithmetic (`+`, `-`, `*`, `/`, `%`);
- prefix unary (`-`, `!`, `*`, `&`, `&mut`);
- postfix: calls, field access, indexing, `?` propagation,
  `?Msg(args)` ask, `!Msg(args)` send, `@duration`, `as` cast.

## Recovery

Top-level recovery is "bump and report": when no item production
matches, the parser records an `unexpected token` error and consumes
one token before retrying. The depth limit produces `SD0030`.

Slice 2 will add expression-level recovery so that an error inside a
function body does not poison the rest of the file.

## Entry points

```rust
pub fn parse(src: &str) -> ParseResult;
pub fn parse_type(src: &str) -> ParseResult;
pub fn parse_pattern(src: &str) -> ParseResult;
pub fn parse_expr(src: &str) -> ParseResult;
```

The latter three exist for testing parser fragments.

## See also

- [`tests/parse_*`](../../tests/) — per-production parser tests.
- [`tests/parser_smoke.rs`](../../tests/) — round-trips every example.
