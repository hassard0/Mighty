# sdust check

Parse and HIR-lower a single Stardust source file; emit diagnostics.

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
- Runs the lexer, parser, AST view, and HIR lowering against it.
- If any diagnostics are produced, renders them with `ariadne` to
  stderr (colorized when stderr is a TTY) and exits 1.
- Otherwise prints `ok: <path>` to stdout and exits 0.

In slice 1 the only checks are lexical, syntactic, and lowering errors.
Type checking, borrow checking, and effect/capability checking are not
yet implemented; a program that lowers cleanly may still be unsound and
will be rejected by later slices.

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
