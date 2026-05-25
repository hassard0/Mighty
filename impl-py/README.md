# `mty` — Python reference implementation of the Mighty front-end

This directory contains the **second independent implementation** of
the Mighty language toolchain, produced from the v1.0-RC2 specification
prose alone (no source-peeking into the Rust reference at
`crates/mty-syntax/`, `crates/mty-ast/`, or `selfhost/`).

It is the single largest v1.0 freeze blocker per the v0.9 spec-freeze
plan: a second implementation validates that the spec is implementable
from prose alone, not just from reading the Rust source.

## Status

| Phase  | Coverage                                                |
|--------|---------------------------------------------------------|
| Lexer  | **shipped** — every token kind in §3, all 20 examples   |
| Parser | **shipped (subset)** — every top-level item kind in §4, |
|        | full expression grammar except a few deferred shapes;   |
|        | every example 01-20 parses with zero diagnostics        |
| HIR / typeck / borrow / codegen | **out of scope** (v0.12+)        |

See `dev/history/notes/PYTHON_IMPL_V0_11_NOTES.md` for the per-file
finding list, the deferred-work backlog, and an audit of every
interpretation call we made where the spec is ambiguous.

## How to run

Requires Python **3.10 or newer** (we use `match` statements and the
walrus operator).

```bash
# from the workspace root
python -m pytest impl-py/tests/

# or, inside impl-py/
python -m pytest tests/
```

To inspect a single source file from the CLI:

```python
>>> from mty import lex, parse
>>> source = open("examples/01_hello.mty").read()
>>> tokens, lex_diags = lex(source)
>>> tree, all_diags = parse(source)
>>> print(tree["_kind"], len(tree["items"]), "items")
file 1 items
```

## Layout

```
impl-py/
  mty/
    __init__.py        - public surface (lex, parse, Token, Diagnostic)
    diagnostics.py     - Diagnostic dataclass + MT-code constants
    lexer.py           - tokeniser
    parser.py          - recursive-descent parser
  tests/
    test_lexer.py      - unit tests for the lexer
    test_parser.py     - unit tests for the parser
    test_examples.py   - sweep examples/01..20.mty through both
  pyproject.toml
  README.md            - this file
```

## What the parser produces

A JSON-friendly tree of plain `dict` nodes. Every node has:

* a `"_kind"` string discriminator (e.g. `"fn"`, `"struct"`,
  `"if_let"`, `"binop"`),
* a `"span"` 2-tuple of byte offsets `(start, end)` into the
  CRLF-normalised source,
* a per-kind set of child fields.

JSON-serialisable nodes make this trivial to diff against the Rust
reference's `--cst` output (we do this in `tests/test_examples.py`
when an `mty` binary is on `$PATH`).

## What the lexer produces

A `list[Token]` ending with a sentinel `TokenKind.EOF` token plus a
`list[Diagnostic]` of any `MT0xxx`-band lexer errors. Whitespace and
comments are first-class trivia tokens (use
`mty.lexer.strip_trivia(tokens)` to drop them).

## License

MIT, same as the rest of the Mighty workspace. See `../LICENSE`.

## What is NOT shipped here (v0.12+ backlog)

* HIR lowering, type checking, borrow checking, code generation.
* A standalone CLI binary (`mty-py-cli`) — for now use the Python REPL.
* Full HTML-literal interpolation parsing (we tokenise it as a single
  literal; the parser does not split the `{ident}` placeholders).
* A second native target — this is a reference implementation, not a
  competing toolchain.
