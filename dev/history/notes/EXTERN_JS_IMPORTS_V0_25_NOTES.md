# v0.25 Track B — `extern js { fn _foo() }` actually emits wasm imports

**Status**: shipped (v0.25 Track B)
**Files touched**:
- `crates/mty-ir/src/ir.rs` — added `ExternBinding` + `Program::extern_bindings`
- `crates/mty-ir/src/lower/items.rs` — `record_extern_bindings` populates the table
- `crates/mty-codegen-wasm/src/emit.rs` — `predeclare_extern_js_imports`, `fn_sig_for_extern_js`, skip-list in `declare_fns` + `emit`
- `crates/mty-codegen-wasm/src/wit.rs` — `collect_extern_js_fns`, `emit_extern_js_interface`, world-import line
- `crates/mty-codegen-wasm/tests/extern_js_imports.rs` — 7 regression tests

## Problem (carried over from v0.24 Track E)

`examples/15_extern_js.mty` declared `extern js { fn _alert(msg: Str) effect dom }` — the canonical "Mighty calls into the JS host" surface. But the wasm32-web emitter treated extern-js fns as ordinary user functions:

1. The HIR lowerer surfaced them as `HirExternBlock` with `abi: Some("js")` and one `FnId` per extern fn.
2. The IR lowerer dropped the ABI tag entirely and emitted a stub `Function` with an empty body returning `()`.
3. The wasm emitter saw a `Function`, gave it a module-local fn slot, and emitted nothing more.

Result: the wasm core module had no `(import "mty:web/js" "_alert" ...)` entry. The user-code call to `_alert("hi")` resolved to the local stub fn — a silent no-op. `extern js` was effectively documentation, not a binding.

## Fix

Three layers, with the IR change designed to be the minimum needed to plumb the ABI tag from HIR to the wasm codegen without touching the dozens of test fixtures that construct `Function` literals directly.

### IR side-table (`Program::extern_bindings`)

A `HashMap<IrFnId, ExternBinding>` (`ExternBinding { abi, name }`) hangs off `Program` alongside `span_table`. Manually-constructed test fixtures leave the slot empty (matches `span_table`'s behaviour). The IR lowerer populates it from each `Item::ExternBlock` via the new `record_extern_bindings` pass in `register_fn_shells`.

### Wasm emitter pre-declare pass

`predeclare_extern_js_imports` runs in `Emitter::emit` immediately before `declare_fns` (same protocol as `predeclare_canvas_imports` and `predeclare_p2_direct_imports` — the function-index space counts imports and module-local funcs together, so any import slot reserved mid-emit shifts every later function's index).

For each `(IrFnId, ExternBinding { abi: "js", name })`:
- Build a wasm signature via `fn_sig_for_extern_js` (string params expand to `(ptr:i32, len:i32)` pairs — matches what `emit_const(Const::Str(...))` pushes at the call site, and the canonical-ABI flat layout the other `mty:web/*` imports use).
- Append `(import "mty:web/js" "<name>" (func ...))` to the import section.
- Record `fn_index[fn_id] = <new import idx>` so call-site dispatch via `FnRef::User(callee)` naturally lands on the import — no separate `BuiltinId::ExternJs` arm needed.
- Mark the fn in `extern_js_fns` so `declare_fns` + the body-emit loop skip it (the empty body would otherwise be a wasted module-local function).

### WIT stub

`emit_wit` adds `import mty:web/js;` to the world *only* when the program declared at least one extern-js fn (keeping the surface clean for unrelated demos), and `append_host_stubs` emits a per-program `interface js { ... }` inside the `mty:web` package listing each declared fn. The stub uses WIT-native types (`string`, `u32`, etc.) — `wit-component`'s canonical-ABI lifter expands them into the same `(ptr, len)` pairs the emitter spliced into the core module.

## Naming convention

- Import module: `mty:web/js` (kebab-case, matches `mty:web/dom`, `mty:web/canvas`, `mty:web/input`, `mty:web/log`).
- Import name: verbatim from the user's source. Leading `_` is preserved (`extern js { fn _alert }` → `(import "mty:web/js" "_alert" ...)`).
- WIT interface name inside the stub: bare `js` (no version pin yet — extern-js is per-program and the surface is unstable. A v0.26 follow-up will add `@0.1` once the host-shim API is locked).

## Why not a `BuiltinId::ExternJs(name)` variant?

Two reasons:

1. The call dispatch in `emit_call` would need a new arm; today's `FnRef::User(callee)` arm already does the right thing once `fn_index` points at the import slot.
2. The IR has hundreds of test fixtures that construct `Function` + `Program` literals. Adding a new `FnRef` variant would force every fixture to either explicitly opt out or rebuild. The side-table approach is sparse — fixtures that don't care leave the field empty and get the legacy behaviour.

## Test coverage (`crates/mty-codegen-wasm/tests/extern_js_imports.rs`)

1. `extern_js_fn_emits_import` — minimal `extern js { fn _foo() }`; wasm has the import.
2. `extern_js_fn_with_args` — `_bar(x: I32, y: I32)`; import sig is `(i32, i32) -> ()`.
3. `extern_js_fn_with_return` — `_len(s: Str) -> U32`; Str param expands to `(i32, i32)`, return is `i32`.
4. `extern_js_call_routes_to_import` — `main()` calls `_foo()`; the wasm body contains `Call(idx)` where `idx` equals `_foo`'s import index.
5. `extern_js_unused_still_imported` — declare-but-don't-call: the import is still emitted (defensive — JS-side feature flags). Also asserts the WIT world has `import mty:web/js;`.
6. `extern_js_underscore_prefix_works` — leading `_` is preserved in the import name AND the fn does NOT appear in the WIT world's export list (`is_exportable_fn` already filters underscore-prefixed names; we confirm the import side honours the convention too).
7. `example_15_extern_js_compiles_with_imports` — pipelines `examples/15_extern_js.mty` end-to-end through parser → HIR → typeck → IR → wasm, then asserts the resulting wasm imports `mty:web/js#_alert`. This is the regression test for the v0.24 Track E gap.

## What Track F can now consume

- `examples/15_extern_js.mty` (and any demo that declares `extern js`) compiles to a Component whose core module has real `(import "mty:web/js" "<name>" ...)` entries — the JS shim can bind them via `instantiate(modBytes, { "mty:web/js": { _alert: (ptr, len) => alert(readStr(ptr, len)) } })`.
- `wit-component` no longer rejects extern-js programs at component-encode time — the `mty:web/js` stub interface in the generated WIT carries the matching signatures.
- The convention is fixed: import module `mty:web/js`, kebab-cased WIT name, verbatim wasm import name with leading underscore preserved.

## Known limitations (v0.26 follow-ups)

- String *returns* from extern-js fns aren't supported yet. The canonical-ABI requires a return-area pointer (see `emit_dom_call`'s `get-text` branch for the reference pattern). Today's `fn_sig_for_extern_js` only handles scalar returns.
- The wasi target ignores extern-js declarations (the wasm-component ecosystem has no `mty:web/js` host). This is documented; non-web targets that want JS interop should use `extern c` instead.
- No version pin on the `mty:web/js` interface yet — extern-js fns vary per program so the stub interface is regenerated each build. A future slice will add a versioned host-stub once the binding shape is locked.
