# mty check

Lex, parse, HIR-lower, and type-check a single Mighty source file;
emit diagnostics.

## Synopsis

```
mty check <PATH>
```

## Arguments

| Name | Description |
|---|---|
| `PATH` | Path to a `.mty` file to check. |

## Options

| Flag | Purpose |
|---|---|
| `-h`, `--help` | Print help. |

## Behavior

- Reads the file as UTF-8.
- Runs the lexer, parser, AST view, and HIR lowering.
- If lowering produced no hard errors, runs the type checker
  (`SD2xxx`).
- Warnings (severity `Warning`, e.g. `MT2015 non_exhaustive_match`)
  are reported but do not affect the exit status.
- If any errors are present, renders them with `ariadne` to stderr
  (colorized when stderr is a TTY) and exits 1.
- Otherwise prints `ok: <path>` to stdout and exits 0.

As of slice 3, `mty check` performs:

1. Lex (MT0001..MT0004)
2. Parse (MT0010..MT0030)
3. HIR lowering (MT1001..MT1002)
4. Type checking (MT2001..MT2025)

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
mty check src/main.mty
mty check examples/07_agent_echo.mty
```

In CI:

```bash
for f in examples/*.mty; do mty check "$f" || exit 1; done
```
