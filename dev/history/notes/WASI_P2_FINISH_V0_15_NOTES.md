# WASI P2 finish — v0.15 notes

This note covers the v0.15 work that:

1. **wires** the `P2DirectImport` constants v0.14 added into the
   core-module emitter, so calls like `std.random.bytes(n)` and
   `std.time.now()` actually splice the versioned
   `wasi:*@0.2.3` import into the produced core module instead of
   falling through to `WasmError::Unsupported`;
2. **flips** the CLI default for `--wasi` on `--target wasm32-wasi`
   from P1 (the v0.13/v0.14 default) to P2;
3. **deprecates** (but does not yet remove) the `wasi:cli/log`
   shim — the canonical-ABI string lift to
   `wasi:cli/stdout@0.2.3#get-stdout` +
   `wasi:io/streams@0.2.3#output-stream.blocking-write-and-flush`
   stays a v0.16 follow-up.

## What ships direct vs. adapter vs. shim

After v0.15, on a `--wasi=p2` build (now the default):

| Mighty surface | transport | core-module import |
|----------------|-----------|---------------------|
| `std.random.bytes(n)` | **direct** | `wasi:random/random@0.2.3#get-random-bytes` |
| `std.time.now()` | **direct** | `wasi:clocks/wall-clock@0.2.3#now` |
| `std.time.monotonic_now()` | **direct** | `wasi:clocks/monotonic-clock@0.2.3#now` |
| `std.time.resolution()` | **direct** | `wasi:clocks/monotonic-clock@0.2.3#resolution` |
| `std.fs.read()` / `write()` / `open()` | adapter | `wasi_snapshot_preview1#fd_*` (translated by the vendored adapter) |
| `std.http.get()` / `post()` | adapter | `wasi_snapshot_preview1#*` (translated by the vendored adapter) |
| `log()` / `print()` | **shim** (deprecated) | `wasi:cli/log#log` (unversioned shim) |

The CLI default flip happens at
`mty_driver::build::WasiPreview::default()` returning `P2` (was
`P1`). `--wasi=p1` is still parsed and still routes through the
legacy import shape — back-compat for any downstream tooling that
pins it.

## Why log stays a shim for v0.15

The brief allowed either a full rewrite or a deferral. The full
rewrite would have to:

1. Mint a fresh function index in the core module for
   `output-stream.blocking-write-and-flush` (a resource method, so
   the canonical-ABI lift needs the receiver in a local).
2. Call `wasi:cli/stdout@0.2.3#get-stdout` at runtime to acquire
   the stream once (likely a lazy-init global), or on every `log()`
   call (simpler but slower).
3. Lift Mighty's `log(ptr: i32, len: i32)` to a canonical-ABI
   `list<u8>` view (push the (ptr, len) pair into a small return
   area), then pass that to the resource method.
4. Drop the returned `Result<_, stream-error>` (the legacy `log()`
   import is `func(msg: string) -> ()`).

That's roughly 30 wasm instructions per `log()` call site + a
resource-handle local, AND the `wasi:io/streams@0.2.3#output-stream`
resource has to be threaded through the WIT world (it already is,
via the vendored P2 surface). For v0.15 we ship the deprecation
marker so anyone inspecting the emitted WIT sees the migration
plan, and defer the implementation to v0.16.

## Component-size comparison

Empirically (release builds of `fn main() {}` on Windows):

| Build | Approx size | Notes |
|-------|-------------|-------|
| P1 component (legacy default through v0.14) | ~3 KB | Bare wrap; no adapter. |
| P2 component, **adapter ON** (v0.15 default) | ~54 KB | Adapter dominates; wit-component strips unused adapter exports but the command shape stays ~50 KB. |
| P2 component, **adapter OFF** (`with_adapter(None)`, opt-in) | ~3 KB | Identical shape to the legacy P1 wrap from the byte-count perspective; differs in that all imports are versioned. |

So the practical cost of the default flip is the ~50 KB the
vendored adapter contributes. The benefit is forward-compat with
strict-P2 hosts — the component is instantiation-ready on the next
generation of wasmtime / jco / wasmer without a P1 polyfill.

## v0.16 follow-ups

1. **Direct `log()` lowering.** Replace the shim with the
   `wasi:cli/stdout@0.2.3` + `wasi:io/streams@0.2.3` lift sketched
   above. Adds ~30 instructions per `log()` site; the WIT shim
   block goes away entirely (closing the v0.15 deprecation).
