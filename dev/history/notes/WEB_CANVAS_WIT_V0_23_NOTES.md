# Web canvas + input WIT — v0.23 Track A notes

## Why this slice

v0.22's demo `05_notetris_web` showed the gap loud and clear: the wasm
Component the codegen emits is header-only — there's no canvas, no
keyboard, no animation-frame surface in the WIT world. That meant
all the game logic ended up in JavaScript with Mighty acting as a
glorified "log" sink. Track A of v0.23 fixes the root cause by
landing two real WIT interfaces (`mty:web/canvas@0.1` +
`mty:web/input@0.1`) plus the matching Mighty-side `std.web`
bindings.

After this slice the target Mighty source shape compiles to a
component whose imports include the canvas + keyboard handles:

```mty
use std.web
agent Game(canvas: Canvas, input: Input) {
  on Tick() {
    canvas.fill_rect(0, 0, 240, 480, 0x1d2230)
  }
  on KeyDown(k: Key) {
    match k {
      Key.ArrowLeft  => move_left(),
      Key.ArrowRight => move_right(),
      Key.Space      => drop_piece(),
      _              => (),
    }
  }
}
```

The lowering work that turns `canvas.fill_rect(...)` into a direct
canonical-ABI import call is owned by Track B; this note covers the
WIT shape Track B pattern-matches against, the Mighty-side resource
wrappers, and the way the host shim (Track D) is expected to
implement the imports.

## What this slice ships

### New WIT files (`crates/mty-codegen-wasm/wit/mty-web/`)

| File         | Package       | Interfaces / Worlds                    |
| ------------ | ------------- | -------------------------------------- |
| `canvas.wit` | `mty:web@0.1` | `interface canvas`                     |
| `input.wit`  | `mty:web@0.1` | `interface input` + `key-event` record |
| `world.wit`  | `mty:web@0.1` | `world agent { import …; export … }`   |

