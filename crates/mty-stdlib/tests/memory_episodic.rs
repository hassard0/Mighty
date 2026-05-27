//! v0.26 Track C — `std.memory.Episodic` integration tests.
//!
//! Covers:
//! - in-memory ring buffer eviction
//! - recent() ordering
//! - search_by_key_prefix
//! - snapshot/restore round-trip
//! - sqlite-backed persistence (gated behind `memory-sqlite`, on by default)

use mty_stdlib::memory::episodic::Episodic;
use serde_json::Value;

#[test]
fn episodic_in_memory_ring_buffer() {
    let mut e = Episodic::in_memory(3);
    for i in 0..5 {
        e.record(&format!("k{i}"), &Value::Number(i.into()));
    }
    // Hard cap of 3 means the first two should have been evicted.
    assert_eq!(e.len(), 3);
    let keys: Vec<String> = e.search_by_key("k").iter().map(|e| e.key.clone()).collect();
    assert_eq!(keys, vec!["k2", "k3", "k4"]);
}

#[test]
fn episodic_recent_returns_in_order() {
    let mut e = Episodic::in_memory(5);
    e.record("first", &Value::String("v1".into()));
    e.record("second", &Value::String("v2".into()));
    e.record("third", &Value::String("v3".into()));
    let recent = e.recent(2);
    assert_eq!(recent.len(), 2);
    // recent() is newest-first.
    assert_eq!(recent[0].key, "third");
    assert_eq!(recent[1].key, "second");
}

#[test]
fn episodic_search_by_key_prefix() {
    let mut e = Episodic::in_memory(10);
    e.record("user:1", &Value::Null);
    e.record("user:2", &Value::Null);
    e.record("session:a", &Value::Null);
    let user_entries = e.search_by_key("user:");
    assert_eq!(user_entries.len(), 2);
    let session_entries = e.search_by_key("session:");
    assert_eq!(session_entries.len(), 1);
    let empty = e.search_by_key("nope:");
    assert!(empty.is_empty());
}

#[test]
fn episodic_clear_empties() {
    let mut e = Episodic::in_memory(5);
    e.record("k", &Value::Null);
    e.record("k2", &Value::Null);
    e.clear();
    assert!(e.is_empty());
}

#[test]
fn episodic_snapshot_restore_roundtrip() {
    let mut e = Episodic::in_memory(4);
    e.record("a", &Value::String("v1".into()));
    e.record("b", &Value::String("v2".into()));
    let snap = e.snapshot_bytes();

    let mut e2 = Episodic::in_memory(4);
    e2.restore_bytes(&snap).unwrap();
    assert_eq!(e2.len(), 2);
    let recent = e2.recent(10);
    assert_eq!(recent[0].key, "b");
    assert_eq!(recent[1].key, "a");
}

#[test]
fn episodic_snapshot_is_deterministic() {
    let mut e = Episodic::in_memory(5);
    // Force identical timestamps in both snapshots by encoding the
    // entries via the snapshot, then re-decoding into a fresh handle
    // (the timestamps in the snapshot are then frozen).
    e.record("x", &Value::Null);
    let s1 = e.snapshot_bytes();
    let s2 = e.snapshot_bytes();
    assert_eq!(s1, s2);
}

#[test]
fn episodic_record_value_round_trips() {
    let mut e = Episodic::in_memory(2);
    let v = serde_json::json!({"q": "what is the answer?", "a": 42});
    e.record("ask", &v);
    let recent = e.recent(1);
    assert_eq!(recent[0].value, v);
}

#[cfg(feature = "memory-sqlite")]
#[test]
fn episodic_sqlite_persists() {
    use tempfile::tempdir;
    let dir = tempdir().unwrap();
    let path = dir.path().join("ep.sqlite");
    {
        let mut e = Episodic::sqlite(&path, 10).unwrap();
        e.record("k1", &Value::String("v1".into()));
        e.record("k2", &Value::String("v2".into()));
        assert_eq!(e.len(), 2);
    }
    // Re-open: rows must survive.
    let e2 = Episodic::sqlite(&path, 10).unwrap();
    assert_eq!(e2.len(), 2);
    let hits = e2.search_by_key("k");
    assert_eq!(hits.len(), 2);
}

#[cfg(feature = "memory-sqlite")]
#[test]
fn episodic_sqlite_respects_max() {
    use tempfile::tempdir;
    let dir = tempdir().unwrap();
    let mut e = Episodic::sqlite(dir.path().join("ep.sqlite"), 3).unwrap();
    for i in 0..5 {
        e.record(&format!("k{i}"), &Value::Number(i.into()));
    }
    // The sqlite backend enforces the cap on insert.
    assert_eq!(e.len(), 3);
}
