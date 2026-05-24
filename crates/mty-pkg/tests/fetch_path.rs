//! Path-fetch + hash verification + hash-mismatch detection.

use mty_pkg::commands;
use mty_pkg::lockfile;

#[test]
fn fetch_copies_path_dep_and_records_hash() {
    let root = tempfile::tempdir().unwrap();
    let dep = root.path().join("mylib");
    std::fs::create_dir_all(&dep).unwrap();
    std::fs::write(
        dep.join("mighty.toml"),
        br#"[package]
name = "mylib"
version = "0.3.0"
edition = "2026"
"#,
    )
    .unwrap();
    std::fs::write(dep.join("lib.sd"), b"fn hello() {}").unwrap();

    std::fs::write(
        root.path().join("mighty.toml"),
        br#"[package]
name = "app"
version = "0.1.0"
edition = "2026"

[deps]
mylib = { path = "mylib" }
"#,
    )
    .unwrap();

    commands::resolve_and_lock(root.path()).unwrap();
    let fetched = commands::fetch_all(root.path()).unwrap();
    assert_eq!(fetched.len(), 1);
    assert!(fetched[0].hash.starts_with("sha256:"));
    assert!(fetched[0].root.exists());

    // Hash got pinned into the lockfile.
    let lock = lockfile::load(&root.path().join("mighty.lock")).unwrap();
    let entry = lock.find("mylib").unwrap();
    assert_eq!(entry.hash.as_deref(), Some(fetched[0].hash.as_str()));
}

#[test]
fn fetch_detects_hash_mismatch_on_tampered_source() {
    let root = tempfile::tempdir().unwrap();
    let dep = root.path().join("mylib");
    std::fs::create_dir_all(&dep).unwrap();
    std::fs::write(
        dep.join("mighty.toml"),
        br#"[package]
name = "mylib"
version = "0.3.0"
edition = "2026"
"#,
    )
    .unwrap();
    std::fs::write(dep.join("lib.sd"), b"fn original() {}").unwrap();

    std::fs::write(
        root.path().join("mighty.toml"),
        br#"[package]
name = "app"
version = "0.1.0"
edition = "2026"

[deps]
mylib = { path = "mylib" }
"#,
    )
    .unwrap();

    commands::resolve_and_lock(root.path()).unwrap();
    commands::fetch_all(root.path()).unwrap();

    // Tamper with the source after the lockfile pinned a hash.
    std::fs::write(dep.join("lib.sd"), b"fn tampered() {}").unwrap();

    let err = commands::fetch_all(root.path()).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("hash mismatch"),
        "expected hash mismatch error, got: {msg}"
    );
}

#[test]
fn list_shows_resolved_tree() {
    let root = tempfile::tempdir().unwrap();
    let dep = root.path().join("mylib");
    std::fs::create_dir_all(&dep).unwrap();
    std::fs::write(
        dep.join("mighty.toml"),
        br#"[package]
name = "mylib"
version = "0.3.0"
edition = "2026"
"#,
    )
    .unwrap();
    std::fs::write(
        root.path().join("mighty.toml"),
        br#"[package]
name = "app"
version = "0.1.0"
edition = "2026"

[deps]
mylib = { path = "mylib" }
"#,
    )
    .unwrap();

    commands::resolve_and_lock(root.path()).unwrap();
    let out = commands::list(root.path()).unwrap();
    assert!(out.contains("app v0.1.0"));
    assert!(out.contains("mylib"));
    assert!(out.contains("(path)"));
}
