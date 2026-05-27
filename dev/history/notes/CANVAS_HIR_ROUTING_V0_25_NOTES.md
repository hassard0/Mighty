# Canvas HIR → IR routing + Unit-fn stack-balance fix — v0.25 Track A notes

Two related fixes that close v0.23 → v0.24 unfinished business from
`dev/history/notes/DEMO06_CANVAS_DIRECT_V0_24_NOTES.md`. After this
slice:

1. Mighty source `canvas.fill_rect(...)` lowers through HIR → IR →
   wasm32-web emit and surfaces as a `mty:web/canvas@0.1#fill-rect`
   WIT import in the embedded core module.
2. Calling a Unit-returning user fn from inside an exported callback
   (`fn keydown(k: U32) { _helper() }`) no longer trips the wasm
   validator with "type mismatch: expected i32 but nothing on stack".

## Fix 1 — Canvas HIR → IR routing

### Root cause

v0.24 Track A landed:

- `BuiltinId::CanvasOp(CanvasOpKind)` IR variant +
  `CanvasOpKind::{Clear, FillRect, StrokeRect, FillText,
  SetFillStyle, Width, Height, RequestAnimationFrame}` pinned
  against the eight `mty:web/canvas@0.1` WIT methods.
- `Emitter::predeclare_canvas_imports` + `Emitter::canvas_import` +
  the `FnRef::Builtin(BuiltinId::CanvasOp(kind))` dispatch arm in
  `emit_call`.

But **no path connected Mighty source to that IR variant**. The IR
lowerer's MethodCall + local-method-call arms checked for `Cap {
family: Dom }` receivers (for `BuiltinId::DomOp`) but had no
analogue for canvas receivers. Source-level `canvas.fill_rect(...)`
fell through to `Rvalue::MethodCall` (generic dispatch) and the
canvas import was never declared.

### Detection key

We can't trust the typed receiver: v0.23-era `std.web.Canvas` isn't
in the type-checker's prelude (no `std.web` module, no `Canvas`
ADT), so the type-checker stamps the canvas handle as
`TyData::Error`. Adding the prelude entries would force a parallel
edit across `mty-types` + every type-checker test that already
depends on the empty-prelude shape — out of scope for this slice.

Instead we taint locals known to hold canvas handles, via a per-fn
`HashSet<Local>` on the IR `FnBuilder`. The taint flows:

1. `lower_call` arm handling `std.web.<X>.<m>(...)` (module-receiver
   effect_invoke). When the full path matches the
   `CANVAS_CONSTRUCTOR_PATH = "std.web.Canvas.new"` constant, the
   temp local receiving the call result is added to `canvas_locals`.
2. `bind_pat_assign` in `lower/stmts.rs`. When a let-binding's rhs
   is a `Move`/`Copy` of a canvas-tainted local, the new binding
   inherits the taint. This closes `let canvas =
   std.web.Canvas.new(...)` (and the `let c2 = canvas` rebinding
   case).
3. `lower_expr` MethodCall arm + `lower_call` local-method-call arm.
   When the receiver resolves to a canvas-tainted local AND the
   method name is one of the eight canonical canvas methods (via
   `canvas_op_for_method` lookup), the call is rewritten to
   `Rvalue::Call { func: FnRef::Builtin(BuiltinId::CanvasOp(kind)) }`
   instead of `Rvalue::MethodCall`.

### Surface-shape quirk

`canvas.fill_rect(...)` parses as `CALL_EXPR` with a `PATH_EXPR`
callee `[canvas, fill_rect]`, NOT as `METHOD_CALL_EXPR`. So the
dispatch lives in `lower_call`'s "local.method(args)" arm, not in
`lower_expr`'s `HirExpr::MethodCall` arm. Both arms got the canvas
branch for symmetry — chained receivers like `(some.expr).fill_rect(...)`
would take the `MethodCall` path — but the dominant shape today goes
through `lower_call`. The HIR test file pins this surface assumption.

### Files touched

- `crates/mty-ir/src/lower/ctx.rs` — add `canvas_locals` field +
  `mark_canvas_local` / `is_canvas_local` helpers on `FnBuilder`.
- `crates/mty-ir/src/lower/stmts.rs` — propagate canvas-taint
  through let-binding hand-offs (`bind_pat_assign`).
- `crates/mty-ir/src/lower/exprs.rs` — canvas-receiver dispatch in
  both `lower_expr::HirExpr::MethodCall` and
  `lower_call::"local.method(args)"` arms; canvas-constructor
  detection in `lower_call::"module-receiver effect_invoke"` arm;
  `canvas_op_for_method`, `is_canvas_handle_receiver`, and
  `CANVAS_CONSTRUCTOR_PATH` helpers.

### Probe (verified end-to-end)

```mty
package canvas_probe

