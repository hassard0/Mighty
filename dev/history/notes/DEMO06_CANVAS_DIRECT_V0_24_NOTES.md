# demo 06 canvas-game — v0.24 refresh notes

Track E notes for the v0.24 refresh of `demos/06_canvas_game` against
the two language wins that landed earlier in v0.24:

- Track A (`aef5225`) — `BuiltinId::CanvasOp(CanvasOpKind)` + the
  wasm32-web emitter's `mty:web/canvas@0.1` import declaration +
  `export fn frame/keydown/keyup` -> embedded-core export-section
  wiring via `is_web_callback_export` in `crates/mty-codegen-wasm/src/web_lower.rs`.
- Track B (`2399391`) — `format!("...", ...)` as a first-class
  macro in `crates/mty-macros/src/stdlib/format.rs`, lowering through
  the prelude `fmt` interner to the SIR and the wasm32-web backend.

## Context (v0.23 -> v0.24 delta)

The v0.23 Track D ship of this demo (commit `13f6d4a`) was a hybrid:
the Mighty agent owned the score / level / lines / `gameover` state
+ the input-intent log stream, but the JS shim still owned the
board / piece / gravity / collision / line-clear logic + the
browser-key -> intent-tag translation table. The v0.23 notes
(`CANVAS_GAME_V0_23_NOTES.md`) called out three blocking gaps:

| v0.23 gap                                                                     | status at v0.24                                          |
| ----------------------------------------------------------------------------- | -------------------------------------------------------- |
| 1. `canvas.fill_rect(...)` doesn't lower from Mighty source                   | **PARTIALLY** closed — IR + emitter ready, HIR side gap  |
| 2. No `format!()` / string interpolation                                      | **CLOSED** by Track B end-to-end through wasm32-web      |
| 3. `frame` / `keydown` / `keyup` don't reach the core-module export table     | **CLOSED** by Track A's `is_web_callback_export`         |

This file documents what we attempted, what worked, and what
required a v0.25 closer.

## Verified working at v0.24 (end-to-end through wasm32-web)

1. **`format!()` inside an exported callback.** The agent's
   `fn keydown(k: U32)` uses `format!("evt:input:unknown:{}", k)`
   to emit an intent line carrying the keycode for unrecognized
   keys. Probe `/tmp/mtyprobe/probe29.mty`:

   ```mty
   fn keydown(k: U32) { log(format!("kd:{}", k)) }
   ```

   builds clean and the embedded core module exports `keydown`. The
   `format!` macro expands through the Track B lowerer + the
   prelude `fmt` interner; the resulting `Str` argument flows
   straight into the `mty:web/log#log` import call.

2. **`fn frame(dt: U32)` reaches the core export table.** Probe
   `/tmp/mtyprobe/probe12.mty`:

   ```mty
   fn frame(dt: U32) { log("f") }
   fn keydown(k: U32) { log("kd") }
   fn keyup(k: U32) { log("ku") }
   ```

   Inspecting the embedded core module via `WebAssembly.Module.exports(...)`:

   ```
   EXPORT main
   EXPORT frame
   EXPORT keydown
   EXPORT keyup
   EXPORT cabi_realloc
   EXPORT memory
   ```

   These are now real callable wasm exports — no more
   `inst.exports.frame is not a function` traps.

## Required-fallback gaps that surfaced during this rewrite

### A. HIR -> IR routing for `canvas.*` method calls is missing.

Track A landed the IR variant + the emitter, but
`crates/mty-ir/src/lower/exprs.rs` doesn't recognise a
`std.web.Canvas`-typed receiver and route the call into
`BuiltinId::CanvasOp(...)`. The MethodCall lowerer only has a
specialisation for `CapFamily::Dom`:

```rust
if is_dom_cap_receiver(ctx, receiver) {
    // … emits BuiltinId::DomOp(method.clone())
}
```

`std.web.Canvas` is a regular opaque struct (no
`CapFamily::Canvas` in `crates/mty-types/src/ty.rs`), so the
method call falls through to `BuiltinId::Extern("canvas.fill_rect")`
which the wasm backend drops on the floor without diagnosis.

**Probe** (`/tmp/mtyprobe/probe5.mty`):

```mty
fn main() {
  let canvas = std.web.Canvas.new(240, 480)
  canvas.fill_rect(0, 0, 240, 480, 487724799)
}
```

Embedded-core import section: only `mty:web/log#log` +
`mty:web/dom#{set-text,get-text,on-click,query}`. **No
`mty:web/canvas` imports** — confirming the source-level
`fill_rect` call never reaches the new emitter machinery.

**v0.25 closer**: add `CapFamily::Canvas` (or a parallel
`is_canvas_handle_receiver` predicate) + a per-method map from the
`std.web.Canvas` method name to the matching `CanvasOpKind`, and
wire it into the MethodCall arm next to the DOM branch.

### B. Calling a Unit-returning user fn from `keydown` / `frame` breaks the v0.24 callback prologue.

**Probe** (`/tmp/mtyprobe/probe22.mty`):

