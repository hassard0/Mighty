//! Parse a fixture GitHub Releases JSON page and confirm we extract
//! the right `(name, version, asset URLs)` tuples.

use mty_pkg::registry;

const FIXTURE: &str = r#"[
    {
        "tag_name": "otel-0.1.0",
        "name": "otel-0.1.0",
        "html_url": "https://github.com/stardust-pkg/registry/releases/tag/otel-0.1.0",
        "body": "[package]\nname=\"otel\"\nversion=\"0.1.0\"\nedition=\"2026\"",
        "assets": [
            {
                "name": "otel-0.1.0.tar.gz",
                "browser_download_url": "https://example.com/otel-0.1.0.tar.gz"
            },
            {
                "name": "otel-0.1.0.tar.gz.sha256",
                "browser_download_url": "https://example.com/otel-0.1.0.tar.gz.sha256"
            }
        ]
    },
    {
        "tag_name": "my-lib-1.2.3",
        "name": "my-lib-1.2.3",
        "html_url": "https://github.com/stardust-pkg/registry/releases/tag/my-lib-1.2.3",
        "body": "snapshot",
        "assets": [
            {
                "name": "my-lib-1.2.3.tar.gz",
                "browser_download_url": "https://example.com/my-lib-1.2.3.tar.gz"
            }
        ]
    },
    {
        "tag_name": "not-a-package-tag",
        "name": "garbage",
        "html_url": "",
        "body": "",
        "assets": []
    }
]"#;

#[test]
fn parses_two_packages_and_skips_garbage() {
    let releases = registry::parse_releases_page(FIXTURE).unwrap();
    assert_eq!(releases.len(), 2);

    let otel = &releases[0];
    assert_eq!(otel.name, "otel");
    assert_eq!(otel.version, "0.1.0");
    assert_eq!(otel.tag, "otel-0.1.0");
    assert!(otel
        .tarball_url
        .as_deref()
        .unwrap()
        .ends_with("otel-0.1.0.tar.gz"));
    assert!(otel.sha256_url.as_deref().unwrap().ends_with(".sha256"));
    assert!(otel.html_url.is_some());
    assert!(otel
        .body_preview
        .as_deref()
        .unwrap()
        .contains("name=\"otel\""));

    let lib = &releases[1];
    assert_eq!(lib.name, "my-lib");
    assert_eq!(lib.version, "1.2.3");
    // Missing sidecar is OK.
    assert!(lib.sha256_url.is_none());
}

#[test]
fn empty_page_yields_empty_vec() {
    let releases = registry::parse_releases_page("[]").unwrap();
    assert!(releases.is_empty());
}

#[test]
fn malformed_json_errors() {
    assert!(registry::parse_releases_page("not json").is_err());
}

#[test]
fn cached_index_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let mut idx = registry::RegistryIndex::new("foo/bar");
    idx.fetched_at = 42;
    idx.releases = registry::parse_releases_page(FIXTURE).unwrap();
    registry::save_cached_index(tmp.path(), &idx).unwrap();
    let back = registry::load_cached_index(tmp.path(), "foo/bar")
        .unwrap()
        .expect("cache exists");
    assert_eq!(back.slug, "foo/bar");
    assert_eq!(back.releases.len(), 2);
    assert_eq!(back.fetched_at, 42);
}
