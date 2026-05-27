# 06 — Notetris (canvas-driven, v0.25 canonical)

v0.25 demo: the canonical **canvas-direct** architecture for web games
in Mighty. The agent in `src/main.mty` is the source of truth for
rendering — every frame the wasm-exported `frame(dt)` callback calls
through real `mty:web/canvas@0.1` WIT imports to paint the playfield,
grid, and HUD. The `web/dom-shim.js` is reduced to **pure host glue**:
WIT bindings + keyboard listeners + RAF loop + a small game-state
mirror that the v0.26 agent-persistence slice will retire.

This is the canonical **web-game template**: future canvas-driven
demos start from this shape.

## What landed in v0.25 (Tracks A–F)

The v0.24 demo carried a "what's missing" table; v0.25 Round 1 closed
every entry on it. Track F (this slice) consumes them end-to-end:

| Mighty-source thing the demo now does           | depends on                              |
| ----------------------------------------------- | --------------------------------------- |
| `canvas.fill_rect(...)` / `fill_text(...)`      | Track A — HIR → IR canvas routing       |
| Helper-fn calls from inside `keydown` callbacks | Track A — Unit-return stack-balance fix |
| `agent Notetris { board: [U32; 200], ... }`     | Track C — array-typed agent fields      |
| `format!("dt: {:>4}ms", dt)`                    | Track D — extended format specs         |
| `format!("...{n} board cells")` shorthand       | Track D — in-scope `{name}`             |
| `let mut board = Vec.with_capacity(200)`        | Track E — real `std.Vec[T]` impl        |
| `protocol NotetrisInput { Left() -> Unit, ... }`| v0.22 stdlib (unchanged)                |
| Frame / keydown / keyup core-module exports     | Track A — `is_web_callback_export`      |

See `dev/history/notes/DEMO06_V2_V0_25_NOTES.md` for the per-track
walkthrough + the 5 narrow v0.26 follow-ups (single-agent
persistence, extern_js end-to-end through component encode, canvas
taint through fn params, `const` match patterns, named-arg
passthrough).

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

| key   | action                       |
| ----- | ---------------------------- |
| ← →   | move                         |
| ↑     | rotate (CW)                  |
| ↓     | soft drop                    |
| Space | hard drop                    |
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
│    let canvas = ...Canvas.new()    │
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
│  (component model artifact)        │
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
│                                    │
│  (game-state mirror still shim-    │
│   side; retired in v0.26)          │
└────────────────────────────────────┘
```

The bidirectional arrow is intentional: Mighty drives the canvas
painting via `mty:web/canvas` imports the shim implements; the shim
drives the `frame` / `keydown` callbacks the wasm exports.

## LOC delta vs v0.24

| file              | v0.22  | v0.23  | v0.24  | v0.25  | v0.24 → v0.25 delta |
| ----------------- | ------ | ------ | ------ | ------ | ------------------- |
| `src/main.mty`    | 131    | 195    | 186    | 313    | +127 (+68 %)        |
| `web/dom-shim.js` | 345    | 235    | 213    | 110    | -103 (-48 %)        |
| `web/index.html`  | 119    | 120    | 120    | 120    | 0                   |
| **total**         | 595    | 550    | 519    | 543    | +24 (+5 %)          |

The Mighty source ~doubled because it now carries the canonical agent
declaration + the full canvas-render path + the `Vec[U32]` board
construction. The shim shrank by 48 % — every byte of canvas-call
translation moved to Mighty source. The remaining shim is split:

- ~30 LOC: pure WIT glue (canvas op bindings + RAF + key listeners +
  boot)
- ~80 LOC: game-state mirror (board + piece tables + helpers + intent
  parser + dynamic board pixel overlay). v0.26's single-agent
  persistence slice retires this layer entirely — the agent
  declaration in `src/main.mty` is the protocol of record.

## What still requires v0.26

Five narrow gaps documented in
`dev/history/notes/DEMO06_V2_V0_25_NOTES.md`:

1. **Single-agent wasm32-web persistence** — moves board / score /
   piece into the agent state region in linear memory; collapses ~80
   LOC of shim mirror.
2. **extern_js end-to-end through component encode** — a kebab-vs-
   leading-underscore drift between `wit.rs::emit_extern_js_interface`
   and `predeclare_extern_js_imports` makes `mty build --target wasm32
   -web` reject `extern js { fn _foo() }`. Workaround: use the intent
   stream (this demo).
3. **Canvas handle taint through fn params** — the IR's taint chain
   only follows `let` rebinds within a single fn; passing a `std.web
   .Canvas` to a helper drops the taint. Workaround: inline canvas
   acquisition in each callback (this demo).
4. **`const` identifier in match patterns** — `match k { KEY_LEFT =>
   ... }` shadow-binds `k`. Workaround: literal patterns + named
   const docs.
5. **`format!` named-arg passthrough** — `format!("{n}", n=v)`
   rejected. Workaround: `{name}` with in-scope bindings.

## Headless smoke

```
[web-smoke] golden phash distance for "canvas_game" = 4 (tol 12)
[web-smoke] PASS [canvas_game] canvas={"w":240,"h":480,"cw":242,"ch":482}
            drew=true phash=3838000000000000
06_canvas_game: PASS (headless-browser smoke + magic bytes)
```

The v0.23 Track E phash golden still matches at distance 4 — the
visual shift from "shim paints the playfield" to "agent paints the
playfield" stays inside the perceptual budget. The phash isn't
re-locked.

## Why "Notetris"?

The canonical four-piece falling-block game is trademarked; demo 05
documents the rationale. This demo keeps the same name so the
side-by-side comparison stays apples-to-apples.
