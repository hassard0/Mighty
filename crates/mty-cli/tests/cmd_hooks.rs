#![cfg(feature = "host-toolchain")]
//! v0.37 T1 — tests for `mty hooks` + the source `.git-hooks/pre-push`
//! script.
//!
//! These tests defend the *content* of the pre-push hook, not just the
//! install/uninstall plumbing. v0.36.1 needed two retags + two main
//! fixes because tracks pushed `.mty` drift that only `mty fmt --check`
//! on Linux catches — the v0.34 T4 hook (`cargo fmt --check` + `cargo
//! clippy`) didn't see it. The hook was strengthened in v0.37 T1 to
//! also run `mty fmt --check` on the known `examples/*.mty`,
//! `demos/*/src/*.mty`, and `tools/gallery/examples/*/main.mty` paths.
//!
//! If a future change deletes one of the three checks from the hook
//! script, CI catches it here. If someone removes the hook file from
//! the repo entirely, CI catches that too (the test fails at the
//! `path exists` assertion).
//!
//! The `mty hooks status` integration test also asserts that the
//! installed hook the binary reports matches the source script's
//! content byte-for-byte, so a "drifted-installed-hook" scenario is
//! caught by a normal `cargo test` run on a developer machine.

use std::path::{Path, PathBuf};
use std::process::Command;

use mty_cli::cmd::hooks::find_repo_root;

/// Walk up from the test process's cwd to the repo root (the directory
/// that contains `.git-hooks/pre-push`). The `mty_cli::cmd::hooks`
/// `find_repo_root` helper looks for `.git`, which works inside a
/// worktree, a normal checkout, and a CI clone alike.
fn repo_root() -> PathBuf {
    find_repo_root().expect("test must run from inside the Mighty repo")
}

#[test]
fn pre_push_hook_script_exists() {
    let p = repo_root().join(".git-hooks").join("pre-push");
    assert!(
        p.exists(),
        ".git-hooks/pre-push must exist (deleting it removes the cheapest pre-push gate; see v0.34 T4 + v0.37 T1)"
    );
}

#[test]
fn pre_push_hook_runs_cargo_fmt_check() {
    let body = read_hook();
    assert!(
        body.contains("cargo fmt --all -- --check"),
        "pre-push hook must run `cargo fmt --all -- --check` (v0.34 T4 contract); body was:\n{body}"
    );
}

#[test]
fn pre_push_hook_runs_cargo_clippy_strict() {
    let body = read_hook();
    assert!(
        body.contains("cargo clippy --workspace --all-targets -- -D warnings"),
        "pre-push hook must run the strict clippy gate (v0.34 T4 contract); body was:\n{body}"
    );
}

#[test]
fn pre_push_hook_runs_mty_fmt_check() {
    let body = read_hook();
    assert!(
        body.contains("mty fmt --check"),
        "pre-push hook must run `mty fmt --check` on known .mty paths (v0.37 T1 contract — stops the v0.36.1-style retag cycle); body was:\n{body}"
    );
}

#[test]
fn pre_push_hook_sweeps_examples_demos_gallery() {
    // The hook is responsible for catching drift in the three known
    // .mty surface paths: top-level examples, per-demo src, and the
    // gallery examples. If a future PR moves any of these under a new
    // tree, this test forces the hook to be updated in lockstep.
    let body = read_hook();
    for pat in [
        "examples/*.mty",
        "demos/*/src/*.mty",
        "tools/gallery/examples/*/main.mty",
    ] {
        assert!(
            body.contains(pat),
            "pre-push hook must sweep `{pat}` for .mty drift (v0.37 T1); body was:\n{body}"
        );
    }
}

#[test]
fn pre_push_hook_honours_skip_env() {
    // MTY_PRE_PUSH_SKIP=1 must still bypass the hook — humans use this
    // for docs-only branches where building mty-cli on a fresh clone
    // would be punitive. v0.34 T4 contract, carried into v0.37 T1.
    let body = read_hook();
    assert!(
        body.contains("MTY_PRE_PUSH_SKIP"),
        "pre-push hook must honour MTY_PRE_PUSH_SKIP=1 escape hatch; body was:\n{body}"
    );
}

