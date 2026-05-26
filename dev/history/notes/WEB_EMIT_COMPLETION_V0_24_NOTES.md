# v0.24 Track A — wasm32-web emitter completion

Closes the v0.23 Track D log-line fallback by giving the wasm32-web
core-module emitter two missing capabilities:

1. **`BuiltinId::CanvasOp` lowering** — `canvas.fill_rect(...)` calls
   that surface in the SIR as `BuiltinId::CanvasOp(CanvasOpKind)`
   are lowered to direct `call $imported_canvas_*` instructions
   against the eight `mty:web/canvas@0.1` WIT imports.

2. **`export fn` reaches the core module export section** — any SIR
   `Function` whose name matches the canonical web-agent export set
   (`frame`, `keydown`, `keyup`) — i.e. the names pinned by Track A's
   stdlib (`WIT_EXPORT_FRAME`, `WIT_EXPORT_KEYDOWN`, `WIT_EXPORT_KEYUP`)
   — now lands in the core module's export section under that exact
   name. The host shim (`web/dom-shim.js`) can `inst.exports.frame(dt)`
   directly without going through the v0.23 log-line fallback.

## Pre-investigation findings (2026-05-26)

### Existing CanvasOp surface

There is **no** `BuiltinId::CanvasOp` variant in `mty-ir/src/ir.rs`
today. The v0.23 SIR enum (`BuiltinId`) carries only
`Log | Print | Panic | Spawn | Move | Fetch | RawPtr | Valid | Null |
Extern(String) | DomOp(String)`. The web stdlib bindings
(`crates/mty-stdlib/src/web/canvas.rs`) record `CanvasCall`s into a
native-fallback log but the SIR lowerer doesn't yet branch on the
Canvas-cap receiver the way it does for the DOM cap.

We add **`BuiltinId::CanvasOp(CanvasOpKind)`** — a string-free typed
variant so backends can match on the canonical op without re-parsing
a method name. `CanvasOpKind` enumerates the eight WIT methods
(`Clear`, `FillRect`, `StrokeRect`, `FillText`, `SetFillStyle`,
`Width`, `Height`, `RequestAnimationFrame`).

This is **additive only**: we do not touch any existing variant; we
add one. The four downstream pattern matches that span the enum
(`mty-ir::dump`, `mty-ir::interp::run`, `mty-codegen-cranelift`) get
one new arm each. The LLVM backend already has a `FnRef::Builtin(_)`
catch-all, so it picks up the no-op fallback automatically.

### WIT import names (Track A v0.23 pinned)

From `crates/mty-stdlib/src/web/canvas.rs`:

```
WIT_IMPORT_CLEAR                       ("mty:web/canvas@0.1", "clear")
WIT_IMPORT_FILL_RECT                   ("mty:web/canvas@0.1", "fill-rect")
WIT_IMPORT_STROKE_RECT                 ("mty:web/canvas@0.1", "stroke-rect")
WIT_IMPORT_FILL_TEXT                   ("mty:web/canvas@0.1", "fill-text")
WIT_IMPORT_SET_FILL_STYLE              ("mty:web/canvas@0.1", "set-fill-style")
WIT_IMPORT_WIDTH                       ("mty:web/canvas@0.1", "width")
WIT_IMPORT_HEIGHT                      ("mty:web/canvas@0.1", "height")
WIT_IMPORT_REQUEST_ANIMATION_FRAME     ("mty:web/canvas@0.1", "request-animation-frame")
```

The emitter declares one core-wasm import per method, lazily, the
first time a function body references it (mirroring the v0.5 DOM
pattern). Each import sits in the import section before any user fn,
and the resulting function-index is cached on the `Emitter` so the
second call to the same op reuses it.

#### Signatures (from `wit/mty-web/canvas.wit`)

