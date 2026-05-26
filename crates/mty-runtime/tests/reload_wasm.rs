//! Hot-reload v0.21 — real-wasm-byte tests.
//!
//! The v0.20 baseline (`tests/reload.rs`) only exercised the
//! `SameProgram` swap path. v0.21 wires the `WasmBytes` variant: the
//! loader pulls the embedded `__mty_agent_type` + `__mty_schema_hash`
//! custom sections, the swap pipeline cross-checks them against the
//! plan, then the per-agent program slot is replaced.
//!
//! These tests synthesize their own minimal wasm modules via
//! `wasm-encoder` so the loader path is exercised end-to-end without
//! depending on the codegen crate (off-limits to this slice).

use mty_runtime::reload::{
    DrainSignal, ModuleSource, Program, ReloadError, ReloadGate, ReloadOptions, ReloadRunner,
    Resumable, SwapPlan, WasmLoadError, SECTION_AGENT_TYPE, SECTION_SCHEMA_HASH,
};
use mty_runtime::AgentId;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

// ---------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
struct Counter {
    n: u64,
}

impl Resumable for Counter {
    const SCHEMA_HASH: u64 = 0xCAFE_F00D_DEAD_BEEF;
}

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

/// Build a minimal mty-shaped wasm module with the right custom
/// sections. The runtime doesn't execute the module — it only
/// inspects the metadata.
fn synth_module(agent_type: &str, schema_hash: u64) -> Vec<u8> {
    let mut module = wasm_encoder::Module::new();
    module.section(&wasm_encoder::CustomSection {
        name: std::borrow::Cow::Borrowed(SECTION_AGENT_TYPE),
        data: std::borrow::Cow::Borrowed(agent_type.as_bytes()),
    });
    let hash_bytes = schema_hash.to_le_bytes();
    module.section(&wasm_encoder::CustomSection {
        name: std::borrow::Cow::Borrowed(SECTION_SCHEMA_HASH),
        data: std::borrow::Cow::Borrowed(&hash_bytes),
    });
    module.finish()
}

// ---------------------------------------------------------------------
// 1. reload with real wasm bytes succeeds + swaps the program slot
// ---------------------------------------------------------------------

#[test]
fn reload_with_real_wasm_bytes_swaps_agent() {
    let desc = fixture_descriptor("Echo", 100);
    let state = Arc::new(Mutex::new(Counter { n: 9 }));
    let gate = Arc::new(ReloadGate::new());
    let program = Arc::new(Mutex::new(Program::new()));

    let new_wasm = synth_module("Echo", Counter::SCHEMA_HASH);
    let plan = SwapPlan {
        agent_id: desc.id,
        agent_type: desc.name.clone(),
        old_schema_hash: Counter::SCHEMA_HASH,
        new_schema_hash: Counter::SCHEMA_HASH,
        module: ModuleSource::WasmBytes(&new_wasm),
        options: ReloadOptions::default(),
    };
    let runner = ReloadRunner {
        plan,
        desc: desc.clone(),
        state: state.clone(),
        gate: gate.clone(),
        drain_signal: None,
        schema_registry: None,
        program: Some(program.clone()),
    };

    let report = runner.run().expect("reload ok");
    assert_eq!(report.agent_id, desc.id.0);
    assert_eq!(report.agent_type, "Echo");

    // Program slot was installed.
    let prog = program.lock();
    assert_eq!(prog.agent_count(), 1);
    let slot = prog.get("Echo").expect("slot present");
    assert_eq!(slot.schema_hash, Counter::SCHEMA_HASH);
    assert_eq!(slot.wasm, new_wasm);
    // State survived the round-trip.
    assert_eq!(state.lock().n, 9);
    // Gate clear.
    assert!(!gate.is_paused());
}

// ---------------------------------------------------------------------
// 2. wasm without mty custom sections is rejected
// ---------------------------------------------------------------------

#[test]
fn reload_rejects_wasm_without_mty_custom_section() {
    let desc = fixture_descriptor("Echo", 101);
    let state = Arc::new(Mutex::new(Counter { n: 0 }));
    let gate = Arc::new(ReloadGate::new());

    // Bare wasm magic with no custom sections.
    let bare = wasm_encoder::Module::new().finish();
    let plan = SwapPlan {
        agent_id: desc.id,
        agent_type: desc.name.clone(),
        old_schema_hash: Counter::SCHEMA_HASH,
        new_schema_hash: Counter::SCHEMA_HASH,
        module: ModuleSource::WasmBytes(&bare),
        options: ReloadOptions::default(),
    };
    let runner = ReloadRunner {
        plan,
        desc,
        state: state.clone(),
        gate: gate.clone(),
        drain_signal: None,
        schema_registry: None,
        program: None,
    };

    let err = runner.run().unwrap_err();
    match &err {
        ReloadError::WasmLoad(WasmLoadError::MissingSection(_)) => {}
        other => panic!("expected WasmLoad(MissingSection), got {other:?}"),
    }
    assert_eq!(err.diag_code(), "MT5064");
    // State untouched, gate clear (loader failed before pausing).
    assert_eq!(state.lock().n, 0);
    assert!(!gate.is_paused());
}

// ---------------------------------------------------------------------
// 3. schema-hash mismatch between module + plan → MT5060 / MT5069
// ---------------------------------------------------------------------

