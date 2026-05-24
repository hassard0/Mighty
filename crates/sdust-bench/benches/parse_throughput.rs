//! Bench: lex + parse a 10 KLOC Stardust file.
//!
//! Spec §0 "tooling baseline" claim — the IDE / formatter / LSP all sit
//! on top of this path. We measure the *cold* parse: every iteration
//! clones the source so the lexer / parser don't get to reuse any
//! per-run heap.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use sdust_bench::fixtures::stardust_10kloc;

fn bench_parse(c: &mut Criterion) {
    let src = stardust_10kloc();
    let bytes = src.len() as u64;

    let mut g = c.benchmark_group("parse_throughput");
    g.throughput(Throughput::Bytes(bytes));

    g.bench_with_input(
        BenchmarkId::new("stardust_full_pipeline", bytes),
        &src,
        |b, src| {
            b.iter(|| {
                let s = src.clone();
                let parsed = sdust_driver::parse_source(black_box(s), "synth.sd".into());
                black_box(parsed);
            })
        },
    );

    g.bench_with_input(BenchmarkId::new("stardust_lex_only", bytes), &src, |b, src| {
        b.iter(|| {
            let tokens = sdust_syntax::lex(black_box(src));
            black_box(tokens);
        })
    });

    g.finish();
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
