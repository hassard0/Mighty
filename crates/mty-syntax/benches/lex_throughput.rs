//! v0.8 microbench: lex throughput + token-cache incremental re-lex.
//!
//! Compares:
//! - `lex_full_10kloc`   — baseline: full lex of the 10 KLOC synth source.
//! - `tokencache_full`   — cold TokenCache::lex (same shape as baseline).
//! - `tokencache_edit`   — incremental edit at midpoint; should be ~100x
//!   faster than the full re-lex.
//! - `parse_throttled`   — parser with `max_diagnostics = 16` vs uncapped.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use mty_syntax::{lex, parse, parse_with_opts, ParseOpts, TokenCache};

fn synth(n_units: usize) -> String {
    let mut out = String::with_capacity(n_units * 80);
    for i in 0..n_units {
        out.push_str(&format!(
            "fn bench_f{i}(x: I64, y: I64) -> I64 {{ let z = x + y\n  z * 2 - x }}\n"
        ));
    }
    out
}

fn bench_lex(c: &mut Criterion) {
    let src = synth(1000); // ~2-3 KLOC
    let bytes = src.len() as u64;

    let mut g = c.benchmark_group("lex_throughput");
    g.throughput(Throughput::Bytes(bytes));

    g.bench_with_input(BenchmarkId::new("lex_full", bytes), &src, |b, src| {
        b.iter(|| {
            let toks = lex(black_box(src));
            black_box(toks);
        })
    });

    g.bench_with_input(
        BenchmarkId::new("tokencache_full", bytes),
        &src,
        |b, src| {
            b.iter(|| {
                let c = TokenCache::lex(black_box(src.as_str()));
                black_box(c);
            })
        },
    );

    // Incremental: build once, then apply a 6-byte insert at midpoint.
    g.bench_with_input(
        BenchmarkId::new("tokencache_edit", bytes),
        &src,
        |b, src| {
            b.iter_batched(
                || TokenCache::lex(src.as_str()),
                |mut c| {
                    let mid = c.source().len() / 2;
                    // Snap to a newline so we don't split a multi-byte token.
                    let mid = c.source()[mid..]
                        .find('\n')
                        .map(|p| mid + p + 1)
                        .unwrap_or(mid);
                    let n = c.apply_edit(mid, mid, "// hi\n");
                    black_box(n);
                },
                criterion::BatchSize::SmallInput,
            )
        },
    );

    g.finish();
}

fn bench_diag_throttle(c: &mut Criterion) {
    // Pathological: 500 stray @ tokens — uncapped parser emits >100 diags.
    let mut bad = String::new();
    for _ in 0..500 {
        bad.push_str("@ ");
    }
    let mut g = c.benchmark_group("diag_throttle");
    g.bench_function("uncapped", |b| b.iter(|| black_box(parse(black_box(&bad)))));
    g.bench_function("capped_16", |b| {
        b.iter(|| {
            black_box(parse_with_opts(
                black_box(&bad),
                ParseOpts {
                    max_diagnostics: 16,
                },
            ))
        })
    });
    g.finish();
}

criterion_group!(benches, bench_lex, bench_diag_throttle);
criterion_main!(benches);
