//! Smoke test: the bench-runner CLI can collect at least one sample
//! per category without panicking. We invoke it as a library (via the
//! same Stardust helpers the runner uses) rather than spawning the
//! binary so the test runs under `cargo test -p sdust-bench` without
//! a build artifact.

use sdust_bench::fixtures::stardust_10kloc;
use std::time::Instant;

#[test]
fn parse_one_iter_runs() {
    let src = stardust_10kloc();
    let t0 = Instant::now();
    let parsed = sdust_driver::parse_source(src, "smoke.sd".into());
    let dur = t0.elapsed();
    assert!(dur.as_millis() < 60_000, "parse took too long: {dur:?}");
    assert!(parsed.diagnostics.iter().all(|d| {
        !matches!(d.severity, sdust_diagnostics::Severity::Error)
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn mailbox_one_iter_runs() {
    use sdust_runtime::mailbox::{Mailbox, MessageFrame, SendPolicy, SmallPayload};
    let mb = Mailbox::new(8, SendPolicy::Block);
    let mut rx = mb.take_receiver().unwrap();
    let f = MessageFrame::fire_and_forget("Ping", SmallPayload::Empty);
    mb.send(f).await.unwrap();
    let r = rx.recv().await.unwrap();
    assert_eq!(r.proto_msg, "Ping");
}
