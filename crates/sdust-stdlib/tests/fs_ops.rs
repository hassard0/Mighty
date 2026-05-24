//! `std.fs` read + write + exists + list_dir roundtrip.

use sdust_stdlib::fs::{exists, list_dir, read, write, FsCap};

#[test]
fn write_read_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let cap = FsCap::unrestricted();
    let path = tmp.path().join("hello.txt");
    write(&cap, &path, b"hello stdlib").unwrap();
    let back = read(&cap, &path).unwrap();
    assert_eq!(back, b"hello stdlib");
}

#[test]
fn exists_reports_correctly() {
    let tmp = tempfile::tempdir().unwrap();
    let cap = FsCap::unrestricted();
    let path = tmp.path().join("x.txt");
    assert!(!exists(&cap, &path));
    write(&cap, &path, b"x").unwrap();
    assert!(exists(&cap, &path));
}

#[test]
fn list_dir_is_sorted() {
    let tmp = tempfile::tempdir().unwrap();
    let cap = FsCap::unrestricted();
    for n in ["c", "a", "b"] {
        write(&cap, &tmp.path().join(n), b"x").unwrap();
    }
    let entries: Vec<String> = list_dir(&cap, tmp.path())
        .unwrap()
        .into_iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries, vec!["a", "b", "c"]);
}

#[test]
fn rooted_cap_denies_outside() {
    let tmp = tempfile::tempdir().unwrap();
    let inside = tmp.path().to_path_buf();
    let cap = FsCap::rooted([inside.clone()]);
    let outside = std::env::temp_dir().join("definitely-not-allowed-stardust-test");
    assert!(read(&cap, &outside).is_err());
    let inside_path = inside.join("ok.txt");
    write(&cap, &inside_path, b"ok").unwrap();
    assert_eq!(read(&cap, &inside_path).unwrap(), b"ok");
}

#[test]
fn write_creates_parent_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let cap = FsCap::unrestricted();
    let nested = tmp.path().join("a").join("b").join("c.txt");
    write(&cap, &nested, b"nested").unwrap();
    assert!(exists(&cap, &nested));
}
