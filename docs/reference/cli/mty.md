# mty

The Mighty compiler CLI.

## Synopsis

```
mty <COMMAND>
```

## Commands

| Command | Purpose |
|---|---|
| [`new`](mty-new.md) | Scaffold a new Mighty package |
| [`fmt`](mty-fmt.md) | Format `.sd` files (or stdin) |
| [`check`](mty-check.md) | Parse + HIR-lower; emit diagnostics |
| [`dump`](mty-dump.md) | Dump intermediate representations |
| [`explain`](mty-explain.md) | Print a human-readable explanation of a diagnostic code |
| [`lsp`](mty-lsp.md) | Run the Mighty Language Server (LSP 3.17) over stdio |
| `help` | Print help for `mty` or a subcommand |

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

## Scope through v0.2

Slice 1 shipped `new`, `fmt`, `check`, and `dump`. Slice 2 added
`explain`. Slices 6-8 added `run` and `build`. v0.2 brings `lsp`
(Language Server), `pkg` (package manager), and `doc` (documentation
generator). Still pending from [spec §29](../../spec/v0.1.md):
`test`, `lint`, `bench`.
