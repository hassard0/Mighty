# mty dump

Dump compiler intermediate representations of a Mighty source file.

## Synopsis

```
mty dump [OPTIONS] <PATH>
```

## Arguments

| Name | Description |
|---|---|
| `PATH` | Path to a `.mty` file. |

## Options

| Flag | Purpose |
|---|---|
| `--cst` | Print the rowan CST (`Debug` form). |
| `--ast` | Print top-level AST item kinds and their source ranges. |
| `--hir` | Print the HIR S-expression. |
| `-h`, `--help` | Print help. |

At least one of `--cst`, `--ast`, or `--hir` must be passed. Multiple
flags may be combined; the dumps appear in CST, AST, HIR order.

## Behavior

- `--cst` shows the lossless concrete syntax tree: every token and node
  with its kind and text. Useful when debugging parser behavior.
- `--ast` walks the file's children and prints each item's `SyntaxKind`
  and `TextRange`. Cheap structural overview.
- `--hir` runs HIR lowering and prints the package as a nested
  S-expression. Useful when debugging lowering and snapshot tests.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | dump succeeded |
| `1` | I/O error reading the file |
| `2` | no dump flag was passed |

## Examples

```bash
mty dump --cst examples/01_hello.mty
mty dump --ast examples/02_struct_enum.mty
mty dump --hir examples/07_agent_echo.mty
```
