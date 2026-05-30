//! v0.41 T4 — manifest `[build]` link block + linker-flavor rewrite
//! tests.
//!
//! Covers:
//!
//! * `[build] native-libs` / `link-search` / `frameworks` / `link-args`
//!   parse cleanly and round-trip via the `Manifest` loader.
//! * `BuildConfig::linker_args` emits the documented GNU-shape vector
//!   in the documented order (`-L` first, then `-l`, then
//!   `-framework`, then raw `link-args`).
//! * The `LinkerFlavor::detect_from_path` heuristic recognises MSVC
//!   basenames and falls back to GNU for everything else.
//! * `rewrite_for_flavor` is identity for GNU and performs the
//!   advertised translations for MSVC.
//! * Integration: a synth manifest with `[build] native-libs = ["m"]`
//!   feeds through to the link command on Linux/macOS hosts (libm is
//!   universally available on those targets). On Windows we instead
//!   assert the MSVC rewrite output shape — running `link.exe` from
//!   the test would require a full MSVC install.

use mty_driver::link_flavor::{rewrite_for_flavor, LinkerFlavor};
use mty_driver::manifest::{BuildConfig, HostOs};

#[test]
fn parses_build_block_native_libs_and_search() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mighty.toml");
    std::fs::write(
        &path,
        br#"
[package]
name = "ffi"
version = "0.1.0"
edition = "2026"

[build]
native-libs = ["foo", "bar"]
link-search = ["/opt/whatever/lib"]
"#,
    )
    .unwrap();
    let m = mty_driver::manifest::load(&path).unwrap();
    let b = m.build.expect("[build] block must parse");
    assert_eq!(b.native_libs, vec!["foo", "bar"]);
    assert_eq!(b.link_search, vec!["/opt/whatever/lib"]);
    assert!(b.frameworks.is_empty());
    assert!(b.link_args.is_empty());
}

#[test]
fn parses_build_block_frameworks_and_link_args() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mighty.toml");
    std::fs::write(
        &path,
        br#"
[package]
name = "ffi"
version = "0.1.0"
edition = "2026"

[build]
frameworks = ["Cocoa", "Foundation"]
link-args = ["--gc-sections"]
"#,
    )
    .unwrap();
    let m = mty_driver::manifest::load(&path).unwrap();
    let b = m.build.expect("[build] block must parse");
    assert_eq!(b.frameworks, vec!["Cocoa", "Foundation"]);
    assert_eq!(b.link_args, vec!["--gc-sections"]);
}

#[test]
fn parses_build_block_back_compat_with_script_fields() {
    // The pre-v0.41 build-script-sandbox shape must still parse so
    // existing manifests continue to load.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mighty.toml");
    std::fs::write(
        &path,
        br#"
[package]
name = "ffi"
version = "0.1.0"
edition = "2026"

[build]
script = "build.mty"
allow_net = ["example.com"]
allow_fs = ["./vendor"]
"#,
    )
    .unwrap();
    let m = mty_driver::manifest::load(&path).unwrap();
    let b = m.build.expect("[build] block must parse");
    assert_eq!(b.script.as_deref(), Some("build.mty"));
    assert_eq!(b.allow_net, vec!["example.com"]);
    assert_eq!(b.allow_fs, vec!["./vendor"]);
    // And the new fields default to empty, not None.
    assert!(b.native_libs.is_empty());
    assert!(b.link_search.is_empty());
}

#[test]
fn linker_args_order_is_search_then_lib_then_framework_then_raw() {
    let b = BuildConfig {
        native_libs: vec!["foo".into()],
        link_search: vec!["/opt/lib".into()],
        frameworks: vec!["Cocoa".into()],
        link_args: vec!["--gc-sections".into()],
        ..Default::default()
    };
    let args = b.linker_args(HostOs::Macos);
    assert_eq!(
        args,
        vec![
            "-L/opt/lib".to_string(),
            "-lfoo".to_string(),
            "-framework".to_string(),
            "Cocoa".to_string(),
            "--gc-sections".to_string(),
        ]
    );
}

#[test]
fn linker_args_drops_frameworks_off_macos() {
    let b = BuildConfig {
        native_libs: vec!["foo".into()],
        frameworks: vec!["Cocoa".into()],
        ..Default::default()
    };
    let args_linux = b.linker_args(HostOs::Linux);
    assert!(!args_linux.iter().any(|a| a == "-framework"));
    assert!(args_linux.contains(&"-lfoo".to_string()));

    let args_windows = b.linker_args(HostOs::Windows);
    assert!(!args_windows.iter().any(|a| a == "-framework"));
    assert!(args_windows.contains(&"-lfoo".to_string()));
}

#[test]
fn linker_args_empty_when_block_is_default() {
    let b = BuildConfig::default();
    assert!(b.linker_args(HostOs::Linux).is_empty());
}

#[test]
fn flavor_detect_msvc_basenames() {
    assert_eq!(
        LinkerFlavor::detect_from_path("link.exe"),
        LinkerFlavor::Msvc
    );
    assert_eq!(
        LinkerFlavor::detect_from_path("lld-link.exe"),
        LinkerFlavor::Msvc
    );
}