#[test]
fn reload_rejects_schema_hash_mismatch() {
    let desc = fixture_descriptor("Echo", 102);
    let state = Arc::new(Mutex::new(Counter { n: 1 }));
    let gate = Arc::new(ReloadGate::new());

    // Module advertises schema hash X but the plan says Y.
    let wasm_hash = Counter::SCHEMA_HASH;
    let wrong_plan_hash = wasm_hash ^ 0xDEAD_BEEF;
    let new_wasm = synth_module("Echo", wasm_hash);
    let plan = SwapPlan {
        agent_id: desc.id,
        agent_type: desc.name.clone(),
        old_schema_hash: Counter::SCHEMA_HASH,
        new_schema_hash: wrong_plan_hash,
        module: ModuleSource::WasmBytes(&new_wasm),
        options: ReloadOptions::default(),
    };
    let runner = ReloadRunner {
        plan,
        desc,
        state,
        gate,
        drain_signal: None,
        schema_registry: None,
        program: None,
    };

    let err = runner.run().unwrap_err();
    // The pipeline reports an Internal error here because the
    // packaging is inconsistent (module + plan disagree about the
    // new shape). The CLI surfaces this as MT5069.
    match &err {
        ReloadError::Internal(m) => assert!(m.contains("schema hash")),
        other => panic!("expected Internal, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// 4. agent-type mismatch between module + plan → MT5065
// ---------------------------------------------------------------------

#[test]
fn reload_rejects_agent_type_mismatch() {
    let desc = fixture_descriptor("Echo", 103);
    let state = Arc::new(Mutex::new(Counter { n: 0 }));
    let gate = Arc::new(ReloadGate::new());

    // Wasm says "Other" but we asked for "Echo".
    let new_wasm = synth_module("Other", Counter::SCHEMA_HASH);
    let plan = SwapPlan {
        agent_id: desc.id,
        agent_type: desc.name.clone(),
        old_schema_hash: Counter::SCHEMA_HASH,
        new_schema_hash: Counter::SCHEMA_HASH,
        module: ModuleSource::WasmBytes(&new_wasm),
        options: ReloadOptions::default(),
    };
    let runner = ReloadRunner {
        plan,
        desc,
        state,
        gate,
        drain_signal: None,
        schema_registry: None,
        program: None,
    };

    let err = runner.run().unwrap_err();
    match &err {
        ReloadError::AgentTypeMismatch {
            requested,
            embedded,
        } => {
            assert_eq!(requested, "Echo");
            assert_eq!(embedded, "Other");
        }
        other => panic!("expected AgentTypeMismatch, got {other:?}"),
    }
    assert_eq!(err.diag_code(), "MT5065");
}

// ---------------------------------------------------------------------
// 5. wasm bytes + condvar drain wakes immediately on mark_idle
// ---------------------------------------------------------------------

#[test]
fn reload_with_wasm_bytes_uses_condvar_drain() {
    let desc = fixture_descriptor("Echo", 104);
    let state = Arc::new(Mutex::new(Counter { n: 2 }));
    let gate = Arc::new(ReloadGate::new());
    let drain = DrainSignal::new_busy();
    let drain2 = drain.clone();
    let handle = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(30));
        drain2.mark_idle();
    });

    let new_wasm = synth_module("Echo", Counter::SCHEMA_HASH);
    let plan = SwapPlan {
        agent_id: desc.id,
        agent_type: desc.name.clone(),
        old_schema_hash: Counter::SCHEMA_HASH,
        new_schema_hash: Counter::SCHEMA_HASH,
        module: ModuleSource::WasmBytes(&new_wasm),
        options: ReloadOptions {
            deadline: std::time::Duration::from_secs(1),
            ..ReloadOptions::default()
        },
    };
    let runner = ReloadRunner {
        plan,
        desc,
        state,
        gate,
        drain_signal: Some(drain),
        schema_registry: None,
        program: None,
    };

    let started = std::time::Instant::now();
    let report = runner.run().expect("reload ok");
    handle.join().unwrap();
    let wall = started.elapsed();
    // Drain woke up promptly (not stuck on a 1 ms busy-poll).
    assert!(wall < std::time::Duration::from_millis(500));
    // Drain elapsed should reflect the actual wait, not the deadline.
    assert!(report.drain_elapsed_ms >= 20);
    assert!(report.drain_elapsed_ms < 200);
}

// ---------------------------------------------------------------------
// 6. Program::with_swapped_agent end-to-end through the runner
// ---------------------------------------------------------------------

#[test]
fn reload_pipeline_swaps_program_slot_visible_externally() {
    let desc = fixture_descriptor("Echo", 105);
    let state = Arc::new(Mutex::new(Counter { n: 5 }));
    let gate = Arc::new(ReloadGate::new());
    let program = Arc::new(Mutex::new(Program::new()));

    // Two consecutive reloads with the same schema hash — the second
    // swap should overwrite (not append) the slot. We deliberately
    // reuse `Counter::SCHEMA_HASH` so the test exercises the
    // "replace existing slot" path rather than the "append new" one.
    for _ in 0..2 {
        let hash = Counter::SCHEMA_HASH;
        let new_wasm = synth_module("Echo", hash);
        let plan = SwapPlan {
            agent_id: desc.id,
            agent_type: desc.name.clone(),
            old_schema_hash: hash,
            new_schema_hash: hash,
            module: ModuleSource::WasmBytes(&new_wasm),
            options: ReloadOptions::default(),
        };
        let runner = ReloadRunner {
            plan,
            desc: desc.clone(),
            state: state.clone(),
            gate: gate.clone(),
            drain_signal: None,
            schema_registry: None,
            program: Some(program.clone()),
        };
        runner.run().expect("reload ok");
    }

    let prog = program.lock();
    assert_eq!(prog.agent_count(), 1);
    assert_eq!(prog.get("Echo").unwrap().schema_hash, Counter::SCHEMA_HASH);
}
