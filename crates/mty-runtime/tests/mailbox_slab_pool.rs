//! v0.3 (A40 closure): slab-pool mailbox tests.
//!
//! Verifies:
//! - Sending N > pool_size messages reuses slots.
//! - FIFO ordering of message delivery is preserved.
//! - Send under Fail policy returns MT5012 when pool + channel full.
//! - Block policy backpressures until handler drains.

use mty_ir::interp::value::Value;
use mty_runtime::mailbox::{Mailbox, MessageFrame, SendPolicy, SmallPayload};
use mty_runtime::slab_pool::{SlabPool, DEFAULT_INLINE_BYTES};
use mty_runtime::RuntimeError;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn slab_reuse_preserves_fifo_for_many_messages() {
    let mb = Arc::new(Mailbox::new(4, SendPolicy::Block));
    let recv = mb.clone();
    let consumer = tokio::spawn(async move {
        let mut out = vec![];
        let mut rx = recv.take_receiver().unwrap();
        for _ in 0..16 {
            let m = rx.recv().await.unwrap();
            out.push(m.proto_msg.clone());
            drop(m); // returns slot to pool
        }
        out
    });
    for i in 0..16 {
        let msg = format!("M{i:02}");
        mb.send(MessageFrame::fire_and_forget(&msg, SmallPayload::Empty))
            .await
            .unwrap();
    }
    let got = consumer.await.unwrap();
    let expected: Vec<String> = (0..16).map(|i| format!("M{i:02}")).collect();
    assert_eq!(got, expected, "FIFO of slab-reused frames");
}

#[tokio::test]
async fn fail_policy_returns_sd5012_when_full() {
    let mb = Mailbox::new(1, SendPolicy::Fail);
    mb.send(MessageFrame::fire_and_forget("A", SmallPayload::Empty))
        .await
        .unwrap();
    let r = mb
        .send(MessageFrame::fire_and_forget("B", SmallPayload::Empty))
        .await;
    let err = r.unwrap_err();
    assert!(matches!(err, RuntimeError::MailboxFull { .. }));
    assert_eq!(err.diag_code(), "MT5012");
}

#[tokio::test(start_paused = true)]
async fn block_policy_backpressures_until_drained() {
    let mb = Arc::new(Mailbox::new(2, SendPolicy::Block));
    // Fill the channel.
    mb.send(MessageFrame::fire_and_forget("A", SmallPayload::Empty))
        .await
        .unwrap();
    mb.send(MessageFrame::fire_and_forget("B", SmallPayload::Empty))
        .await
        .unwrap();
    // Third send should block. Race with a timeout to confirm.
    let mb2 = mb.clone();
    let send_fut = tokio::spawn(async move {
        mb2.send(MessageFrame::fire_and_forget("C", SmallPayload::Empty))
            .await
    });
    // Give the channel a chance to register backpressure.
    tokio::time::advance(Duration::from_millis(1)).await;
    assert!(
        !send_fut.is_finished(),
        "send should still be blocked on Block policy"
    );
    // Drain one message; the blocked send should now complete.
    let mut rx = mb.take_receiver().unwrap();
    let _ = rx.recv().await.unwrap();
    let _ = tokio::time::timeout(Duration::from_millis(100), send_fut)
        .await
        .expect("send to complete after drain")
        .unwrap();
}

#[test]
fn slab_pool_basic_acquire_release() {
    let pool = SlabPool::new(3);
    assert_eq!(pool.free_count(), 3);
    let a = pool.try_acquire(b"foo").unwrap();
    let b = pool.try_acquire(b"bar").unwrap();
    let c = pool.try_acquire(b"baz").unwrap();
    assert!(pool.try_acquire(b"x").is_none());
    drop((a, b, c));
    assert_eq!(pool.free_count(), 3);
}

#[test]
fn slab_pool_inline_bytes_default() {
    let pool = SlabPool::default();
    assert_eq!(pool.inline_bytes(), DEFAULT_INLINE_BYTES);
}

#[tokio::test]
async fn mailbox_introspection_tracks_slot_usage() {
    let mb = Mailbox::new(4, SendPolicy::Block);
    let s0 = mb.introspect();
    assert_eq!(s0.slab_used, 0);
    assert_eq!(s0.slab_capacity, 4);
    mb.send(MessageFrame::fire_and_forget("A", SmallPayload::Empty))
        .await
        .unwrap();
    mb.send(MessageFrame::fire_and_forget("B", SmallPayload::Empty))
        .await
        .unwrap();
    let s1 = mb.introspect();
    assert_eq!(s1.slab_used, 2, "two frames in-flight");
    // Drain & verify slots return.
    let mut rx = mb.take_receiver().unwrap();
    let _ = rx.recv().await.unwrap();
    let _ = rx.recv().await.unwrap();
    let s2 = mb.introspect();
    assert_eq!(s2.slab_used, 0, "all slots returned after drain");
}

#[tokio::test]
async fn payload_overflow_when_large_proto_name() {
    // 200-char proto name will spill the slab inline payload (default
    // 64 bytes) into the overflow box. The mailbox must still
    // function; we're checking the spill path doesn't crash.
    let big_name: String = "X".repeat(200);
    let mb = Mailbox::new(2, SendPolicy::Block);
    mb.send(MessageFrame::fire_and_forget(
        &big_name,
        SmallPayload::inline(vec![Value::Unit]),
    ))
    .await
    .unwrap();
    let m = mb.recv().await.unwrap();
    assert_eq!(m.proto_msg, big_name);
}
