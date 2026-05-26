# `mty serve`

Built-in dev server for a Mighty **web-game** package. v0.23 Track C.

`mty serve` reads `mighty.toml`, builds the package with
`--target wasm32-web`, then serves `web/index.html`, every static
asset under `web/`, and the freshly-built wasm module on
`127.0.0.1:<port>`. With `--watch`, it file-watches `src/` and
rebuilds + pushes a reload to the page over a websocket on every
change.

## Synopsis

```
mty serve [--port N] [--watch] [--manifest-dir DIR]
```

## Flags

| Flag | Description |
|------|-------------|
| `--port <N>` | TCP port to bind on `127.0.0.1`. Default `8000`. |
| `--watch` | Watch `src/` and rebuild on every change. Pushes a reload to connected pages over `ws://127.0.0.1:<port>/_reload`. |
| `--manifest-dir <DIR>` | Package root (default: current directory). |

## Routes

| Method | Path | Behavior |
|--------|------|----------|
| GET | `/` | Serves `web/index.html` with `Content-Type: text/html; charset=utf-8`. |
| GET | `/<asset>` | Serves `web/<asset>` with a mime type derived from the extension (`.js`, `.css`, `.json`, `.svg`, `.png`, `.jpg`, `.txt`, `.md`, etc.). |
| GET | `/main.wasm` | Serves the freshly-built wasm artefact (typically `target/main.wasm`) with `Content-Type: application/wasm` and `Cache-Control: no-cache`. |
| GET | `/_reload` | Websocket upgrade endpoint. The server pushes the string `reload` after every successful `--watch` rebuild; the page template's `dom-shim.js` calls `location.reload()` on receipt. |
| any | `/<path with ..>` | `403 forbidden` (defence against path-escape attempts). |
| any | unknown path | `404 not found`. |

## What it expects

The current working directory (or `--manifest-dir`) must contain:

```
.
├── mighty.toml
├── src/
│   └── main.mty
└── web/
    └── index.html        # served at `/`
```

The fastest way to get one is `mty new --template web-game <name>`.

## Build configuration

`mty serve` always builds with:

| Setting | Value |
|---------|-------|
| target | `wasm32-web` |
| mode | `debug` (fast rebuild + smaller binary on disk) |
| output | `<pkg>/target/main.wasm` |
| wasi-preview | default (`p2`) |
| component | yes (the same envelope `mty build --target wasm32-web` emits) |

The browser-side dom-shim that ships with the `web-game` template
extracts the inner core module from the component envelope at load
time — see the comments in `templates/web-game/web/dom-shim.js`.

## `--watch` reload protocol

1. The page opens `ws://127.0.0.1:<port>/_reload` on first load.
2. `mty serve` completes the RFC 6455 opening handshake
   (`Sec-WebSocket-Accept` is `base64(sha1(key + magic))`).
3. On each successful rebuild, the server writes a single
   server-to-client text frame containing `reload`.
4. The page's `dom-shim.js` calls `location.reload()` on receipt.
5. If the websocket closes (e.g. the server stops), the shim
   retries every 1s.

## Examples

```bash
# Scaffold a fresh web-game and start the dev loop.
mty new --template web-game asteroids
cd asteroids
mty serve --watch

# Pick a non-default port.
mty serve --port 9000

# Run from outside the package directory.
mty serve --manifest-dir ./asteroids
```

## Exit codes

| Code | Meaning |
|------|---------|
| `0` | Clean shutdown (Ctrl-C). |
| `1` | Frontend build error in the user's package (diagnostics printed). |
| `2` | Backend build error, missing `mighty.toml`, missing `web/`, or bind failure. |

## See also

- [`mty new`](mty-new.md) — `--template web-game` produces the
  scaffold this command serves.
- [`mty build`](mty-build.md) — the same backend pipeline,
  invoked from the CLI.
- `crates/mty-codegen-wasm/wit/mty-web/canvas.wit` — the v0.24
  canvas binding the agent will drive directly once Track A
  lands.
