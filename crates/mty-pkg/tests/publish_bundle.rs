//! End-to-end bundle test: build a fixture package, verify the
//! tarball + sha256 sidecar match.

use flate2::read::GzDecoder;
use std::fs;
use std::path::Path;

fn write_fixture(root: &Path) {
    fs::write(
        root.join("mighty.toml"),
        r#"
[package]
name = "fixture"
version = "0.4.0"
edition = "2026"
"#,
    )
    .unwrap();
    fs::write(root.join("main.mty"), b"fn main() { println(\"hi\") }").unwrap();
    fs::create_dir_all(root.join("src/inner")).unwrap();
    fs::write(root.join("src/lib.mty"), b"// lib").unwrap();
    fs::write(root.join("src/inner/util.mty"), b"// util").unwrap();
}

#[test]
fn bundle_writes_tar_gz_and_sidecar_with_matching_sha256() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path());

    let outcome = mty_pkg::publish::bundle(dir.path()).unwrap();
    assert!(outcome.bundle_path.exists());
    assert!(outcome.sha256_path.exists());

    let bytes = fs::read(&outcome.bundle_path).unwrap();
    let computed = mty_pkg::hash::hash_bytes(&bytes);
    assert_eq!(computed, outcome.hash);

    let side = fs::read_to_string(&outcome.sha256_path).unwrap();
    let hex = outcome.hash.trim_start_matches("sha256:");
    assert!(side.starts_with(hex), "sidecar lacks hex: {side:?}");
    assert!(
        side.contains(&format!("{}.tar.gz", outcome.tag)),
        "sidecar lacks filename: {side:?}"
    );

    // Tar contents include the expected files under the
    // `<name>-<version>/` prefix.
    let dec = GzDecoder::new(&bytes[..]);
    let mut archive = tar::Archive::new(dec);
    let mut paths: Vec<String> = archive
        .entries()
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path().unwrap().to_string_lossy().into_owned())
        .collect();
    paths.sort();
    assert!(paths.iter().any(|p| p.ends_with("fixture-0.4.0/mighty.toml")));
    assert!(paths.iter().any(|p| p.ends_with("fixture-0.4.0/main.mty")));
    assert!(paths
        .iter()
        .any(|p| p.ends_with("fixture-0.4.0/src/lib.mty")));
    assert!(paths
        .iter()
        .any(|p| p.ends_with("fixture-0.4.0/src/inner/util.mty")));
}

#[test]
fn bundle_re_run_produces_identical_sha256() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path());
    let a = mty_pkg::publish::bundle(dir.path()).unwrap();
    let b = mty_pkg::publish::bundle(dir.path()).unwrap();
    assert_eq!(a.hash, b.hash);
    let ba = fs::read(&a.bundle_path).unwrap();
    let bb = fs::read(&b.bundle_path).unwrap();
    assert_eq!(ba, bb, "tarballs differ between runs");
}

#[test]
fn bundle_extraction_round_trips_through_fetcher() {
    // Bundle, then extract via the fetcher's extraction path; the
    // resulting tree must include every file we wrote.
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path());
    let outcome = mty_pkg::publish::bundle(dir.path()).unwrap();
    let bytes = fs::read(&outcome.bundle_path).unwrap();

    let dst = tempfile::tempdir().unwrap();
    mty_pkg::fetch::registry::extract_targz(&bytes, dst.path()).unwrap();

    let unpacked_root = dst.path().join("fixture-0.4.0");
    assert!(unpacked_root.join("mighty.toml").exists());
    assert!(unpacked_root.join("main.mty").exists());
    assert!(unpacked_root.join("src/lib.mty").exists());
    assert!(unpacked_root.join("src/inner/util.mty").exists());
}
