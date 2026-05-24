//! Lockfile parse + serialize roundtrips.

use mty_pkg::lockfile::{self, LockedPackage, Lockfile, DEFAULT_REGISTRY};

#[test]
fn roundtrip_via_disk() {
    let mut lock = Lockfile::new();
    lock.upsert(LockedPackage {
        name: "std".into(),
        version: "0.1.0".into(),
        source: LockedPackage::registry_source(DEFAULT_REGISTRY),
        hash: Some("sha256:aaaa".into()),
        dependencies: vec![],
    });
    lock.upsert(LockedPackage {
        name: "otel".into(),
        version: "0.1.0".into(),
        source: LockedPackage::registry_source(DEFAULT_REGISTRY),
        hash: Some("sha256:bbbb".into()),
        dependencies: vec!["std".into()],
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mighty.lock");
    lockfile::save(&lock, &path).unwrap();

    let reloaded = lockfile::load(&path).unwrap();
    assert_eq!(reloaded.version, 1);
    assert_eq!(reloaded.packages.len(), 2);
    let std_entry = reloaded.find("std").unwrap();
    assert_eq!(std_entry.hash.as_deref(), Some("sha256:aaaa"));
    let otel_entry = reloaded.find("otel").unwrap();
    assert_eq!(otel_entry.dependencies, vec!["std".to_string()]);
}

#[test]
fn parses_handwritten_lockfile() {
    let body = r#"
version = 1

[[package]]
name = "foo"
version = "1.2.3"
source = "registry+https://pkg.mighty.dev"
hash = "sha256:cafe"
dependencies = ["bar"]

[[package]]
name = "bar"
version = "0.1.0"
source = "path+file:///tmp/bar"
"#;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("mighty.lock");
    std::fs::write(&p, body).unwrap();
    let lock = lockfile::load(&p).unwrap();
    assert_eq!(lock.packages.len(), 2);
    let foo = lock.find("foo").unwrap();
    assert_eq!(foo.version, "1.2.3");
    assert_eq!(foo.dependencies, vec!["bar".to_string()]);
    let bar = lock.find("bar").unwrap();
    assert!(bar.source.starts_with("path+"));
    assert!(bar.hash.is_none());
}
