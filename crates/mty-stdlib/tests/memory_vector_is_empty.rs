//! v0.27 Track E (QoL gap #1) — `VectorStore::is_empty()` integration
//! tests.
//!
//! Demo 07 worked around the missing predicate by re-indexing every run
//! (the local backend is idempotent on same id). The Rust-side method
//! has been on `VectorStore` since v0.26, but it wasn't part of the
//! exercised contract. These tests pin it down + cover `clear()` so the
//! v0.27 follow-up doesn't regress.

use mty_stdlib::memory::vector::VectorStore;
use std::collections::HashMap;
use tempfile::tempdir;

#[test]
fn vector_new_is_empty() {
    let dir = tempdir().unwrap();
    let s = VectorStore::local(dir.path().join("v.json"));
    assert!(s.is_empty(), "fresh local vector store should be empty");
    assert_eq!(s.len(), 0);
}

#[test]
fn vector_after_upsert_not_empty() {
    let dir = tempdir().unwrap();
    let mut s = VectorStore::local(dir.path().join("v.json"));
    s.upsert("a", "hello world", HashMap::new()).unwrap();
    assert!(!s.is_empty(), "store with one record must not report empty");
    assert_eq!(s.len(), 1);
}

#[test]
fn vector_after_clear_is_empty() {
    let dir = tempdir().unwrap();
    let mut s = VectorStore::local(dir.path().join("v.json"));
    s.upsert("a", "hello world", HashMap::new()).unwrap();
    s.upsert("b", "goodbye world", HashMap::new()).unwrap();
    assert_eq!(s.len(), 2);
    s.clear().unwrap();
    assert!(s.is_empty(), "store after clear() must report empty");
    assert_eq!(s.len(), 0);
}

#[test]
fn vector_after_delete_last_is_empty() {
    let dir = tempdir().unwrap();
    let mut s = VectorStore::local(dir.path().join("v.json"));
    s.upsert("solo", "only entry", HashMap::new()).unwrap();
    assert!(!s.is_empty());
    s.delete("solo").unwrap();
    assert!(
        s.is_empty(),
        "deleting the last record should leave the store empty"
    );
}

#[test]
fn vector_clear_persists_to_disk() {
    // Round-trip across constructors: clear must write `[]` to disk
    // so the next `VectorStore::local(path)` sees the empty state.
    let dir = tempdir().unwrap();
    let path = dir.path().join("v.json");
    {
        let mut s = VectorStore::local(&path);
        s.upsert("a", "alpha", HashMap::new()).unwrap();
        s.upsert("b", "beta", HashMap::new()).unwrap();
        assert_eq!(s.len(), 2);
        s.clear().unwrap();
    }
    // New construction reads the persisted empty state.
    let s2 = VectorStore::local(&path);
    assert!(s2.is_empty(), "post-clear vector.json must reload as empty");
}

#[test]
fn qdrant_constructor_is_empty() {
    // No network IO — the qdrant backend reports empty until the
    // first upsert (the live HTTP path is wired in v0.27 follow-up).
    let s = VectorStore::qdrant("http://127.0.0.1:6333", "test_mem");
    assert!(s.is_empty());
}