fn main() {
  let canvas = std.web.Canvas.new(240, 480)
  canvas.fill_rect(0, 0, 240, 480, 487724799)
}
```

`mty dump --ir` after the fix:

```
fn0: main() -> Unit {
  let mut _0: Unit /* _ret */
  let mut _1: {error}
  let _2: {error} /* canvas */
  let mut _3: {error}

  bb0:
    _1 := effect[0](std.web.Canvas.new, 240, 480)
    _2 := move _1
    _3 := call @canvas.fill_rect(0, 0, 240, 480, 487724799)
    return move _3
}
```

`mty build --target wasm32-web` then emits a Component whose
embedded core module imports `mty:web/canvas` `fill-rect`. No
changes to the dom-shim contract.

## Fix 2 — Unit-returning user-fn stack-balance regression

### Root cause

`crates/mty-codegen-wasm/src/emit.rs::emit_call` — the
`FnRef::User(callee)` arm emitted a `call <idx>` instruction and
returned. But `emit_assign` always emits either `local.set` or
`drop` afterwards, and BOTH expect a value on the stack. A user fn
declared `fn ... -> ()` lowers to a `(func ... (result))` wasm
signature via `Self::fn_sig_for`, so the call leaves ZERO values
on the stack — the next `local.set` / `drop` then fails wasm
validation with "expected i32 but nothing on stack".

Every other arm of `emit_call` already pushes a placeholder
`i32.const 0` after a void call for exactly this reason (`Log` /
`DomOp` / `CanvasOp` for void ops / P2 direct-imports). The User
arm was the lone gap.

### Why v0.22 / v0.23 didn't trip on this

The pre-v0.24 callback shape (`agent on Reset() { ... }` handlers)
was never dispatched from the exported wasm functions because the
core module didn't export `frame` / `keydown` / `keyup`. The handler
bodies dead-code-eliminated. v0.24 Track A added
`is_web_callback_export` and started exporting the callbacks, which
made calling a Unit-returning helper from inside `fn keydown(k:
U32) { _helper() }` reachable for the first time — and tripped the
gap.

### Fix

```rust
FnRef::User(callee) => {
    // ... emit call ...
    let callee_returns_value = self
        .prog
        .fns
        .iter()
        .find(|f| f.id == *callee)
        .map(|f| !matches!(f.ret_ty, IrTy::Unit | IrTy::Never))
        .unwrap_or(true);
    if !callee_returns_value {
        wfn.instruction(&I::I32Const(0));
    }
    Ok(())
}
```

`unwrap_or(true)` is the conservative default — if we can't find
the callee fn (unreachable in practice), assume it returns a value
and don't push the placeholder (which would cause an "extra value
on stack" error). The lookup is `O(n)` over `prog.fns`, which is
fine: this code path runs once per call site at compile time and
`n` is small (the largest demo today has ~10 user fns).

### Files touched

- `crates/mty-codegen-wasm/src/emit.rs` — the `FnRef::User` arm
  fix in `emit_call`.

### Probe (verified)

```mty
package unit_probe

fn _h() { log("h") }
fn frame(dt: U32) { _h() }
fn main() { _h() }
```

Compiles cleanly post-fix. Pre-fix:
```
build error: wasm: wasm codegen: invalid module: component encode:
  failed to validate component output: type mismatch: expected i32
  but nothing on stack (at offset 0x1b2)
```

## What Track F can now consume

- Mighty source `canvas.fill_rect(...)` is wire-level ready. The
  v0.24 demo 06's "v0.25 closer A" gap is closed; demo 06 can drop
  its JS-side board-pixel mirror and own the canvas Mighty-side
  using `std.web.Canvas` directly.
- Helpers can be factored out of `keydown` / `frame` / `keyup` —
  e.g. `fn handle_left() { log("evt:input:left") }` called from
  `keydown` — without tripping wasm validation. The demo's inline
  `match` dispatch can stay (clean already) or move into helpers as
  the readability pendulum prefers.

## Tests

- `crates/mty-hir/tests/canvas_method_lowering.rs` — 5 tests pinning
  the HIR dot-call shape (the IR layer's pattern-match contract).
- `crates/mty-ir/tests/canvas_lowering.rs` — 7 tests covering the
  IR-side routing: positive (all 8 ops), negative (unknown method
  falls through, non-canvas receiver doesn't misroute), and taint
  propagation (let-rebind, multiple call sites).
- `crates/mty-codegen-wasm/tests/wasm32_web_emit_completion.rs` —
  extended with 5 new tests: 3 for the stack-balance fix
  (`unit_return_user_fn_from_keydown_callback_no_stack_imbalance`,
  `unit_return_user_fn_called_multiple_times_no_stack_imbalance`,
  `non_unit_returning_user_fn_still_works`) + 2 source-to-wasm
  end-to-end tests (`canvas_method_via_hir_lowers_to_wit_import`,
  `canvas_multiple_methods_via_hir_all_emit_imports`).

Total: 17 new tests across 3 new/extended files, plus all 10 v0.24
Track A completion tests still pass.
