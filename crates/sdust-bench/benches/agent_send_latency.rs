//! Bench: end-to-end fire-and-forget latency between sender and
//! receiver running on the same tokio runtime.
//!
//! Spec §0 "agent-first" headline. We measure the minimum-overhead
//! shape: one sender, one mailbox, recv on the same task. Anything
//! larger (full agent SIR turn, supervisor dispatch) layers atop this.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use sdust_runtime::mailbox::{Mailbox, MessageFrame, SendPolicy, SmallPayload};
use std::time::Duration;

fn bench_latency(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let mut g = c.benchmark_group("agent_send_latency");
    g.measurement_time(Duration::from_secs(5));

    g.bench_function("stardust_mailbox_send_recv", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mb = Mailbox::new(8, SendPolicy::Block);
                let mut rx = mb.take_receiver().unwrap();
                let frame =
                    MessageFrame::fire_and_forget("Ping", SmallPayload::Empty);
                mb.send(frame).await.unwrap();
                let r = rx.recv().await.unwrap();
                black_box(r);
            })
        })
    });

    g.bench_function("stardust_mailbox_try_send_recv", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mb = Mailbox::new(8, SendPolicy::Fail);
                let mut rx = mb.take_receiver().unwrap();
                let frame =
                    MessageFrame::fire_and_forget("Ping", SmallPayload::Empty);
                mb.try_send(frame).unwrap();
                let r = rx.recv().await.unwrap();
                black_box(r);
            })
        })
    });

    g.finish();
}

criterion_group!(benches, bench_latency);
criterion_main!(benches);
