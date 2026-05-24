//! Network smoke test — fetches a small public GitHub release and
//! validates the index path end-to-end. Ignored by default so the
//! suite stays hermetic; opt in with
//! `cargo test -p mty-pkg --test registry_fetch -- --ignored`.

#[test]
#[ignore]
fn fetches_real_release_index() {
    let tmp = tempfile::tempdir().unwrap();
    // Use a small, stable public repo with at least one release. The
    // `stardust-pkg/registry` slug doesn't exist yet — we hit a real
    // repo here so the smoke test catches transport breakage. Switch
    // to the official registry once v0.5 spins it up.
    let slug = "octocat/Hello-World";
    let result = mty_pkg::fetch::registry::load_index_for(tmp.path(), slug, true);
    // This repo has no Mighty-shaped tags, so the index parses with
    // zero releases — but the *call* must succeed (status 200 + valid
    // JSON pages). That's the real smoke test.
    let idx = result.expect("network call to GitHub Releases API");
    assert_eq!(idx.slug, slug);
    // Cache file written.
    let cache = mty_pkg::registry::cache_path(tmp.path(), slug);
    assert!(cache.exists());
}
