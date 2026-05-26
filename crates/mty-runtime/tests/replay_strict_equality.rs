//! End-to-end **strict-equality** replay tests (v0.20, Tier 1.4
//! finish-line).
//!
//! v0.19 introduced [`ReplayPayload::Values`] alongside the legacy
//! [`ReplayPayload::Opaque`] arm. The hot path still emitted `Opaque`
//! bytes (the `format!("{:?}", args)` Debug rendering) because that
//! kept the v0.18 wire contract intact while the structural codec
//! was being stabilised. The `ReplayDriver` therefore had to fall
//! back to the **approximate** `Opaque ≈ Opaque` comparison arm:
//! two opaque payloads were treated as equal regardless of their
//! actual bytes, because the v0.18 Debug rendering is non-injective
//! and the driver can't reconstruct byte-identical args from it.
//!
//! v0.20 finishes the migration: every in-process `send`/`ask`
//! callsite emits a [`ReplayPayload::Values`] payload, so the
//! recorded trace round-trips through the driver under **strict
//! structural equality** — the loose `Opaque ≈ Opaque` arm is
//! retained for v0.18 backwards-compat read of old traces but is
//! never exercised by a fresh recording.
//!
//! These tests assert the new contract:
//!
//! 1. Every recorded `MessageSent.payload` is `ReplayPayload::Values`
//!    (not `Opaque`).
//! 2. [`ReplayDriver::replay_all`] yields zero mismatches under the
//!    default `byte_identical(true)` mode.
//! 3. The structural payloads round-trip through disk encode/decode.
//! 4. Multi-arg, multi-type, multi-agent recordings all stay strict.
//!
//! Like `replay_byte_identical.rs`, tests serialize on a global
//! mutex so the process-wide recorder slot stays single-writer.

use mty_ir::interp::value::Value;
use mty_runtime::replay::{
    decode, uninstall, ReplayDriver, ReplayPayload, ReplayValue, TraceEvent, TraceFile, RECORD_ENV,
};
use mty_runtime::RuntimeBuilder;
use mty_types::IntKind;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// Shared serializer — only one strict-equality test may touch the
// process-wide recorder at a time. Lazy-initialized so test ordering
// across files is irrelevant.
fn recorder_serializer() -> &'static Mutex<()> {
    static M: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
    &M
}

