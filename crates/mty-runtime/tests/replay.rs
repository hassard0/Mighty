//! Integration tests for v0.17 deterministic-replay (Tier 1.4).
//!
//! These exercise the recorder + replayer surface end-to-end. The
//! recorder is driven directly (rather than via a real `Runtime`)
//! because the v0.17 slice keeps recorder calls explicit at the
//! capture sites — wiring them into the agent/runtime hot path is
//! the v0.18 follow-up (see REPLAY_V0_17_NOTES.md).

use mty_runtime::replay::{
    decode, encode, CountingStepHandler, Recorder, ReplayError, Replayer, StepHandler, TraceCodec,
    TraceEvent, TraceFile, TRACE_MAGIC, TRACE_WIRE_VERSION,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static N: AtomicU64 = AtomicU64::new(0);
fn tmp_path(label: &str) -> PathBuf {
    let n = N.fetch_add(1, Ordering::Relaxed);
    let mut p = std::env::temp_dir();
    p.push(format!(
        "mty-replay-it-{}-{}-{}.bin",
        label,
        std::process::id(),
        n
    ));
    p
}

#[test]
fn recorder_captures_spawn() {
    let r = Recorder::new(tmp_path("spawn"), 0, 1);
    r.record_spawn(7, "echo::Worker", None);
    let evs = r.events_snapshot();
    assert_eq!(evs.len(), 1);
    match &evs[0] {
        TraceEvent::Spawn {
            agent_id,
            agent_type,
            supervisor,
        } => {
            assert_eq!(*agent_id, 7);
            assert_eq!(agent_type, "echo::Worker");
            assert!(supervisor.is_none());
        }
        other => panic!("expected Spawn, got {other:?}"),
    }
}

#[test]
fn recorder_captures_message_send_and_dispatch() {
    let r = Recorder::new(tmp_path("send"), 0, 1);
    r.record_spawn(1, "Echo", None);
    r.record_message_sent(0, 1, "Ping", b"hello".to_vec());
    let idx = r.record_message_handled(1, "Ping", 42);
    assert_eq!(idx, 0);
    let evs = r.events_snapshot();
    assert_eq!(evs.len(), 3);
    assert!(matches!(&evs[1], TraceEvent::MessageSent { msg, .. } if msg == "Ping"));
    assert!(matches!(
        &evs[2],
        TraceEvent::MessageHandled {
            msg_idx: 0,
            elapsed_us: 42,
            ..
        }
    ));
}

#[test]
fn wire_format_round_trips_through_disk() {
    let path = tmp_path("rt");
    let r = Recorder::new(&path, 0xDEAD_BEEF, 4);
    r.record_spawn(1, "Echo", None);
    r.record_spawn(2, "Echo", Some(1));
    r.record_message_sent(1, 2, "Hello", vec![]);
    r.record_message_handled(2, "Hello", 100);
    r.record_clock_read(2, 12_345);
    r.record_random_read(2, vec![1, 2, 3, 4]);
    r.record_io_read(2, "file:/tmp/x", b"data".to_vec());
    r.record_budget_exhausted(2, "cpu");
    r.record_exit(2, "normal");
    r.flush_to_disk().unwrap();

    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(TRACE_MAGIC));
    let back = decode(&bytes).unwrap();
    assert_eq!(back.version, TRACE_WIRE_VERSION);
    assert_eq!(back.runtime_seed, 0xDEAD_BEEF);
    assert_eq!(back.worker_count, 4);
    assert_eq!(back.events.len(), 9);

    // And the in-memory equivalent matches the on-disk one byte for byte.
    let again = encode(&back, TraceCodec::Json).unwrap();
    assert_eq!(again, bytes);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_dump_json_matches_record() {
    let r = Recorder::new(tmp_path("dump"), 0, 1);
    r.record_spawn(1, "Echo", None);
    r.record_message_sent(0, 1, "Ping", vec![]);
    r.record_message_handled(1, "Ping", 5);
    r.record_exit(1, "normal");

    let trace = r.to_trace_file();
    let event_count = trace.events.len();
    let replayer = Replayer::new(trace);

    let mut buf = Vec::new();
    let written = replayer.dump_json(&mut buf).unwrap();
    assert_eq!(written, event_count);
    let dumped = String::from_utf8(buf).unwrap();
    assert_eq!(dumped.lines().count(), event_count);
    // Sanity: first JSON line is the spawn event.
    let first: serde_json::Value = serde_json::from_str(dumped.lines().next().unwrap()).unwrap();
    assert!(first["event"]["Spawn"]["agent_id"] == 1);
}

#[test]
fn replay_step_handler_visits_every_event() {
    let r = Recorder::new(tmp_path("step"), 0, 1);
    r.record_spawn(1, "Echo", None);
    for i in 0..5 {
        r.record_message_sent(0, 1, "Ping", vec![i]);
        r.record_message_handled(1, "Ping", 10);
    }
    r.record_exit(1, "normal");

    let replayer = Replayer::new(r.to_trace_file());
    let mut h = CountingStepHandler::new();
    let n = replayer.step(&mut h).unwrap();
    assert_eq!(n, 12); // 1 spawn + 5 send + 5 handled + 1 exit
    assert_eq!(h.spawn_count, 1);
    assert_eq!(h.message_sent_count, 5);
    assert_eq!(h.message_handled_count, 5);
    assert_eq!(h.exit_count, 1);
}

#[test]
fn self_consistency_passes_on_real_recording() {
    let r = Recorder::new(tmp_path("selfok"), 0, 1);
    r.record_spawn(1, "Echo", None);
    r.record_spawn(2, "Echo", None);
    r.record_message_sent(0, 1, "Ping", vec![]);
    r.record_message_handled(1, "Ping", 1);
    r.record_message_sent(1, 2, "Forwarded", vec![]);
    r.record_message_handled(2, "Forwarded", 1);
    r.record_exit(1, "normal");
    r.record_exit(2, "normal");

    let replayer = Replayer::new(r.to_trace_file());
    replayer.verify_self_consistent().unwrap();
}

#[test]
fn replay_from_path_works_end_to_end() {
    let path = tmp_path("e2e");
    let r = Recorder::new(&path, 5, 1);
    r.record_spawn(1, "Echo", None);
    r.record_message_sent(0, 1, "M", vec![]);
    r.record_message_handled(1, "M", 1);
    r.flush_to_disk().unwrap();

    let replayer = Replayer::from_path(&path).unwrap();
    assert_eq!(replayer.summary().event_count, 3);
    assert_eq!(replayer.summary().runtime_seed, 5);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn decode_rejects_garbage() {
    let err = decode(b"deadbeef").unwrap_err();
    assert!(matches!(err, mty_runtime::replay::RecorderError::BadMagic));
}

#[test]
fn step_handler_can_abort_replay() {
    struct Stop(usize);
    impl StepHandler for Stop {
        fn on_event(&mut self, i: usize, _ev: &TraceEvent) -> Result<(), String> {
            if i >= self.0 {
                Err(format!("stop at {i}"))
            } else {
                Ok(())
            }
        }
    }
    let r = Recorder::new(tmp_path("abort"), 0, 1);
    r.record_spawn(1, "Echo", None);
    r.record_message_sent(0, 1, "M", vec![]);
    r.record_message_handled(1, "M", 1);
    r.record_exit(1, "normal");
    let replayer = Replayer::new(r.to_trace_file());

    let mut h = Stop(2);
    let err = replayer.step(&mut h).unwrap_err();
    match err {
        ReplayError::HandlerAborted { index, .. } => assert_eq!(index, 2),
        other => panic!("expected HandlerAborted, got {other:?}"),
    }
}

#[test]
fn summary_reports_total_handler_microseconds() {
    let r = Recorder::new(tmp_path("us"), 0, 1);
    r.record_spawn(1, "Echo", None);
    r.record_message_handled(1, "A", 100);
    r.record_message_handled(1, "A", 200);
    r.record_message_handled(1, "A", 50);
    let trace: TraceFile = r.to_trace_file();
    let s = trace.summary();
    assert_eq!(s.total_handler_elapsed_us, 350);
    assert_eq!(s.message_handled_count, 3);
}
