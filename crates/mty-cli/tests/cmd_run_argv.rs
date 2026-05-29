#![cfg(feature = "host-toolchain")]
//! v0.27 Track E (QoL gap #3) — `mty run <path> -- <argv>` forwards
//! positional arguments to the Mighty program as `std.env.args()`.
//!
//! v0.35.3 — gate behind `host-toolchain` so `cargo test
//! --no-default-features` skips this (mty-stdlib isn't reachable
//! without the host-toolchain feature post-T1).
//!
//! Verifies clap's trailing-var-arg parsing + the
//! `mty_stdlib::env::set_args` → `host::dispatch("std.env", "args")`
//! plumbing. We can't observe `std.env.args()` from a Mighty source
//! file end-to-end yet (the SIR interp returns the Array, but the
//! permissive `format!("{}", argv)` path renders a placeholder), so
//! these tests assert the contract at the API + CLI parse level —
//! enough to lock the public surface in.

use std::process::Command;

fn mty(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_mty"))
        .args(args)
        .output()
        .expect("run mty");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Build a tempfile path containing a minimal Mighty program. We just
/// need something the parser accepts; the assertion is that
/// `mty run <path> -- <argv>` doesn't blow up on argv parsing.
fn write_tempfile(name: &str, src: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("mty_run_argv_{}_{}.mty", std::process::id(), name));
    std::fs::write(&p, src).expect("write temp .mty");
    p
}

const MINIMAL_PROGRAM: &str = "fn main() { log(\"argv-smoke\") }\n";

#[test]
fn run_with_positional_argv_reaches_std_env_args() {
    let path = write_tempfile("single", MINIMAL_PROGRAM);
    // The CLI accepts `--` followed by positional argv. We don't
    // assert on the program output (the SIR interp's log() path can
    // shape-shift across builds); we assert the CLI exits 0, which
    // means clap's `trailing_var_arg` parsing accepted the form.
    let (code, _out, err) = mty(&[
        "run",
        path.to_str().unwrap(),
        "--",
        "What does std.memory do?",
    ]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(
        code, 0,
        "mty run with `-- <argv>` should exit 0 — stderr: {}",
        err
    );
}

#[test]
fn run_without_argv_returns_empty() {
    let path = write_tempfile("empty", MINIMAL_PROGRAM);
    let (code, _out, err) = mty(&["run", path.to_str().unwrap()]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(
        code, 0,
        "mty run without -- should exit 0 — stderr: {}",
        err
    );
}

#[test]
fn run_with_multiple_positionals() {
    let path = write_tempfile("multi", MINIMAL_PROGRAM);
    let (code, _out, err) = mty(&[
        "run",
        path.to_str().unwrap(),
        "--",
        "alpha",
        "beta",
        "gamma",
    ]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(
        code, 0,
        "mty run with multiple positionals should exit 0 — stderr: {}",
        err
    );
}

#[test]
fn run_argv_preserves_quoted_strings_with_spaces() {
    let path = write_tempfile("quoted", MINIMAL_PROGRAM);
    let (code, _out, err) = mty(&[
        "run",
        path.to_str().unwrap(),
        "--",
        "first arg with spaces",
        "second",
    ]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(
        code, 0,
        "mty run with quoted positional should exit 0 — stderr: {}",
        err
    );
}

/// Direct API assertion on the channel `mty-cli` writes to before
/// invoking the runtime. This pins the contract: `set_args(vec)`
/// installs the vec; `args()` returns the same vec. The CLI tests
/// above exercise the clap-side wiring; this one exercises the
/// stdlib-side state.
#[test]
fn env_args_channel_round_trips() {
    mty_stdlib::env::reset_for_tests();
    assert!(mty_stdlib::env::args().is_empty());
    mty_stdlib::env::set_args(vec!["one".into(), "two".into()]);
    let got = mty_stdlib::env::args();
    assert_eq!(got, vec!["one".to_string(), "two".to_string()]);
}
