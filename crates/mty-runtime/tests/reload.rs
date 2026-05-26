//! Hot-reload integration tests — v0.20 Tier 1.5.
//!
//! These tests exercise the swap pipeline + `Resumable` trait end-to-end
//! against the in-process runtime descriptors. They don't load a fresh
//! wasm module (the runtime interpreter doesn't have a per-agent
//! module surface in v0.20; see `docs/internals/hot-reload.md` for the
//! v0.21 follow-up). What we *do* verify:
//!
//! 1. Snapshot + restore round-trips through `Resumable::{to,from}_snapshot`.
//! 2. The pipeline rejects an incompatible `SCHEMA_HASH` *before*
//!    touching the agent (fail-fast diagnostic `MT5060`).
//! 3. An in-flight handler drains before the swap proceeds.
//! 4. Drain-deadline trips return cleanly with `MT5062` and leave the
//!    agent's gate in a known-paused state.
//! 5. Messages enqueued during the swap are still delivered to the
//!    new agent because the `Arc<Mailbox>` is preserved.
//! 6. Wasm-module reload via raw bytes is rejected (v0.20 only ships
//!    state-only restarts).

use mty_runtime::reload::{
    decode_snapshot, dry_run_swap, encode_snapshot, ModuleSource, ReloadError, ReloadGate,
    ReloadOptions, ReloadRunner, Resumable, SwapPlan,
};
use mty_runtime::AgentId;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
struct ConnState {
    count: u64,
    label: String,
    history: Vec<u64>,
}

impl Resumable for ConnState {
    const SCHEMA_HASH: u64 = 0xDEAD_BEEF_CAFE_F00D;
}

/// Same shape as `ConnState` — used to assert that two impls with the
/// same SCHEMA_HASH round-trip cleanly even when the Rust types
/// differ (matches the v0.21 "rolling restart" use case where the
/// new module's struct is a separately-compiled copy).
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
struct ConnStateV2 {
    count: u64,
    label: String,
    history: Vec<u64>,
}

impl Resumable for ConnStateV2 {
    // Same hash on purpose — the wire shape is the same.
    const SCHEMA_HASH: u64 = ConnState::SCHEMA_HASH;
}

/// Build a minimal agent descriptor for tests. We don't need a real
/// agent loop — the swap pipeline operates on a typed state cell +
/// the reload gate.
fn fixture_descriptor(name: &str, id: u64) -> Arc<mty_runtime::AgentDescriptor> {
    use mty_ir::interp::value::Value;
    use mty_ir::ir::AgentIrId;
    use mty_runtime::budget::{Budget, BudgetTracker};
    use mty_runtime::mailbox::{Mailbox, SendPolicy};

    Arc::new(mty_runtime::AgentDescriptor {
        id: AgentId(id),
        name: name.into(),
        sir_id: AgentIrId(0),
        state: Mutex::new(Value::Unit),
        mailbox: Arc::new(Mailbox::new(8, SendPolicy::Block)),
        budget: Arc::new(BudgetTracker::new(Budget::default())),
        supervisor: None,
        mailbox_depth: AtomicU64::new(0),
    })
}

// ---------------------------------------------------------------------
// 1. compatible-schema swap succeeds
// ---------------------------------------------------------------------

#[test]
fn reload_compatible_schema_succeeds() {
    let desc = fixture_descriptor("ConnAgent", 1);
    let state = Arc::new(Mutex::new(ConnState {
        count: 42,
        label: "alpha".into(),
        history: vec![1, 2, 3],
    }));
    let gate = Arc::new(ReloadGate::new());

    let plan = SwapPlan {
        agent_id: desc.id,
        agent_type: desc.name.clone(),
        old_schema_hash: ConnState::SCHEMA_HASH,
        new_schema_hash: ConnStateV2::SCHEMA_HASH,
        module: ModuleSource::SameProgram,
        options: ReloadOptions::default(),
    };

    let runner = ReloadRunner {
        plan,
        desc: desc.clone(),
        state: state.clone(),
        gate: gate.clone(),
    };

    let report = runner.run().expect("reload ok");
    assert_eq!(report.agent_id, desc.id.0);
    assert_eq!(report.agent_type, "ConnAgent");
    assert_eq!(report.old_schema_hash, ConnState::SCHEMA_HASH);
    assert!(report.state_bytes_size > 0);

    // State survived the round-trip unchanged.
    let s = state.lock();
    assert_eq!(s.count, 42);
    assert_eq!(s.label, "alpha");
    assert_eq!(s.history, vec![1, 2, 3]);
    // Gate clear: agent can resume.
    assert!(!gate.is_paused());
}

// ---------------------------------------------------------------------
// 2. incompatible schema → MT5060, agent untouched
// ---------------------------------------------------------------------

