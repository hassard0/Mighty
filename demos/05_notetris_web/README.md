# Demo 05 — Notetris (log-driven web)

Notetris as a localhost web app. The Mighty agent in
[`src/main.mty`](src/main.mty) owns the game state through the
`NotetrisInput` protocol; every input emits a `log("evt:…")` line
that the JS host parses to drive a `<canvas>` renderer.

This is the **log-driven** web-game pattern. Demo 06 is the same
game rebuilt around the canvas-direct emit path — the side-by-side
makes the v0.22 → v0.25 web-emitter evolution concrete.

## What this demonstrates

| Surface | What this demo does |
|---|---|
| `mty build --target wasm32-web` | Emits a Component-Model component the JS host instantiates. |
| `package` / `protocol` / `agent` / `export fn` | All four surface shapes on a wasm-targeted compilation unit. |
| `mighty:web/log` WIT import | Host imports `mighty.log(ptr, len)` and parses `evt:…` lines. |
| `requestAnimationFrame` callbacks | JS host calls the wasm exports on every input + frame; round-trip is real. |
| Full game loop in source | Score / level / lines / hard drop / line clearing / gravity-by-level all live in `.mty`. |

Brought to its current shape by **v0.22** (`mty:web/canvas@0.1`
WIT stubs landed in this release; demo 05 keeps the log-driven
path; demo 06 switches to canvas-direct).

## Run it

```bash
# 1. build the compiler if you haven't already
cargo build -p mty-cli

# 2. build + smoke-check the wasm component
bash demos/05_notetris_web/smoke.sh

# 3. serve on localhost:8000
bash demos/05_notetris_web/web/serve.sh
# (or:  pwsh demos/05_notetris_web/web/serve.ps1 )
```

Open <http://localhost:8000> and play with:

| key   | action |
|-------|--------|
| ← →   | move |
| ↑     | rotate (CW) |
| ↓     | soft drop |
| Space | hard drop |
| R     | reset (also after Game Over) |

## Architecture (log-driven)

```
src/main.mty
   │
   │  log("evt:left") / log("evt:rotate") / log("evt:reset") …
   │  log("state:score=12,lines=3,level=1") …
   ▼
target/main.wasm (Component Model)
   │
   │  imports: mty:web/log#log
   │  exports: start, on_input, frame
   ▼
web/dom-shim.js
   │
   │  parses "evt:…" / "state:…" lines, mirrors state, paints canvas
   ▼
<canvas id="board"> + keyboard listeners
```

The agent is the source of truth for game logic; the JS shim
mirrors state by parsing log lines and paints the canvas. Demo 06
inverts that — the agent paints directly through `mty:web/canvas`
imports, and the shim shrinks to pure host glue.

## What this demo deliberately keeps

The log-driven pattern is the lowest-friction way to land Mighty
in a browser today: it works against any wasm host that can
satisfy a `log(ptr, len)` import. When you need to render against
a real Canvas2D context with no parsing in the middle, see Demo 06
(canvas-direct).
