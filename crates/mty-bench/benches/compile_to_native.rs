//! Bench: end-to-end compile time of a 1 KLOC Mighty source.
//!
//! Goes through the same `parse → lower → typeck → borrowck → SIR →
//! wasm-core emit` path as `mty build --target wasm --core-only`.
//! We use the wasm core backend (not native cranelift) because the
//! native linker may be absent on Windows CI; wasm emit is the most
//! portable measure of the compiler's hot path.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use mty_codegen_cranelift::artifact::BuildMode;
use mty_codegen_wasm::WasmTarget;
use mty_driver::{build_wasm, BuildOptions, BuildTarget};
use std::time::Duration;

fn bench_compile(c: &mut Criterion) {
    let src = mty_bench::fixtures::synth_source(100); // ~1 KLOC

    let mut g = c.benchmark_group("compile_to_native");
    g.sample_size(20);
    g.measurement_time(Duration::from_secs(20));

    g.bench_function("stardust_1kloc_wasm_core_release", |b| {
        let mut counter = 0u64;
        b.iter(|| {
            counter += 1;
            let tmp = tempfile::tempdir().unwrap();
            let opts = BuildOptions {
                target: BuildTarget::Wasm(WasmTarget::Wasi),
                mode: BuildMode::Release,
                out_dir: tmp.path().to_path_buf(),
                binary_name: format!("c{counter}"),
                no_component: true,
                wasi_preview: mty_driver::build::WasiPreview::P1,
                user_wit: None,
            };
            let outcome = build_wasm(src.clone(), "compile.mty".into(), &opts, WasmTarget::Wasi);
            black_box(outcome);
        })
    });

    g.finish();
}

criterion_group!(benches, bench_compile);
criterion_main!(benches);