#[test]
fn pre_push_hook_sentinel_unchanged() {
    // The sentinel is what `mty hooks install` uses to identify a
    // previously-installed Mighty hook (so re-running install
    // overwrites our own hook but refuses to clobber a hand-written
    // one). Changing the sentinel breaks every existing
    // contributor's install and forces them to pass `--force`.
    let body = read_hook();
    assert!(
        body.contains("Mighty pre-push hook — v0.34 T4."),
        "sentinel string must stay `Mighty pre-push hook — v0.34 T4.` (changing it breaks `mty hooks install` idempotence; bump deliberately if needed and document in docs/contributing.md); body was:\n{body}"
    );
}

#[test]
fn mty_hooks_status_reports_installed_hook() {
    // Integration: run `mty hooks install` then `mty hooks status` in a
    // synthetic repo and assert status sees the hook. We can't use the
    // worktree's own .git/hooks (that would clobber the developer's
    // installed hook), so we shell out in a fresh tempdir laid out as
    // a minimal repo with the source `.git-hooks/pre-push`.
    let root = repo_root();
    let source_hook = root.join(".git-hooks").join("pre-push");
    let source_body = std::fs::read_to_string(&source_hook).expect("read source hook");

    let tmp = fresh_tmpdir("mty_hooks_status");
    std::fs::create_dir_all(tmp.join(".git").join("hooks")).expect("mkdir .git/hooks");
    std::fs::create_dir_all(tmp.join(".git-hooks")).expect("mkdir .git-hooks");
    std::fs::write(tmp.join(".git-hooks").join("pre-push"), &source_body)
        .expect("write source hook into tmp");

    // mty hooks install (run from inside the tempdir).
    let install = mty(&tmp, &["hooks", "install"]);
    assert_eq!(install.0, 0, "install exit code: {install:?}");

    // Confirm the hook landed and matches the source.
    let installed = std::fs::read_to_string(tmp.join(".git").join("hooks").join("pre-push"))
        .expect("read installed hook");
    assert_eq!(
        installed, source_body,
        "installed hook content must match `.git-hooks/pre-push` byte-for-byte"
    );

    // mty hooks status should now report the installed hook.
    let status = mty(&tmp, &["hooks", "status"]);
    assert_eq!(status.0, 0, "status exit code: {status:?}");
    assert!(
        status.1.contains("Mighty pre-push hook installed"),
        "status stdout must report the installed hook; got:\nstdout: {}\nstderr: {}",
        status.1,
        status.2,
    );
}

#[test]
fn mty_hooks_install_is_idempotent() {
    // Re-running `mty hooks install` over our own hook must succeed
    // without --force (v0.37 T1: when the hook script body changes,
    // every contributor will re-run install to pick up the new
    // checks; a force-required path would be silent surface area for
    // "I forgot to update my hook").
    let root = repo_root();
    let source_body = std::fs::read_to_string(root.join(".git-hooks").join("pre-push"))
        .expect("read source hook");

    let tmp = fresh_tmpdir("mty_hooks_idem");
    std::fs::create_dir_all(tmp.join(".git").join("hooks")).expect("mkdir .git/hooks");
    std::fs::create_dir_all(tmp.join(".git-hooks")).expect("mkdir .git-hooks");
    std::fs::write(tmp.join(".git-hooks").join("pre-push"), &source_body)
        .expect("write source hook into tmp");

    let first = mty(&tmp, &["hooks", "install"]);
    assert_eq!(first.0, 0, "first install: {first:?}");
    let second = mty(&tmp, &["hooks", "install"]);
    assert_eq!(
        second.0, 0,
        "second install (no --force) must succeed: {second:?}"
    );
}

// ---------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------

fn read_hook() -> String {
    let p = repo_root().join(".git-hooks").join("pre-push");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("failed to read {}: {e}", p.display()))
}

fn mty(cwd: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_mty"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run mty");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn fresh_tmpdir(label: &str) -> PathBuf {
    let mut d = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    d.push(format!("mty-{label}-{nanos}-{}", std::process::id()));
    std::fs::create_dir_all(&d).expect("mkdir tmpdir");
    d
}
