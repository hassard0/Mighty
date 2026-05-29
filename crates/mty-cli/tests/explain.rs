#![cfg(feature = "host-toolchain")]
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

#[test]
fn explain_known_code_succeeds() {
    let (code, stdout, _stderr) = mty(&["explain", "MT0001"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Unexpected token"), "stdout: {}", stdout);
}

#[test]
fn explain_lowercase_prefix_works() {
    let (code, stdout, _) = mty(&["explain", "sd0010"]);
    assert_eq!(code, 0);
    assert!(
        stdout.to_lowercase().contains("expected an item"),
        "stdout: {}",
        stdout
    );
}

#[test]
fn explain_bare_number_works() {
    let (code, stdout, _) = mty(&["explain", "1001"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Unresolved name"), "stdout: {}", stdout);
}

#[test]
fn explain_unknown_code_fails() {
    let (code, _stdout, stderr) = mty(&["explain", "MT9999"]);
    assert_eq!(code, 1);
    assert!(
        stderr.to_lowercase().contains("unknown"),
        "stderr: {}",
        stderr
    );
}

#[test]
fn explain_bad_format_fails() {
    let (code, _stdout, stderr) = mty(&["explain", "wat"]);
    assert_eq!(code, 2);
    assert!(
        stderr.to_lowercase().contains("expected"),
        "stderr: {}",
        stderr
    );
}
