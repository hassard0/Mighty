# mty new

Scaffold a new Mighty package.

## Synopsis

```
mty new <NAME>
```

## Arguments

| Name | Description |
|---|---|
| `NAME` | Package name. A directory of this name is created in the current working directory. |

## What it produces

```
<NAME>/
├── mighty.toml
└── src/
    └── main.sd
```

`mighty.toml`:

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
  log("hello, Mighty")
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
mty new hello
cd hello
mty check src/main.sd
```
