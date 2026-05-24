#[test]
fn loads_minimal_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("star.toml");
    std::fs::write(
        &path,
        br#"
[package]
name = "x"
version = "0.1.0"
edition = "2026"
"#,
    )
    .unwrap();
    let m = sdust_driver::manifest::load(&path).unwrap();
    assert_eq!(m.package.name, "x");
    assert_eq!(m.package.profile, "host");
}

#[test]
fn loads_manifest_with_deps() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("star.toml");
    std::fs::write(
        &path,
        br#"
[package]
name = "y"
version = "0.2.0"
edition = "2026"
profile = "edge"

[deps]
std = "0.1"
otel = "0.1"
"#,
    )
    .unwrap();
    let m = sdust_driver::manifest::load(&path).unwrap();
    assert_eq!(m.package.profile, "edge");
    assert_eq!(m.deps.len(), 2);
    assert!(m.deps.contains_key("std"));
}

#[test]
fn pipeline_parses_and_lowers() {
    let parsed = sdust_driver::parse_source("fn main() {}".to_string(), "test.sd".to_string());
    assert_eq!(parsed.diagnostics.len(), 0);
    let (pkg, diags) = sdust_driver::lower(&parsed);
    assert_eq!(diags.len(), 0);
    assert_eq!(pkg.fns.len(), 1);
}
