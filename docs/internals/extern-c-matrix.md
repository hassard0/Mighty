# Extern C signature matrix (v0.36 Track T2 + v0.37 Track T3)

This document is the human-readable mirror of `tests/extern_c_matrix/`.
It pins which C-ABI shapes Mighty can call end-to-end today, what the
manifest contract is for linking against vendored archives, and which
shapes are deferred to v0.38.

> v0.37 T3 closed the call-site ergonomics gaps that previously forced
> rows 3-10 of the matrix to ship via wrapper functions. Real Mighty
> source code can now spell those shapes directly. See the **v0.37
> ergonomics** section near the end of this doc.

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
| 03 | `extern c fn foo(p: *const u8, len: usize) -> i32` | works (v0.37 direct) | ~~wrapper-pattern~~ — v0.37 T3 lifts the wrapper; Mighty `Str` literals coerce to `*U8` at the call site. The fixture still ships the original wrapper for ABI coverage; the IDE agent (and real users) can call directly. |
| 04 | `extern c fn foo(p: *mut i32)` (out-param) | works (v0.37 direct) | ~~wrapper-pattern~~ — v0.37 T3 lifts the wrapper; `&mut local` produces a `*mut I32` at the call site. Borrow check rules are unchanged. |
| 05 | `extern c fn foo(s: Struct) -> i32` (by-value) | works (v0.37 direct) | ~~wrapper-pattern~~ — v0.37 T3 ships struct-literal-at-FFI-arg, so `foo(Point { x: 1, y: 2 })` typechecks directly. |
| 06 | `extern c fn foo(s: *const Struct)` | works (v0.37 direct) | ~~wrapper-pattern~~ — v0.37 T3's `&local` of a struct produces a `*const Struct`. Pattern of choice for FFI handles (HWND, FILE*, wgpu::Device*). |
| 07 | `extern c fn foo() -> Struct` | works (wrapper) | Small struct returned in a register. The wrapper stays for the matrix because Mighty doesn't yet model returned-struct binding (you can call the fn but discarding the return is awkward — v0.38 follow-up). |
| 08 | `extern c fn foo(arr: *const [i32; 4])` | works (v0.37 direct) | ~~wrapper-pattern~~ — v0.37 T3's `&local_array` produces the array pointer. Identical ABI slot to any other pointer. |
| 09 | `extern c fn foo(s: *const Str)` (Str ↔ C const char*) | works (v0.37 direct) | ~~wrapper-pattern~~ — v0.37 T3 coerces Mighty Str literals/locals to `*U8` (= `const char *` on every host). |
| 10 | `extern c fn foo(s: *mut Str) -> usize` (caller-owned buf) | works (wrapper) | The classic `snprintf` shape. Wrapper stays because Mighty doesn't yet expose a mutable `Str` buffer surface (you'd need a `[U8; N]` and pass `&mut buf[0]`, which v0.37 T3 partially covers — full coverage is v0.38). |
| 11 | `extern c fn foo(cb: extern fn(i32) -> i32)` | works (wrapper) | Function pointer. Wrapper synthesises the callback C-side. Mighty function-pointer surface is a v0.38 follow-up. |
| 12 | Variadic (decl) | parse / typeck / linker decl shipped (v0.37 T6) | `extern c fn printf(fmt: *U8, ...) -> I32` parses, typechecks, and lowers to a `Linkage::Import` declaration. Calls with **only the fixed-arity prefix** work end-to-end on the cranelift backend. Calls passing extra varargs surface a `CodegenError::Unsupported` — cranelift 0.132 has no vararg `Signature` flag, so a per-call-site signature import is the v0.38 follow-up. |

## v0.37 ergonomics — the FFI surface is now ergonomic

v0.37 Track T3 (commit body tagged `v0.37 T3: FFI coercions`) shipped
three call-site coercions that close the wrapper-pattern gap. Mighty
source code can spell every shape on rows 3, 4, 5, 6, 8, 9 directly
now.

### Surface 1 — `Str → *U8` coercion

```mighty
extern c {
  fn ffi_take(p: *U8, len: USize) -> I32
}

fn main() {
  let _ = ffi_take("hello", 5)  // v0.37 T3 reads the Str's ptr half
}
```

The Mighty `Str` is a (ptr, len) aggregate stored in a 16-byte stack
slot. At an `extern c` arg position whose declared type is `*U8`,
typeck records the arg in `coerce_str_to_ptr` and SIR lowering emits
`Rvalue::StrPtr(arg)`. The cranelift backend reads offset 0 (the
ptr) and passes it as the i64 scalar. `intern_string` already
null-terminates the literal data, so the resulting `*U8` is directly
usable as a `const char *` on the C side.

