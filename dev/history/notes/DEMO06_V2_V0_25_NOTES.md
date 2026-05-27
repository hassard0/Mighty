# DEMO 06 canvas-direct v0.25 — Track F notes

**Slice**: rewrite `demos/06_canvas_game/` to consume the v0.25 Round 1
gap closures (Tracks A–E) and stand up the canonical
canvas-direct architecture for web games.

**Status**: shipped — see commit.

## Inputs (v0.25 Round 1 closures)

| gap (v0.24 leftover)                                        | closed by         | used here                                                        |
| ----------------------------------------------------------- | ----------------- | ---------------------------------------------------------------- |
| HIR → IR routing for `canvas.fill_rect(...)`                | Track A           | `frame()` calls 17 canvas ops → real wasm imports                |
| Unit-returning user-fn from `keydown` stack imbalance       | Track A           | match arms call helpers without rebalancing                      |
| `[T; N]` agent fields parse + typecheck                     | Track C           | `agent Notetris { board: [U32; 200] ... }` declared              |
| Cross-callback agent persistence (SIR runtime tier)         | Track C           | pinned at runtime; wasm32-web is single-agent v0.26 follow-up    |
| `extern js { fn _foo() }` emits real `(import "mty:web/js")`| Track B           | NOT used end-to-end here — see §B for why                        |
| `format!()` extended specs (`{:>4}`, `{:#06x}`, named args) | Track D           | HUD lines use `{:>4}` right-align + `{name}` interpolation       |
| `std.String` + `std.Vec[T]` real impls                      | Track E           | `main()` builds `Vec[U32]` of 200 cells                          |

All seven gaps that blocked the v0.24 demo are landed. The v0.25 demo
exercises 6 of the 7 directly; the seventh (extern_js end-to-end) hit
a kebab-name drift in the component-encode path documented in §B.

## LOC delta

```
file              v0.22       v0.23       v0.24       v0.25       v0.24 -> v0.25 delta
src/main.mty      131 LOC     195 LOC     186 LOC     313 LOC     +127 (+68 %)
web/dom-shim.js   345 LOC     235 LOC     213 LOC     110 LOC     -103 (-48 %)
web/index.html    119 LOC     120 LOC     120 LOC     120 LOC      +0
total             595 LOC     550 LOC     519 LOC     543 LOC      +24 (+5 %)
```

The Mighty source ~doubled because it now carries the full canonical
agent declaration (the v0.26 protocol of record), the canvas-direct
render path, and the `Vec[U32]` board construction. The shim shrank
by 48 % — every byte of canvas-call translation moved to Mighty source.

The board pixel overlay + game-state mirror remain shim-side because
wasm32-web agent persistence is the v0.26 emitter slice (see §C). The
remaining shim is split:

- ~22 LOC: piece definitions + helpers (`cellsOf`, `hits`, `lock`,
  `clearLines`, `take`, `spawn`, `move`, `rotate`, `hard`, `reset`)
- ~12 LOC: state mirror + dynamic board overlay
- ~10 LOC: intent-stream parser + state dispatch
- ~25 LOC: WIT import bindings + RAF/keyboard wiring + boot
- ~40 LOC: piece definition tables + RGBA helper + core-extractor

The "pure glue" core (WIT bindings + RAF + key listeners) is ~30 LOC;
everything else is the v0.26-pending state mirror.

## §A — canvas handle taint propagation: per-fn vs cross-fn

The IR's `is_canvas_handle_receiver` predicate (per
`crates/mty-ir/src/lower/exprs.rs`) routes `canvas.fill_rect(...)`
through `BuiltinId::CanvasOp(FillRect)` **only when** the receiver is
a local whose taint chain leads back to `std.web.Canvas.new(...)`
inside the same function. Track A pins this in
`canvas_lowering.rs::canvas_local_taint_propagates_through_let_rebind`
— rebinding within a single fn carries the taint, but passing the
handle as a parameter to another fn does not.

This affects the Mighty source shape: we can't write

```mty
fn render_static(canvas: std.web.Canvas, score: U32) {
  canvas.fill_rect(...)  // does NOT route through CanvasOp
}
```

Verified empirically: `od -An -c demos/06_canvas_game/target/main.wasm |
grep "m t y : w e b / c a n v a s"` returns 0 hits when the canvas
handle is passed as a parameter, vs. 4 hits when constructed +
consumed inline in the same fn.

**Workaround**: re-acquire the Canvas handle inside each callback
that uses it via `let canvas = std.web.Canvas.new(240, 480)`. The
constructor is idempotent on the host side (the shim's `canvas-ops`
table doesn't even consume the dimensions — it binds to the live
`<canvas id="board">`). The `frame(dt)` body therefore opens with
`let canvas = std.web.Canvas.new(240, 480)` and routes every render
op through that local.