These files are reference shapes — they're *not* concatenated into
the generated per-pkg WIT (the per-pkg generator at
`crates/mty-codegen-wasm/src/wit.rs` emits an inline nested-package
stub for `wit-parser::Resolve` to chew on, so the on-disk files
serve as docs + Track D's host-shim contract).

### Canvas surface (`mty:web/canvas@0.1`)

| Method                     | Signature                                                  |
| -------------------------- | ---------------------------------------------------------- |
| `clear`                    | `func()`                                                   |
| `fill-rect`                | `func(x: s32, y: s32, w: u32, h: u32, color: u32)`         |
| `stroke-rect`              | `func(x: s32, y: s32, w: u32, h: u32, color: u32)`         |
| `fill-text`                | `func(text: string, x: s32, y: s32, color: u32)`           |
| `set-fill-style`           | `func(color: u32)`                                         |
| `width`                    | `func() -> u32`                                            |
| `height`                   | `func() -> u32`                                            |
| `request-animation-frame`  | `func()` — host calls back into exported `frame(dt-ms)`    |

Colors are packed `0xRRGGBBAA` u32; the host shim translates to a
`rgba(...)` CSS string before calling into the Canvas2D context.

### Input surface (`mty:web/input@0.1`)

| Item                | Signature                                                  |
| ------------------- | ---------------------------------------------------------- |
| `key-event` record  | `{ key: string, repeat: bool }`                            |
| `subscribe-keydown` | `func()` — host pushes `keydown(k)` exports                |
| `subscribe-keyup`   | `func()` — host pushes `keyup(k)` exports                  |

The guest's `keydown(k)` / `keyup(k)` exports are matched by name in
Track D's host shim. The shim passes the raw `KeyboardEvent.key`
string; the generated agent stub decodes via
`std.web.Key::from_dom_string`.

### World composition (`mty:web/agent@0.1`)

```
world agent {
    import canvas;
    import input;
    import log;
    export frame: func(dt-ms: u32);
    export keydown: func(k: string);
    export keyup: func(k: string);
}
```

### Mighty-side bindings (`crates/mty-stdlib/src/web/`)

* `canvas.rs` — `Canvas` resource + `CanvasCall` record-recorder for
  native fallback tests. Wraps every WIT method with a Rust call.
* `input.rs` — `Input` resource + `Key` enum + `InputCall`
  record-recorder. Provides `Key::from_dom_string` /
  `Key::to_dom_string` for the keydown decode path.
* `mod.rs` — re-exports plus the `WIT_INTERFACE_*` / `WIT_WORLD_*`
  canonical name constants Track B / Track D pin against.

## Design choices

### Why a host-driven animation loop?

The natural alternative is a guest-driven `Tick` event with a
fixed-rate `Timer`. We picked host-driven (`request-animation-frame`
→ exported `frame(dt-ms)`) for three reasons:

1. **Browser scheduling**: the compositor throttles
   `requestAnimationFrame` when the tab is in the background. A
   guest-driven loop would burn CPU on hidden tabs.
2. **No threading dependency**: a guest-side `Timer` would need
   `wasi:io/poll` plumbing on `wasm32-web`, which today doesn't have
   a Mighty-side primitive.
3. **Demo parity with 05_notetris_web's JS**: the JS shim already
   uses RAF; matching the shape avoids surprising the demo author.

### Why colors as packed u32?

WIT doesn't have a native color type and adding a `record color {
r,g,b,a: u8 }` would mean 4 lowering steps per call. The host shim
unpacks once; the guest gets a single int it can construct cheaply
(`0x1d_22_30_ff` reads as RGB-then-alpha when written with the `_`
separator).

### Why a recording `InputCall` / `CanvasCall` enum in the native fallback?

Two reasons:

1. **`mty run` JIT path**: the native runtime has no canvas / window
   so methods would have to be no-ops. The recorder lets `std.test`
   agents *assert* the guest asked for the right sequence (e.g.
   `expect_canvas_calls!(...)`) without dragging a headless-browser
   dep into the test harness.
2. **Track D integration tests**: when Track D wires the host shim
   up to a real browser, the same recorder shape lets the smoke test
   compare the guest's recorded call log against the host's
   replay-log.

## Integration with the rest of v0.23

| Track | Owns                                       | This slice's contract                            |
| ----- | ------------------------------------------ | ------------------------------------------------ |
| A     | this slice                                 | WIT shape + Mighty-side bindings                 |
| B     | `crates/mty-codegen-wasm/src/emit.rs`      | lowers `Canvas.fill_rect` to a WIT import call   |
| C     | `crates/mty-cli/**`                        | defaults `--target=wasm32-web` for `--web` demos |
| D     | `tests/web-smoke/**` + `demos/**`          | host shim binding canvas + input to DOM          |

Track B pattern-matches on the `WIT_IMPORT_*` constants in
`crates/mty-stdlib/src/web/{canvas,input}.rs`. Track D pattern-matches
on the `WIT_EXPORT_{FRAME,KEYDOWN,KEYUP}` constants and the iface
namespaces emitted into the per-pkg world.

## Tests

`crates/mty-codegen-wasm/tests/web_canvas_wit.rs` (8 tests, all green):

* `wit_world_includes_canvas_and_input` — generated WIT for
  `wasm32-web` contains both `import mty:web/canvas;` +
  `import mty:web/input;`.
* `canvas_fill_rect_lowers_to_wit_import` — host stub declares the
  `fill-rect` / `clear` / `fill-text` / `request-animation-frame`
  method shapes.
* `canvas_geometry_accessors_are_present` — `width()` / `height()` /
  `set-fill-style(color)` show up in the stub.
* `input_keydown_subscribes` — `subscribe-keydown` /
  `subscribe-keyup` / `record key-event` are in the stub.
* `wasi_target_does_not_get_canvas_or_input` — WASI builds do *not*
  pull in the canvas / input imports.
* `web_world_round_trips_with_canvas_and_input` — full re-parse via
  `wit_parser::Resolve` succeeds, world name + package id match the
  generated header.
* `stdlib_constants_match_emitted_wit` — `WIT_IMPORT_*` constants in
  `mty_stdlib::web::*` stay in lockstep with the codegen-side
  emitted method names.
* `key_decoder_round_trips` — `Key::from_dom_string` /
  `to_dom_string` round-trip is total for every variant the host
  emits.

Plus the per-module unit tests in `mty-stdlib::web::canvas::tests`
(6 tests) + `mty-stdlib::web::input::tests` (7 tests).

## Follow-ups (not in this slice)

* **Track B lowering**: pattern-match `std.web.Canvas::*` SIR calls
  onto direct WIT imports of the names declared here. Mirror the
  `P2DirectImport` enum's structure.
* **Sprite atlas**: a 2D-context-only API can't blit images cheaply.
  v0.24 should add `mty:web/sprite@0.1` with an `image` resource +
  `draw-image(image, sx, sy, sw, sh, dx, dy, dw, dh)` once we have
  an `image` resource in WASI 0.3.
* **Mouse + touch**: out of scope for v0.23 (notetris is keyboard-
  only). The natural shape is `mty:web/pointer@0.1` with a
  `pointer-event` record carrying `(x, y, button, kind)`.
* **Audio**: out of scope, will land as `mty:web/audio@0.1` once
  Track E's `Resound`-style synthesiser host wiring lands.
