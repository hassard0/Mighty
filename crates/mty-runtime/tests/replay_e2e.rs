//! End-to-end tests for v0.18 replay-recorder wiring (Tier 1.4).
//!
//! These exercise the recorder driven by a real `Runtime` rather than
//! the direct-API surface in `tests/replay.rs`. They verify the
//! Runtime hot path (spawn / send / ask / handler dispatch / IO / budget)
//! actually populates a trace file when `MTY_RECORD_TRACE` is set, and
//! that the disabled path stays free of recorder calls.
//!
//! These tests use `serial_test`-style manual serialization (via a
//! global Mutex) because the recorder is process-wide and concurrent
//! installs would race. The Mutex is held for the duration of each
//! test — short enough that wall-clock impact is negligible.

use mty_ir::interp::value::Value;
use mty_runtime::replay::{decode, global_recorder, uninstall, TraceEvent, RECORD_ENV};
use mty_runtime::RuntimeBuilder;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// Global serializer — only one e2e test may touch the process-wide
// recorder at a time. Lazy-initialized so any test file ordering
// works.
fn recorder_serializer() -> &'static Mutex<()> {
    static M: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
    &M
}

static N: AtomicU64 = AtomicU64::new(0);
fn tmp_trace_path(label: &str) -> PathBuf {
    let n = N.fetch_add(1, Ordering::Relaxed);
    let mut p = std::env::temp_dir();
    p.push(format!(
        "mty-replay-e2e-{}-{}-{}.bin",
        label,
        std::process::id(),
        n
    ));
    p
}

fn compile(src: &str) -> Arc<mty_ir::ir::Program> {
    use mty_driver::pipeline::{lower, lower_to_sir, parse_source, type_and_borrow_check};
    let parsed = parse_source(src.to_string(), "test.mty".to_string());
    let (pkg, _diags) = lower(&parsed);
    let _ = type_and_borrow_check(&pkg);
    let (prog, _diags) = lower_to_sir(&pkg);
    Arc::new(prog)
}

const ECHO_SRC: &str = r#"
protocol Echo { Ping(s: Str) -> Str }
agent Echoer: Echo { on Ping(s: Str) -> s }
fn main() { () }
"#;

