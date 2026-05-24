# Demo 02 — `counter_web`

A clickable counter rendered into a browser, backed by a Stardust agent
compiled to a **Wasm Component Model component** via
`sdust build --target wasm32-web`.

> **v0.5 dogfood update.** The Wasm Component now imports the full
> `stardust:web/dom` interface (`set-text`, `get-text`, `on-click`,
> `query`) — see `crates/sdust-codegen-wasm/src/wit.rs`. The
> companion JS shim at [`web/dom-shim.js`](web/dom-shim.js) satisfies
> these imports against `document.*`, so the JS side no longer has
> to parse log lines to update the counter. The `log("count=1")`
> stopgap in this demo remains for back-compat with the existing
> loader; the real DOM-binding path is exercised by
> `crates/sdust-codegen-wasm/tests/dom_imports.rs`.

## Layout

```
02_counter_web/
  star.toml            # package manifest (host profile, no deps)
  src/main.sd          # Counter agent + exported `bump` / `start` fns
  web/
    index.html         # zero-dep loader + UI host
    serve.sh           # Python static server (port 8000)
    serve.ps1          # PowerShell equivalent
  smoke.sh / smoke.ps1 # build + Component-shape validation
  target/main.wasm     # produced by `sdust build`
  README.md            # this file
```

## Build

From the repo root:

```bash
cargo build -p sdust-cli
./target/debug/sdust check demos/02_counter_web/src/main.sd
./target/debug/sdust build --target wasm32-web \
    --out-dir demos/02_counter_web/target \
    demos/02_counter_web/src/main.sd
```

The build writes a Component Model component to
`demos/02_counter_web/target/main.wasm`. The Component preamble
(`\0asm\x0d\0\x01\0`) is what `wasm-tools component validate` looks
for; the smoke script verifies the same bytes inline so the demo can
be checked without external tooling installed.

## View in a browser

```bash
bash demos/02_counter_web/web/serve.sh
```

```powershell
pwsh demos\02_counter_web\web\serve.ps1
```

Either copies the freshly-built `main.wasm` + `index.html` into a
`.stage/` directory and serves them with Python's `http.server` on
port 8000 (override with `PORT=…`). Open
<http://localhost:8000> and click "+1". Every click invokes the
exported `bump` fn on the wasm Component; the agent logs `count++`
through the `stardust:web/log#log` import; the JS host parses the log
line and updates the visible number.

## What this demo does NOT (yet) do

Browsers don't run Component Model components natively today — you
normally transpile with
[`jco`](https://github.com/bytecodealliance/jco):

```bash
npx @bytecodealliance/jco@latest transpile main.wasm
```

To keep this demo zero-dependency, `index.html` instead **scans the
component bytes for the embedded core module** (a wasm whose
preamble starts with `\0asm\x01\0\0\0`) and instantiates that
directly, providing a JS shim for the single `log` import. This is a
demonstration loader, not a production pattern — the moment Stardust
adds another host import (DOM bindings, networking) the canonical
`jco` flow takes over.

The Stardust wasm32-web backend wires the `log` import end-to-end
(see `crates/sdust-codegen-wasm/src/wit.rs`); the WIT *stubs* DOM
bindings (`get-element-by-id`, `set-text`) but the emit-side lowerer
hasn't filled them in yet (slice 8 deferred them). That's why the
counter value is rendered by the JS host parsing log lines rather
than by a direct `dom.set_text("#count", n.to_str())` call.

## Smoke test

```bash
bash demos/02_counter_web/smoke.sh
```

The script:

1. `sdust check`s the source.
2. `sdust build`s the Component.
3. Verifies the preamble bytes are the Component Model magic.
4. Sanity-checks the artifact size (>200 bytes).
5. Greps for the `stardust:web/log` import string inside the
   embedded WIT.
6. Runs the same `.sd` source under the host interpreter and confirms
   it logs the expected `counter_web: built` line.

Prints `02_counter_web: PASS (component size = N bytes)` on success.
