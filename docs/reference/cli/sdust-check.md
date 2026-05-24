# sdust check

Lex, parse, HIR-lower, and type-check a single Stardust source file;
emit diagnostics.

## Synopsis

```
sdust check <PATH>
```

## Arguments

| Name | Description |
|---|---|
| `PATH` | Path to a `.sd` file to check. |

## Options

| Flag | Purpose |
|---|---|
| `-h`, `--help` | Print help. |

## Behavior

- Reads the file as UTF-8.
- Runs the lexer, parser, AST view, and HIR lowering.
- If lowering produced no hard errors, runs the type checker
  (`SD2xxx`).
- Warnings (severity `Warning`, e.g. `SD2015 non_exhaustive_match`)
  are reported but do not affect the exit status.
- If any errors are present, renders them with `ariadne` to stderr
  (colorized when stderr is a TTY) and exits 1.
- Otherwise prints `ok: <path>` to stdout and exits 0.

As of slice 3, `sdust check` performs:

1. Lex (SD0001..SD0004)
2. Parse (SD0010..SD0030)
3. HIR lowering (SD1001..SD1002)
4. Type checking (SD2001..SD2025)

Still deferred to later slices: ownership / affine / borrow checking
(slice 4), effect closure + capability narrowing (slice 5), trait
coherence + dispatch (slice 4+), exhaustiveness as an error (slice 5).

See the [diagnostic codes](../diagnostics.md) page for the registry of
`SDxxxx` codes.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | parsed and lowered without errors |
| `1` | one or more diagnostics emitted, or an I/O error |

## Examples

```bash
sdust check src/main.sd
sdust check examples/07_agent_echo.sd
```

In CI:

```bash
for f in examples/*.sd; do sdust check "$f" || exit 1; done
```