```mty
fn _h() { log("h") }
fn frame(dt: U32) { _h() }
fn main() { _h() }
```

```
build error: wasm: wasm codegen: invalid module: component encode:
  failed to validate component output: type mismatch: expected i32
  but nothing on stack (at offset 0x1b2)
```

The same `_h()` called from `main()` *also* fails, so this isn't
specific to the new `frame` / `keydown` / `keyup` exports — it's
a v0.24 emitter regression on Unit-returning user-fn calls in
general. The existing v0.23 demo 06 didn't hit it because its
agent `on Reset() { ... }` etc. handlers are never dispatched from
the exported callbacks (dead-code-eliminated), so no Unit call
crosses the export boundary.

Workaround: inline the dispatch logic into the exported
callbacks (`match k { 37 => log(...), ... }`). The v0.24 demo
source does exactly this — the `agent Notetris { on Left() ... }`
handlers stay as the protocol-of-record but the runtime work
happens inline in `fn keydown(k: U32)`.

**v0.25 closer**: pin down the stack-balance bug in the codegen.
Likely candidate: `declare_fns` does not push the implicit
stack-frame `i32` for callsites that target Unit-returning user
fns when the result isn't consumed (the v0.23 path always sank
the result into a temp; the v0.24 export-shape path skips that).
A regression test under `crates/mty-codegen-wasm/tests/` against
`probe22.mty` shape would lock it.

### C. Agent fields don't survive across exported callback invocations.

`agent Notetris { score = 0; on Left() { score += 1 } }` compiles
cleanly. But the wasm exports `keydown(k: U32)` etc. are
free-standing functions in the embedded core module; they don't
hold a handle to a spawned `Notetris` instance, so the score
mutation happens in a fresh agent each time and isn't observable
across calls.

The v0.23 demo papered over this by emitting the canonical intent
log line and letting the JS shim maintain durable state. The
v0.24 demo does the same. **v0.25 closer**: add a top-level
`spawn`-once + `send` pattern that the export-fn prologue can
dispatch into, or wire `let mut` at module scope into a wasm
global the callbacks share.

### D. Arrays in agent fields don't parse.

**Probe** (`/tmp/mtyprobe/probe9.mty`):

```mty
agent Game {
  board: [U32; 200] = [0; 200]
}
```

```
[MT0001] Error: expected agent member (`on`, `fn`, or state field)
```

The agent state-field grammar is `name = value` (no type
annotation, no array literal). Even if the parser learnt the
fixed-size array shape, the 200-cell board would still need cap-C
above to persist across callback invocations.

### E. `extern js { fn _foo(...) effect dom }` declarations don't surface as wasm imports.

**Probe** (`/tmp/mtyprobe/probe6.mty`): declared a handful of
`_canvas_*` externs in an `extern js` block. The embedded core
module's import section is unchanged from the no-extern baseline.
The `extern js` decl is parsed + type-checked but the wasm32-web
emitter doesn't yet route it to a `(import "...")` entry. (Native
backends handle `extern c { ... }` via the `cdylib` path; the wasm
side hasn't grown its analogue.)

This means we can't sneak ahead of cap-A by declaring
`extern js { fn _canvas_fill_rect(...) }` — the binding compiles
to a fn declaration the emitter then drops.

## Shape we shipped at v0.24

- **Mighty source** (186 LOC): the `agent Notetris` declaration
  stays (now the protocol-of-record) + inline dispatch in `fn
  keydown(k: U32)` + `format!()`-based intent emission with the
  keycode interpolated for unrecognized keys.
- **JS shim** (213 LOC, -22 vs v0.23): host glue only — no
  browser-key -> intent translation table (the wasm does it via
  `format!`), no `setOnFrame` RAF wrapper (RAF directly calls
  `exp.frame(dt)`), no `exp?.input_*?.()` defensive chain
  (Track A's exports are real now). The game-logic mirror
  (board / piece / gravity / collision / line-clear) stays
  because cap-A above blocks moving it Mighty-side.
- **Headless smoke** (`MTY_WEB_SMOKE=1 bash demos/06_canvas_game/smoke.sh`):
  PASS at phash distance 0 against the v0.23 Track E golden
  (`tests/web-smoke/golden/canvas_game.phash`) — the rewrite is
  visually identical to the v0.23 shape, no golden re-lock needed.

## Footprint snapshot

| artifact            | bytes  |
| ------------------- | ------ |
| `target/main.wasm`  | 2389   |

Embedded core module:

| section  | contents                                                |
| -------- | ------------------------------------------------------- |
| imports  | `mty:web/log#log` + 4x `mty:web/dom#{...}`              |
| exports  | `main`, `frame`, `keydown`, `keyup`, `cabi_realloc`, `memory` |

Comparison vs v0.23 (commit `13f6d4a`): identical imports + 3 new
exports (`frame`, `keydown`, `keyup`). The artifact is ~50 bytes
smaller because the v0.23 source carried the redundant v0.22-shape
`input_left()` etc. exports alongside the canonical shape; the
v0.24 source ships only the canonical shape.
