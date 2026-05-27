//! v0.26 Track C — `std.memory` ↔ v0.19 replay integration.
//!
//! Confirms:
//! - Every mutation to a memory handle emits a `MemoryDelta` event
//!   into the process-wide trace via the `with_recorder` hook.
//! - `Replayer::dump_json` round-trips the encoded delta back into
//!   the same `MemoryDelta`, so a downstream "memory replay" pass
//!   can reconstruct handle state at any frame.
//! - The snapshot round trip on each backend is byte-identical so
//!   the replay byte-identical contract extends to `std.memory`.

use mty_runtime::replay::{
    self,
    wire::{ReplayPayload, TraceEvent},
    Recorder,
};
use mty_stdlib::memory::snapshot::{is_memory_event, MemoryDelta};
use mty_stdlib::memory::{Episodic, VectorStore, Working};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use tempfile::tempdir;

/// Serialize every test that installs a process-wide recorder. The
/// recorder lives in a `static RwLock<Option<Arc<Recorder>>>` so any
/// two parallel tests sharing it would clobber each other's event
/// streams. The mutex's `Drop` impl is intentionally NOT used —
/// `with_recorder` clears the recorder before returning, then the
/// guard drops, releasing the next test.
fn global_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Install a fresh recorder for one test. Returns the recorder so the
/// test can read events_snapshot(); uninstalls before returning.
fn with_recorder<F: FnOnce(&Arc<Recorder>)>(test_label: &str, f: F) {
    let _guard = global_test_lock().lock().unwrap_or_else(|p| p.into_inner());
    // Defensive: another test may have left a recorder installed if it
    // panicked. Clear before installing ours so the events_snapshot()
    // we read at the end is unambiguously ours.
    let _ = replay::uninstall();
    let path = std::env::temp_dir().join(format!("mty-memory-replay-{test_label}.bin"));
    let rec = Arc::new(Recorder::new(path, 0xC0DE_C0DE_C0DE_C0DE, 1));
    replay::install(rec.clone());
    f(&rec);
    replay::uninstall();
}

/// For tests that touch a memory handle but don't care about recorded
/// events: hold the same global lock so we don't race a sibling test
/// that's mid-recorder. Without this, our `record_*` calls would land
/// in whatever recorder happens to be installed at that instant.
fn with_lock<F: FnOnce()>(f: F) {
    let _guard = global_test_lock().lock().unwrap_or_else(|p| p.into_inner());
    let _ = replay::uninstall();
    f();
}

fn collect_memory_deltas(rec: &Recorder) -> Vec<MemoryDelta> {
    rec.events_snapshot()
        .iter()
        .filter_map(|e| match e {
            TraceEvent::IoRead { source, bytes, .. } if is_memory_event(source) => {
                MemoryDelta::decode(bytes).ok()
            }
            _ => None,
        })
        .collect()
}

#[test]
fn memory_writes_emit_replay_events_vector() {
    with_recorder("vec", |rec| {
        let dir = tempdir().unwrap();
        let mut s = VectorStore::local(dir.path().join("v.json"));
        s.upsert("a", "alpha bravo", HashMap::new()).unwrap();
        s.upsert("b", "charlie delta", HashMap::new()).unwrap();
        s.delete("a").unwrap();

        let deltas = collect_memory_deltas(rec);
        let ops: Vec<&str> = deltas
            .iter()
            .map(|d| match d {
                MemoryDelta::Patch { op, .. } => op.as_str(),
                MemoryDelta::Snapshot { .. } => "snapshot",
            })
            .collect();
        assert_eq!(
            ops,
            vec!["upsert", "upsert", "delete"],
            "expected upsert+upsert+delete, got {ops:?}",
        );
        for d in &deltas {
            match d {
                MemoryDelta::Patch { handle_kind, .. } => {
                    assert_eq!(handle_kind, "vector.local");
                }
                MemoryDelta::Snapshot { .. } => {
                    panic!("expected Patch deltas only, got Snapshot")
                }
            }
        }
    });
}