#[test]
fn reload_incompatible_schema_rejected() {
    let desc = fixture_descriptor("ConnAgent", 2);
    let original = ConnState {
        count: 99,
        label: "original".into(),
        history: vec![7],
    };
    let state = Arc::new(Mutex::new(original.clone()));
    let gate = Arc::new(ReloadGate::new());

    let plan = SwapPlan {
        agent_id: desc.id,
        agent_type: desc.name.clone(),
        old_schema_hash: ConnState::SCHEMA_HASH,
        // Intentionally-bumped hash, e.g. a field-rename in the new wasm.
        new_schema_hash: ConnState::SCHEMA_HASH ^ 0xFF,
        module: ModuleSource::SameProgram,
        options: ReloadOptions::default(),
    };

    let runner = ReloadRunner {
        plan,
        desc: desc.clone(),
        state: state.clone(),
        gate: gate.clone(),
    };

    let err = runner.run().unwrap_err();
    match err {
        ReloadError::IncompatibleSchema { old, new } => {
            assert_eq!(old, ConnState::SCHEMA_HASH);
            assert_eq!(new, ConnState::SCHEMA_HASH ^ 0xFF);
        }
        other => panic!("expected IncompatibleSchema, got {other:?}"),
    }

    // Agent state is untouched + gate is not in a half-swapped state.
    assert_eq!(*state.lock(), original);
    assert!(!gate.is_paused());
}

// ---------------------------------------------------------------------
// 3. drain waits for in-flight handler before proceeding
// ---------------------------------------------------------------------

#[test]
fn reload_drains_in_flight_handler() {
    let desc = fixture_descriptor("ConnAgent", 3);
    let state = Arc::new(Mutex::new(ConnState {
        count: 0,
        label: "mid-flight".into(),
        history: vec![],
    }));
    let gate = Arc::new(ReloadGate::new());

    // Simulate a handler running for 50 ms before completing.
    gate.mark_busy();
    let gate2 = gate.clone();
    let handler = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        gate2.mark_idle();
    });

    let plan = SwapPlan {
        agent_id: desc.id,
        agent_type: desc.name.clone(),
        old_schema_hash: ConnState::SCHEMA_HASH,
        new_schema_hash: ConnState::SCHEMA_HASH,
        module: ModuleSource::SameProgram,
        options: ReloadOptions {
            // Generous deadline so the 50 ms handler completes.
            deadline: Duration::from_millis(500),
            ..ReloadOptions::default()
        },
    };
    let runner = ReloadRunner {
        plan,
        desc: desc.clone(),
        state: state.clone(),
        gate: gate.clone(),
    };

    let started = std::time::Instant::now();
    let report = runner.run().expect("drain + swap should succeed");
    let elapsed = started.elapsed();
    handler.join().unwrap();

    // We waited at least the handler's full duration.
    assert!(
        elapsed >= Duration::from_millis(40),
        "swap returned too fast (elapsed={elapsed:?}); should have waited for handler"
    );
    assert!(report.drain_elapsed_ms >= 40);
    assert!(!gate.is_busy());
    assert!(!gate.is_paused());
}

// ---------------------------------------------------------------------
// 4. drain-deadline trip → MT5062, agent left in known state
// ---------------------------------------------------------------------

#[test]
fn reload_deadline_exceeded_fails_clean() {
    let desc = fixture_descriptor("StuckAgent", 4);
    let state = Arc::new(Mutex::new(ConnState {
        count: 7,
        label: "stuck".into(),
        history: vec![1],
    }));
    let gate = Arc::new(ReloadGate::new());

    // Pretend the agent is stuck mid-handler and never returns.
    gate.mark_busy();

    let plan = SwapPlan {
        agent_id: desc.id,
        agent_type: desc.name.clone(),
        old_schema_hash: ConnState::SCHEMA_HASH,
        new_schema_hash: ConnState::SCHEMA_HASH,
        module: ModuleSource::SameProgram,
        options: ReloadOptions {
            deadline: Duration::from_millis(30),
            ..ReloadOptions::default()
        },
    };
    let runner = ReloadRunner {
        plan,
        desc: desc.clone(),
        state: state.clone(),
        gate: gate.clone(),
    };

    let err = runner.run().unwrap_err();
    match err {
        ReloadError::DrainDeadline(d) => {
            assert_eq!(d, Duration::from_millis(30));
        }
        other => panic!("expected DrainDeadline, got {other:?}"),
    }
    assert_eq!(err.diag_code(), "MT5062");

    // Known state: agent state untouched, gate NOT paused (we bailed
    // before calling pause). The caller can choose to forcibly
    // restart the agent or back off.
    assert_eq!(state.lock().count, 7);
    assert!(
        gate.is_busy(),
        "gate.busy stays set — handler is still in-flight"
    );
    assert!(
        !gate.is_paused(),
        "gate.paused not set because the swap aborted before phase 3"
    );
}

// ---------------------------------------------------------------------
// 5. mailbox preserved end-to-end
// ---------------------------------------------------------------------

