# sdust

The Stardust compiler CLI.

## Synopsis

```
sdust <COMMAND>
```

## Commands

| Command | Purpose |
|---|---|
| [`new`](sdust-new.md) | Scaffold a new Stardust package |
| [`fmt`](sdust-fmt.md) | Format `.sd` files (or stdin) |
| [`check`](sdust-check.md) | Parse + HIR-lower; emit diagnostics |
| [`dump`](sdust-dump.md) | Dump intermediate representations |
| `help` | Print help for `sdust` or a subcommand |

## Global options

| Flag | Purpose |
|---|---|
| `-h`, `--help` | Print help |
| `-V`, `--version` | Print version |

## Exit codes

| Code | Meaning |
|---|---|
| `0` | success |
| `1` | command-level failure (I/O error, diagnostics emitted, etc.) |
| `2` | usage error (e.g. `dump` with no `--ast`/`--cst`/`--hir`) |

Individual subcommands document any additional codes.

## Scope in slice 1

Slice 1 ships only `new`, `fmt`, `check`, and `dump`. The longer
toolchain in [spec §29](../../spec/v0.1.md) — `build`, `run`, `test`,
`lint`, `doc`, `bench`, `pkg`, `lsp` — lands in later slices.
