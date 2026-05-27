//! v0.26 Track C — `std.memory.VectorStore` integration tests.
//!
//! Default backend coverage:
//! - local (in-memory + JSON-on-disk)
//! - snapshot/restore round-trip
//! - delete + not-found
//! - persistence across restart
//!
//! The qdrant test is gated behind `#[ignore]` so the default
//! `cargo test` run is offline; run with `--ignored` against a live
//! qdrant on `:6333` to exercise the network path.

use mty_stdlib::memory::vector::{VectorErr, VectorStore};
use mty_stdlib::memory::MemoryHandle;
use serde_json::Value;
use std::collections::HashMap;
use tempfile::tempdir;

fn meta() -> HashMap<String, Value> {
    HashMap::new()
}

#[test]
fn vector_local_upsert_search_roundtrip() {
    let dir = tempdir().unwrap();
    let mut s = VectorStore::local(dir.path().join("v.json"));
    // 10 docs spread across two topic clusters.
    let cluster_a = [
        ("a1", "anthropic claude opus assistant"),
        ("a2", "anthropic claude haiku reply"),
        ("a3", "claude sonnet generates response"),
        ("a4", "anthropic safety alignment claude"),
        ("a5", "opus claude reasoning long context"),
    ];
    let cluster_b = [
        ("b1", "tokyo japan sushi ramen kyoto"),
        ("b2", "japan ramen noodles tokyo bowls"),
        ("b3", "kyoto temples japan gardens shrine"),
        ("b4", "osaka japan takoyaki tokyo street food"),
        ("b5", "japan sushi tokyo fresh fish market"),
    ];
    for (id, text) in cluster_a.iter().chain(cluster_b.iter()) {
        s.upsert(id, text, meta()).unwrap();
    }
    assert_eq!(s.len(), 10);

    let hits = s.search("anthropic claude", 5).unwrap();
    assert_eq!(hits.len(), 5);
    // Top hits should be from cluster A.
    let top_a_count = hits.iter().filter(|h| h.id.starts_with('a')).count();
    assert!(
        top_a_count >= 4,
        "expected >=4 cluster-A hits, got {top_a_count}"
    );
}

#[test]
fn vector_local_persists_across_restart() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("persist.json");
    {
        let mut s = VectorStore::local(&path);
        s.upsert("alpha", "alpha bravo charlie", meta()).unwrap();
        s.upsert("delta", "delta echo foxtrot", meta()).unwrap();
        s.flush().unwrap();
    }
    // Re-open from disk and confirm the records survived.
    let s2 = VectorStore::local(&path);
    assert_eq!(s2.len(), 2);
    let hits = s2.search("alpha", 1).unwrap();
    assert_eq!(hits[0].id, "alpha");
}

#[test]
fn vector_local_delete_then_not_found() {
    let dir = tempdir().unwrap();
    let mut s = VectorStore::local(dir.path().join("v.json"));
    s.upsert("x", "the only entry", meta()).unwrap();
    assert_eq!(s.len(), 1);
    s.delete("x").unwrap();
    assert_eq!(s.len(), 0);
    let err = s.delete("x").unwrap_err();
    assert!(matches!(err, VectorErr::NotFound(_)));
}

#[test]
fn vector_snapshot_restore_roundtrip() {
    let dir = tempdir().unwrap();
    let mut s = VectorStore::local(dir.path().join("orig.json"));
    s.upsert("p", "alpha bravo", meta()).unwrap();
    s.upsert("q", "charlie delta", meta()).unwrap();
    let snap = s.snapshot_bytes();
    assert!(!snap.is_empty());

    // Build a fresh store at a different path and restore.
    let mut s2 = VectorStore::local(dir.path().join("restored.json"));
    assert!(s2.is_empty());
    s2.restore_bytes(&snap).unwrap();
    assert_eq!(s2.len(), 2);

    // Same query → same top hit.
    let original_top = s.search("alpha", 1).unwrap();
    let restored_top = s2.search("alpha", 1).unwrap();
    assert_eq!(original_top[0].id, restored_top[0].id);
}

#[test]
fn vector_snapshot_is_deterministic() {
    let dir = tempdir().unwrap();
    let mut s = VectorStore::local(dir.path().join("v.json"));
    s.upsert("a", "first", meta()).unwrap();
    s.upsert("b", "second", meta()).unwrap();
    let snap1 = s.snapshot_bytes();
    let snap2 = s.snapshot_bytes();
    assert_eq!(snap1, snap2, "snapshot must be deterministic");
}

#[test]
fn vector_metadata_round_trips_through_search() {
    let dir = tempdir().unwrap();
    let mut s = VectorStore::local(dir.path().join("v.json"));
    let mut m = HashMap::new();
    m.insert("topic".to_string(), Value::String("ai".into()));
    m.insert("score".to_string(), Value::Number(42.into()));
    s.upsert("doc", "anthropic claude opus", m.clone()).unwrap();
    let hits = s.search("claude", 1).unwrap();
    assert_eq!(hits[0].metadata, m);
}

#[test]
fn vector_qdrant_construct_is_offline() {
    let s = VectorStore::qdrant("http://127.0.0.1:6333", "researcher");
    // Construction must not perform IO.
    assert_eq!(s.kind(), "vector.qdrant");
    assert!(s.is_empty());
}

#[test]
#[ignore = "requires a running qdrant on http://127.0.0.1:6333"]
fn vector_qdrant_against_test_server() {
    // Even when qdrant is reachable, the v0.26 cached-records path
    // gives us search results without a live HTTP round trip. The
    // memory-qdrant feature pins the real client wiring.
    let mut s = VectorStore::qdrant("http://127.0.0.1:6333", "mty-memory-test");
    s.upsert("doc-1", "the quick brown fox", meta()).unwrap();
    let hits = s.search("fox", 1).unwrap();
    assert_eq!(hits[0].id, "doc-1");
}
