# Formatter

The Mighty formatter is a Wadler/Lindig pretty-printer that operates
over the rowan CST. It lives in
[`crates/mty-fmt/`](../../crates/mty-fmt/).

In slice 1 the formatter is an identity pass: it parses the source,
walks the CST, and re-emits the text verbatim. The combinator engine is
fully built — slice 2 replaces the per-node stub with real format rules.

## Entry point

```rust
pub fn format(green: GreenNode) -> String;
```

## Doc combinators

[`doc.rs`](../../crates/mty-fmt/src/doc.rs) defines `Doc`, the
abstract document description:

```rust
pub enum Doc {
    Nil,
    Text(Rc<str>),
    Line,
    SoftLine,
    Nest(usize, Box<Doc>),
    Group(Box<Doc>),
    Concat(Box<Doc>, Box<Doc>),
}
```

Constructor methods: `Doc::nil()`, `Doc::text(s)`, `Doc::line()`,
`Doc::softline()`, `Doc::nest(n, d)`, `Doc::group(d)`,
`Doc::concat(a, b)`, `Doc::concat_all(parts)`, `Doc::join(sep, parts)`.

## Printer

[`printer.rs`](../../crates/mty-fmt/src/printer.rs) implements the
standard Wadler/Lindig layout algorithm:

```rust
pub struct Layout { pub width: usize }   // default 100
pub fn pretty(doc: &Doc, layout: &Layout) -> String;
```

The printer walks the tree with an explicit stack, tracking the current
column. When a `Group` is encountered it uses a `fits` lookahead to
decide whether the whole group renders on the current line in `Flat`
mode; otherwise the group switches to `Break` mode and its `Line`s
become newlines.

## Per-node rules

[`fmt/`](../../crates/mty-fmt/src/fmt/) is the per-node dispatch.
Submodules:

| Module | Productions |
|---|---|
| `items.rs` | fn, struct, enum, use, mod, type, impl, trait, const, extern, export, macro |
| `agents.rs` | agent, protocol, supervisor |
| `concurrency.rs` | arena, task scope, budget, sandbox |
| `exprs.rs` | every expression |
| `patterns.rs` | every pattern |
| `types.rs` | every type expression |

`fmt::file(node) -> Doc` is the top-level entry. In slice 1 it returns
`Doc::text(node.text().to_string())`; slice 2 will dispatch by kind.

## Trivia

[`trivia.rs`](../../crates/mty-fmt/src/trivia.rs) tracks comments
and blank lines that the per-node formatter must preserve. Real
formatting needs to attach trivia to its nearest node and re-emit it
in the right place. Slice 2 wires this into the per-node rules.

## Round-trip tests

[`tests/fmt/`](../../tests/fmt/) feeds every example file through
`mty fmt` and asserts byte-identity. This is how slice 1 proves the
identity pass actually preserves source — and how slice 2 will prove
the real formatter is idempotent.

## See also

- [Reference: `mty fmt`](../reference/cli/mty-fmt.md)
- [Architecture](architecture.md)