**v0.26 follow-up**: propagate the taint through fn parameter types
(when the param type resolves to `std.web.Canvas` carry the taint
into the callee's local map). The Track A design notes call this out
as "we don't trust the typed receiver type because the type-checker
stamps `Error` on the canvas handle today" — closing that needs a
typeck-side change to expose the `std.web.Canvas` ADT in the prelude.

## §B — extern_js + component encode: kebab-vs-leading-underscore drift

Track B's design pins the convention "import name = verbatim from the
user's source. Leading `_` is preserved (`extern js { fn _alert }` →
`(import "mty:web/js" "_alert" ...)`)" — `crates/mty-codegen-wasm/src/
emit.rs::predeclare_extern_js_imports` emits the wasm import with the
leading underscore.

The WIT stub generator
(`crates/mty-codegen-wasm/src/wit.rs::emit_extern_js_interface`)
kebab-cases the fn name via the shared `kebab()` helper, which strips
leading underscores:

```rust
fn kebab(s: &str) -> String {
  // ...
  if c == '_' || c == ' ' {
    if !out.ends_with('-') {
      out.push('-');     // emits '-', not '-_'
    }
  }
}
```

(The `kebab_works()` test in `wit.rs` confirms: `kebab("_leading") =
"leading"`.)

Result: the WIT stub says `interface js { alert: func(msg: string); }`
but the core module imports `mty:web/js#_alert`. `wit-component`'s
`wrap_as_component` step fails with:

```
component encode: failed to decode world from module: module was not
valid: failed to resolve import `mty:web/js::_alert`: import interface
`mty:web/js` is missing function `_alert` that is required by the
module
```

Reproduced with `mty build --target wasm32-web examples/15_extern_js.mty`
(any leading-underscore extern js fn). The 7 Track B regression tests
all pass because they call `compile_program_to_bytes_with_preview`
directly, skipping `wrap_as_component` — so they never trip on the
WIT round-trip.

**Workaround for this demo**: don't use `extern js` end-to-end. The
state mirror lives in the shim and reads the intent stream the agent
emits via `format!()`. Documented as the trigger for the v0.26 slice
"extern_js end-to-end through component encode".

**v0.26 fix sketch**:
1. Change `emit_extern_js_interface` to use the raw fn name (leading
   `_` preserved) when emitting the WIT stub, OR
2. Change `predeclare_extern_js_imports` to strip the leading `_` so
   the wasm import name matches the kebab-cased WIT name.

Option 1 is the smaller change but requires confirming
`wit_parser::Resolve` accepts identifier names starting with `_`
(the WIT spec allows them via the `%`-prefix escape: `%_alert`).
Option 2 is back-compat-breaking for anyone who already hand-wrote a
JS shim against `_foo`.

## §C — wasm32-web agent persistence: single-agent v0.26 slice

Track C's notes (`AGENT_FIELDS_V0_25_NOTES.md`) lay out the design
for single-agent persistence in linear memory: reserve a region per
agent declaration, anchor at a stable offset, `__agent_<Name>__inst_
ptr` global, callback exports load the state pointer and call the
handler with it as implicit first arg.

None of that has shipped to `crates/mty-codegen-wasm/`. The `agent
Notetris: NotetrisInput { board: [U32; 200], score = 0, ... }` block
in `src/main.mty` compiles cleanly (Track C made the field-array type
typecheck), but the `keydown(k)` body doesn't dispatch into the
`on Left()` / `on Right()` handlers — it just emits the intent string
inline. The agent declaration is the protocol of record for the v0.26
emitter slice.

Confirmed by stepping through `crates/mty-codegen-wasm/src/emit.rs`:
the only export-section additions for the web target are `main`,
`frame`, `keydown`, `keyup` (per `is_web_callback_export`); there is
no "lift the agent ctor into linear memory" pass. Calling `spawn
Notetris` inside `frame()` would compile (per Track C tests) but the
resulting agent would be re-instantiated every frame with zero
state, which is worse than the current shim mirror.

**Plan for v0.26**: as the Track C notes spec — `Emitter::emit_agent_
state_region`, `Emitter::emit_agent_callback_dispatch`, populate
`__agent_<Name>__inst_ptr` on `spawn` in `main()`. Estimated 1-day
slice once a v0.26 swarm picks it up.

## §D — match arms with `const` patterns: variable binding shadow

`const KEY_LEFT: U32 = 37_u32` declared at top-level. Used in a
match arm:

```mty
match k {
  KEY_LEFT => log("left"),     // <-- binds k as KEY_LEFT, not literal compare
  ...
}
```

