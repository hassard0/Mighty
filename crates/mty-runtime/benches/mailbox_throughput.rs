//! v0.8 microbench: mailbox producer/consumer throughput.
//!
//! Compares:
//! - `single_recv`: classic `rx.recv().await` loop.
//! - `batched_recv`: `try_recv_many` drain in 32-msg batches.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use mty_runtime::mailbox::{try_recv_many, Mailbox, MessageFrame, SendPolicy, SmallPayload};
use std::sync::Arc;
use std::time::Duration;

const N: usize = 10_000;

fn bench_mailbox(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let mut g = c.benchmark_group("mailbox_throughput_v0_8");
    g.throughput(Throughput::Elements(N as u64));
    g.measurement_time(Duration::from_secs(6));

    g.bench_function("single_recv_empty_payload", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mb = Arc::new(Mailbox::new(N, SendPolicy::Block));
                let mut rx = mb.take_receiver().unwrap();
                let prod = {
                    let mb = mb.clone();
                    tokio::spawn(async move {
                        for _ in 0..N {
                            let f = MessageFrame::fire_and_forget("M", SmallPayload::Empty);
                            mb.send(f).await.unwrap();
                        }
                    })
                };
                for _ in 0..N {
                    let r = rx.recv().await.unwrap();
                    black_box(r);
                }
                prod.await.unwrap();
            })
        })
    });

    g.bench_function("batched_recv_empty_payload", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mb = Arc::new(Mailbox::new(N, SendPolicy::Block));
                let mut rx = mb.take_receiver().unwrap();
                let prod = {
                    let mb = mb.clone();
                    tokio::spawn(async move {
                        for _ in 0..N {
                            let f = MessageFrame::fire_and_forget("M", SmallPayload::Empty);
                            mb.send(f).await.unwrap();
                        }
                    })
                };
                let mut buf = Vec::with_capacity(64);
                let mut consumed = 0usize;
                while consumed < N {
                    let n = try_recv_many(&mut rx, &mut buf, 64);
                    if n > 0 {
                        consumed += n;
                        for f in buf.drain(..) {
                            black_box(f);
                        }
                    } else if let Some(f) = rx.recv().await {
                        consumed += 1;
                        black_box(f);
                    } else {
                        break;
                    }
                }
                prod.await.unwrap();
            })
        })
    });

    g.finish();
}

criterion_group!(benches, bench_mailbox);
criterion_main!(benches);
