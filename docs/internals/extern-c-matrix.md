# Extern C signature matrix (v0.36 Track T2)

This document is the human-readable mirror of `tests/extern_c_matrix/`.
It pins which C-ABI shapes Mighty can call end-to-end today, what the
manifest contract is for linking against vendored archives, and which
shapes are deferred to v0.37 (with the workaround the matrix tests
adopt in the meantime).

Audience: anyone shipping an FFI Mighty app — first-class downstream
consumers are the native-IDE track (`C:\Users\ihass\mighty-ide`) and
any user wrapping a C SDK (wgpu, winit, libsodium, …).

## TL;DR

1. Declare each native library in `mighty.toml` under one or more
   `[[extern_lib]]` blocks (schema below).
2. Declare each C-ABI function in an `extern c { ... }` block at the
   top of your Mighty source. Signatures use the standard Mighty type
   syntax — `*U8` for pointers, `USize` for `size_t`, `I32` / `I64` for
   `int32_t` / `int64_t`, etc.
3. Call those functions like any other Mighty fn. The cranelift backend
   declares them as `Linkage::Import` and the system linker resolves
   them from the archives you named in step 1.

## Manifest schema — `[[extern_lib]]`

```toml
[package]
name    = "ffi_demo"
version = "0.1.0"
edition = "2026"

[[extern_lib]]
name = "winit"
kind = "static"               # or "dynamic"; default = static
path = "vendor/libwinit.a"    # optional; falls back to -l<name>

# Cross-platform raw linker flags (always applied).
link_args = ["--whole-archive"]

# Host-OS-specific flags. Filtered by `cfg(target_os)` at build time.
link_args_macos   = ["-framework", "Cocoa"]
link_args_linux   = ["-lxkbcommon", "-lX11"]
link_args_windows = ["Userenv.lib"]
```

Fields:

| Field                | Required | Notes |
|----------------------|----------|-------|
| `name`               | yes      | Logical name. Used as `-l<name>` when `path` is absent. |
| `kind`               | no       | `"static"` (default) or `"dynamic"`. Case-insensitive. |
| `path`               | no       | Filesystem path relative to the manifest directory. Bypasses the linker search path. |
| `link_args`          | no       | Raw flags appended after the archive. |
| `link_args_linux`    | no       | Linux-only flags. |
| `link_args_macos`    | no       | macOS-only flags. |
| `link_args_windows`  | no       | Windows-only flags. |

Multiple `[[extern_lib]]` entries are honored in source order. The
linker walks them in the same order, so put archives that need other
archives' symbols *first*.

## Mighty `extern c` block

```mighty
extern c {
  fn winit_create_window(title: *U8, w: I32, h: I32) -> *Mut WindowHandle
  fn winit_destroy_window(handle: *Mut WindowHandle)
}
```

- The block tag is the **ABI** — currently `c` and `js`. (`js` is the
  wasm-host browser binding; see `docs/internals/wasm.md`.)
- Each fn's signature parses with the regular Mighty type grammar.
- The compiler stores an `ExternBinding { abi, name }` per fn in
  `Program::extern_bindings`. The cranelift backend reads that table
  during `declare_fns` and declares the symbol as `Linkage::Import`
  (vs `Linkage::Local` for bodied fns). `define_fn` skips Import-fns
  entirely — the linker owns the body.

## Signature matrix

Each row maps to a fixture under `tests/extern_c_matrix/`. The "Status"
column reflects v0.36 reality. Rows marked **wrapper-pattern** ship
today via a tiny zero-arg C entrypoint that builds the value(s) on the
C side; the row's Mighty source still pins the link surface and the
matrix test still proves the .a is reachable.

