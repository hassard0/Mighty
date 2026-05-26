# 06 — Notetris (canvas-driven, v0.24 refresh)

v0.24 demo: the same canonical Notetris game as demo 05, rewired to
target the Track A `mty:web/canvas@0.1` + `mty:web/input@0.1` WIT
interfaces and the v0.24 Track A/B language wins. The Mighty agent in
`src/main.mty` owns the score / level / lines / `gameover` state via a
`NotetrisInput` protocol; the shim provides the canvas + input WIT
import surface against a real `<canvas>` and keyboard listeners.

This is the v0.23 **forcing-function demo** for the new WIT surfaces.
v0.23 shipped it as a hybrid (agent owned input/score, shim owned
board/rendering) because three language gaps blocked the cleaner
architecture. **Two of the three closed in v0.24**:

| gap                                                       | closed by         |
| --------------------------------------------------------- | ----------------- |
| `BuiltinId::CanvasOp` + `mty:web/canvas` import emission  | Track A (aef5225) |
| `frame` / `keydown` / `keyup` reach the core export table | Track A (aef5225) |
| `format!()` string interpolation                          | Track B (2399391) |

What still requires a v0.25 closer:

| gap                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| HIR -> IR routing for `canvas.fill_rect(...)` method calls on a `std.web.Canvas`-typed receiver — the IR + emitter are ready; the HIR side isn't wired |
| Calling a Unit-returning user fn from `keydown` / `frame` triggers a stack-balance error in the v0.24 callback prologue                                |
| Top-level `let mut` / per-callback persistent state — agent fields exist but the exported callbacks don't dispatch into them                           |

See `dev/history/notes/DEMO06_CANVAS_DIRECT_V0_24_NOTES.md` for the
full per-attempt log.

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

## What this demo exercises (v0.24)

- `mty build --target wasm32-web` produces a Component Model component
  that imports `mty:web/canvas` + `mty:web/input` (Track A's WIT
  surfaces) alongside the existing `mty:web/log` + `mty:web/dom`.
- The shim implements all four `mty:web/*` imports against real
  Canvas2D + `window.addEventListener('keydown')` listeners.
- The Mighty source declares `fn frame(dt: U32)` / `fn keydown(k: U32)`
  / `fn keyup(k: U32)`; the v0.24 emitter (`is_web_callback_export` in
  `crates/mty-codegen-wasm/src/web_lower.rs`) lifts those into the
  embedded core module's export section so the JS host can call
  `inst.exports.frame(dt)` straight through — no more `exp?.input_*?.()`
  fallback chain.
- The agent's keydown dispatcher uses `format!("evt:input:unknown:{}", k)`
  for unrecognized keycodes — the first end-to-end use of Track B's
  `format!()` macro in a wasm32-web build.

## LOC delta vs v0.23

Side-by-side with the v0.23 shape (commit `13f6d4a`):

| file              | demo 05 (v0.22) | demo 06 (v0.23) | demo 06 (v0.24) | v0.23 -> v0.24 delta |
| ----------------- | --------------- | --------------- | --------------- | -------------------- |
| `src/main.mty`    | 131 LOC         | 195 LOC         | 186 LOC         | -9                   |
| `web/dom-shim.js` | 345 LOC         | 235 LOC         | 213 LOC         | -22 (-9%)            |
| `web/index.html`  | 119 LOC         | 120 LOC         | 120 LOC         | 0                    |

The Mighty source got slightly smaller because the v0.23 demo carried
both the Track A canonical export shape AND a legacy v0.22 per-input
export shape (`input_left()`, `input_right()`, …). The v0.24 source
ships only the canonical shape — Track A's export wiring makes the
v0.22 fallback redundant.

The shim shrank because:

- It no longer maintains a browser-key -> intent-tag translation table.
  The browser keycode is forwarded directly to the wasm `keydown(k)`
  export, and the wasm uses `format!()` to emit the canonical
  `evt:input:<kind>` line the shim's renderer parses.
- It no longer has a defensive `exp?.input_*?.()` fallback chain — the
  v0.24 emitter actually exports `frame` / `keydown` / `keyup`, so
  `exp.keydown(keycode)` is a direct call.
- It no longer wraps RAF behind a `setOnFrame()` callback indirect — the
  RAF loop calls `exp.frame(dt)` directly and drives gravity host-side.

What's still in the shim (~70% of the file): the board / piece /
gravity / collision / line-clear logic. Until the HIR -> IR canvas
routing lands in v0.25, the agent can't draw pixels directly, so the
shim still owns the actual rendering and a parallel mirror of the
board state derived from the agent's intent log.

### Headless smoke

The v0.23 Track E phash golden (`tests/web-smoke/golden/canvas_game.phash`)
still matches at distance 0 — the v0.24 rewrite is visually identical
to the v0.23 shape (same canvas dimensions, same render path, same
initial board fill).

## Why "Notetris"?

The canonical four-piece falling-block game is trademarked; demo 05
documents the rationale. This demo keeps the same name so the
side-by-side comparison stays apples-to-apples.
