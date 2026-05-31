#![cfg(feature = "host-toolchain")]
//! v0.42 T5 — integration tests for `mty fmt` (L26 safety pass).
//!
//! Two behaviors are pinned here:
//!
//! 1. `mty fmt` is non-destructive on inputs the formatter cannot prove
//!    safe. Specifically, the v0.36 sharp-edge where pointing `mty fmt`
//!    at a 6480-byte plain-text file truncated it to 1 byte (because
//!    the parser recovered to an empty FILE tree with no diagnostics)
//!    must NOT happen. The formatter now refuses non-`.mty` extensions
//!    up-front and refuses to write when the input parses to an empty
//!    tree but contains non-whitespace bytes.
//!
//! 2. The existing pre-push hook invariant — `mty fmt --check` on every
//!    `.mty` file in `examples/` and `crates/mty-stdlib/src/` — keeps
//!    returning 0 after the safety pass. This test exercises the same
//!    invocations the hook runs (see `.git-hooks/pre-push`).

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
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates
    p.pop(); // workspace root
    p
}

fn run_fmt(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(cargo_bin())
        .arg("fmt")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("spawn mty fmt");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn run_fmt_stdin(args: &[&str], stdin_bytes: &[u8]) -> (i32, String, String) {
    use std::io::Write;
    let mut child = Command::new(cargo_bin())
        .arg("fmt")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mty fmt --stdin");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(stdin_bytes)
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn write_temp(name: &str, contents: &[u8]) -> PathBuf {
    // `keep()` returns the PathBuf and leaks the TempDir on purpose: the
    // tests inspect the file after the process exits, and `tempdir()`'s
    // RAII would delete the directory the moment the helper returned.
    let dir = tempfile::tempdir().unwrap().keep();
    let p = dir.join(name);
    std::fs::write(&p, contents).unwrap();
    p
}

fn size(p: &Path) -> u64 {
    std::fs::metadata(p).unwrap().len()
}

// ---------------------------------------------------------------------
// L26 destructive-truncation fix — non-`.mty` extension is refused.
// ---------------------------------------------------------------------

#[test]
fn mty_fmt_refuses_non_mty_extension_and_preserves_file_size() {
    // 100-ish bytes of definitely-not-Mighty text.
    let payload = b"This is a plain text file.\nIt has multiple lines.\nNot Mighty code at all.\nSome chars: <>?{}()*&^\n";
    let p = write_temp("long.txt", payload);
    let before = size(&p);
    assert_eq!(before, payload.len() as u64);

    let (code, _stdout, stderr) = run_fmt(&[p.to_str().unwrap()]);

    assert_ne!(code, 0, "mty fmt on a .txt MUST exit non-zero");
    let after = size(&p);
    assert_eq!(
        after, before,
        "mty fmt on a .txt MUST NOT change the file size (was {before}, now {after})"
    );
    assert!(
        stderr.to_lowercase().contains(".mty"),
        "stderr should explain only .mty files are accepted; got: {stderr}"
    );
}

#[test]
fn mty_fmt_refuses_non_mty_extension_even_with_check() {
    let payload = b"plain text again, with --check this time.\n";
    let p = write_temp("notes.txt", payload);
    let (code, _stdout, _stderr) = run_fmt(&["--check", p.to_str().unwrap()]);
    assert_ne!(code, 0, "--check on a .txt MUST exit non-zero");
    assert_eq!(size(&p), payload.len() as u64);
}

// ---------------------------------------------------------------------
// L26 parse-failure guard — broken .mty is refused, file untouched.
// ---------------------------------------------------------------------

#[test]
fn mty_fmt_refuses_to_write_on_parse_failure() {
    let src = b"fn ( {\n}\n"; // malformed
    let p = write_temp("bad.mty", src);
    let before = std::fs::read(&p).unwrap();

    let (code, _stdout, stderr) = run_fmt(&[p.to_str().unwrap()]);

    assert_ne!(code, 0, "mty fmt on malformed .mty MUST exit non-zero");
    let after = std::fs::read(&p).unwrap();
    assert_eq!(before, after, "file MUST be unchanged on parse failure");
    assert!(
        stderr.to_lowercase().contains("parse"),
        "stderr should mention parse failure; got: {stderr}"
    );
}

#[test]
fn mty_fmt_check_refuses_on_parse_failure() {
    let src = b"fn ( {\n}\n";
    let p = write_temp("bad2.mty", src);
    let before = std::fs::read(&p).unwrap();
    let (code, _stdout, _stderr) = run_fmt(&["--check", p.to_str().unwrap()]);
    assert_ne!(code, 0, "--check on malformed .mty MUST exit non-zero");
    assert_eq!(before, std::fs::read(&p).unwrap());
}

// ---------------------------------------------------------------------
// Valid canonical .mty: --check exits 0.
// ---------------------------------------------------------------------

#[test]
fn mty_fmt_check_passes_on_canonical_file() {
    // The simplest canonical Mighty program: single fn, exactly one
    // trailing newline. Matches what the formatter would emit for this
    // shape.
    let src = b"fn main() {\n  log(\"hi\")\n}\n";
    let p = write_temp("ok.mty", src);
    let (code, _stdout, stderr) = run_fmt(&["--check", p.to_str().unwrap()]);
    assert_eq!(
        code, 0,
        "--check on canonical .mty MUST exit 0; stderr={stderr}"
    );
}

// ---------------------------------------------------------------------
// stdin path: parse-failure refusal still applies.
// ---------------------------------------------------------------------

#[test]
fn mty_fmt_stdin_refuses_on_parse_failure() {
    let (code, stdout, stderr) = run_fmt_stdin(&["--stdin"], b"fn ( {\n}\n");
    assert_ne!(code, 0, "--stdin on malformed source MUST exit non-zero");
    assert!(
        stdout.is_empty(),
        "stdout MUST be empty on parse failure; got {stdout:?}"
    );
    assert!(
        stderr.to_lowercase().contains("parse"),
        "stderr should mention parse failure; got {stderr:?}"
    );
}

#[test]
fn mty_fmt_stdin_passes_canonical_through() {
    let src = "fn main() {\n  log(\"hi\")\n}\n";
    let (code, stdout, _stderr) = run_fmt_stdin(&["--stdin"], src.as_bytes());
    assert_eq!(code, 0);
    assert_eq!(stdout, src);
}

// ---------------------------------------------------------------------
// Existing pre-push hook invariant: --check on examples/ and stdlib/.
// ---------------------------------------------------------------------

#[test]
fn mty_fmt_check_passes_on_repo_examples_and_stdlib() {
    let root = workspace_root();
    let examples = root.join("examples");
    let stdlib = root.join("crates").join("mty-stdlib").join("src");
    let args = [examples.to_str().unwrap(), stdlib.to_str().unwrap()];
    let (code, stdout, stderr) = run_fmt(&["--check", args[0], args[1]]);
    assert_eq!(
        code, 0,
        "pre-push hook invariant broken: `mty fmt --check examples/ crates/mty-stdlib/src/` exited {code}.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