| # | Shape | Status | Notes |
|---|---|---|---|
| 01 | `extern c fn foo() -> i32` (no args) | works | Simplest shape. Pins call-conv + return register. |
| 02 | `extern c fn foo(a: i32, b: i32) -> i32` | works | Primitive in / out. Pins arg register ordering. |
| 03 | `extern c fn foo(p: *const u8, len: usize) -> i32` | works (wrapper) | Mighty typeck still rejects `Str → *U8` coercion (v0.37). The fixture allocates the buffer C-side and forwards through a real pointer-taking helper. |
| 04 | `extern c fn foo(p: *mut i32)` (out-param) | works (wrapper) | Same wrapper rationale — Mighty's borrowck doesn't yet hand out raw addresses of locals. |
| 05 | `extern c fn foo(s: Struct) -> i32` (by-value) | works (wrapper) | Small (≤16-byte) structs ride a single register on every host ABI we target. Mighty struct-literal-as-FFI-arg is the v0.37 follow-up. |
| 06 | `extern c fn foo(s: *const Struct)` | works (wrapper) | The "opaque handle" shape every system FFI uses (HWND, FILE*, wgpu::Device*). |
| 07 | `extern c fn foo() -> Struct` | works (wrapper) | Small struct returned in a register. Wrapper forwards to a print so the harness can verify bytes. |
| 08 | `extern c fn foo(arr: *const [i32; 4])` | works (wrapper) | Pointer-to-fixed-array. Identical ABI slot to any other pointer. |
| 09 | `extern c fn foo(s: *const Str)` (Str ↔ C const char*) | works (wrapper) | Mighty's `Const::Str` interns null-terminated UTF-8, so the pointer is `const char *`-compatible. |
| 10 | `extern c fn foo(s: *mut Str) -> usize` (caller-owned buf) | works (wrapper) | The classic `snprintf` shape. |
| 11 | `extern c fn foo(cb: extern fn(i32) -> i32)` | works (wrapper) | Function pointer. Wrapper synthesises the callback C-side. |
| 12 | Variadic (decl) | parse / typeck / linker decl shipped (v0.37 T6) | `extern c fn printf(fmt: *U8, ...) -> I32` parses, typechecks, and lowers to a `Linkage::Import` declaration. Calls with **only the fixed-arity prefix** work end-to-end on the cranelift backend. Calls passing extra varargs are still a cranelift-side `CodegenError::Unsupported` — cranelift 0.132 has no vararg `Signature` flag, so a per-call-site signature import is the v0.38 follow-up. |

## v0.37 follow-ups — Mighty-side ergonomics

These are the gaps that today force the wrapper pattern. Closing them
is purely typeck / borrowck / lowering work — the linking + signature
plumbing is already shipped.

1. **`Str → *U8` coercion** at extern call sites (rows 03, 09).
   Should be a typeck blanket-impl, not require an explicit cast.
2. **Address-of for FFI locals** so a Mighty `let mut x: I32 = 0`
   followed by `mty_row04(&x)` borrowchecks (row 04).
3. **Struct literal as FFI arg** — pass a Mighty `Point { x: 1, y: 2 }`
   directly into a C `Point` param (rows 05, 06, 07).

## v0.37 T6 — variadic externs (parse / typeck / decl)

Lands the `...` token and the full parse → HIR → SIR plumbing for
variadic C signatures. What works today on the cranelift backend:

* **Declaration.** `extern c fn printf(fmt: *U8, ...) -> I32` parses
  (the `...` is wrapped in a `VARIADIC_MARKER` CST node sibling to the
  trailing `FN_PARAM`s), lowers to `HirFn { is_variadic: true, ... }`,
  flows into `FnDef.is_variadic`, and the SIR `ExternBinding` carries
  the flag so every backend can see it.
* **Typeck.** `synth_call` recognises a single-segment `Path` callee
  that resolves to a variadic `FnDef`, switches the strict
  `params.len() != args.len()` check to `args.len() >= params.len()`,
  and synthesises a fresh inference variable for each extra arg
  (typed independently). Below-fixed-arity calls still emit MT2005.
* **Codegen — fixed-arity prefix.** Calls that pass exactly the
  fixed-arity prefix (e.g. `printf(fmt)`) lower like any other extern
  C call: the linker resolves the symbol, the declared signature is
  exact, the call instruction validates.
* **Codegen — variadic call extension.** Calls with extra args
  (`printf(fmt, 1, 2)`) surface a clean `CodegenError::Unsupported`
  pointing at this doc. Cranelift 0.132's `Signature` has no
  first-class vararg flag and `declare_function` rejects re-declaring
  the same symbol with a different signature, so per-call-site
  signature handling needs `Function::import_signature` +
  `call_indirect` via `func_addr` of the linked symbol. **Tracked for
  v0.38.**
