# Demo 02 — `counter_web`

A clickable counter rendered into a browser, backed by a Mighty
agent compiled to a **Wasm Component Model component** via
`mty build --target wasm32-web`. This is the first "Mighty runs in
the browser" demo; demos 05 and 06 build on the same pipeline for
real games.

## What this demonstrates

| Surface | What this demo does |
|---|---|
| `mty build --target wasm32-web` | Emits a Component Model artefact at `target/main.wasm`. |
| Component Model + `mighty:web/log` import | Counter logs `count=N` through the WIT-defined `log` import. |
| WIT-defined DOM bindings | `mighty:web/dom` (`set-text`, `get-text`, `on-click`, `query`) imported by the Component; satisfied by the JS shim. |
| Exported wasm fns called by the host | `start()` boots, `bump()` increments on every click. |
| Zero-dependency browser loader | `index.html` instantiates the embedded core module without `jco`. |

Brought to its current shape by **v0.5** (full `mighty:web/dom`
WIT interface emitted by `crates/mty-codegen-wasm/src/wit.rs`; JS
shim at [`web/dom-shim.js`](web/dom-shim.js) satisfies the
imports against `document.*`).

## Layout

```
02_counter_web/
├── README.md
├── mighty.toml            # host-profile package, no deps
├── src/main.mty           # Counter agent + exported `bump` / `start` fns
├── web/
│   ├── index.html         # zero-dep loader + UI host
│   ├── serve.sh / serve.ps1   # Python static server (port 8000)
│   └── dom-shim.js        # WIT-binding glue
├── smoke.sh / smoke.ps1   # build + Component-shape validation
└── target/main.wasm       # produced by `mty build`
```

## Build

From the repo root:

```bash
cargo build -p mty-cli
./target/debug/mty check demos/02_counter_web/src/main.mty
./target/debug/mty build --target wasm32-web \
    --out-dir demos/02_counter_web/target \
    demos/02_counter_web/src/main.mty
```

The build writes a Component Model component to
`demos/02_counter_web/target/main.wasm`. The Component preamble
(`\0asm\x0d\0\x01\0`) is what `wasm-tools component validate`
looks for; the smoke script verifies the same bytes inline so the
demo can be checked without external tooling installed.

## View in a browser

```bash
bash demos/02_counter_web/web/serve.sh
```

```powershell
pwsh demos\02_counter_web\web\serve.ps1
```

Either copies the freshly-built `main.wasm` + `index.html` into a
`.stage/` directory and serves them with Python's `http.server` on
port 8000 (override with `PORT=…`). Open <http://localhost:8000>
and click "+1".

Every click invokes the exported `bump` fn on the wasm Component;
the agent emits `count=N` through the `mighty:web/log` import; the
JS host parses the log line and updates the visible number. The
counter value is rendered by the JS host parsing log lines so the
demo works without `jco`-class tooling — when a user-facing DOM-set
binding ships through the canvas-direct emitter, the loader
shrinks accordingly (demo 06 is the canonical "agent owns the
canvas" pattern).

## Smoke test

```bash
bash demos/02_counter_web/smoke.sh
```

The script:

1. `mty check`s the source.
2. `mty build`s the Component.
3. Verifies the preamble bytes are the Component Model magic.
4. Sanity-checks the artifact size (>200 bytes).
5. Greps for the `mighty:web/log` import string inside the
   embedded WIT.
6. Runs the same source under the host interpreter and confirms it
   logs the expected `counter_web: built` line.

Prints `02_counter_web: PASS (component size = N bytes)` on success.

## Production transpile path

Browsers don't run Component Model components natively today —
the canonical production path is
[`jco`](https://github.com/bytecodealliance/jco):

```bash
npx @bytecodealliance/jco@latest transpile main.wasm
```

To keep this demo zero-dependency, `index.html` scans the
component bytes for the embedded core module (a wasm whose
preamble starts with `\0asm\x01\0\0\0`) and instantiates that
directly, providing a JS shim for the imports. This is a
demonstration loader, not a production pattern — the moment your
component grows past a single core module, switch to `jco`.
