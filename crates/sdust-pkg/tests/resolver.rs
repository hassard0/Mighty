//! Resolver integration tests — registry stub, path deps, conflicts.

use sdust_driver::manifest::{Dep, DetailedDep, Manifest, Package};
use sdust_pkg::Resolver;
use std::collections::BTreeMap;

fn pkg(name: &str, version: &str) -> Package {
    Package {
        name: name.into(),
        version: version.into(),
        edition: "2026".into(),
        profile: "host".into(),
    }
}

#[test]
fn happy_path_single_registry_dep() {
    let mut deps = BTreeMap::new();
    deps.insert("std".into(), Dep::Version("0.1".into()));
    let m = Manifest {
        package: pkg("app", "0.1.0"),
        deps,
        build: None,
    };
    let dir = tempfile::tempdir().unwrap();
    let lock = Resolver::new(dir.path()).resolve(&m).unwrap();
    assert_eq!(lock.packages.len(), 1);
    assert_eq!(lock.packages[0].name, "std");
    assert_eq!(lock.packages[0].version, "0.1.0");
}

#[test]
fn transitive_via_path_dep() {
    let root = tempfile::tempdir().unwrap();
    let sub = root.path().join("sub");
    let subsub = root.path().join("subsub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::create_dir_all(&subsub).unwrap();

    // subsub: leaf
    std::fs::write(
        subsub.join("star.toml"),
        br#"
[package]
name = "subsub"
version = "0.0.5"
edition = "2026"
"#,
    )
    .unwrap();

    // sub: depends on subsub by path
    std::fs::write(
        sub.join("star.toml"),
        br#"
[package]
name = "sub"
version = "0.2.0"
edition = "2026"

[deps]
subsub = { path = "../subsub" }
"#,
    )
    .unwrap();

    // root manifest depends on sub by path
    let mut deps = BTreeMap::new();
    deps.insert(
        "sub".into(),
        Dep::Detailed(DetailedDep {
            path: Some("sub".into()),
            ..Default::default()
        }),
    );
    let m = Manifest {
        package: pkg("app", "0.1.0"),
        deps,
        build: None,
    };

    let lock = Resolver::new(root.path()).resolve(&m).unwrap();
    let names: Vec<_> = lock.packages.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"sub"));
    assert!(names.contains(&"subsub"));
    let sub_entry = lock.packages.iter().find(|p| p.name == "sub").unwrap();
    assert_eq!(sub_entry.version, "0.2.0");
    assert!(sub_entry.dependencies.contains(&"subsub".to_string()));
}

#[test]
fn version_conflict_errors_when_two_path_versions_disagree() {
    // Two path deps that both pull in `leaf` from different paths
    // with different versions. The resolver records the first, then
    // errors on the second.
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    let leaf_a = root.path().join("leaf_a");
    let leaf_b = root.path().join("leaf_b");
    for d in [&a, &b, &leaf_a, &leaf_b] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(
        leaf_a.join("star.toml"),
        br#"[package]
name = "leaf"
version = "0.1.0"
edition = "2026"
"#,
    )
    .unwrap();
    std::fs::write(
        leaf_b.join("star.toml"),
        br#"[package]
name = "leaf"
version = "0.2.0"
edition = "2026"
"#,
    )
    .unwrap();
    std::fs::write(
        a.join("star.toml"),
        br#"[package]
name = "a"
version = "0.1.0"
edition = "2026"

[deps]
leaf = { path = "../leaf_a" }
"#,
    )
    .unwrap();
    std::fs::write(
        b.join("star.toml"),
        br#"[package]
name = "b"
version = "0.1.0"
edition = "2026"

[deps]
leaf = { path = "../leaf_b" }
"#,
    )
    .unwrap();
    let mut deps = BTreeMap::new();
    deps.insert(
        "a".into(),
        Dep::Detailed(DetailedDep {
            path: Some("a".into()),
            ..Default::default()
        }),
    );
    deps.insert(
        "b".into(),
        Dep::Detailed(DetailedDep {
            path: Some("b".into()),
            ..Default::default()
        }),
    );
    let m = Manifest {
        package: pkg("app", "0.1.0"),
        deps,
        build: None,
    };
    let err = Resolver::new(root.path()).resolve(&m).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("conflict"),
        "expected version conflict, got: {msg}"
    );
}
