//! Two configured registries — the package only exists in the
//! *second* registry, which must still resolve cleanly.

use mty_driver::manifest::{Dep, Manifest, Package};
use mty_pkg::registry::{RegistryConfig, RegistryIndex, RegistryRelease};
use mty_pkg::resolver::Resolver;
use std::collections::BTreeMap;

fn rel(name: &str, version: &str) -> RegistryRelease {
    RegistryRelease {
        name: name.into(),
        version: version.into(),
        tag: format!("{name}-{version}"),
        tarball_url: None,
        sha256_url: None,
        html_url: None,
        body_preview: None,
    }
}

fn pkg(name: &str) -> Package {
    Package {
        name: name.into(),
        version: "0.1.0".into(),
        edition: "2026".into(),
        profile: "host".into(),
    }
}

#[test]
fn package_in_second_registry_resolves() {
    let dir = tempfile::tempdir().unwrap();
    let mut r = Resolver::with_registry_config(
        dir.path(),
        RegistryConfig {
            default: Some("primary/idx".into()),
            extras: vec!["secondary/idx".into()],
            ..Default::default()
        },
    );
    let mut idx_primary = RegistryIndex::new("primary/idx");
    idx_primary.releases.push(rel("other-pkg", "1.0.0"));
    let mut idx_secondary = RegistryIndex::new("secondary/idx");
    idx_secondary.releases.push(rel("target", "0.2.0"));
    idx_secondary.releases.push(rel("target", "0.2.5"));
    r.injected_indexes.push(idx_primary);
    r.injected_indexes.push(idx_secondary);

    let mut deps = BTreeMap::new();
    deps.insert("target".into(), Dep::Version("^0.2".into()));
    let m = Manifest {
        package: pkg("app"),
        deps,
        build: None,
        cluster: None,
    };
    let lock = r.resolve(&m).unwrap();
    assert_eq!(lock.packages.len(), 1);
    assert_eq!(lock.packages[0].name, "target");
    assert_eq!(lock.packages[0].version, "0.2.5");
    assert_eq!(lock.packages[0].source, "registry+gh://secondary/idx");
}

#[test]
fn first_registry_wins_on_duplicate_name_version() {
    let dir = tempfile::tempdir().unwrap();
    let mut r = Resolver::with_registry_config(
        dir.path(),
        RegistryConfig {
            default: Some("primary/idx".into()),
            extras: vec!["secondary/idx".into()],
            ..Default::default()
        },
    );
    let mut idx_primary = RegistryIndex::new("primary/idx");
    idx_primary.releases.push(rel("dup", "1.0.0"));
    let mut idx_secondary = RegistryIndex::new("secondary/idx");
    idx_secondary.releases.push(rel("dup", "1.0.0"));
    r.injected_indexes.push(idx_primary);
    r.injected_indexes.push(idx_secondary);

    let mut deps = BTreeMap::new();
    deps.insert("dup".into(), Dep::Version("1.0.0".into()));
    let m = Manifest {
        package: pkg("app"),
        deps,
        build: None,
        cluster: None,
    };
    let lock = r.resolve(&m).unwrap();
    assert_eq!(lock.packages[0].source, "registry+gh://primary/idx");
}

#[test]
fn unknown_package_falls_back_to_requirement_floor() {
    let dir = tempfile::tempdir().unwrap();
    let mut r = Resolver::with_registry_config(
        dir.path(),
        RegistryConfig {
            default: Some("primary/idx".into()),
            extras: vec![],
            ..Default::default()
        },
    );
    let mut idx = RegistryIndex::new("primary/idx");
    idx.releases.push(rel("something-else", "1.0.0"));
    r.injected_indexes.push(idx);

    let mut deps = BTreeMap::new();
    deps.insert("ghost".into(), Dep::Version("^0.3.2".into()));
    let m = Manifest {
        package: pkg("app"),
        deps,
        build: None,
        cluster: None,
    };
    let lock = r.resolve(&m).unwrap();
    assert_eq!(lock.packages[0].version, "0.3.2");
    assert_eq!(lock.packages[0].source, "registry+gh://primary/idx");
}
