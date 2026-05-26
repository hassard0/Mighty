# `mty` — Python reference implementation of the Mighty front-end

This directory contains the **second independent implementation** of
the Mighty language toolchain, produced from the v1.0-RC2 specification
prose alone (no source-peeking into the Rust reference at
`crates/mty-syntax/`, `crates/mty-ast/`, or `selfhost/`).

It is the single largest v1.0 freeze blocker per the v0.9 spec-freeze
plan: a second implementation validates that the spec is implementable
from prose alone, not just from reading the Rust source.

## Status

| Phase           | Coverage                                                                       |
|-----------------|--------------------------------------------------------------------------------|
| Lexer           | **shipped** — every token kind in §3, all 23 examples                          |
| Parser          | **shipped (subset)** — every top-level item kind in §4, all 23 examples        |
| HIR + lowering  | **shipped (v0.17)** — name-resolved typed-dataclass tree, all 23 examples lower clean |
| Type checker    | **shipped (subset, v0.17)** — H-M-style inference with TyAny absorption; 23/23 examples typeck clean |
| Borrow checker  | **out of scope** (v0.18+)                                                      |
| Codegen         | **out of scope** (v0.18+)                                                      |

See `dev/history/notes/PYTHON_IMPL_V0_11_NOTES.md` for the v0.11
front-end findings and
`dev/history/notes/PYTHON_IMPL_V0_17_NOTES.md` for the v0.17
HIR + typeck findings (per-example matrix, recovered-from-died-agent
audit, v0.18 follow-ups).

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
    hir.py             - HIR dataclass tree (v0.17)
    lower.py           - parser-AST -> HIR with name resolution (v0.17)
    typeck.py          - H-M inference subset + diagnostic emit (v0.17)
  tests/
    test_lexer.py            - unit tests for the lexer
    test_parser.py           - unit tests for the parser
    test_examples.py         - sweep examples/*.mty through lex+parse
    test_hir.py              - HIR + lowering unit tests (v0.17)
    test_typeck.py           - type checker unit tests (v0.17)
    test_examples_typeck.py  - full pipeline sweep on every example (v0.17)
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

## HIR + type checker (v0.17)

The HIR (`mty.hir`) is the parser AST after name resolution, normalised
into typed dataclass nodes. The lowerer (`mty.lower`) walks the parser
dict-tree and produces an `HirModule`; it resolves identifiers to
`Resolution` records (`local`, `item`, `variant`, `builtin`, `unknown`)
and emits MT15xx-band diagnostics for duplicate items and unsupported
shapes.

The type checker (`mty.typeck`) runs Hindley-Milner-style inference
over the HIR. It models:

* Scalar prelude types (`I8..I128`, `U8..U128`, `F32/F64`, `Bool`,
  `Str`, `Char`, `Unit`)
* User-defined `struct`, `enum`, and `type_alias` items
* `Option`, `Result`, tuples, arrays, refs, fn types
* Inference variables with proper Robinson unification + occurs check
* Per-arg fn-call type/arity checks
* Struct-literal field-set matching
* If/match branch unification

What it does NOT model in v0.17 (these gracefully degrade via the
`TyAny` absorbing type — they don't break inference):

* Effect rows (treated as type-erased)
* Trait/impl method dispatch (method calls return `TyAny`)
* Agents, protocols, supervisors, `spawn`, `ask/send` sugars
* Macros and macro calls
* Lifetimes / borrow checking

Coverage matrix:

| Example                       | typeck status |
|-------------------------------|---------------|
| 01_hello.mty                  | clean         |
| 02_struct_enum.mty            | clean         |
| 03_generic_fn.mty             | clean         |
| 04_result_propagation.mty     | clean         |
| 05_match_expr.mty             | clean         |
| 06_for_while_loop.mty         | clean         |
| 07_agent_echo.mty             | clean         |
| 08_agent_state.mty            | clean         |
| 09_send_ask_deadline.mty      | clean         |
| 10_supervisor.mty             | clean         |
| 11_budget_block.mty           | clean         |
| 12_arena.mty                  | clean         |
| 13_capabilities.mty           | clean         |
| 14_extern_c.mty               | clean         |
| 15_extern_js.mty              | clean         |
| 16_macro.mty                  | clean         |
| 17_unsafe.mty                 | clean         |
| 18_sandbox.mty                | clean         |
| 19_backend_service.mty        | clean         |
| 20_frontend_component.mty     | clean         |
| 21_wasi_preview2.mty          | clean         |
| 22_effect_row.mty             | clean         |
| 23_multi_row.mty              | clean         |

All 23 examples typeck clean. Note: this is partly because effect-row
typeck and agent/protocol typeck are absorbed into `TyAny`. v0.18 will
tighten this and re-baseline.

## What is NOT shipped here (v0.18+ backlog)

* Borrow checker, code generation.
* A standalone CLI binary (`mty-py-cli`) — for now use the Python REPL.
* Full HTML-literal interpolation parsing (we tokenise it as a single
  literal; the parser does not split the `{ident}` placeholders).
* Effect-row inference (currently type-erased).
* Trait/impl dispatch and method-call resolution.
* A second native target — this is a reference implementation, not a
  competing toolchain.
