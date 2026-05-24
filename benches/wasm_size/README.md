# wasm_size comparators

Single-shot measurement: emit a representative app to wasm and record
the byte size. The Stardust impl is run by
`crates/sdust-bench/src/bin/sdust-bench-runner.rs` and recorded in
`docs/benchmarks/wasm_size.md`.

For the comparators, use the same source files as `compile_to_native`
(see `../compile_to_native/generate_sources.sh`) compiled to wasm:

| Impl | Command | Result file |
|---|---|---|
| Stardust | `sdust build --target wasi --no-component --release synth.sd` | `synth.wasm` |
| Rust | `cargo build --release --target wasm32-unknown-unknown` | `target/.../synth.wasm` |
| Go (TinyGo) | `tinygo build -target=wasi -o synth.wasm synth.go` | `synth.wasm` |
| C++ (Emscripten) | `emcc -O2 synth.cpp -o synth.wasm` | `synth.wasm` |

This benchmark is intentionally **single-shot**: wasm size is
deterministic given an input + toolchain version, so percentiles don't
apply. The recorded number in `docs/benchmarks/wasm_size.md` is the
exact byte size from one build.
