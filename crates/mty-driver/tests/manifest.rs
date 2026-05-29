#[test]
fn loads_minimal_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mighty.toml");
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
    let m = mty_driver::manifest::load(&path).unwrap();
    assert_eq!(m.package.name, "x");
    assert_eq!(m.package.profile, "host");
}

#[test]
fn loads_manifest_with_deps() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mighty.toml");
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
    let m = mty_driver::manifest::load(&path).unwrap();
    assert_eq!(m.package.profile, "edge");
    assert_eq!(m.deps.len(), 2);
    assert!(m.deps.contains_key("std"));
}

#[test]
fn pipeline_parses_and_lowers() {
    let parsed = mty_driver::parse_source("fn main() {}".to_string(), "test.mty".to_string());
    assert_eq!(parsed.diagnostics.len(), 0);
    let (pkg, diags) = mty_driver::lower(&parsed);
    assert_eq!(diags.len(), 0);
    assert_eq!(pkg.fns.len(), 1);
}

// v0.36 Track T2 — [[extern_lib]] manifest schema. Each test covers one
// shape of the new block so a regression to the parser surfaces with a
// specific failing case rather than a generic deserialize error.

#[test]
fn extern_lib_static_with_explicit_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mighty.toml");
    std::fs::write(
        &path,
        br#"
[package]
name = "ffi"
version = "0.1.0"
edition = "2026"

[[extern_lib]]
name = "winit"
kind = "static"
path = "vendor/libwinit.a"
"#,
    )
    .unwrap();
    let m = mty_driver::manifest::load(&path).unwrap();
    assert_eq!(m.extern_libs.len(), 1);
    let lib = &m.extern_libs[0];
    assert_eq!(lib.name, "winit");
    assert!(lib.is_static());
    assert!(!lib.is_dynamic());
    assert_eq!(lib.path.as_deref(), Some("vendor/libwinit.a"));
}

#[test]
fn extern_lib_dynamic_no_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mighty.toml");
    std::fs::write(
        &path,
        br#"
[package]
name = "ffi"
version = "0.1.0"
edition = "2026"

[[extern_lib]]
name = "z"
kind = "dynamic"
"#,
    )
    .unwrap();
    let m = mty_driver::manifest::load(&path).unwrap();
    let lib = &m.extern_libs[0];
    assert!(lib.is_dynamic());
    assert!(lib.path.is_none());
}

#[test]
fn extern_lib_kind_defaults_to_static() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mighty.toml");
    std::fs::write(
        &path,
        br#"
[package]
name = "ffi"
version = "0.1.0"
edition = "2026"

[[extern_lib]]
name = "winit"
path = "vendor/libwinit.a"
"#,
    )
    .unwrap();
    let m = mty_driver::manifest::load(&path).unwrap();
    assert!(m.extern_libs[0].is_static());
    assert_eq!(m.extern_libs[0].kind, "static");
}

#[test]
fn extern_lib_multiple_entries_keep_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mighty.toml");
    std::fs::write(
        &path,
        br#"
[package]
name = "ffi"
version = "0.1.0"
edition = "2026"

[[extern_lib]]
name = "wgpu"
path = "vendor/libwgpu.a"

[[extern_lib]]
name = "winit"
path = "vendor/libwinit.a"

[[extern_lib]]
name = "m"
"#,
    )
    .unwrap();
    let m = mty_driver::manifest::load(&path).unwrap();
    assert_eq!(m.extern_libs.len(), 3);
    assert_eq!(m.extern_libs[0].name, "wgpu");
    assert_eq!(m.extern_libs[1].name, "winit");
    assert_eq!(m.extern_libs[2].name, "m");
    assert!(m.extern_libs[2].path.is_none());
}

#[test]
fn extern_lib_link_args_cross_platform() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mighty.toml");
    std::fs::write(
        &path,
        br#"
[package]
name = "ffi"
version = "0.1.0"
edition = "2026"

[[extern_lib]]
name = "graphics"
path = "vendor/libgraphics.a"
link_args = ["--whole-archive"]
"#,
    )
    .unwrap();
    let m = mty_driver::manifest::load(&path).unwrap();
    assert_eq!(m.extern_libs[0].link_args, vec!["--whole-archive"]);
    // The cross-platform set surfaces in resolved_link_args regardless
    // of host.
    let resolved = m.extern_libs[0].resolved_link_args(mty_driver::manifest::HostOs::Linux);
    assert!(resolved.contains(&"--whole-archive".to_string()));
}

