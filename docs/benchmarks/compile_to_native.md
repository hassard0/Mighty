# compile_to_native

**Workload:** end-to-end compile of a 1 002-line synthetic Stardust
source (`synth_source(100)`) to a wasm-core module (release mode).
We use the wasm-core backend, not native cranelift, because:

1. The cranelift backend requires an external linker which may be
   absent on Windows CI.
2. The wasm-core path goes through the same `parse → lower →
   typeck → borrowck → SIR → emit` pipeline; the only difference is
   the final lowering step.

For a true "native" number, swap `--target wasm-core` for the native
cranelift backend on a host with `link.exe` / `ld` installed.

**Spec alignment:** §0 "compile speed" pillar — the developer
edit-compile cycle is the user-visible metric here.

## Numbers

| Impl | Median | Lines | Lines/sec | Notes |
|---|---|---|---|---|
| Stardust v0.6 → wasm-core (release) | 7.88 ms | 1002 | ~127k LoC/sec | full pipeline |
| Rust 1.95 → release native | (pending — Reference env) | 1002 | | `cargo build --release` |
| Go 1.22 → native | (pending — Reference env) | 1002 | | `go build` |
| C++ clang++ -O2 → native | (pending — Reference env) | 1002 | | `clang++ -O2 -std=c++20` |

### Recorded values (this host, 2026-05-24)

```
compile_to_native      median=     7.875 ms  p95=     8.642 ms  p99=     8.642 ms
```

## Interpretation

**127k LoC/sec** on a fresh build is a healthy starting point — Rust
+ rustc averages ~10-50k LoC/sec on similar synthetic inputs (the
exact figure depends massively on dependency count). The Stardust
fixture is simpler than typical Rust code (no traits, no generics in
the fixture), so the comparison is *Stardust's hot path* vs *Rust's
hot path including dep resolution*, which is unfair to Rust.

A fairer comparison: time `cargo build --release` on a zero-dep crate
with the same shape as our synth source. That's what the comparator
in `benches/compile_to_native/rust/` exists for.

Expected outcome once comparators run:

- **Rust**: ~3-10x slower (LLVM optimisation pipeline + linker).
- **Go**: ~2-5x faster (Go compiler is famously fast).
- **C++**: ~10-30x slower (template + header processing).

So Stardust v0.6 should land roughly where Go is — which is the
right neighborhood for "compile speed pillar" alignment.

## v0.7+ optimisation targets

- **Parallelise the type-checker** (today it's single-threaded).
- **Pre-built stdlib metadata** to skip stdlib re-lowering on every
  build.
- **Incremental compilation** for the IDE path (today every parse
  rebuilds the world).

Tracked in: `BENCHMARKS_V0_6_NOTES.md` § Compile to Native.