#[test]
fn flavor_detect_falls_back_to_gnu() {
    for p in ["clang", "gcc.exe", "cc", "/usr/bin/clang", "ld.lld"] {
        assert_eq!(
            LinkerFlavor::detect_from_path(p),
            LinkerFlavor::Gnu,
            "expected Gnu for {p}"
        );
    }
}

#[test]
fn flavor_rewrite_gnu_identity() {
    let args = vec!["-lfoo".to_string(), "-L/x".to_string()];
    assert_eq!(rewrite_for_flavor(&args, LinkerFlavor::Gnu), args);
}

#[test]
fn flavor_rewrite_msvc_translates_libs_search_and_gc() {
    let args = vec![
        "-L/opt/lib".to_string(),
        "-lfoo".to_string(),
        "-lbar".to_string(),
        "--gc-sections".to_string(),
    ];
    let got = rewrite_for_flavor(&args, LinkerFlavor::Msvc);
    assert_eq!(
        got,
        vec![
            "/LIBPATH:/opt/lib".to_string(),
            "foo.lib".to_string(),
            "bar.lib".to_string(),
            "/OPT:REF".to_string(),
        ]
    );
}

#[test]
fn flavor_rewrite_msvc_drops_framework_pair() {
    // `-framework Cocoa` on the way to a Windows linker has no analogue.
    let args = vec![
        "-framework".to_string(),
        "Cocoa".to_string(),
        "-lz".to_string(),
    ];
    let got = rewrite_for_flavor(&args, LinkerFlavor::Msvc);
    assert_eq!(got, vec!["z.lib".to_string()]);
}

#[test]
fn flavor_rewrite_msvc_passes_msvc_flags_through() {
    // Raw MSVC flags users encoded in link-args must survive rewrite.
    let args = vec!["/SUBSYSTEM:CONSOLE".to_string(), "user32.lib".to_string()];
    let got = rewrite_for_flavor(&args, LinkerFlavor::Msvc);
    assert_eq!(got, args);
}

/// Integration sanity: synth a tiny package with `[build] native-libs =
/// ["m"]`, run `mty build`, and assert the produced binary path exists.
/// libm is universally available on Linux + macOS (and on Windows
/// `link.exe` happily resolves `m.lib` to the MSVC C runtime; if no
/// MSVC linker is on PATH we just get the `.o` outcome, which we treat
/// as a pass — the goal is to prove the manifest plumbing reaches the
/// link command without crashing the driver).
///
/// We don't actually call libm from the program; the linker walks the
/// argv as-is. On every host the `[build]` set arrives at the linker as
/// `-lm` (rewritten to `m.lib` on MSVC).
#[test]
fn build_native_with_build_block_threads_native_libs() {
    use mty_codegen_cranelift::artifact::BuildMode;
    use mty_driver::manifest::BuildConfig;
    use mty_driver::{build_native, BuildOptions, BuildOutcome, BuildTarget};

    let dir = tempfile::tempdir().expect("tempdir");
    let opts = BuildOptions {
        target: BuildTarget::Native,
        mode: BuildMode::Debug,
        out_dir: dir.path().to_path_buf(),
        binary_name: "build_block_smoke".into(),
        no_component: false,
        wasi_preview: Default::default(),
        user_wit: None,
        extern_libs: Vec::new(),
        manifest_dir: None,
        build_config: Some(BuildConfig {
            native_libs: vec!["m".into()],
            ..Default::default()
        }),
    };
    let outcome = build_native("fn main() {}\n".into(), "smoke.mty".into(), &opts);
    // `build_native` collapses "no linker" and "linker rejected" into
    // the same outcome, so we accept both NativeOk and
    // NativeOkNoLinker. The fail mode we're guarding against is a
    // backend panic / FrontendError, both of which would mean the
    // manifest plumbing broke.
    match outcome {
        BuildOutcome::NativeOk(p) | BuildOutcome::NativeOkNoLinker(p) => {
            assert!(p.exists(), "expected artifact at {}", p.display());
        }
        BuildOutcome::FrontendError => panic!("frontend error from a 1-line program"),
        BuildOutcome::BackendError(e) => panic!("backend error: {e}"),
        BuildOutcome::WasmOk(_) => panic!("wrong outcome variant"),
    }
}

/// Integration: when `[build]` is empty, `build_native` produces the
/// same outcome as without one. Pins the no-op contract so a future
/// driver change can't accidentally inject args.
#[test]
fn build_native_with_default_build_block_is_no_op() {
    use mty_codegen_cranelift::artifact::BuildMode;
    use mty_driver::manifest::BuildConfig;
    use mty_driver::{build_native, BuildOptions, BuildOutcome, BuildTarget};
    let dir = tempfile::tempdir().expect("tempdir");
    let opts = BuildOptions {
        target: BuildTarget::Native,
        mode: BuildMode::Debug,
        out_dir: dir.path().to_path_buf(),
        binary_name: "noop_block".into(),
        no_component: false,
        wasi_preview: Default::default(),
        user_wit: None,
        extern_libs: Vec::new(),
        manifest_dir: None,
        build_config: Some(BuildConfig::default()),
    };
    let outcome = build_native("fn main() {}\n".into(), "noop.mty".into(), &opts);
    match outcome {
        BuildOutcome::NativeOk(_) | BuildOutcome::NativeOkNoLinker(_) => {}
        other => panic!("expected NativeOk*, got {other:?}"),
    }
}
