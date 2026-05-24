# sdust fmt

Format `.sd` files in place, or format from standard input.

## Synopsis

```
sdust fmt [OPTIONS] [PATHS]...
```

## Arguments

| Name | Description |
|---|---|
| `PATHS` | Files or directories to format. Directories are walked recursively for `*.sd`. Ignored when `--stdin` is set. |

## Options

| Flag | Purpose |
|---|---|
| `--stdin` | Read source from stdin, write formatted output to stdout. |
| `--check` | Do not modify files. Exit non-zero if any file would be reformatted. |
| `-h`, `--help` | Print help. |

## Behavior

- For each file, parses the source, runs the formatter, and writes the
  result back if it differs.
- Lossless: the formatter operates over the rowan CST, so trivia
  (comments, whitespace) is preserved.
- In slice 1 the formatter is an identity pass — see the
  [internals page](../../internals/formatter.md). It still reads,
  parses, and re-emits the file, so a file with a parse error will
  silently round-trip its broken text.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | nothing to do, or all files formatted successfully |
| `1` | `--check` ran and one or more files would be reformatted; or an I/O error |

## Examples

Format a single file in place:

```bash
sdust fmt src/main.sd
```

Format every `.sd` file under `src/`:

```bash
sdust fmt src/
```

Use as a pre-commit gate:

```bash
sdust fmt --check src/
```

Pipe through stdin:

```bash
cat src/main.sd | sdust fmt --stdin
```
