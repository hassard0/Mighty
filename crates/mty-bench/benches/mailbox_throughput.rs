//! Bench: producer/consumer mailbox throughput.
//!
//! Pushes N messages from a tokio task into a bounded mailbox and
//! drains them on the bench task. Reports throughput as messages/sec.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use mty_runtime::mailbox::{Mailbox, MessageFrame, SendPolicy, SmallPayload};
use std::sync::Arc;
use std::time::Duration;

fn bench_mailbox(c: &mut Criterion) {
    const N: usize = 10_000;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let mut g = c.benchmark_group("mailbox_throughput");
    g.throughput(Throughput::Elements(N as u64));
    g.measurement_time(Duration::from_secs(8));

    g.bench_function("stardust_mailbox_1p1c", |b| {
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

    g.finish();
}

criterion_group!(benches, bench_mailbox);
criterion_main!(benches);
