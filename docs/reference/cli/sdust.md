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
| [`explain`](sdust-explain.md) | Print a human-readable explanation of a diagnostic code |
| [`lsp`](sdust-lsp.md) | Run the Stardust Language Server (LSP 3.17) over stdio |
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

## Scope through v0.2

Slice 1 shipped `new`, `fmt`, `check`, and `dump`. Slice 2 added
`explain`. Slices 6-8 added `run` and `build`. v0.2 brings `lsp`
(Language Server), `pkg` (package manager), and `doc` (documentation
generator). Still pending from [spec §29](../../spec/v0.1.md):
`test`, `lint`, `bench`.
