# sdust new

Scaffold a new Stardust package.

## Synopsis

```
sdust new <NAME>
```

## Arguments

| Name | Description |
|---|---|
| `NAME` | Package name. A directory of this name is created in the current working directory. |

## What it produces

```
<NAME>/
├── star.toml
└── src/
    └── main.sd
```

`star.toml`:

```toml
[package]
name = "<NAME>"
version = "0.1.0"
edition = "2026"
profile = "host"

[deps]
```

`src/main.sd`:

```sd
fn main() {
  log("hello, Stardust")
}
```

## Behavior

- Refuses to overwrite an existing directory of the same name.
- Creates the package directory and its `src` subdirectory.
- Writes the manifest and the entry-point source.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | package created |
| `1` | directory already exists, or I/O error writing the scaffold |

## Examples

```bash
sdust new hello
cd hello
sdust check src/main.sd
```
