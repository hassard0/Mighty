//! Auth-store loader: round-trip a token through `auth.toml` and
//! confirm `token_for(slug)` finds it. Also covers the
//! `GITHUB_TOKEN` env-var fallback.

use sdust_pkg::registry::AuthStore;

#[test]
fn round_trip_token_via_file() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("auth.toml");

    let mut store = AuthStore::default();
    store.set_token("acme/registry", "ghp_unit_test_token");
    store.set_token("other/private", "ghp_other");
    store.save(&p).unwrap();

    let reloaded = AuthStore::load(&p).unwrap();
    assert_eq!(
        reloaded.tokens.get("acme/registry").map(String::as_str),
        Some("ghp_unit_test_token")
    );
    assert_eq!(
        reloaded.tokens.get("other/private").map(String::as_str),
        Some("ghp_other")
    );
}

#[test]
fn token_for_finds_per_slug() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("auth.toml");
    let mut store = AuthStore::default();
    store.set_token("only/here", "tok1");
    store.save(&p).unwrap();

    let reloaded = AuthStore::load(&p).unwrap();
    std::env::remove_var("GITHUB_TOKEN");
    assert_eq!(reloaded.token_for("only/here").as_deref(), Some("tok1"));
    // Unknown slug → fallback path. With no env var set, returns None.
    assert!(reloaded.token_for("never/seen").is_none());
}

#[test]
fn missing_file_yields_empty_store() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("missing.toml");
    let store = AuthStore::load(&p).unwrap();
    assert!(store.tokens.is_empty());
}

#[test]
fn env_var_fallback_takes_over() {
    let store = AuthStore::default();
    std::env::set_var("GITHUB_TOKEN", "from-env-var");
    assert_eq!(
        store.token_for("anywhere/at-all").as_deref(),
        Some("from-env-var")
    );
    std::env::remove_var("GITHUB_TOKEN");
}
