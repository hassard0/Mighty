#![cfg(feature = "host-toolchain")]
//! v0.35 T3 — integration tests for `mty fix --apply`.
//!
//! These tests spawn the real binary to confirm the CLI surface
//! holds end-to-end. They complement the rich unit tests in
//! `crates/mty-cli/src/cmd/fix.rs` (which exercise the policy +
//! filtering logic without I/O).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn cargo_bin() -> PathBuf {
    let mut p = std::env::current_exe().unwrap();
    // .../target/debug/deps/<test>.exe → .../target/debug/mty[.exe]
    p.pop();
    p.pop();
    let exe = if cfg!(windows) { "mty.exe" } else { "mty" };
    p.push(exe);
    p
}

fn workspace_root() -> PathBuf {
    // .../crates/mty-cli/Cargo.toml → workspace root is two up.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates
    p.pop(); // workspace root
    p
}

/// The taint marquee example — known to produce one MT4099 envelope
/// with three untaint alternatives.
fn taint_example() -> PathBuf {
    workspace_root()
        .join("examples")
        .join("33_taint_basics.mty")
}

#[test]
fn mty_fix_requires_apply_flag() {
    let bin = cargo_bin();
    if !Path::new(&bin).exists() {
        eprintln!("skipping: mty binary not built (run `cargo build -p mty-cli`)");
        return;
    }
    let out = Command::new(&bin)
        .arg("fix")
        .arg(taint_example())
        .output()
        .expect("spawn");
    // No --apply → exit code 2 with a usage hint.
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--apply"),
        "stderr should mention --apply, got: {}",
        stderr
    );
}

#[test]
fn mty_fix_apply_dry_run_pipes_clean_diff() {
    let bin = cargo_bin();
    if !Path::new(&bin).exists() {
        eprintln!("skipping: mty binary not built (run `cargo build -p mty-cli`)");
        return;
    }
    // `mty check --format json examples/33_taint_basics.mty | mty fix --apply --from-stdin --dry-run`
    let mut check = Command::new(&bin)
        .args([
            "check",
            "--format",
            "json",
            taint_example().to_str().unwrap(),
        ])
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn check");
    let check_stdout = check.stdout.take().expect("check stdout");
    let fix = Command::new(&bin)
        .args(["fix", "--apply", "--from-stdin", "--dry-run"])
        .stdin(Stdio::from(check_stdout))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn fix");
    let _ = check.wait();
    let stdout = String::from_utf8_lossy(&fix.stdout);
    let stderr = String::from_utf8_lossy(&fix.stderr);
    assert_eq!(
        fix.status.code(),
        Some(0),
        "fix should succeed; stderr: {}",
        stderr
    );
    // Dry-run should emit a diff to stdout.
    assert!(
        stdout.contains("--- a/"),
        "expected unified diff in stdout, got: {}",
        stdout
    );
    // Stderr should mention the applied code.
    assert!(
        stderr.contains("MT4099"),
        "stderr should mention applied code; got: {}",
        stderr
    );
}

#[test]
fn mty_fix_apply_with_code_filter() {
    let bin = cargo_bin();
    if !Path::new(&bin).exists() {
        eprintln!("skipping: mty binary not built");
        return;
    }
    // Same pipe, but filter to a code that doesn't match → 0 applied.
    let mut check = Command::new(&bin)
        .args([
            "check",
            "--format",
            "json",
            taint_example().to_str().unwrap(),
        ])
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn check");
    let check_stdout = check.stdout.take().expect("check stdout");
    let fix = Command::new(&bin)
        .args([
            "fix",
            "--apply",
            "--from-stdin",
            "--dry-run",
            "--code",
            "MT9999",
        ])
        .stdin(Stdio::from(check_stdout))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn fix");
    let _ = check.wait();
    let stderr = String::from_utf8_lossy(&fix.stderr);
    assert_eq!(fix.status.code(), Some(0));
    assert!(
        stderr.contains("no fixes applied"),
        "stderr should report no fixes; got: {}",
        stderr
    );
}

#[test]
fn mty_fix_help_documents_flags() {
    let bin = cargo_bin();
    if !Path::new(&bin).exists() {
        eprintln!("skipping: mty binary not built");
        return;
    }
    let out = Command::new(&bin)
        .args(["fix", "--help"])
        .output()
        .expect("spawn");
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Confirm every documented flag is in --help.
    for flag in &[
        "--apply",
        "--code",
        "--alternative",
        "--threshold",
        "--dry-run",
        "--interactive",
        "--from-stdin",
    ] {
        assert!(
            stdout.contains(flag),
            "mty fix --help should document {}, got: {}",
            flag,
            stdout
        );
    }
}
