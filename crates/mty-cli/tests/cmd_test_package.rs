#![cfg(feature = "host-toolchain")]
//! v0.41 T2 — multi-file package resolution conformance.
//!
//! These tests exercise the L13-blocking surface (`mty test` against
//! a package whose `tests/` files `use lib.{fn}` of a sibling `src/`
//! module). Before v0.41 T2, `mty test` parsed each test file in
//! isolation and the `use` resolved to a silent default — making
//! every assertion against a sibling module pass-by-accident or
//! fail-by-noise. The fix assembles every `src/**/*.mty` + the test
//! file into one HIR `Package` so name resolution sees the real
//! definitions.
//!
//! Three positive paths + two error-surface paths:
//!   - `multi_file_package_resolves_sibling_module_call` — the
//!     canonical L13 reproducer: `src/lib.mty + src/util.mty +
//!     tests/integration.mty` where the test calls fns from both
//!     modules; passes only if the assembled package sees them.
//!   - `test_helper_in_src_is_not_dispatched_as_test` — a fn named
//!     `test_helper` defined in `src/` shouldn't be run as a test
//!     just because its name starts with `test_`; only fns lowered
//!     from the test file itself dispatch.
//!   - `missing_module_surfaces_MT2029` — `use ghost.{...}` of a
//!     nonexistent module should fail with a clear error instead of
//!     silently no-op'ing.
//!   - `missing_symbol_surfaces_MT2030` — `use lib.{typo}` of a real
//!     module but missing symbol should likewise fail with MT2030.
//!   - `standalone_tests_dir_keeps_legacy_behavior` — `mty test` on a
//!     plain `tests/` directory (no surrounding `mighty.toml`) keeps
//!     the pre-v0.41 single-file shape so the existing stdlib unit
//!     test runner doesn't regress.

use std::path::Path;
use std::process::Command;

fn mty(cwd: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_mty"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run mty");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(p, body).unwrap();
}

fn manifest(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2026\"\n")
}

#[test]
fn multi_file_package_resolves_sibling_module_call() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write(root, "mighty.toml", &manifest("ml13"));
    write(
        root,
        "src/lib.mty",
        "pub fn answer() -> I32 { return 42; }\n",
    );
    write(
        root,
        "src/util.mty",
        "pub fn double(x: I32) -> I32 { return x + x; }\n",
    );
    write(
        root,
        "tests/integration.mty",
        "use lib.{answer};\n\
         use util.{double};\n\
         \n\
         fn test_uses_both_modules() {\n\
           if answer() != 42 { panic(\"answer wrong\"); }\n\
           if double(21) != 42 { panic(\"double wrong\"); }\n\
         }\n",
    );
    let (code, stdout, stderr) = mty(root, &["test"]);
    assert_eq!(
        code, 0,
        "expected exit 0 (resolved against package), got {code}.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("ok"),
        "expected an `ok` line — stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("1 passed; 0 failed"),
        "expected summary line — stdout:\n{stdout}"
    );
}

#[test]
fn test_helper_in_src_is_not_dispatched_as_test() {
    // A `pub fn test_helper(...)` in `src/` shouldn't be auto-run as
    // a test once the package gets merged. Only fns from the
    // tests/ file dispatch — verified by deliberately failing
    // `test_helper` and expecting the run to still pass.
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write(root, "mighty.toml", &manifest("ml13helper"));
    write(
        root,
        "src/lib.mty",
        "pub fn answer() -> I32 { return 42; }\n\
         pub fn test_helper() { panic(\"never run\"); }\n",
    );
    write(
        root,
        "tests/integration.mty",
        "use lib.{answer};\n\
         \n\
         fn test_uses_module() {\n\
           if answer() != 42 { panic(\"wrong\"); }\n\
         }\n",
    );
    let (code, stdout, stderr) = mty(root, &["test"]);
    assert_eq!(
        code, 0,
        "expected exit 0, got {code}.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stdout.contains("test_helper"),
        "src/ helper should not have dispatched — stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("1 passed; 0 failed"),
        "expected one test passed — stdout:\n{stdout}"
    );
}

#[test]
fn missing_module_surfaces_mt2029() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write(root, "mighty.toml", &manifest("ml13ghost"));
    write(
        root,
        "src/lib.mty",
        "pub fn answer() -> I32 { return 42; }\n",
    );
    write(
        root,
        "tests/integration.mty",
        "use ghost.{whatever};\n\
         \n\
         fn test_anything() {}\n",
    );
    let (code, stdout, _stderr) = mty(root, &["test"]);
    assert_eq!(code, 1, "expected exit 1 (MT2029 surfaced)");
    assert!(
        stdout.contains("no module named `ghost`"),
        "expected MT2029 message — stdout:\n{stdout}"
    );
}

#[test]
fn missing_symbol_surfaces_mt2030() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write(root, "mighty.toml", &manifest("ml13typo"));
    write(
        root,
        "src/lib.mty",
        "pub fn answer() -> I32 { return 42; }\n",
    );
    write(
        root,
        "tests/integration.mty",
        "use lib.{answr};\n\
         \n\
         fn test_anything() {}\n",
    );
    let (code, stdout, _stderr) = mty(root, &["test"]);
    assert_eq!(code, 1, "expected exit 1 (MT2030 surfaced)");
    assert!(
        stdout.contains("symbol `answr` not found in module `lib`"),
        "expected MT2030 message — stdout:\n{stdout}"
    );
}

#[test]
fn standalone_tests_dir_keeps_legacy_behavior() {
    // `mty test` without a manifest stays in the pre-v0.41 single-file
    // shape (the existing `crates/mty-stdlib/tests/test_runner.rs`
    // fixture relies on it). Verify by dropping a `tests/` dir into
    // a manifest-less working dir.
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    // No mighty.toml.
    write(
        root,
        "tests/a.mty",
        "fn test_standalone() {\n\
           if 1 + 1 != 2 { panic(\"math broke\"); }\n\
         }\n",
    );
    let (code, stdout, stderr) = mty(root, &["test"]);
    assert_eq!(
        code, 0,
        "exit 0 expected, got {code}.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("1 passed; 0 failed"),
        "expected summary line — stdout:\n{stdout}"
    );
}