* **Wasm backend.** Any program containing a variadic extern fn
  fails the wasm compile with `WasmError::Unsupported`, regardless of
  whether the fn is actually called. Core wasm has no varargs ABI
  and the Component Model FFI surface forbids it. Use the cranelift
  backend instead.

### v0.38 follow-up

Finish the cranelift call extension: at every call site where a
variadic extern is invoked with non-empty extras, build a per-call
`ir::Signature` from the actual SIR arg types, register it via
`Function::import_signature`, materialise the callee's address via
`func_addr` against the `Linkage::Import` declaration, and use
`call_indirect`. The wasm-side stance does NOT change — variadic
externs stay rejected.

## Practical examples

### Static link against a vendored archive

```toml
# mighty.toml
[package]
name    = "winit_demo"
version = "0.1.0"
edition = "2026"

[[extern_lib]]
name = "winit_shim"
kind = "static"
path = "vendor/libwinit_shim.a"
link_args_macos = ["-framework", "Cocoa", "-framework", "CoreFoundation"]
link_args_linux = ["-lX11", "-lxkbcommon"]
link_args_windows = ["Userenv.lib"]
```

```mighty
// src/main.mty
extern c {
  fn winit_demo_open_window() -> I32
}

fn main() {
  let rc = winit_demo_open_window()
  log("opened with rc=...")
}
```

```sh
mty build src/main.mty --release
./target/main
```

### Dynamic library (system-search)

```toml
[[extern_lib]]
name = "z"
kind = "dynamic"
# no `path` → linker emits `-lz` and searches LD_LIBRARY_PATH /
# DYLD_FALLBACK_LIBRARY_PATH / PATH at runtime.
```

### Multiple archives in dependency order

```toml
# wgpu depends on winit's surface helpers; declare winit first so its
# symbols are on the command line when the linker walks wgpu's
# unresolved set.
[[extern_lib]]
name = "winit"
path = "vendor/libwinit.a"

[[extern_lib]]
name = "wgpu"
path = "vendor/libwgpu.a"
```

## Runtime symbol stub (test-only)

The cranelift backend pre-declares every `mty_runtime_*` symbol as a
`Linkage::Import`. Even when the Mighty program doesn't call any of
them, the symbol references still land in the emitted `.o`. Real
deployments link against `libmty_runtime.a` (in development) or a
profiled runtime build. The matrix tests don't need the real runtime,
so the test harness in `crates/mty-driver/tests/extern_c_matrix.rs`
builds a tiny no-op stub archive (`build_runtime_stub`) and threads it
through alongside the row's own archive. See the comment block at the
top of `build_runtime_stub` for details — it's a useful pattern for
any external integrator who wants to ship a stand-alone FFI binary
without pulling the full runtime.

## Where the wiring lives

| Concern | File |
|---|---|
| `[[extern_lib]]` parse + types | `crates/mty-driver/src/manifest.rs` (`ExternLib`, `HostOs`) |
| Manifest → flat linker args | `crates/mty-driver/src/build.rs` (`build_linker_args`) |
| Linker invocation (extra args) | `crates/mty-codegen-cranelift/src/object.rs` (`link_executable_with_libs`) |
| `Linkage::Import` for extern fns | `crates/mty-codegen-cranelift/src/lower.rs` (`declare_fns`) |
| Extern fn signature propagation | `crates/mty-types/src/items.rs` (the pre-pass at the top of `check_package_typed`) |
| `extern_bindings` table | `crates/mty-ir/src/ir.rs` (`Program::extern_bindings`, `ExternBinding`) |
| Matrix tests | `crates/mty-driver/tests/extern_c_matrix.rs` |
| Manifest tests | `crates/mty-driver/tests/manifest.rs` (rows starting `extern_lib_*` + `build_linker_args_*`) |

## Demo

`demos/11_ffi_winit_stub/` ships a minimal scaffold for FFI app
authors — a `mighty.toml` with the `[[extern_lib]]` block, a
`winit_shim.c` that compiles on every host, and a `main.mty` calling
into it. The smoke check is gated on `MTY_FFI_SMOKE=1` so CI doesn't
open a real window.
