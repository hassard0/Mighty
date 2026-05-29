# Demo 11 — FFI scaffold (v0.36 T2 + v0.37 T3)

Minimal end-to-end Mighty + C scaffold demonstrating the first-class
FFI surface: `[[extern_lib]]` blocks in `mighty.toml`, `extern c { ... }`
declarations in source, and the cranelift backend emitting
`Linkage::Import` symbols the system linker resolves against the
vendored archive.

## v0.37 ergonomics — the FFI surface is now ergonomic

v0.36 T2 shipped the link plumbing but the typeck still rejected
three common call shapes. v0.37 T3 closes them, and this demo now
exercises all three side-by-side:

| Surface                      | What changed                                                      |
|------------------------------|-------------------------------------------------------------------|
| `Str → *U8` at call site     | Pass Mighty string literals straight into `*U8` C params.         |
| `&mut local` for out-params  | Mighty out-params via `&mut x`; the C side writes through `*I32`. |
| Struct literal as FFI arg    | Pass `Rect { x: 0, y: 0, w: 100, h: 50 }` inline at the call.     |

See `examples/41_ffi_clean.mty` for a minimal showcase of each, and
`docs/internals/extern-c-matrix.md` for the matrix entries these
replace.

## Files

- `mighty.toml` — declares the static lib (`vendor/libwinit_shim.a`),
  plus the per-platform `link_args_*` flags a real binding would use.
- `src/main.mty` — calls five no-op extern C entry points covering all
  three coercion surfaces.
- `vendor/winit_shim.c` — no-op stub. A real binding replaces this
  with libwinit/libwgpu glue.
- `smoke.sh` / `smoke.ps1` — gated on `MTY_FFI_SMOKE=1`. Compiles the
  shim, builds the demo, runs the binary, asserts the shim's stderr
  marker lines appear.

## Smoke

```sh
MTY_FFI_SMOKE=1 ./smoke.sh
```

The smoke is opt-in because the build needs a C compiler + `ar` on
PATH; CI doesn't carry one by default. The full matrix
(`tests/extern_c_matrix/`) drives the same plumbing under
`cargo test -p mty-driver --test extern_c_matrix`.

## Extending to a real winit binding

1. Replace `vendor/winit_shim.c` with the real binding's `.c` files
   (or vendor `libwinit.a` directly).
2. Update `mighty.toml`:
   ```toml
   [[extern_lib]]
   name = "winit"
   kind = "static"
   path = "vendor/libwinit.a"
   link_args_macos   = ["-framework", "Cocoa", "-framework", "CoreFoundation"]
   link_args_linux   = ["-lX11", "-lxkbcommon", "-ldl"]
   link_args_windows = ["Userenv.lib", "Dwmapi.lib"]
   ```
3. Update `src/main.mty`'s extern block to declare winit's full surface.

See `docs/internals/extern-c-matrix.md` for the signature shapes
known-good in v0.36 + v0.37 and remaining follow-ups (variadics).
