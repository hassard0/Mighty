# 06 — Notetris (canvas-driven, v0.23)

v0.23 demo: the same canonical Notetris game as demo 05, but rewired
to target the Track A `mty:web/canvas@0.1` + `mty:web/input@0.1`
WIT interfaces. The Mighty agent in `src/main.mty` owns the score /
level / lines state + the input-intent stream; the shim provides the
canvas + input WIT import surface against a real `<canvas>` and
keyboard listeners.

This is the v0.23 **forcing-function demo** for the new WIT
surfaces. It proves the host-side bindings compose end-to-end and
documents the language gaps that still keep some game logic in the
JS shim (see "Status today vs the v0.24 lift" below).

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
|-------|------------------------------|
| ← →   | move                         |
| ↑     | rotate (CW)                  |
| ↓     | soft drop                    |
| Space | hard drop                    |
| R     | reset (also after Game Over) |

## What this demo exercises

- `mty build --target wasm32-web` produces a Component Model component
  that imports `mty:web/canvas` + `mty:web/input` (Track A's WIT
  surfaces) alongside the existing `mty:web/log` + `mty:web/dom`.
- The shim implements all four `mty:web/*` imports against real
  Canvas2D + `window.addEventListener('keydown')` listeners.
- The Mighty agent (`src/main.mty`) owns the score / level / lines
  counters via a `NotetrisInput` protocol, and exposes both the
  Track A canonical `frame` / `keydown` / `keyup` export shape AND
  the legacy v0.22 per-input exports the shim today calls.

## Status today vs the v0.24 lift

Side-by-side with demo 05 (the v0.22-era version of the same game):

| file              | demo 05 (v0.22) | demo 06 (v0.23) | delta  |
|-------------------|-----------------|-----------------|--------|
| `src/main.mty`    | 131 LOC         | 195 LOC         | +64    |
| `web/dom-shim.js` | 345 LOC         | 235 LOC         | **−110** (−32 %) |
| `web/index.html`  | 119 LOC         | 120 LOC         | +1     |

The Mighty source grew because it now declares the Track A export
shape (`frame(dt-ms)` / `keydown(k)` / `keyup(k)`) alongside the
v0.22-compatible per-input exports — both surfaces are wired so the
demo stays playable on today's lowerer.

The shim shrank because:

- It no longer reinvents `log()`-line parsing into game intent —
  the wasm exports a fixed `input_*` function per intent and the
  shim dispatches by export name.
- The renderer is routed through the Track A `mty:web/canvas`
  bindings, so the `bindings` object below the `// ---- Track A WIT
  import surface` banner *is* the WIT surface (one object literal,
  ~20 lines). When the agent starts driving it directly, the
  game-logic mirror block below it deletes — the shim drops to
  ~50 lines of pure WIT glue.

### Language gaps (v0.24 closes these)

The brief expected the shim to drop to ~30-60 lines on v0.23. Three
language gaps in `mty-codegen-wasm` block that today:

1. **`canvas.fill_rect(...)` doesn't lower from Mighty source.** The
   WIT interface is declared in the world's import list, but the
   IR-to-Wasm lowerer in `crates/mty-codegen-wasm/src/emit.rs` only
   knows how to translate `log()` calls and `BuiltinId::DomOp`
   builtins (the `mty:web/dom` set). The Track A `mty:web/canvas`
   builtins haven't been added to `BuiltinId` yet.
2. **No string interpolation / `format!()`.** Per the
   `MT6001 unknown macro` error, dynamic-string composition isn't
   available, so the agent can't emit per-cell
   `evt:cell:x:y:c` lines. (The `examples/05_match_expr.mty`
   comment notes the same constraint at the `log("...")` call site.)
3. **`export fn` declarations don't reach the core module's export
   table.** Only `main` and `cabi_realloc` are exported from the
   embedded core (see `emit.rs` `// emit cabi_realloc + export
   memory + export main`). The shim therefore calls into named
   `input_*` exports defensively (`exp?.input_left?.()`) so the
   round-trip is best-effort today; when the export-name wiring
   lands the shim's `keydown(ev.key)` handler routes straight into
   the wasm-side `keydown(k: Str)` and the v0.22-shape exports
   become redundant.

Notes on what we tried + chose: see
`dev/history/notes/CANVAS_GAME_V0_23_NOTES.md`.

## Anatomy of the v0.24 shim

When the three gaps above close the shim becomes essentially the
`makeCanvas(...)` + `makeInput(...)` + the `// ---- Boot` block
that loads the wasm and binds the imports. Estimated final size:
~50 LOC. The Mighty agent absorbs the game-logic mirror via
`canvas.fill_rect(...)` calls inside its handlers.

## Why "Notetris"?

The canonical four-piece falling-block game is trademarked; demo 05
documents the rationale. This demo keeps the same name so the
side-by-side comparison stays apples-to-apples.