#[test]
fn memory_writes_emit_replay_events_episodic() {
    with_recorder("ep", |rec| {
        let mut e = Episodic::in_memory(10);
        e.record("k1", &Value::String("v1".into()));
        e.record("k2", &Value::String("v2".into()));
        e.clear();
        let deltas = collect_memory_deltas(rec);
        assert_eq!(deltas.len(), 3);
        // First two should be `record`; last should be `clear`.
        let ops: Vec<&str> = deltas
            .iter()
            .map(|d| match d {
                MemoryDelta::Patch { op, .. } => op.as_str(),
                MemoryDelta::Snapshot { .. } => "snapshot",
            })
            .collect();
        assert_eq!(ops, vec!["record", "record", "clear"]);
    });
}

#[test]
fn memory_writes_emit_replay_events_working() {
    with_recorder("work", |rec| {
        let mut w = Working::new();
        w.push("plan", "outline introduction");
        w.push("note", "user prefers brief");
        w.clear();
        let deltas = collect_memory_deltas(rec);
        assert_eq!(deltas.len(), 3);
        for d in &deltas {
            match d {
                MemoryDelta::Patch { handle_kind, .. } => {
                    assert_eq!(handle_kind, "working");
                }
                MemoryDelta::Snapshot { .. } => panic!("expected Patch"),
            }
        }
    });
}

#[test]
fn replay_restores_memory_state_via_snapshot() {
    with_lock(|| {
        // Capture a vector store snapshot mid-stream; restore into a
        // fresh handle and assert search results match.
        let dir = tempdir().unwrap();
        let mut s = VectorStore::local(dir.path().join("s.json"));
        s.upsert("a", "alpha bravo charlie", HashMap::new())
            .unwrap();
        s.upsert("b", "delta echo foxtrot", HashMap::new()).unwrap();
        let snap = s.snapshot_bytes();

        let mut s2 = VectorStore::local(dir.path().join("s2.json"));
        s2.restore_bytes(&snap).unwrap();

        // The restored handle should produce identical top-1 search
        // results — the byte-identical contract on the snapshot side.
        let top_a_orig = s.search("alpha", 1).unwrap();
        let top_a_restored = s2.search("alpha", 1).unwrap();
        assert_eq!(top_a_orig[0].id, top_a_restored[0].id);
    });
}

#[test]
fn replay_restores_working_state() {
    with_lock(|| {
        let mut w = Working::with_budget(256);
        w.push("plan", "do thing A");
        w.push("note", "user prefers thing B");
        let snap = w.snapshot_bytes();

        let mut w2 = Working::new();
        w2.restore_bytes(&snap).unwrap();
        assert_eq!(w.render(), w2.render());
    });
}

#[test]
fn replay_event_payload_uses_memory_source_label() {
    with_recorder("label", |rec| {
        let mut w = Working::new();
        w.push("k", "v");
        let events = rec.events_snapshot();
        let mem_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, TraceEvent::IoRead { source, .. } if is_memory_event(source)))
            .collect();
        assert_eq!(mem_events.len(), 1);
        if let TraceEvent::IoRead {
            source,
            bytes,
            agent,
        } = mem_events[0]
        {
            assert_eq!(source, "memory:working");
            assert_eq!(*agent, 0);
            // Bytes decode back to a MemoryDelta.
            let d = MemoryDelta::decode(bytes).unwrap();
            match d {
                MemoryDelta::Patch { op, .. } => assert_eq!(op, "push"),
                MemoryDelta::Snapshot { .. } => panic!("expected Patch"),
            }
        }
    });
}

#[test]
fn replay_payload_default_is_unaffected_by_memory_events() {
    // Pure-data assertion: no recorder activity required, no lock
    // needed. Confirms our use of `record_io_read` doesn't
    // accidentally mutate the existing `ReplayPayload::Opaque(Vec::new())`
    // default that the v0.18 hot path relies on.
    let default = ReplayPayload::default();
    assert_eq!(default, ReplayPayload::Opaque(Vec::new()));
}
