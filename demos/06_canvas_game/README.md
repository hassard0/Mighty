# Demo 06 — Notetris (canvas-direct)

The canonical **canvas-direct web-game template** in Mighty. The
agent in [`src/main.mty`](src/main.mty) is the source of truth for
rendering: every frame the wasm-exported `frame(dt)` callback calls
real `mty:web/canvas@0.1` WIT imports to paint the playfield, grid,
and HUD. The companion [`web/dom-shim.js`](web/dom-shim.js) is
reduced to **pure host glue** — WIT bindings + keyboard listeners
+ RAF loop.

Compare with [Demo 05](../05_notetris_web/README.md) for the
log-driven pattern; this demo is the v0.25 evolution of that
shape.

## What this demonstrates

| Surface | What this demo does |
|---|---|
| `mty:web/canvas@0.1` WIT imports | `canvas.fill_rect`, `set_fill_style`, `fill_text`, `stroke_rect` called directly from `.mty`. |
| Wasm32-web emit completion | Track A — HIR → IR canvas routing; unit-return stack-balance fix for helper-fn calls inside web callbacks. |
| Array-typed agent fields | `agent Notetris { board: [U32; 200], ... }` — Track C. |
| Extended `format!` spec | `format!("dt: {:>4}ms", dt)` with alignment + precision + named args; Track D. |
| `std.Vec[T]` foundational impl | `let mut board = Vec.with_capacity(200)` — Track E. |
| Frame / keydown / keyup core-module exports | Track A — `is_web_callback_export` routes browser-driven callbacks to real wasm exports. |
| Web-game template | Future canvas-driven demos start from this layout. |

Brought to its current shape by **v0.25 (Tracks A–F)**. The
five v0.26 follow-ups (single-agent persistence, extern_js
end-to-end, canvas-handle taint, const-match patterns, named-arg
passthrough) are tracked in
`dev/history/notes/DEMO06_V2_V0_25_NOTES.md`.

## Run it

```bash
# 1. build the compiler if you haven't already
cargo build -p mty-cli

# 2. build + smoke-check the wasm component
bash demos/06_canvas_game/smoke.sh

# 3. serve on localhost:8000
bash demos/06_canvas_game/web/serve.sh
# (or:  pwsh demos/06_canvas_game/web/serve.ps1 )
```

Open <http://localhost:8000> and play with:

| key   | action |
|-------|--------|
| ← →   | move |
| ↑     | rotate (CW) |
| ↓     | soft drop |
| Space | hard drop |
| R     | reset (also after Game Over) |

## Architecture

```
┌────────────────────────────────────┐
│  src/main.mty  (Mighty source)     │
│                                    │
│  protocol NotetrisInput { ... }    │
│  agent Notetris {                  │
│    board: [U32; 200]               │
│    score = 0_u32                   │
│    ...                             │
│  }                                 │
│                                    │
│  fn frame(dt) {                    │
│    let canvas = Canvas.new()       │
│    canvas.set_fill_style(BG)       │
│    canvas.fill_rect(...)           │
│    canvas.fill_text(format!(...))  │
│  }                                 │
│                                    │
│  fn keydown(k) {                   │
│    match k { 37 => log("left") .. }│
│  }                                 │
└──────────────┬─────────────────────┘
               │ mty build --target wasm32-web
               ▼
┌────────────────────────────────────┐
│  target/main.wasm                  │
│  (component model artefact)        │
│                                    │
│  imports:                          │
│    mty:web/log#log                 │
│    mty:web/canvas#fill-rect        │
│    mty:web/canvas#set-fill-style   │
│    mty:web/canvas#fill-text        │
│    mty:web/canvas#stroke-rect      │
│  exports:                          │
│    main, frame, keydown, keyup     │
└──────────────┬─────────────────────┘
               │ web/dom-shim.js
               ▼
┌────────────────────────────────────┐
│  <canvas id="board">  +  window    │
│                                    │
│  mty:web/canvas → Canvas2D ctx     │
│  mty:web/input  → window listeners │
│  RAF → inst.exports.frame(dt)      │
│  ev   → inst.exports.keydown(kc)   │
└────────────────────────────────────┘
```

The bidirectional arrow is intentional: Mighty drives the canvas
painting via `mty:web/canvas` imports the shim implements; the
shim drives the `frame` / `keydown` callbacks the wasm exports.

## LOC delta vs prior versions

| file              | v0.22  | v0.23  | v0.24  | v0.25  | v0.24 → v0.25 |
| ----------------- | ------ | ------ | ------ | ------ | ------------- |
| `src/main.mty`    | 131    | 195    | 186    | 313    | +127 (+68 %)  |
| `web/dom-shim.js` | 345    | 235    | 213    | 110    | -103 (-48 %)  |
| `web/index.html`  | 119    | 120    | 120    | 120    | 0             |
| **total**         | 595    | 550    | 519    | 543    | +24 (+5 %)    |

The Mighty source ~doubled because it now carries the canonical
agent declaration + the full canvas-render path + the `Vec[U32]`
board construction. The shim shrank by 48 % — every byte of
canvas-call translation moved to Mighty source.

## Headless smoke

```
[web-smoke] golden phash distance for "canvas_game" = 4 (tol 12)
[web-smoke] PASS [canvas_game] canvas={"w":240,"h":480,"cw":242,"ch":482}
            drew=true phash=3838000000000000
06_canvas_game: PASS (headless-browser smoke + magic bytes)
```

The headless-browser smoke captures the canvas, computes a
perceptual hash, and asserts the distance from the golden hash
stays under tolerance 12. The visual shift from "shim paints" to
"agent paints" stays well inside that budget.

## Why "Notetris"?

The canonical four-piece falling-block game name is trademarked;
Demo 05 documents the rationale. Both demos keep the same name so
the side-by-side comparison stays apples-to-apples.