| Method                  | Core-Wasm shape                       |
| ----------------------- | -------------------------------------- |
| clear                   | `() -> ()`                             |
| fill-rect               | `(i32, i32, i32, i32, i32) -> ()`      |
| stroke-rect             | `(i32, i32, i32, i32, i32) -> ()`      |
| fill-text               | `(i32, i32, i32, i32, i32) -> ()`     (ptr,len,x,y,color) |
| set-fill-style          | `(i32) -> ()`                          |
| width                   | `() -> i32`                            |
| height                  | `() -> i32`                            |
| request-animation-frame | `() -> ()`                             |

`s32` and `u32` both lower to wasm `i32` at the canonical-ABI flat
layer. `string` (in `fill-text`) lowers to the `(ptr, len)` pair
convention the emitter already uses for `log` strings.

### Export-fn → core export section — current behaviour

`Emitter::declare_fns` (emit.rs:816) currently exports only `main`:

```rust
if f.name == "main" {
    self.export_section.export("main", ExportKind::Func, fn_idx);
}
```

The downstream WIT generator (`wit.rs::is_exportable_fn`) already
considers any non-underscore-prefixed fn as exportable; the
mismatch is that the WIT contract advertises `export frame: func(...)`
but the core module never installs a `frame` export. The JS host's
`inst.exports.frame(t)` therefore traps with "frame is not a function".

The fix: when `target == Web`, also export every fn whose name is in
the canonical web-export set (`frame`, `keydown`, `keyup`). Other
non-underscore-prefixed user fns are kept exportable too (matches
the WIT contract). For `wasm32-wasi` the existing `main`-only
behaviour is preserved verbatim.

## Implementation plan

1. `crates/mty-ir/src/ir.rs`: add `CanvasOpKind` enum + extend
   `BuiltinId` with `CanvasOp(CanvasOpKind)`. New helper
   `CanvasOpKind::as_wit_method() -> &'static str`.

2. Touch the four downstream pattern-match sites with a one-line arm
   each:
   - `mty-ir::dump` (debug print)
   - `mty-ir::interp::run` (routes through `host.extern_call("canvas.<op>", ..)`
     so headless tests get the same shape as DomOp)
   - `mty-codegen-cranelift::lower` (return zero-placeholder — canvas
     is wasm32-web-only, same as DomOp)
   - LLVM backend: no change needed; the existing `FnRef::Builtin(_)`
     fallback already returns zero.

3. `crates/mty-codegen-wasm/src/web_lower.rs` (NEW): self-contained
   helper that declares each canvas import on demand and emits the
   right argument lowering for each op.

4. `crates/mty-codegen-wasm/src/emit.rs`:
   - Add `web_lower::CanvasImports` state to `Emitter`.
   - Add a `FnRef::Builtin(BuiltinId::CanvasOp(kind))` arm to
     `emit_call` that pushes args + dispatches via the helper.
   - In `declare_fns`, additionally export any fn whose name is in
     the canonical web-export set (`frame`, `keydown`, `keyup`) on
     the Web target.

5. `crates/mty-codegen-wasm/src/lib.rs`: re-export the new helper +
   the `CanvasOpKind` enum so external test crates can build SIR
   programs that exercise this.

6. Tests: 7+ as listed in the spec. Each uses `wasmparser` to walk
   the emitted core module's import/export sections and assert the
   right names land.

## Risks + mitigations

- **Risk**: adding a `BuiltinId` variant breaks ~4 downstream crates.
  - Mitigation: every change is one new pattern arm, never a
    modification to an existing one. Workspace build runs in <2 min.

- **Risk**: blanket-exporting every non-underscore fn surfaces
  internal helpers in the core module unexpectedly.
  - Mitigation: only export the three canonical web-callback names
    (`frame`, `keydown`, `keyup`) plus `main`. Other user fns stay
    hidden — matches the v0.23 surface.

- **Risk**: the v0.23 `wasm32_web_core` regression suite asserts
  exactly which exports are present.
  - Mitigation: it asserts `main` is exported and validates the
    whole component. Adding more exports doesn't break either.
