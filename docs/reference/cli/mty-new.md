# mty new

Scaffold a new Mighty package.

## Synopsis

```
mty new <NAME> [--template <TEMPLATE>]
```

## Arguments

| Name | Description |
|---|---|
| `NAME` | Package name. A directory of this name is created in the current working directory. |
| `--template <TEMPLATE>` | Template name. Default `blank`. Built-in templates: `blank`, `web-game`. |

## Templates

| Name | Files | When to use |
|------|-------|-------------|
| `blank` (default) | `mighty.toml` + `src/main.mty` (one-line `log` body) | Library or CLI starting point. |
| `web-game` | adds `web/index.html` + `web/dom-shim.js` + a canvas-driven `GameInput` agent + a `README.md` | Browser-hosted game scaffold ready for `mty serve`. See [`mty serve`](mty-serve.md). |

## What `blank` produces

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

## What `web-game` produces

```
<NAME>/
├── README.md            # how to drive the dev loop
├── mighty.toml
├── src/
│   └── main.mty         # GameInput protocol + agent + export fn keystrokes
└── web/
    ├── index.html       # canvas + log panel + reload-ws bootstrap
    └── dom-shim.js      # wasm loader + canvas mirror + reload-ws client
```

The agent in `src/main.mty` uses the v0.22 `log("evt:...")` pattern
the v0.24 canvas WIT binding will eventually replace; see the NOTE
comment near the top of the generated file and
`crates/mty-codegen-wasm/wit/mty-web/canvas.wit`.

## Behavior

- Refuses to overwrite an existing directory of the same name.
- Creates the package directory and its `src` subdirectory.
- Writes every file the chosen template ships, substituting
  `{{NAME}}` with the package name.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | package created |
| `1` | directory already exists, or I/O error writing the scaffold |
| `2` | unknown `--template` value |

## Examples

```bash
# Default blank template.
mty new hello
cd hello
mty check src/main.mty

# Web-game scaffold + immediate dev loop.
mty new --template web-game asteroids
cd asteroids
mty serve --watch
```
