#![cfg(feature = "host-toolchain")]
//! v0.35 T2 — integration tests for `mty agent --record` and
//! `mty agent --replay`.
//!
//! Drives the binary in `--single-shot` mode under stdio so the tests
//! don't need a transport up. The recorder + replay logic is
//! transport-agnostic; the `agent_http.rs` and `agent_unix.rs` files
//! each cover one transport's recorder path.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn mty_bin() -> &'static str {
    env!("CARGO_BIN_EXE_mty")
}

fn run_record_single_shot(req: &str, rec_path: &std::path::Path) -> i32 {
    let mut child = Command::new(mty_bin())
        .args([
            "agent",
            "--single-shot",
            "--record",
            rec_path.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn record");
    {
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(req.as_bytes()).unwrap();
    }
    let out = child.wait_with_output().unwrap();
    out.status.code().unwrap_or(-1)
}

fn run_replay(rec_path: &std::path::Path) -> (i32, String, String) {
    let out = Command::new(mty_bin())
        .args(["agent", "--replay", rec_path.to_str().unwrap()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn replay");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

fn tmp_rec_path() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("rec.ndjson");
    (d, p)
}

// ===========================================================================
// Record
// ===========================================================================

#[test]
fn record_single_shot_explain_writes_entry() {
    let (_dir, path) = tmp_rec_path();
    let code = run_record_single_shot(r#"{"op":"explain","code":"MT0001"}"#, &path);
    assert_eq!(code, 0);
    let body = std::fs::read_to_string(&path).unwrap();
    let line = body.lines().next().expect("at least one line");
    let v: serde_json::Value = serde_json::from_str(line).unwrap();
    assert_eq!(v["request"], r#"{"op":"explain","code":"MT0001"}"#);
    assert!(v["response"]
        .as_str()
        .unwrap_or("")
        .contains("\"op\":\"explain\""));
    assert!(v["response"]
        .as_str()
        .unwrap_or("")
        .contains("\"kind\":\"done\""));
}

#[test]
fn record_creates_parent_directories() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("nested/sub/rec.ndjson");
    let code = run_record_single_shot(r#"{"op":"halt"}"#, &nested);
    assert_eq!(code, 0);
    assert!(nested.is_file());
}

#[test]
fn record_appends_existing_file() {
    let (_dir, path) = tmp_rec_path();
    std::fs::write(&path, "{\"request\":\"prior\",\"response\":\"prior\"}\n").unwrap();
    let _ = run_record_single_shot(r#"{"op":"halt"}"#, &path);
    let body = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"prior\""));
}

// ===========================================================================
// Replay round-trip
// ===========================================================================

#[test]
fn replay_recorded_session_matches() {
    let (_dir, path) = tmp_rec_path();
    let code = run_record_single_shot(r#"{"op":"explain","code":"MT0001"}"#, &path);
    assert_eq!(code, 0);
    let (rcode, _stdout, stderr) = run_replay(&path);
    assert_eq!(rcode, 0, "replay drift: {}", stderr);
    assert!(stderr.contains("all match"));
}

#[test]
fn replay_detects_drift() {
    let (_dir, path) = tmp_rec_path();
    // Hand-build an entry with a wrong response.
    let entry = serde_json::json!({
        "request": r#"{"op":"halt"}"#,
        "response": "{\"kind\":\"done\",\"exit_code\":42}\n",
    });
    std::fs::write(&path, entry.to_string() + "\n").unwrap();
    let (rcode, _stdout, stderr) = run_replay(&path);
    assert_eq!(rcode, 1);
    assert!(stderr.contains("drift"));
}

#[test]
fn replay_missing_file_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nope.ndjson");
    let (rcode, _stdout, stderr) = run_replay(&path);
    assert_eq!(rcode, 2);
    assert!(stderr.contains("read"));
}

#[test]
fn replay_malformed_file_exits_2() {
    let (_dir, path) = tmp_rec_path();
    std::fs::write(&path, "not json\n").unwrap();
    let (rcode, _stdout, stderr) = run_replay(&path);
    assert_eq!(rcode, 2);
    assert!(stderr.contains("not JSON"));
}
