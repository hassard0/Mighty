#![cfg(feature = "host-toolchain")]

use std::process::Command;

#[test]
fn mty_version_reports_language_milestone() {
    let out = Command::new(env!("CARGO_BIN_EXE_mty"))
        .arg("--version")
        .output()
        .expect("run mty --version");
    assert!(out.status.success(), "mty --version should succeed");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(mty_cli::MIGHTY_VERSION),
        "expected version output to contain {}, got {stdout:?}",
        mty_cli::MIGHTY_VERSION
    );
    assert!(
        !stdout.contains("0.1.0"),
        "public CLI version should not expose the internal crate version: {stdout:?}"
    );
}
