//! Bench: wasm output size for a representative app.
//!
//! Size is a single-shot measurement so we don't actually "bench" in
//! the criterion sense — we wrap a single iteration so the result is
//! still recorded in `target/criterion/wasm_size/` alongside the rest.
//! The actual byte counts are emitted to stdout and copied into
//! `docs/benchmarks/wasm_size.md`.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use mty_codegen_cranelift::artifact::BuildMode;
use mty_codegen_wasm::WasmTarget;
use mty_driver::{build_wasm, BuildOptions, BuildOutcome, BuildTarget};

fn bench_wasm_size(c: &mut Criterion) {
    let mut g = c.benchmark_group("wasm_size");
    g.sample_size(10);

    let units = 50;
    let src = mty_bench::fixtures::synth_source(units);

    g.bench_function("stardust_50unit_wasm_core_bytes", |b| {
        let mut counter = 0u64;
        b.iter(|| {
            counter += 1;
            let tmp = tempfile::tempdir().unwrap();
            let opts = BuildOptions {
                target: BuildTarget::Wasm(WasmTarget::Wasi),
                mode: BuildMode::Release,
                out_dir: tmp.path().to_path_buf(),
                binary_name: format!("s{counter}"),
                no_component: true,
                wasi_preview: mty_driver::build::WasiPreview::P1,
                user_wit: None,
            };
            let outcome = build_wasm(src.clone(), "size.mty".into(), &opts, WasmTarget::Wasi);
            let bytes = match outcome {
                BuildOutcome::WasmOk(p) => std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0),
                _ => 0,
            };
            black_box(bytes);
        })
    });

    g.finish();
}

criterion_group!(benches, bench_wasm_size);
criterion_main!(benches);