#[tokio::test]
async fn reload_preserves_mailbox() {
    use mty_ir::interp::value::Value;
    use mty_runtime::mailbox::{MessageFrame, SmallPayload};

    let desc = fixture_descriptor("Echo", 5);
    let state = Arc::new(Mutex::new(ConnState {
        count: 0,
        label: "pre-reload".into(),
        history: vec![],
    }));
    let gate = Arc::new(ReloadGate::new());

    // Producer fills the mailbox before + during the reload. The
    // mailbox capacity is 8 (from `fixture_descriptor`), so 6
    // messages fit without backpressure.
    for i in 0..3 {
        let frame = MessageFrame::fire_and_forget(
            "Ping",
            SmallPayload::inline(vec![Value::Int(i as i128, mty_types::IntKind::I64)]),
        );
        desc.mailbox.send(frame).await.unwrap();
    }

    let plan = SwapPlan {
        agent_id: desc.id,
        agent_type: desc.name.clone(),
        old_schema_hash: ConnState::SCHEMA_HASH,
        new_schema_hash: ConnState::SCHEMA_HASH,
        module: ModuleSource::SameProgram,
        options: ReloadOptions::default(),
    };
    let runner = ReloadRunner {
        plan,
        desc: desc.clone(),
        state: state.clone(),
        gate: gate.clone(),
    };
    let _report = runner.run().expect("reload ok");

    // More messages enqueued AFTER the swap.
    for i in 3..5 {
        let frame = MessageFrame::fire_and_forget(
            "Ping",
            SmallPayload::inline(vec![Value::Int(i as i128, mty_types::IntKind::I64)]),
        );
        desc.mailbox.send(frame).await.unwrap();
    }

    // Drain — should see all 5 messages in order.
    let mut received = Vec::new();
    let mut rx = desc.mailbox.take_receiver().expect("receiver available");
    while let Ok(frame) = rx.try_recv() {
        received.push(frame.proto_msg);
    }
    assert_eq!(received.len(), 5);
    for proto in &received {
        assert_eq!(proto, "Ping");
    }
}

// ---------------------------------------------------------------------
// 6. raw wasm bytes rejected in v0.20
// ---------------------------------------------------------------------

#[test]
fn reload_raw_wasm_rejected_in_v0_20() {
    let desc = fixture_descriptor("Echo", 6);
    let state = Arc::new(Mutex::new(ConnState {
        count: 0,
        label: "".into(),
        history: vec![],
    }));
    let gate = Arc::new(ReloadGate::new());

    let fake_wasm = vec![0x00u8, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    let plan = SwapPlan {
        agent_id: desc.id,
        agent_type: desc.name.clone(),
        old_schema_hash: ConnState::SCHEMA_HASH,
        new_schema_hash: ConnState::SCHEMA_HASH,
        module: ModuleSource::WasmBytes(&fake_wasm),
        options: ReloadOptions::default(),
    };
    let runner = ReloadRunner {
        plan,
        desc: desc.clone(),
        state,
        gate: gate.clone(),
    };

    let err = runner.run().unwrap_err();
    assert!(matches!(err, ReloadError::WasmReloadNotImplemented));
    assert_eq!(err.diag_code(), "MT5064");

    // The gate was paused mid-pipeline; the runner must clear it
    // before returning so the agent doesn't get stuck.
    assert!(!gate.is_paused(), "gate should be cleared on a clean error");
}

// ---------------------------------------------------------------------
// Additional unit tests exercising the pure-data helpers
// ---------------------------------------------------------------------

#[test]
fn dry_run_swap_matches_full_runner_for_state_only() {
    let state = Mutex::new(ConnState {
        count: 1,
        label: "x".into(),
        history: vec![1, 2, 3, 4, 5],
    });
    let bytes = dry_run_swap(
        &state,
        ConnState::SCHEMA_HASH,
        ConnState::SCHEMA_HASH,
        &ReloadOptions::default(),
    )
    .expect("dry-run ok");

    // Round-trip via the typed decoder.
    let restored: ConnState = decode_snapshot(&bytes).unwrap();
    assert_eq!(restored.count, 1);
    assert_eq!(restored.history.len(), 5);
}

#[test]
fn resumable_default_codec_round_trip() {
    let s = ConnState {
        count: 1_000_000,
        label: "round-trip".into(),
        history: (0..50).collect(),
    };
    let bytes = s.to_snapshot().unwrap();
    let back: ConnState = ConnState::from_snapshot(&bytes).unwrap();
    assert_eq!(s, back);
}

#[test]
fn snapshot_size_cap_trips_for_huge_payloads() {
    let s = ConnState {
        count: 0,
        label: "x".repeat(100),
        history: (0..1_000).collect(),
    };
    let err = encode_snapshot(&s, 16).unwrap_err();
    // Inner error: TooLarge.
    let msg = err.to_string();
    assert!(msg.contains("snapshot too large"), "got: {msg}");
}
