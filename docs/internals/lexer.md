# Lexer

The lexer turns a UTF-8 source string into a sequence of tokens. It is
built on [logos](https://docs.rs/logos) and lives in
[`crates/mty-syntax/src/lexer.rs`](https://github.com/hassard0/Mighty/blob/main/crates/mty-syntax/src/lexer.rs).

## Token kinds

All token kinds are variants of the
[`SyntaxKind`](https://github.com/hassard0/Mighty/blob/main/crates/mty-syntax/src/syntax_kind.rs) enum.
Variant attributes carry the logos patterns:

```rust
#[token("agent")]
AGENT_KW,
#[regex(r"[0-9]+(?:ns|us|ms|s|m|h)")]
DURATION_LITERAL,
#[regex(r#""([^"\\]|\\.)*""#)]
STRING_LITERAL,
```

A single `SyntaxKind` enum represents both tokens (lexer outputs) and
nodes (parser outputs). The split is structural: variants above
`FILE` are token-only.

## API

```rust
pub fn lex(src: &str) -> Vec<LexedToken<'_>>;
```

Each `LexedToken` has:

- `kind: SyntaxKind`
- `text: &'src str` — the matched slice
- `start: usize`, `end: usize` — byte offsets

The lexer always pushes a synthetic `EOF` token as the last element, so
the parser can peek without bounds checking.

Unknown bytes produce `SyntaxKind::ERROR` tokens; the parser converts
those into diagnostics with code `MT0001`.

## Trivia

Whitespace and comments are kept as tokens, not stripped, so the CST is
truly lossless and the formatter can round-trip them. The parser calls
`skip_trivia` between productions:

```rust
SyntaxKind::WHITESPACE
SyntaxKind::LINE_COMMENT
SyntaxKind::BLOCK_COMMENT
SyntaxKind::DOC_COMMENT
```

Doc comments (`///`) are tokenized separately from line comments so that
later slices can attach them to following items.

## Literal forms

| Kind | Pattern |
|---|---|
| `INT_LITERAL` | `123`, `123u32`, `0xff` (suffix-typed) |
| `FLOAT_LITERAL` | `1.5`, `1.5f64` |
| `DURATION_LITERAL` | `10ns`, `5us`, `3ms`, `2s`, `1m`, `1h` |
| `SIZE_LITERAL` | `64B`, `4KiB`, `128MiB`, `1GiB` |
| `STRING_LITERAL` | `"..."` with backslash escapes |
| `CHAR_LITERAL` | `'c'` |
| `HTML_LITERAL` | `html"..."` |

Slice-1 gap: raw size suffixes like `1k` / `1m` (used in spec §16.1)
are not recognized. Example `11_budget_block.sd` works around this.

## Slice-2 work

- `1k`, `1m`, `1g` numeric multipliers.
- Better recovery on unterminated strings (emit `MT0002` with a span).
- Lex hexadecimal, binary, and underscored numeric forms with explicit
  base prefixes.
