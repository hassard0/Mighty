# compile_to_native

**Workload:** end-to-end compile of a 1 002-line synthetic Mighty
source (`synth_source(100)`) to a wasm-core module (release mode).
We use the wasm-core backend, not native cranelift, because:

1. The cranelift backend requires an external linker which may be
   absent on Windows CI.
2. The wasm-core path goes through the same `parse → lower →
   typeck → borrowck → MtyIR → emit` pipeline; the only difference is
   the final lowering step.

For a true "native" number, swap `--target wasm-core` for the native
cranelift backend on a host with `link.exe` / `ld` installed.

**Spec alignment:** §0 "compile speed" pillar — the developer
edit-compile cycle is the user-visible metric here.

## Numbers

| Impl | Median | Lines | Lines/sec | Notes |
|---|---|---|---|---|
| Mighty v0.6 → wasm-core (release) | 7.88 ms | 1002 | ~127k LoC/sec | full pipeline |
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
exact figure depends massively on dependency count). The Mighty
fixture is simpler than typical Rust code (no traits, no generics in
the fixture), so the comparison is *Mighty's hot path* vs *Rust's
hot path including dep resolution*, which is unfair to Rust.

A fairer comparison: time `cargo build --release` on a zero-dep crate
with the same shape as our synth source. That's what the comparator
in `benches/compile_to_native/rust/` exists for.

Expected outcome once comparators run:

- **Rust**: ~3-10x slower (LLVM optimisation pipeline + linker).
- **Go**: ~2-5x faster (Go compiler is famously fast).
- **C++**: ~10-30x slower (template + header processing).

So Mighty v0.6 should land roughly where Go is — which is the
right neighborhood for "compile speed pillar" alignment.

## v0.7+ optimisation targets

- **Parallelise the type-checker** (today it's single-threaded).
- **Pre-built stdlib metadata** to skip stdlib re-lowering on every
  build.
- **Incremental compilation** for the IDE path (today every parse
  rebuilds the world).

Tracked in: `BENCHMARKS_V0_6_NOTES.md` § Compile to Native.

## v0.8 update

| Optimisation                  | Status     | Delta                                                                                                                  |
|-------------------------------|------------|------------------------------------------------------------------------------------------------------------------------|
| Parallel monomorphisation     | REGRESSION | `run_parallel` 1.8–8x SLOWER than sequential at all tested sizes; per-fn `specialize` cost (~1-2 µs) doesn't amortise std::thread::scope spin-up. `run()` reverted to dispatch to `run_sequential`. API kept for future when per-fn typeck-per-instantiation lands. |
| HashMap pre-sizing in lower   | DONE       | `LowerCtx::new` pre-sizes the 4 maps; `declare_fns` reuses a scratch `param_tys` Vec across fns. No microbench (the win is rehash avoidance on programs with >> 30 fns, hard to measure on the 100-fn synth fixture). |
| Pre-built stdlib metadata     | DEFER      | Lives in mty-types (not owned by this swarm).                                                                          |
| Incremental compilation       | DEFER      | Deep change; the v0.8 brief explicitly defers this.                                                                    |

Microbench: `crates/mty-codegen-cranelift/benches/typeck_parallel.rs`
(sequential vs parallel at small_4g / medium_32g / large_256g sizes).
Interpretation log: `BENCHMARKS_V0_8_NOTES.md`.

**Honest finding**: the v0.6 backlog's "parallelise the type-checker"
expectation was based on rustc's deep per-fn typeck cost. Mighty's
current mono pass is much cheaper per fn, so parallel doesn't pay
off until per-fn cost grows. Re-benchmark when typeck propagation
lands.
