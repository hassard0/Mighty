//! Bench: HTTP/1.1 GET round-trip on the in-process std.http server.
//!
//! We're measuring Stardust's runtime overhead (accept → parse → write
//! → close), not a fully-tuned load generator. For cross-language
//! comparison under wrk2 see `benches/http_server_throughput/run.sh`.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use sdust_runtime::http::serve_in_memory;
use std::time::Duration;

fn bench_http(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()
        .unwrap();

    let mut g = c.benchmark_group("http_server_throughput");
    g.measurement_time(Duration::from_secs(8));

    // Single-shot request — measures per-request latency.
    g.bench_function("stardust_http_single_get", |b| {
        let (handle, port) = rt.block_on(async {
            serve_in_memory(|_req| (200u16, "ok".to_string())).await
        });
        let addr = format!("127.0.0.1:{port}");
        b.iter(|| {
            rt.block_on(async {
                let d = sdust_bench::http::single_get(&addr, "/").await.unwrap();
                black_box(d);
            })
        });
        handle.abort();
    });

    // Batch of 100 sequential GETs — exercises the accept loop more.
    g.bench_function("stardust_http_seq_100", |b| {
        let (handle, port) = rt.block_on(async {
            serve_in_memory(|_req| (200u16, "ok".to_string())).await
        });
        let addr = format!("127.0.0.1:{port}");
        b.iter(|| {
            rt.block_on(async {
                let d = sdust_bench::http::sequential_get(&addr, "/", 100)
                    .await
                    .unwrap();
                black_box(d);
            })
        });
        handle.abort();
    });

    g.finish();
}

criterion_group!(benches, bench_http);
criterion_main!(benches);
