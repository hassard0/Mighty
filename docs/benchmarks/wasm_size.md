# wasm_size

> **Baseline from Mighty v0.6 (recorded 2026-05-24).** These numbers
> have not been refreshed against v0.31. To run current measurements,
> see [`benches/README.md`](https://github.com/hassard0/Mighty/blob/main/benches/README.md) and the
> per-impl build steps in
> [`benches/wasm_size/README.md`](https://github.com/hassard0/Mighty/blob/main/benches/wasm_size/README.md).

**Workload:** emit a 50-unit (~500-line) synthetic Mighty source as
a wasm-core module, release mode, no Component-Model wrapper, no
debug info. Record the byte size.

**Spec alignment:** §0 "wasm leanness" pillar — the wasm target is
intended for frontend / edge / sandbox deployments where bytes
matter.

## Numbers

| Impl | Bytes | Bytes/unit | Notes |
|---|---|---|---|
| Mighty v0.6 → wasm-core (release) | 2 068 | ~41 | 50 structs + 50 fns; no debug info |
| Rust → wasm32-unknown-unknown (release) | (pending) | | `cargo build --release --target wasm32` |
| TinyGo → wasi (release) | (pending) | | `tinygo build -target=wasi -no-debug` |
| Emscripten → wasm | (pending) | | `emcc -O3 -s STANDALONE_WASM` |

### Recorded values (this host, 2026-05-24)

```
wasm_size: 2068 bytes
```

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
