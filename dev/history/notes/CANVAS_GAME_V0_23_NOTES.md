# demo 06 canvas-game — v0.23 language-gap notes

Track D notes for the `06_canvas_game` rewrite of Notetris targeting
the Track A `mty:web/canvas@0.1` + `mty:web/input@0.1` WIT surfaces.

## Goal (per the v0.23 forcing-function plan)

- Agent owns the canvas via `canvas.fill_rect(...)` calls inside
  Mighty source.
- Agent owns input via `input.subscribe-keydown` + the exported
  `keydown(k: Str)` callback (per `wit/mty-web/world.wit`).
- JS shim drops from demo 05's 345-LOC game-mirror to ~30–60 LOC
  of WIT-import glue (canvas2d + window listeners).

## Reality on today's compiler (`HEAD = cd0e1fd`)

Three lowering gaps block the "pure WIT-glue shim" target:

### 1. `mty:web/canvas` calls don't lower from Mighty source

`crates/mty-codegen-wasm/src/emit.rs` translates `log()` (via
`mty:web/log`) and the four `mty:web/dom` builtins
(`set-text` / `get-text` / `on-click` / `query`) into core-wasm
imports. There is no `BuiltinId::CanvasOp(...)` variant yet —
`FnRef::Builtin(BuiltinId::DomOp(op))` is the only `BuiltinId::*Op`
match arm in `lower_call`. Calls like `canvas.fill_rect(...)` from
Mighty source therefore don't resolve.

Inspected `wasm32-web` output for a probe with one such call: the
core module's import section lists only `mty:web/log#log` + the
four `mty:web/dom` imports; no `mty:web/canvas` imports are
emitted.

### 2. No string interpolation / `format!()` macro

`MT6001 unknown macro 'format!'`. The agent can `let s = "literal"`
and call `log(s)` but cannot compose dynamic strings. So the
canonical v0.22-style `log("evt:cell:" + x + ":" + y + ":" + c)`
per-cell event stream is impossible from Mighty source.

The v0.22 demo 05 didn't hit this because its shim ran the entire
game and the wasm `log("evt:move:left")` calls were all static
literals. Demo 06's "agent owns per-cell state" target needs
either string interp OR the canvas WIT lowering — without either,
the per-cell board state cannot leave the wasm boundary.

### 3. `export fn` declarations don't reach the core-module export table

Probe: declared `export fn frame(dt_ms: U32)` + `export fn keydown(k: Str)`
+ several `export fn get_*() -> I32`. Built with `--target
wasm32-web` and inspected the embedded core module: the export
section contained only `main`, `cabi_realloc`, and `memory`. The
WIT world (component-level) declares the exports correctly, but
they don't materialize as core-module exports the browser-side
`WebAssembly.instantiate` can address.

This matches the defensive `wasmExports?.move_left?.()` shape in
demo 05's shim — optional chaining because the export may not
exist.

## Fallback chosen

Use the **log-tag + per-input-export hybrid** from demo 05, but
restructure the shim so the canvas / input WIT bindings are the
production surface:

- Agent owns score / level / lines + emits `evt:input:<kind>` log
  lines on every keypress. Each handler is a real
  `on Left()` / `on Right()` / ... handler on the `Notetris`
  agent with proper state mutation (score increments on
  soft / hard drop).
- Agent declares the Track A canonical exports
  (`frame(dt_ms)` / `keydown(k)` / `keyup(k)`) so the source is
  ready for v0.24, even though only `main` reaches the core
  export table today.
- Agent also declares v0.22-shape `input_left()` / `input_right()`
  / ... exports so the shim can call into the wasm for each input
  intent. These don't end up in the core export table either, but
  the shim's optional-chaining short-circuits keep the demo
  playable while the language catches up.
- Shim wires `mty:web/canvas` + `mty:web/input` import bindings
  into a `bindings` object the *renderer* uses — so when the
  agent lowering lands, the renderer doesn't move. Today the
  shim's game-logic mirror drives the bindings; tomorrow the
  agent does.

## Shape of the v0.24 lift

When the three gaps close the change is mechanical:

1. Add `BuiltinId::CanvasOp(CanvasOp::FillRect | ::Clear | ::FillText
   | ::SetFillStyle | ::Width | ::Height | ::RequestAnimationFrame
   | ::StrokeRect)` and a matching `emit_canvas_call(...)` in
   `emit.rs` (mirror `emit_dom_call`). Import names come from
   `crates/mty-stdlib/src/web/canvas.rs::WIT_IMPORT_*`.
2. Wire `BuiltinId::InputOp(InputOp::SubscribeKeyDown | ::SubscribeKeyUp)`
   for the input surface.
3. Land string interpolation (or at minimum `Int::to_str()` +
   `Str::concat`) so the agent can compose dynamic event strings.
4. Wire `export fn` declarations into the core module's export
   table (today's `emit_world_export_fn` writes the WIT world but
   not the core-wasm export section).

After those land, `demos/06_canvas_game/src/main.mty` gets a
~40-line rewrite that puts every draw + every state-change log
inside the agent — and the shim's game-logic mirror block deletes
in one pass, dropping `dom-shim.js` from 235 to ~50 LOC.

## Files

- `demos/06_canvas_game/src/main.mty` — agent + protocol + Track A
  export shape + v0.22-compat exports.
- `demos/06_canvas_game/web/dom-shim.js` — Track A WIT bindings +
  game-logic mirror (will shrink).
- `demos/06_canvas_game/web/index.html` — canvas + score panel.
- `demos/06_canvas_game/smoke.sh` — `mty check` / `fmt --check` /
  `build --target wasm32-web` + Component magic-bytes +
  `MTY_WEB_SMOKE=1` opt-in headless-browser stage.

## v0.22 LOC comparison (delta vs `demos/05_notetris_web`)

| file              | demo 05 | demo 06 | delta |
|-------------------|---------|---------|-------|
| `src/main.mty`    | 131     | 195     | +64   |
| `web/dom-shim.js` | 345     | 235     | −110  |
| `web/index.html`  | 119     | 120     | +1    |
