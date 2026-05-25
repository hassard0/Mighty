//! v0.8 microbench: agent send latency (P50/P99).
//!
//! Compares the slab fast-path for `SmallPayload::Empty` against
//! `SmallPayload::Inline` (which still goes through the slab admit).

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use mty_runtime::mailbox::{Mailbox, MessageFrame, SendPolicy, SmallPayload};
use std::time::Duration;

fn bench_latency(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let mut g = c.benchmark_group("agent_send_latency_v0_8");
    g.measurement_time(Duration::from_secs(5));

    // Empty payload — exercises the fast path (no slab acquire).
    g.bench_function("send_recv_empty", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mb = Mailbox::new(8, SendPolicy::Block);
                let mut rx = mb.take_receiver().unwrap();
                let frame = MessageFrame::fire_and_forget("Ping", SmallPayload::Empty);
                mb.send(frame).await.unwrap();
                let r = rx.recv().await.unwrap();
                black_box(r);
            })
        })
    });

    // Inline payload of 1 value — slab admit + stack-resident descriptor.
    g.bench_function("send_recv_inline_1", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mb = Mailbox::new(8, SendPolicy::Block);
                let mut rx = mb.take_receiver().unwrap();
                let frame = MessageFrame::fire_and_forget(
                    "Ping",
                    SmallPayload::inline(vec![mty_ir::interp::value::Value::Int(
                        7i128,
                        mty_types::IntKind::I64,
                    )]),
                );
                mb.send(frame).await.unwrap();
                let r = rx.recv().await.unwrap();
                black_box(r);
            })
        })
    });

    // try_send (no awaits) — exercises the synchronous fast path.
    g.bench_function("try_send_empty", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mb = Mailbox::new(8, SendPolicy::Fail);
                let mut rx = mb.take_receiver().unwrap();
                let frame = MessageFrame::fire_and_forget("Ping", SmallPayload::Empty);
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