2. **Direct `std.fs.*` lowering.** Currently adapter-routed. The
   target shape is
   `wasi:filesystem/preopens@0.2.3#get-directories` (one-shot at
   init) + `wasi:filesystem/types@0.2.3#descriptor.read-via-stream`
   / `write-via-stream`. Same canonical-ABI plumbing as `log()`;
   biggest hurdle is plumbing the descriptor resource through SIR.
3. **Direct `std.http.*` lowering.** Adapter-routed today. Target
   shape is `wasi:http/outgoing-handler@0.2.3#handle` returning a
   future. Requires async-friendly canonical-ABI plumbing — easier
   if the v0.16 async-runtime work lands first.
4. **Adapter opt-out by default.** After (1)–(3), the adapter
   only adds size for builds that mix P1-shaped calls with the
   P2 path. Make `embed_adapter = None` the default at that
   point.
5. **Tier-2 P1 (v1.0 RC4).** Once direct lowering covers
   everything, P1 stays as a tier-2 target (still emitted on
   request) and the documentation tree assumes P2.

## Test coverage delta

- `crates/mty-codegen-wasm/tests/preview2.rs` grew by 6 tests
  (existing 18 → 24):
  - `random_bytes_emits_direct_p2_import`
  - `time_now_emits_direct_p2_import`
  - `time_monotonic_now_emits_direct_p2_import`
  - `time_resolution_emits_direct_p2_import`
  - `random_bytes_under_p1_skips_direct_import` (back-compat pin)
  - `log_shim_still_present_with_deprecation_note`
- `crates/mty-driver/tests/wasi_default.rs` (new, 5 tests):
  - `default_wasi_preview_is_p2`
  - `default_wasi_preview_produces_p2_component_for_wasm`
  - `explicit_p1_still_works_for_wasm`
  - `wasi_preview_parse_back_compat`
  - `native_build_ignores_wasi_preview_default`

Total: +11 tests across two crates.

## Owned-file summary

- `crates/mty-codegen-wasm/src/emit.rs` — added `EmitWasiPreview`
  enum + `BuildOptions.wasi_preview` field +
  `compile_program_to_bytes_with_preview` entry point + Emitter
  `p2_direct_import` cache + Extern dispatch arm in `emit_call`.
- `crates/mty-codegen-wasm/src/preview2.rs` — switched
  `compile_program_to_bytes_p2` to call the new with-preview
  entry point passing `P2`; added `Hash` derive to `P2DirectImport`
  so the cache works; added the `// DEPRECATED:` comment to the
  cli/log shim text.
- `crates/mty-codegen-wasm/src/lib.rs` — re-exported
  `EmitWasiPreview` + `compile_program_to_bytes_with_preview`.
- `crates/mty-codegen-wasm/tests/preview2.rs` — added 6 tests
  covering the direct dispatch + log shim deprecation marker.
- `crates/mty-driver/src/build.rs` — flipped `WasiPreview` default
  to `P2`; updated docstrings; threaded `wasi_preview` through the
  P1-path `WasmBuildOptions` so `--wasi=p1 --no-component` still
  picks the right import shape.
- `crates/mty-driver/tests/wasi_default.rs` (new) — 5 tests
  pinning the default flip + back-compat opt-out.
- `crates/mty-cli/src/cmd/build.rs` — replaced explicit
  `WasiPreview::P1` default with `WasiPreview::default()`;
  updated comment.
- `crates/mty-cli/src/main.rs` — updated `--wasi` help text.
- `docs/reference/wasi.md` — updated TL;DR, compatibility matrix,
  status section, versioning, and roadmap to reflect the v0.15
  default flip + wiring.

## Intentionally NOT touched

- `crates/mty-stdlib/src/random.rs` and
  `crates/mty-stdlib/src/time.rs` — the `P2_DIRECT_IMPORT_*`
  constants already match `P2DirectImport::import_pair()` (the
  preview2 test `p2_direct_import_names_match_stdlib_constants`
  has been pinning this since v0.14). No edit needed.
- `examples/21_wasi_preview2.mty` — still compiles unchanged under
  the new default (`mty build … --target wasm32-wasi` now emits a
  P2 component without the explicit `--wasi=p2` flag). The source
  itself is fine.
- Other agents' in-flight files (`mty-syntax`, `mty-types`,
  `mty-macros`, etc.) — outside scope, untouched.
