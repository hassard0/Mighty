use sdust_runtime::mailbox::{Mailbox, MessageFrame, SendPolicy, SmallPayload};
use sdust_sir::interp::value::Value;
use std::time::Duration;

#[tokio::test]
async fn fifo_and_bounded() {
    let mb = Mailbox::new(2, SendPolicy::Fail);
    let frame1 =
        MessageFrame::fire_and_forget("Ping", SmallPayload::inline(vec![Value::Unit]));
    let frame2 =
        MessageFrame::fire_and_forget("Pong", SmallPayload::inline(vec![Value::Unit]));
    mb.try_send(frame1).unwrap();
    mb.try_send(frame2).unwrap();
    assert!(mb
        .try_send(MessageFrame::fire_and_forget("X", SmallPayload::Empty))
        .is_err());
    let r1 = mb.recv().await.unwrap();
    assert_eq!(r1.proto_msg, "Ping");
    let r2 = mb.recv().await.unwrap();
    assert_eq!(r2.proto_msg, "Pong");
}

#[tokio::test]
async fn ask_reply() {
    let mb = Mailbox::new(8, SendPolicy::Block);
    let (frame, reply_rx) =
        MessageFrame::ask("Query", SmallPayload::Empty, Some(Duration::from_secs(1)));
    mb.try_send(frame).unwrap();
    let r = mb.recv().await.unwrap();
    assert_eq!(r.proto_msg, "Query");
    r.reply
        .unwrap()
        .send(Ok(Value::Int(7, sdust_types::IntKind::I32)))
        .unwrap();
    let v = reply_rx.await.unwrap().unwrap();
    assert!(matches!(v, Value::Int(7, _)));
}

#[tokio::test]
async fn drop_policy_silently_drops_when_full() {
    let mb = Mailbox::new(1, SendPolicy::Drop);
    mb.send(MessageFrame::fire_and_forget("A", SmallPayload::Empty))
        .await
        .unwrap();
    // Second send under Drop succeeds (returns Ok) but doesn't enqueue.
    mb.send(MessageFrame::fire_and_forget("B", SmallPayload::Empty))
        .await
        .unwrap();
    let r = mb.recv().await.unwrap();
    assert_eq!(r.proto_msg, "A");
}
