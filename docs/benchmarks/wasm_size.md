# wasm_size

> **Last refreshed: v0.38 (2026-05-29) on vulcan** (Dell, Intel Xeon,
> 16 cores, Ubuntu 24.04, Rust 1.95.0, PGO release-pgo profile).
> Mighty number is v0.38 (byte-exact reproduction of the v0.33
> output — wasm-core emit is deterministic, so this is a confirmation
> not a re-measurement). Rust / TinyGo / Emscripten comparators
> retain the v0.6 baseline pending a wasm-toolchain refresh on the
> benchmark host (tracked as v0.39 follow-up).

**Workload:** emit a 50-unit (~500-line) synthetic Mighty source as
a wasm-core module, release mode, no Component-Model wrapper, no
debug info. Record the byte size.

**Spec alignment:** §0 "wasm leanness" pillar — the wasm target is
intended for frontend / edge / sandbox deployments where bytes
matter.

## Numbers

| Impl | Bytes | Bytes/unit | Notes |
|---|---|---|---|
| Mighty v0.38 → wasm-core (release-pgo) | 2 698 | ~54 | 50 structs + 50 fns; no debug info |
| Rust → wasm32-unknown-unknown (release) | (pending) | | `cargo build --release --target wasm32` |
| TinyGo → wasi (release) | (pending) | | `tinygo build -target=wasi -no-debug` |
| Emscripten → wasm | (pending) | | `emcc -O3 -s STANDALONE_WASM` |

### Recorded values (vulcan, 2026-05-29, v0.38)

```
wasm_size: 2698 bytes
```

A +30% bump vs the v0.6 baseline (2068 bytes) — accounted for by
v0.15+ stdlib intrinsics that the wasm-core lowerer now emits
direct lowerings for (versioned `wasi:*@0.2.3` import shape;
`std.random` + `std.time` direct calls). Tracked as a v0.34
follow-up: function deduplication + a Wasm size-budget gate to
prevent silent regressions.

### v0.6 baseline (Windows 11 dev laptop, 2026-05-24)

For continuity: Mighty v0.6 emitted **2 068 bytes** for the same
50-unit fixture. The growth is stdlib WASI integration, not
codegen regression.

## Interpretation

**~41 bytes per declaration** is excellent for a starter wasm output.
The wasm-core lowerer is a slice-8 minimal emitter — it doesn't
include a stdlib, doesn't generate WIT wrappers (`--no-component`),
and elides debug info in release mode. For perspective:

- A `wasm32-unknown-unknown` "hello world" Rust binary typically
  starts at ~30-100 KB (because rustc bakes in panic infrastructure
  + libstd shards).
- TinyGo's smallest output is ~4-10 KB even with `-no-debug`.
- An Emscripten `puts("hello")` is ~15-40 KB (libc + runtime).

So a 2 KB wasm for 100 declarations is competitive with hand-written
wat. The trade-off: Mighty's wasm output is minimal but doesn't
embed a richer runtime (no panic handler, no async scheduler — those
live host-side and are wired via imports).

## v0.7+ optimisation targets

- **Function deduplication**: many of our synth-source `bench_fN`
  bodies are identical post-lowering. CSE-style dedup would shrink
  output 30-50%.
- **Constant-folding pass** before emission (today the wasm backend
  emits arithmetic verbatim).
- **Compressed sections** (the wasm spec permits `gzip` of custom
  sections; component-model output already does this).

Tracked in: `BENCHMARKS_V0_6_NOTES.md` § Wasm Size.