Pre-v0.37 workaround: build the string in a C wrapper and pass through
`uint8_t *`. The matrix fixture under `tests/extern_c_matrix/row_03_ptr_in/`
still uses that pattern so the test pins the link-level ABI even when
the typeck path is bypassed — but real Mighty code should call directly.

### Surface 2 — `&local` / `&mut local` for FFI out-params

```mighty
extern c {
  fn ffi_out(p: *I32) -> Unit
}

fn main() {
  let mut x: I32 = 0
  ffi_out(&mut x)
  log("x was filled by C")
}
```

`&` (immutable) and `&mut` (mutable) are already prefix unary ops in
the parser. Typeck records the arg in `coerce_addr_of`; the existing
`HirExpr::Borrow` lowering allocates a Ref-typed temp whose slot holds
the place's address (cranelift sees this as an i64 scalar) — the
call-arg path passes it straight through.

Borrow check is unchanged: `&mut x` is an exclusive borrow held for
the duration of the call expression, and shared `&x` borrows allow
aliased reads. The compiler does not insert a runtime barrier; if the
C side leaks the pointer past the borrow's lifetime that's UB, same as
in C/Rust.

### Surface 3 — struct literal as FFI arg

```mighty
struct Rect { x: I32, y: I32, w: I32, h: I32 }

extern c {
  fn ffi_draw_rect(r: Rect) -> Unit
}

fn main() {
  ffi_draw_rect(Rect { x: 0, y: 0, w: 100, h: 50 })
}
```

The parser already accepts struct literals at expression position
inside call arguments. v0.37 T3 locks in the typeck path: the struct
literal flows through `synth_expr → check_expr` as a normal ADT value
and the cranelift backend emits an `AdtInit` then passes the slot
address. Small (≤ 16-byte) structs ride a single ABI register on x86_64,
ARM64 and RISC-V — wgpu/winit's `Point`-, `Color`-, `Extent3d`-shaped
arguments all fit.

### Where the v0.37 wiring lives

| Concern                              | File                                                                       |
|--------------------------------------|----------------------------------------------------------------------------|
| `FnDef.extern_abi` marker            | `crates/mty-types/src/defs.rs` (`FnDef::extern_abi`)                       |
| Extern-block ABI propagation         | `crates/mty-types/src/resolve.rs` (extern-block branch of `declare_item`)  |
| Call-site coercion gate              | `crates/mty-types/src/check.rs` (`try_extern_c_coercion`, `callee_is_extern_c`) |
| Side tables                          | `crates/mty-types/src/lib.rs` (`TypedPackage::coerce_str_to_ptr`, `::coerce_addr_of`) |
| IR rvalue for Str→*U8                | `crates/mty-ir/src/ir.rs` (`Rvalue::StrPtr`)                               |
| IR lowering wiring                   | `crates/mty-ir/src/lower/exprs.rs` (`lower_call` per-arg coercion branch)  |
| Cranelift StrPtr lowering            | `crates/mty-codegen-cranelift/src/lower.rs` (`Rvalue::StrPtr` arm)         |
| Tests                                | `crates/mty-types/tests/ffi_coercions_v037.rs` (18 cases across all three) |
| Demo                                 | `demos/11_ffi_winit_stub/` (now uses all three surfaces)                   |
| Example                              | `examples/41_ffi_clean.mty` (minimal side-by-side showcase)                |

## Remaining v0.38 follow-ups

1. **Variadic extern decls** — `extern c fn printf(fmt: *U8, ...)`
   (row 12). Needs a cranelift `Signature` vararg marker.
2. **Mutable Str / caller-owned buffer ergonomics** for row 10's
   `snprintf` shape — needs first-class mutable byte-buffer
   binding surface in Mighty (`let mut buf: [U8; 256] = [0u8; 256]`
   and `ffi(buf as *mut U8)` should work cleanly).
3. **Returned-struct binding** — calling row 07 (`extern c fn make()
   -> Point`) and binding the result to a `let` should round-trip
   the small-struct return register correctly.
4. **Function pointer surface** — Mighty fn values flowing into
   `extern fn(I32) -> I32` arg slots (row 11).
5. **Optional `#[ffi_nul_ok]` fast path** — for Str → *U8 where the
   caller has already null-terminated, skip the safety check. Default
   in v0.37 is the safe (null-terminated-via-intern_string) path.

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