#[test]
fn extern_lib_link_args_per_platform_filter() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mighty.toml");
    std::fs::write(
        &path,
        br#"
[package]
name = "ffi"
version = "0.1.0"
edition = "2026"

[[extern_lib]]
name = "winit"
path = "vendor/libwinit.a"
link_args_macos = ["-framework", "Cocoa"]
link_args_linux = ["-lxkbcommon"]
link_args_windows = ["Userenv.lib"]
"#,
    )
    .unwrap();
    let m = mty_driver::manifest::load(&path).unwrap();
    let lib = &m.extern_libs[0];

    use mty_driver::manifest::HostOs;
    let mac = lib.resolved_link_args(HostOs::Macos);
    assert_eq!(mac, vec!["-framework".to_string(), "Cocoa".to_string()]);

    let lin = lib.resolved_link_args(HostOs::Linux);
    assert_eq!(lin, vec!["-lxkbcommon".to_string()]);

    let win = lib.resolved_link_args(HostOs::Windows);
    assert_eq!(win, vec!["Userenv.lib".to_string()]);

    // Other host: only the cross-platform link_args (here empty) survive.
    let other = lib.resolved_link_args(HostOs::Other);
    assert!(other.is_empty());
}

#[test]
fn extern_lib_back_compat_manifests_without_section() {
    // Manifests that predate v0.36 must still load cleanly with an
    // empty `extern_libs` Vec.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mighty.toml");
    std::fs::write(
        &path,
        br#"
[package]
name = "old"
version = "0.1.0"
edition = "2026"
"#,
    )
    .unwrap();
    let m = mty_driver::manifest::load(&path).unwrap();
    assert!(m.extern_libs.is_empty());
}

#[test]
fn extern_lib_round_trips_via_serialize() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mighty.toml");
    std::fs::write(
        &path,
        br#"
[package]
name = "ffi"
version = "0.1.0"
edition = "2026"

[[extern_lib]]
name = "winit"
kind = "static"
path = "vendor/libwinit.a"
link_args_macos = ["-framework", "Cocoa"]
"#,
    )
    .unwrap();
    let m = mty_driver::manifest::load(&path).unwrap();
    let path2 = dir.path().join("mighty.round.toml");
    mty_driver::manifest::save(&m, &path2).unwrap();
    let m2 = mty_driver::manifest::load(&path2).unwrap();
    assert_eq!(m2.extern_libs.len(), 1);
    assert_eq!(m2.extern_libs[0].name, "winit");
    assert_eq!(
        m2.extern_libs[0].link_args_macos,
        vec!["-framework", "Cocoa"]
    );
}

#[test]
fn extern_lib_kind_case_insensitive() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mighty.toml");
    std::fs::write(
        &path,
        br#"
[package]
name = "ffi"
version = "0.1.0"
edition = "2026"

[[extern_lib]]
name = "z"
kind = "Dynamic"
"#,
    )
    .unwrap();
    let m = mty_driver::manifest::load(&path).unwrap();
    assert!(m.extern_libs[0].is_dynamic());
}

// v0.36 Track T2 — `build_linker_args` translation tests. These pin
// the manifest → linker flat-arg contract so the codegen-crate's
// `link_executable_with_libs` keeps receiving the same shape.

#[test]
fn build_linker_args_emits_paths_then_link_args() {
    use mty_driver::build::build_linker_args;
    use mty_driver::manifest::ExternLib;
    let lib = ExternLib {
        name: "winit".into(),
        kind: "static".into(),
        path: Some("vendor/libwinit.a".into()),
        link_args: vec!["--whole-archive".into()],
        ..Default::default()
    };
    let args = build_linker_args(&[lib], None);
    assert_eq!(args[0], "vendor/libwinit.a");
    assert!(args.contains(&"--whole-archive".to_string()));
}

#[test]
fn build_linker_args_falls_back_to_dash_l_when_no_path() {
    use mty_driver::build::build_linker_args;
    use mty_driver::manifest::ExternLib;
    let lib = ExternLib {
        name: "z".into(),
        kind: "dynamic".into(),
        path: None,
        ..Default::default()
    };
    let args = build_linker_args(&[lib], None);
    assert_eq!(args, vec!["-lz".to_string()]);
}

#[test]
fn build_linker_args_resolves_relative_paths_against_manifest_dir() {
    use mty_driver::build::build_linker_args;
    use mty_driver::manifest::ExternLib;
    let lib = ExternLib {
        name: "winit".into(),
        kind: "static".into(),
        path: Some("vendor/libwinit.a".into()),
        ..Default::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let args = build_linker_args(&[lib], Some(dir.path()));
    let joined = std::path::Path::new(&args[0]);
    assert!(joined.is_absolute() || joined.starts_with(dir.path()));
    assert!(args[0].contains("libwinit.a"));
}

#[test]
fn build_linker_args_preserves_entry_order() {
    use mty_driver::build::build_linker_args;
    use mty_driver::manifest::ExternLib;
    let libs = vec![
        ExternLib {
            name: "wgpu".into(),
            path: Some("vendor/libwgpu.a".into()),
            kind: "static".into(),
            ..Default::default()
        },
        ExternLib {
            name: "winit".into(),
            path: Some("vendor/libwinit.a".into()),
            kind: "static".into(),
            ..Default::default()
        },
    ];
    let args = build_linker_args(&libs, None);
    // Order: wgpu archive, then winit archive.
    let wgpu_idx = args.iter().position(|s| s.contains("libwgpu")).unwrap();
    let winit_idx = args.iter().position(|s| s.contains("libwinit")).unwrap();
    assert!(wgpu_idx < winit_idx);
}