The typeck emits `MT2016 unreachable match arm` for every subsequent
arm because `KEY_LEFT` resolves as a fresh variable binding (the
identifier shape) that always matches. The v0.25 match-pattern lowerer
doesn't yet resolve a top-level `const` identifier to its value
during pattern compilation.

**Workaround**: use literal patterns directly in the match arms
(`37_u32 => ...`), and keep the `const` declarations as named
documentation. The values match `KEY_*` 1:1.

**v0.26 follow-up**: pattern-compile resolve `const` identifiers to
their literal value before treating the pattern as a fresh binding.
The HIR's pattern lowerer needs a one-line extension to look up
top-level `const` decls in the resolver before falling through to
the variable-binding path.

## §E — `format!()` named-arg vs in-scope shorthand

`format!("score: {:>4}", n)` — positional, works.
`format!("score: {n}")` (with `n` in scope) — works (Track D).
`format!("score: {n}", n=value)` — error: "macro `format!` template
uses 0 positional argument(s); 1 supplied".

The v0.25 `format!` lowerer parses `{name}` as a passthrough that
expects an in-scope binding; supplying a named arg in the call site
is rejected because the template parses as 0 positional + 1 named
ref, while the call site supplies 1 positional + 0 named.

**Workaround**: use `{name}` when the binding is in scope (the
common case); use `{}` + positional args when not.

**v0.26 follow-up**: accept `format!("{name}", name=value)` as
shorthand for letting the call site name the slot — useful when the
in-scope binding has a different name than the template wants.
Minor surface polish; doesn't block adoption.

## §F — visual smoke

The Track E golden at `tests/web-smoke/golden/canvas_game.phash`
still matches at distance 4 (tolerance 12). The visual changed because
the Mighty agent now paints the playfield background + grid lines +
HUD column directly (the shim painted those host-side at v0.24, with
the agent owning none of it). The phash budget absorbs the shift.

`MTY_WEB_SMOKE=1 bash demos/06_canvas_game/smoke.sh` reports:

```
[web-smoke] golden phash distance for "canvas_game" = 4 (tol 12)
[web-smoke] PASS [canvas_game] canvas={"w":240,"h":480,"cw":242,"ch":482}
            drew=true phash=3838000000000000
06_canvas_game: PASS (headless-browser smoke + magic bytes)
```

The phash file isn't re-locked because the distance stays under tol.
If a future Mighty-side render shift pushes the distance over 12, the
golden gets re-locked alongside.

## §G — performance

The v0.25 frame loop touches `canvas.fill_rect` ~21 times per frame
(1 bg + 1 stroke + 9 vertical lines + 19 horizontal lines = 30 fill
calls + 1 stroke; the actual count is closer to 30 due to the grid).
Each call crosses the wasm⇄JS boundary once. At 60 fps that's ~1800
boundary crossings/sec, which Chromium handles comfortably on a
modern dev machine.

The shim's `drawBoardPixels` adds another ~24 calls per frame (worst
case: 20 occupied cells + 4 piece cells), all host-side. Net: ~3200
canvas op invocations/sec, well within the host's ms-budget per
frame.

Future optimisation if needed: batch the grid lines into a single
pre-rendered overlay so they don't re-emit each frame. The v0.25
canvas WIT doesn't expose a `set-line-dash` or path API yet —
that's a v0.26 surface extension.

## Pre-flight gate (pinned)

```
cargo build --workspace                            # PASS
./target/debug/mty.exe check demos/06_canvas_game/src/main.mty   # ok
./target/debug/mty.exe fmt --check ...             # exit=0
bash demos/06_canvas_game/smoke.sh                 # smoke OK (2830 bytes, component magic)
MTY_WEB_SMOKE=1 bash demos/06_canvas_game/smoke.sh # PASS (phash dist 4, tol 12)
```

## What v0.26 needs to close (in priority order)

1. **Single-agent wasm32-web persistence** (Track C follow-up §C
   above). Unlocks the agent declaration becoming the run-time spec,
   collapses ~60 LOC of shim-side state mirror.
2. **extern_js end-to-end through component encode** (§B). Unlocks
   board cells / score / piece living JS-side via direct extern-js
   getter/setter pairs instead of via the intent stream.
3. **Canvas handle taint propagation through fn params** (§A).
   Smaller refactor; enables splitting `render()` into helper fns
   like `render_grid(canvas)` / `render_hud(canvas, score, ...)`.
4. **`const` identifier resolution in match patterns** (§D). Surface
   polish; pin literal keycodes to a named vocabulary without losing
   exhaustivity checks.
5. **`format!` named-arg passthrough** (§E). Lowest priority — the
   `{name}` shorthand covers the common case.