/// Helper to set/unset the env var around a closure. We can't use
/// `std::env::set_var` racing across tests without holding the
/// serializer.
struct EnvGuard {
    key: &'static str,
    prev: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let prev = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, prev }
    }
    fn unset(key: &'static str) -> Self {
        let prev = std::env::var(key).ok();
        std::env::remove_var(key);
        Self { key, prev }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match self.prev.take() {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

#[test]
fn recording_round_trip() {
    let _g = recorder_serializer().lock();
    let _ = uninstall();
    let path = tmp_trace_path("round_trip");
    let _env = EnvGuard::set(RECORD_ENV, path.to_str().unwrap());

    let prog = compile(ECHO_SRC);
    let rt = RuntimeBuilder::new().workers(1).build(prog);
    let rt_arc = rt.scheduler.rt.clone();
    rt_arc.block_on(async {
        let a = rt.spawn_agent("Echoer", vec![]).await.unwrap();
        let b = rt.spawn_agent("Echoer", vec![]).await.unwrap();
        // Two asks so we get two MessageSent + two MessageHandled.
        let _ = rt
            .ask(&a, "Ping", vec![Value::Str("hi".into())], None)
            .await
            .unwrap();
        let _ = rt
            .ask(&b, "Ping", vec![Value::Str("hello".into())], None)
            .await
            .unwrap();
        let _ = rt.shutdown().await;
    });

    // After shutdown the recorder has flushed + uninstalled.
    assert!(
        global_recorder().is_none(),
        "recorder should be uninstalled"
    );
    let bytes = std::fs::read(&path).expect("trace file should exist");
    let trace = decode(&bytes).expect("decode trace");
    let mut spawn = 0usize;
    let mut sent = 0usize;
    let mut handled = 0usize;
    let mut exit = 0usize;
    for ev in &trace.events {
        match ev {
            TraceEvent::Spawn { .. } => spawn += 1,
            TraceEvent::MessageSent { .. } => sent += 1,
            TraceEvent::MessageHandled { .. } => handled += 1,
            TraceEvent::Exit { .. } => exit += 1,
            _ => {}
        }
    }
    assert!(spawn >= 2, "expected >=2 Spawn events, got {spawn}");
    assert!(sent >= 2, "expected >=2 MessageSent events, got {sent}");
    assert!(
        handled >= 2,
        "expected >=2 MessageHandled events, got {handled}"
    );
    // Exit events may or may not appear depending on whether the
    // agent loop drained — at minimum, the recorder did flush.
    let _ = exit;
    let _ = std::fs::remove_file(&path);
}

#[test]
fn disabled_when_env_unset() {
    let _g = recorder_serializer().lock();
    let _ = uninstall();
    let _env = EnvGuard::unset(RECORD_ENV);

    let prog = compile(ECHO_SRC);
    let rt = RuntimeBuilder::new().workers(1).build(prog);
    // Right after build: no recorder installed.
    assert!(
        global_recorder().is_none(),
        "no MTY_RECORD_TRACE => no recorder"
    );
    let rt_arc = rt.scheduler.rt.clone();
    rt_arc.block_on(async {
        let h = rt.spawn_agent("Echoer", vec![]).await.unwrap();
        let _ = rt
            .ask(&h, "Ping", vec![Value::Str("x".into())], None)
            .await
            .unwrap();
        let _ = rt.shutdown().await;
    });
    assert!(global_recorder().is_none());
}

#[test]
fn recording_captures_distinct_agent_ids() {
    let _g = recorder_serializer().lock();
    let _ = uninstall();
    let path = tmp_trace_path("agent_ids");
    let _env = EnvGuard::set(RECORD_ENV, path.to_str().unwrap());

    let prog = compile(ECHO_SRC);
    let rt = RuntimeBuilder::new().workers(1).build(prog);
    let rt_arc = rt.scheduler.rt.clone();
    rt_arc.block_on(async {
        let _a = rt.spawn_agent("Echoer", vec![]).await.unwrap();
        let _b = rt.spawn_agent("Echoer", vec![]).await.unwrap();
        let _c = rt.spawn_agent("Echoer", vec![]).await.unwrap();
        let _ = rt.shutdown().await;
    });
    let bytes = std::fs::read(&path).expect("trace file");
    let trace = decode(&bytes).unwrap();
    // Distinct agent_ids on Spawn events.
    let mut ids: Vec<u64> = trace
        .events
        .iter()
        .filter_map(|e| match e {
            TraceEvent::Spawn { agent_id, .. } => Some(*agent_id),
            _ => None,
        })
        .collect();
    ids.sort();
    ids.dedup();
    assert!(ids.len() >= 3, "expected 3 distinct spawn ids, got {ids:?}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn recorder_survives_unknown_handler_trap() {
    let _g = recorder_serializer().lock();
    let _ = uninstall();
    let path = tmp_trace_path("trap");
    let _env = EnvGuard::set(RECORD_ENV, path.to_str().unwrap());

    let prog = compile(ECHO_SRC);
    let rt = RuntimeBuilder::new().workers(1).build(prog);
    let rt_arc = rt.scheduler.rt.clone();
    rt_arc.block_on(async {
        let h = rt.spawn_agent("Echoer", vec![]).await.unwrap();
        // "Bogus" is not a handler on Echoer => the agent loop traps.
        // We use ask with a short deadline so the test doesn't hang
        // if the trap doesn't reply.
        let _ = rt
            .ask(
                &h,
                "Bogus",
                vec![],
                Some(std::time::Duration::from_millis(500)),
            )
            .await; // ignore result — may be HandlerNotFound or deadline.
        let _ = rt.shutdown().await;
    });
    // Recorder must have flushed despite the trap path. The file
    // should at minimum contain the Spawn + the MessageSent.
    let bytes = std::fs::read(&path).expect("trace should still be written");
    let trace = decode(&bytes).unwrap();
    let has_spawn = trace
        .events
        .iter()
        .any(|e| matches!(e, TraceEvent::Spawn { .. }));
    let has_sent = trace
        .events
        .iter()
        .any(|e| matches!(e, TraceEvent::MessageSent { .. }));
    assert!(has_spawn, "Spawn must be in trace even after trap");
    assert!(has_sent, "MessageSent must be in trace even after trap");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn fire_and_forget_send_captured() {
    let _g = recorder_serializer().lock();
    let _ = uninstall();
    let path = tmp_trace_path("send");
    let _env = EnvGuard::set(RECORD_ENV, path.to_str().unwrap());

    let prog = compile(ECHO_SRC);
    let rt = RuntimeBuilder::new().workers(1).build(prog);
    let rt_arc = rt.scheduler.rt.clone();
    rt_arc.block_on(async {
        let h = rt.spawn_agent("Echoer", vec![]).await.unwrap();
        for _ in 0..3 {
            rt.send(&h, "Ping", vec![Value::Str("fire".into())])
                .await
                .unwrap();
        }
        // Give the agent a beat to drain its mailbox before we tear down.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let _ = rt.shutdown().await;
    });
    let bytes = std::fs::read(&path).expect("trace file");
    let trace = decode(&bytes).unwrap();
    let sent = trace
        .events
        .iter()
        .filter(|e| matches!(e, TraceEvent::MessageSent { msg, .. } if msg == "Ping"))
        .count();
    assert!(sent >= 3, "expected 3 MessageSent(Ping), got {sent}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn message_handled_carries_monotonic_msg_idx() {
    let _g = recorder_serializer().lock();
    let _ = uninstall();
    let path = tmp_trace_path("msgidx");
    let _env = EnvGuard::set(RECORD_ENV, path.to_str().unwrap());

    let prog = compile(ECHO_SRC);
    let rt = RuntimeBuilder::new().workers(1).build(prog);
    let rt_arc = rt.scheduler.rt.clone();
    rt_arc.block_on(async {
        let h = rt.spawn_agent("Echoer", vec![]).await.unwrap();
        let _ = rt
            .ask(&h, "Ping", vec![Value::Str("a".into())], None)
            .await
            .unwrap();
        let _ = rt
            .ask(&h, "Ping", vec![Value::Str("b".into())], None)
            .await
            .unwrap();
        let _ = rt
            .ask(&h, "Ping", vec![Value::Str("c".into())], None)
            .await
            .unwrap();
        let _ = rt.shutdown().await;
    });
    let bytes = std::fs::read(&path).expect("trace file");
    let trace = decode(&bytes).unwrap();
    // For the single agent we spawned, msg_idx on MessageHandled events
    // should be 0, 1, 2 in order.
    let handled: Vec<u64> = trace
        .events
        .iter()
        .filter_map(|e| match e {
            TraceEvent::MessageHandled { msg_idx, .. } => Some(*msg_idx),
            _ => None,
        })
        .collect();
    assert!(handled.len() >= 3, "expected >=3 handled, got {handled:?}");
    // The first three should be strictly 0,1,2 (monotonic per agent).
    assert_eq!(handled[0], 0, "first msg_idx must be 0, got {handled:?}");
    assert_eq!(handled[1], 1, "second msg_idx must be 1, got {handled:?}");
    assert_eq!(handled[2], 2, "third msg_idx must be 2, got {handled:?}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn empty_path_env_treated_as_unset() {
    let _g = recorder_serializer().lock();
    let _ = uninstall();
    // Empty path = treated as unset by install_from_env.
    let _env = EnvGuard::set(RECORD_ENV, "");

    let prog = compile(ECHO_SRC);
    let rt = RuntimeBuilder::new().workers(1).build(prog);
    assert!(
        global_recorder().is_none(),
        "empty env should not install a recorder"
    );
    let rt_arc = rt.scheduler.rt.clone();
    rt_arc.block_on(async {
        let _ = rt.shutdown().await;
    });
}

#[test]
fn recorder_uninstalled_after_shutdown() {
    let _g = recorder_serializer().lock();
    let _ = uninstall();
    let path = tmp_trace_path("uninstall");
    let _env = EnvGuard::set(RECORD_ENV, path.to_str().unwrap());

    let prog = compile(ECHO_SRC);
    let rt = RuntimeBuilder::new().workers(1).build(prog);
    // During Runtime lifetime: recorder is installed.
    assert!(global_recorder().is_some());
    let rt_arc = rt.scheduler.rt.clone();
    rt_arc.block_on(async {
        let _ = rt.shutdown().await;
    });
    // After shutdown: recorder is gone.
    assert!(
        global_recorder().is_none(),
        "shutdown should uninstall recorder"
    );
    let _ = std::fs::remove_file(&path);
}