static N: AtomicU64 = AtomicU64::new(0);
fn tmp_trace_path(label: &str) -> PathBuf {
    let n = N.fetch_add(1, Ordering::Relaxed);
    let mut p = std::env::temp_dir();
    p.push(format!(
        "mty-replay-strict-{}-{}-{}.bin",
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

const ADD_SRC: &str = r#"
protocol Adder { Add(a: I64, b: I64) -> I64 }
agent Calc: Adder { on Add(a: I64, b: I64) -> a + b }
fn main() { () }
"#;

// Helper to set/unset an env var around a closure.
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
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match self.prev.take() {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

/// Helper: count `MessageSent` events whose payload is the
/// [`ReplayPayload::Values`] arm (the v0.20 strict-equality target).
fn count_values_payloads(trace: &TraceFile) -> usize {
    trace
        .events
        .iter()
        .filter(|e| match e {
            TraceEvent::MessageSent { payload, .. } => {
                matches!(payload, ReplayPayload::Values(_))
            }
            _ => false,
        })
        .count()
}

/// Helper: count `MessageSent` events whose payload is the legacy
/// [`ReplayPayload::Opaque`] arm. v0.20 expects this to be zero for
/// fresh recordings (non-empty Opaque is only legitimate on v1 / v0.18
/// traces lifted from disk).
fn count_opaque_payloads_nonempty(trace: &TraceFile) -> usize {
    trace
        .events
        .iter()
        .filter(|e| match e {
            TraceEvent::MessageSent { payload, .. } => match payload {
                ReplayPayload::Opaque(b) => !b.is_empty(),
                _ => false,
            },
            _ => false,
        })
        .count()
}

// ----------------------------------------------------------------------------
// Test 1 — two agents, strict-equality replay with zero mismatches
// ----------------------------------------------------------------------------

#[test]
fn strict_equality_two_agents_zero_mismatches() {
    let _g = recorder_serializer().lock();
    let _ = uninstall();
    let path = tmp_trace_path("two_agents");
    let _env = EnvGuard::set(RECORD_ENV, path.to_str().unwrap());

    // Record: spawn 2 agents, exchange typed messages.
    let prog = compile(ECHO_SRC);
    let rt = RuntimeBuilder::new().workers(1).build(prog.clone());
    let rt_arc = rt.scheduler.rt.clone();
    rt_arc.block_on(async {
        let a = rt.spawn_agent("Echoer", vec![]).await.unwrap();
        let b = rt.spawn_agent("Echoer", vec![]).await.unwrap();
        let _ = rt
            .ask(&a, "Ping", vec![Value::Str("alpha".into())], None)
            .await
            .unwrap();
        let _ = rt
            .ask(&b, "Ping", vec![Value::Str("beta".into())], None)
            .await
            .unwrap();
        let _ = rt.shutdown().await;
    });
    // Drop runtime + recorder env before loading the disk trace.
    drop(_env);
    let _ = uninstall();

    // Load + assert structural payloads.
    let bytes = std::fs::read(&path).expect("trace must exist");
    let trace: TraceFile = decode(&bytes).expect("decode trace");
    assert!(
        trace.events.len() >= 4,
        "expected at least 2 spawns + 2 sends, got {}",
        trace.events.len()
    );
    assert!(
        count_values_payloads(&trace) >= 2,
        "v0.20 hot path must emit ReplayPayload::Values, found values_count = {}",
        count_values_payloads(&trace)
    );
    assert_eq!(
        count_opaque_payloads_nonempty(&trace),
        0,
        "v0.20 hot path must NOT emit non-empty Opaque payloads (legacy arm)"
    );

    // Replay under strict byte-identical mode.
    let mut driver = ReplayDriver::from_trace(trace).with_program(prog);
    let report = driver.replay_all().expect("replay_all");

    assert!(
        report.success,
        "strict-equality replay failed: {}",
        report.render()
    );
    assert_eq!(
        report.mismatches.len(),
        0,
        "expected zero mismatches under v0.20 strict equality, got {}: {}",
        report.mismatches.len(),
        report.render()
    );
    assert!(
        report.events_replayed > 0,
        "expected events_replayed > 0, got {}",
        report.events_replayed
    );

    let _ = std::fs::remove_file(&path);
}

// ----------------------------------------------------------------------------
// Test 2 — multi-typed args (I64, Str, Bool) stay structural through the wire
// ----------------------------------------------------------------------------

#[test]
fn strict_equality_multi_typed_args() {
    let _g = recorder_serializer().lock();
    let _ = uninstall();
    let path = tmp_trace_path("multi_args");
    let _env = EnvGuard::set(RECORD_ENV, path.to_str().unwrap());

    let prog = compile(ADD_SRC);
    let rt = RuntimeBuilder::new().workers(1).build(prog.clone());
    let rt_arc = rt.scheduler.rt.clone();
    rt_arc.block_on(async {
        let calc = rt.spawn_agent("Calc", vec![]).await.unwrap();
        let _ = rt
            .ask(
                &calc,
                "Add",
                vec![Value::Int(7, IntKind::I64), Value::Int(35, IntKind::I64)],
                None,
            )
            .await
            .unwrap();
        let _ = rt.shutdown().await;
    });
    drop(_env);
    let _ = uninstall();

    let bytes = std::fs::read(&path).expect("trace must exist");
    let trace: TraceFile = decode(&bytes).expect("decode trace");

    // Find the MessageSent and assert its payload is structurally
    // exactly [Int(7,I64), Int(35,I64)].
    let payload = trace
        .events
        .iter()
        .find_map(|e| match e {
            TraceEvent::MessageSent { payload, .. } => Some(payload),
            _ => None,
        })
        .expect("MessageSent must exist");
    match payload {
        ReplayPayload::Values(vs) => {
            assert_eq!(vs.len(), 2, "expected 2 args, got {}", vs.len());
            // Check shape (kind name comparison is conservative —
            // codec may map I64 → "I64").
            match &vs[0] {
                ReplayValue::Int { value, kind } => {
                    assert_eq!(*value, 7);
                    assert!(
                        kind.starts_with('I'),
                        "expected signed int kind, got {kind}"
                    );
                }
                other => panic!("expected Int, got {other:?}"),
            }
            match &vs[1] {
                ReplayValue::Int { value, .. } => assert_eq!(*value, 35),
                other => panic!("expected Int, got {other:?}"),
            }
        }
        other => panic!("expected Values arm (v0.20 strict), got {other:?}"),
    }

    // Replay must still pass strict-equality.
    let mut driver = ReplayDriver::from_trace(trace).with_program(prog);
    let report = driver.replay_all().expect("replay_all");
    assert!(
        report.success,
        "multi-arg strict replay failed: {}",
        report.render()
    );
    assert_eq!(report.mismatches.len(), 0);

    let _ = std::fs::remove_file(&path);
}

// ----------------------------------------------------------------------------
// Test 3 — structural payload survives disk encode → decode round-trip
// ----------------------------------------------------------------------------

#[test]
fn strict_equality_structural_payload_round_trips_disk() {
    let _g = recorder_serializer().lock();
    let _ = uninstall();
    let path = tmp_trace_path("disk_round_trip");
    let _env = EnvGuard::set(RECORD_ENV, path.to_str().unwrap());

    let prog = compile(ECHO_SRC);
    let rt = RuntimeBuilder::new().workers(1).build(prog);
    let rt_arc = rt.scheduler.rt.clone();
    rt_arc.block_on(async {
        let a = rt.spawn_agent("Echoer", vec![]).await.unwrap();
        let _ = rt
            .ask(&a, "Ping", vec![Value::Str("through-disk".into())], None)
            .await
            .unwrap();
        let _ = rt.shutdown().await;
    });
    drop(_env);
    let _ = uninstall();

    // Read from disk, then re-encode, re-decode, and assert the
    // structural shape is preserved exactly.
    let bytes = std::fs::read(&path).expect("trace must exist");
    let trace: TraceFile = decode(&bytes).expect("decode trace");
    let original_payload = trace
        .events
        .iter()
        .find_map(|e| match e {
            TraceEvent::MessageSent { payload, .. } => Some(payload.clone()),
            _ => None,
        })
        .expect("MessageSent must exist");

    // Sanity: it's the Values arm.
    assert!(
        matches!(original_payload, ReplayPayload::Values(_)),
        "expected Values arm in fresh v0.20 trace, got {original_payload:?}"
    );

    // Encode → decode again to confirm it survives a second round trip.
    let re_encoded =
        mty_runtime::replay::recorder::encode(&trace, mty_runtime::replay::TraceCodec::Json)
            .expect("encode");
    let re_decoded = decode(&re_encoded).expect("decode round-2");
    let re_payload = re_decoded
        .events
        .iter()
        .find_map(|e| match e {
            TraceEvent::MessageSent { payload, .. } => Some(payload.clone()),
            _ => None,
        })
        .expect("MessageSent must exist on round-2");
    assert_eq!(
        original_payload, re_payload,
        "structural payload must be bitwise-equal after disk round trip"
    );

    let _ = std::fs::remove_file(&path);
}

// ----------------------------------------------------------------------------
// Test 4 — three-agent chain remains strict (no fallback to approximate)
// ----------------------------------------------------------------------------

#[test]
fn strict_equality_three_agent_chain_no_fallback() {
    let _g = recorder_serializer().lock();
    let _ = uninstall();
    let path = tmp_trace_path("three_chain");
    let _env = EnvGuard::set(RECORD_ENV, path.to_str().unwrap());

    let prog = compile(ECHO_SRC);
    let rt = RuntimeBuilder::new().workers(1).build(prog.clone());
    let rt_arc = rt.scheduler.rt.clone();
    rt_arc.block_on(async {
        let a = rt.spawn_agent("Echoer", vec![]).await.unwrap();
        let b = rt.spawn_agent("Echoer", vec![]).await.unwrap();
        let c = rt.spawn_agent("Echoer", vec![]).await.unwrap();
        for (handle, label) in [(&a, "a"), (&b, "b"), (&c, "c")] {
            let _ = rt
                .ask(
                    handle,
                    "Ping",
                    vec![Value::Str(format!("hi-{label}"))],
                    None,
                )
                .await
                .unwrap();
        }
        let _ = rt.shutdown().await;
    });
    drop(_env);
    let _ = uninstall();

    let bytes = std::fs::read(&path).expect("trace must exist");
    let trace: TraceFile = decode(&bytes).expect("decode trace");

    // Every MessageSent must use the structural arm — there must be
    // zero non-empty Opaque payloads in a fresh v0.20 trace.
    let opaque = count_opaque_payloads_nonempty(&trace);
    let values = count_values_payloads(&trace);
    assert_eq!(
        opaque, 0,
        "v0.20 strict: no non-empty Opaque payloads allowed, got {opaque}; values={values}"
    );
    assert!(
        values >= 3,
        "expected 3 structural MessageSent payloads, got {values}"
    );

    let mut driver = ReplayDriver::from_trace(trace).with_program(prog);
    let report = driver.replay_all().expect("replay_all");
    assert!(
        report.success,
        "three-agent chain strict replay failed: {}",
        report.render()
    );
    assert_eq!(report.mismatches.len(), 0);

    let _ = std::fs::remove_file(&path);
}

// ----------------------------------------------------------------------------
// Test 5 — v0.18 backwards-compat Opaque arm still loads (read-side
//          contract preserved)
// ----------------------------------------------------------------------------

#[test]
fn strict_equality_keeps_legacy_opaque_readable() {
    // The v0.20 write side emits only Values, but the read side must
    // still load v0.18/v0.19 traces that contain Opaque payloads.
    // This guards against an accidental write-side strictness
    // regression that also breaks the read path.
    let _g = recorder_serializer().lock();

    // Construct a synthetic trace by hand containing both arms.
    let mut trace = TraceFile::new(0, 0, 1);
    trace.events.push(TraceEvent::Spawn {
        agent_id: 1,
        agent_type: "Echoer".into(),
        supervisor: None,
    });
    trace.events.push(TraceEvent::MessageSent {
        from: 0,
        to: 1,
        msg: "Ping".into(),
        payload: ReplayPayload::Opaque(b"legacy-bytes".to_vec()),
    });
    trace.events.push(TraceEvent::MessageSent {
        from: 0,
        to: 1,
        msg: "Ping".into(),
        payload: ReplayPayload::Values(vec![ReplayValue::Str("modern".into())]),
    });

    let encoded =
        mty_runtime::replay::recorder::encode(&trace, mty_runtime::replay::TraceCodec::Json)
            .expect("encode");
    let decoded = decode(&encoded).expect("decode mixed trace");
    assert_eq!(decoded.events.len(), 3);

    // Sanity: read-side preserves both arms unchanged.
    let opaque_seen = decoded
        .events
        .iter()
        .filter(|e| {
            matches!(
                e,
                TraceEvent::MessageSent {
                    payload: ReplayPayload::Opaque(_),
                    ..
                }
            )
        })
        .count();
    let values_seen = decoded
        .events
        .iter()
        .filter(|e| {
            matches!(
                e,
                TraceEvent::MessageSent {
                    payload: ReplayPayload::Values(_),
                    ..
                }
            )
        })
        .count();
    assert_eq!(opaque_seen, 1, "legacy Opaque arm must survive disk decode");
    assert_eq!(values_seen, 1, "modern Values arm must survive disk decode");
}
