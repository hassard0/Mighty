//! Git-fetch integration test. `#[ignore]` by default because it
//! requires network access. Run with `cargo test -p sdust-pkg -- --ignored`.

use sdust_pkg::fetch;
use sdust_pkg::lockfile::LockedPackage;

#[test]
#[ignore = "requires network access to github.com"]
fn clones_small_repo_and_checks_out_rev() {
    let dir = tempfile::tempdir().unwrap();
    let locked = LockedPackage {
        name: "rust-octocrab-test".into(),
        version: "0.0.0".into(),
        // Tiny well-known Rust hello-world that has been stable for
        // years. If GitHub ever yanks this, swap for any small repo
        // with a known rev.
        source: LockedPackage::git_source(
            "https://github.com/rust-lang/cargo",
            // First-ever commit, ~10KB tree; safe and stable.
            Some("23eb492c248c7f5a45a85c5d36b3658e6857c7ec"),
        ),
        hash: None,
        dependencies: vec![],
    };
    let fetched = fetch::fetch_one(dir.path(), &locked).unwrap();
    assert!(fetched.root.exists());
    assert!(fetched.hash.starts_with("sha256:"));
}
