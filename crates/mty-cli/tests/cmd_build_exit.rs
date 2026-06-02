#![cfg(feature = "host-toolchain")]
//! v0.46 T2 (L50) — `mty build` exit-code + MTY_LINKER discovery tests.
//!
//! Covers:
//!   * When the requested target is native and no linker can be
//!     discovered, `mty build` MUST exit non-zero (was previously 0,
//!     silently shipping an object-only output that CI scripts could
//!     not catch).
//!   * `--emit obj` reinstates the historic "object-only is OK" path
//!     so dedicated CI flows that don't need a runnable executable
//!     don't break.
//!   * The discovery diagnostic surfaces every candidate that was
//!     tried (each env var + each PATH candidate name + outcome), so
//!     a misconfigured `MTY_LINKER` is debuggable from the build log
//!     alone.

use std::path::PathBuf;
use std::process::Command;

fn mty_bin() -> &'static str {
    env!("CARGO_BIN_EXE_mty")
}

fn write_tempfile(name: &str, src: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("mty_build_t2_{}_{}.mty", std::process::id(), name));
    std::fs::write(&p, src).expect("write temp .mty");
    p
}

/// Minimal PATH the child needs to load Windows system DLLs even when
/// the test wants to "empty out" PATH for linker discovery. Without
/// this, spawning the child hangs/loads forever waiting on DLLs that
/// live under `system32` on Windows. On non-Windows we accept that an
/// empty PATH genuinely is empty.
#[cfg(windows)]
fn minimal_path() -> String {
    let sysroot = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    format!("{sysroot}\\System32;{sysroot}")
}
#[cfg(not(windows))]
fn minimal_path() -> String {
    "/usr/bin:/bin".to_string()
}

fn run_build(args: &[&str], env: &[(&str, &str)], env_remove: &[&str]) -> (i32, String, String) {
    let mut cmd = Command::new(mty_bin());
    cmd.arg("build");
    for a in args {
        cmd.arg(a);
    }
    for k in env_remove {
        cmd.env_remove(k);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("spawn mty build");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// v0.46 T2 — native build without a linker must exit non-zero.
///
/// We point `MTY_LINKER` at a value that resolves nowhere and stub
/// `PATH` to an empty directory so the platform fallback can't rescue
/// it either. The build MUST exit non-zero and the diagnostic MUST
/// list every candidate it tried (the L50 ask).
#[test]
fn build_native_without_linker_exits_nonzero() {
    let out_dir = tempfile::tempdir().expect("tempdir");
    let src = write_tempfile("noexe", "fn main() {}\n");
    let out_arg = out_dir.path().display().to_string();
    let path = minimal_path();
    let (code, _stdout, stderr) = run_build(
        &[src.to_str().unwrap(), "--out-dir", &out_arg],
        &[
            ("MTY_LINKER", "definitely-not-a-real-linker-xyz"),
            ("PATH", &path),
        ],
        &["STARDUST_LINKER"],
    );
    assert_ne!(code, 0, "expected non-zero exit, stderr=\n{stderr}");
    assert!(
        stderr.contains("$MTY_LINKER"),
        "diagnostic should mention $MTY_LINKER, stderr=\n{stderr}"
    );
    // The diagnostic must spell out every PATH candidate (per L50:
    // "say what was actually tried"). Pick one platform-relevant
    // name to assert.
    let expected_cand = if cfg!(windows) { "clang.exe" } else { "clang" };
    assert!(
        stderr.contains(expected_cand),
        "diagnostic should mention PATH candidates, stderr=\n{stderr}"
    );
    assert!(
        stderr.contains("--emit obj"),
        "diagnostic should hint about --emit obj, stderr=\n{stderr}"
    );
}

/// v0.46 T2 — `--emit obj` reinstates the historic "object-only
/// success" exit code, so CI flows that genuinely don't need a
/// runnable executable still pass.
#[test]
fn build_native_with_emit_obj_succeeds_without_linker() {
    let out_dir = tempfile::tempdir().expect("tempdir");
    let src = write_tempfile("emitobj", "fn main() {}\n");
    let out_arg = out_dir.path().display().to_string();
    let path = minimal_path();
    let (code, stdout, stderr) = run_build(
        &[
            src.to_str().unwrap(),
            "--out-dir",
            &out_arg,
            "--emit",
            "obj",
        ],
        &[
            ("MTY_LINKER", "definitely-not-a-real-linker-xyz"),
            ("PATH", &path),
        ],
        &["STARDUST_LINKER"],
    );
    assert_eq!(
        code, 0,
        "expected --emit obj to succeed without a linker, stderr=\n{stderr}\nstdout=\n{stdout}"
    );
    assert!(
        stdout.contains("wrote object"),
        "expected object-only success message, stdout=\n{stdout}"
    );
    // The .o must be on disk.
    let obj = out_dir.path().join("mty_build_t2_obj.o");
    // The actual basename comes from the source file stem; just
    // verify some .o exists in the out dir.
    let any_o = std::fs::read_dir(out_dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| e.path().extension().and_then(|s| s.to_str()) == Some("o"));
    assert!(any_o, "expected an .o file in {}, missing", obj.display());
}

/// v0.46 T2 — `--emit` rejects unknown values up front (a typo
/// should not silently fall through to "build as native exe").
#[test]
fn build_rejects_unknown_emit_value() {
    let out_dir = tempfile::tempdir().expect("tempdir");
    let src = write_tempfile("emitbad", "fn main() {}\n");
    let out_arg = out_dir.path().display().to_string();
    let (code, _stdout, stderr) = run_build(
        &[
            src.to_str().unwrap(),
            "--out-dir",
            &out_arg,
            "--emit",
            "whatever",
        ],
        &[],
        &[],
    );
    assert_eq!(code, 2, "expected exit 2 for bad --emit, stderr=\n{stderr}");
    assert!(
        stderr.contains("unknown --emit value"),
        "expected emit-rejection diagnostic, stderr=\n{stderr}"
    );
}
