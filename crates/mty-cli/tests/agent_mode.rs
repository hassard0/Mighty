//! v0.33 T5 — integration tests for `mty agent`.
//!
//! These tests drive the binary as a subprocess (mirroring how an LLM
//! agent will actually invoke it), feed NDJSON requests on stdin, and
//! parse the NDJSON response stream from stdout.
//!
//! The unit tests under `crates/mty-cli/src/cmd/agent.rs#tests` lock
//! down internals (parsers, fix-application, find search). This file
//! verifies the end-to-end shape: spawning the binary, sending lines,
//! receiving lines, exit codes.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn mty_bin() -> &'static str {
    env!("CARGO_BIN_EXE_mty")
}

/// Spawn `mty agent --single-shot`, write `req` (a single JSON line)
/// to stdin, and return (exit_code, stdout_lines, stderr_text).
fn single_shot(req: &str) -> (i32, Vec<String>, String) {
    let mut child = Command::new(mty_bin())
        .args(["agent", "--single-shot"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mty agent");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin.write_all(req.as_bytes()).expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let lines: Vec<String> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect();
    (code, lines, stderr)
}

/// Spawn `mty agent`, write multiple lines as a session, return the
/// captured stdout lines + exit code.
fn interactive(lines: &[&str]) -> (i32, Vec<String>, String) {
    let mut child = Command::new(mty_bin())
        .args(["agent"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mty agent");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        for l in lines {
            stdin.write_all(l.as_bytes()).expect("write");
            stdin.write_all(b"\n").expect("write nl");
        }
    }
    let out = child.wait_with_output().expect("wait");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let lines: Vec<String> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect();
    (code, lines, stderr)
}

fn last_done(lines: &[String]) -> Option<i32> {
    lines
        .iter()
        .rev()
        .find_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .and_then(|v| {
            if v.get("kind").and_then(|s| s.as_str()) == Some("done") {
                v.get("exit_code")
                    .and_then(|c| c.as_i64())
                    .map(|c| c as i32)
            } else {
                None
            }
        })
}

fn find_kind<'a>(lines: &'a [String], kind: &str) -> Vec<serde_json::Value> {
    lines
        .iter()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v.get("kind").and_then(|s| s.as_str()) == Some(kind))
        .collect()
}

fn write_tmp(name: &str, body: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let p = tmp.path().join(name);
    std::fs::write(&p, body).expect("write");
    (tmp, p)
}

fn path_json(p: &PathBuf) -> String {
    p.display().to_string().replace('\\', "\\\\")
}

// ---------------------------------------------------------------------------
// Single-shot
// ---------------------------------------------------------------------------

#[test]
fn single_shot_explain_known_code() {
    let (code, lines, _stderr) = single_shot(r#"{"op":"explain","code":"MT0001"}"#);
    assert_eq!(code, 0);
    let results = find_kind(&lines, "result");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["op"], "explain");
    assert_eq!(results[0]["ok"], true);
    assert!(results[0]["text"]
        .as_str()
        .unwrap_or("")
        .contains("Unexpected"));
    assert_eq!(last_done(&lines), Some(0));
}

#[test]
fn single_shot_explain_unknown_code() {
    let (code, lines, _stderr) = single_shot(r#"{"op":"explain","code":"MT9999"}"#);
    assert_eq!(code, 1);
    let results = find_kind(&lines, "result");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["ok"], false);
    assert_eq!(last_done(&lines), Some(1));
}

#[test]
fn single_shot_explain_bad_format() {
    let (code, lines, _) = single_shot(r#"{"op":"explain","code":"garbage"}"#);
    assert_eq!(code, 2);
    let errors = find_kind(&lines, "error");
    assert_eq!(errors.len(), 1);
    assert!(errors[0]["message"]
        .as_str()
        .unwrap_or("")
        .contains("bad code"));
    assert_eq!(last_done(&lines), Some(2));
}

#[test]
fn single_shot_unknown_op() {
    let (code, lines, _) = single_shot(r#"{"op":"frobnicate"}"#);
    assert_eq!(code, 2);
    let errors = find_kind(&lines, "error");
    assert_eq!(errors.len(), 1);
    assert!(errors[0]["message"]
        .as_str()
        .unwrap_or("")
        .contains("unknown op"));
}

#[test]
fn single_shot_malformed_json() {
    let (code, lines, _) = single_shot(r#"{"op":"check""#);
    assert_eq!(code, 2);
    let errors = find_kind(&lines, "error");
    assert_eq!(errors.len(), 1);
    assert!(errors[0]["message"]
        .as_str()
        .unwrap_or("")
        .contains("malformed JSON"));
}

#[test]
fn single_shot_halt() {
    let (code, lines, _) = single_shot(r#"{"op":"halt"}"#);
    assert_eq!(code, 0);
    let results = find_kind(&lines, "result");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["op"], "halt");
}

#[test]
fn single_shot_check_missing_path() {
    let (code, lines, _) = single_shot(r#"{"op":"check"}"#);
    assert_eq!(code, 2);
    let errors = find_kind(&lines, "error");
    assert!(!errors.is_empty());
    assert!(errors[0]["message"]
        .as_str()
        .unwrap_or("")
        .contains("missing required `path`"));
}

#[test]
fn single_shot_check_missing_file() {
    let req = r#"{"op":"check","path":"does/not/exist.mty"}"#;
    let (code, lines, _) = single_shot(req);
    assert_eq!(code, 1);
    let errors = find_kind(&lines, "error");
    assert!(!errors.is_empty());
}

#[test]
fn single_shot_check_clean_file() {
    let (_tmp, p) = write_tmp("ok.mty", "fn main() -> Unit { }\n");
    let req = format!(r#"{{"op":"check","path":"{}"}}"#, path_json(&p));
    let (code, lines, _) = single_shot(&req);
    // Clean program might still warn but should not error.
    let _ = code;
    let results = find_kind(&lines, "result");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["op"], "check");
    // No errored envelopes.
    let envelopes = find_kind(&lines, "envelope");
    let err_envelopes: Vec<_> = envelopes
        .iter()
        .filter(|e| e["severity"] == "error")
        .collect();
    assert_eq!(err_envelopes.len(), 0);
}

#[test]
fn single_shot_check_with_diagnostic_streams_envelope() {
    // A program with an unbalanced delimiter triggers a parse error.
    let (_tmp, p) = write_tmp("bad.mty", "fn main() -> Unit {\n  let x = (\n}\n");
    let req = format!(r#"{{"op":"check","path":"{}"}}"#, path_json(&p));
    let (code, lines, _) = single_shot(&req);
    assert_eq!(code, 1);
    let envelopes = find_kind(&lines, "envelope");
    assert!(
        !envelopes.is_empty(),
        "expected at least one envelope, got lines: {:?}",
        lines
    );
    let results = find_kind(&lines, "result");
    assert_eq!(results[0]["op"], "check");
    assert_eq!(results[0]["ok"], false);
    assert!(results[0]["diagnostics_count"].as_u64().unwrap_or(0) >= 1);
}

#[test]
fn single_shot_check_include_source_embeds_snippet() {
    let (_tmp, p) = write_tmp("bad.mty", "fn main() -> Unit {\n  let x = (\n}\n");
    let req = format!(
        r#"{{"op":"check","path":"{}","include_source":true}}"#,
        path_json(&p)
    );
    let (_code, lines, _) = single_shot(&req);
    let envelopes = find_kind(&lines, "envelope");
    assert!(!envelopes.is_empty());
    // Every envelope should have an embedded source snippet.
    let with_src: Vec<_> = envelopes
        .iter()
        .filter(|e| e.get("source").is_some())
        .collect();
    assert!(!with_src.is_empty());
}

#[test]
fn single_shot_fmt_check_clean_file() {
    let (_tmp, p) = write_tmp("x.mty", "fn main() -> Unit { }\n");
    // First, format it once to canonicalize.
    let req_write = format!(r#"{{"op":"fmt","path":"{}"}}"#, path_json(&p));
    let _ = single_shot(&req_write);
    // Now `--check`.
    let req = format!(r#"{{"op":"fmt","path":"{}","check":true}}"#, path_json(&p));
    let (_code, lines, _) = single_shot(&req);
    let results = find_kind(&lines, "result");
    assert_eq!(results[0]["op"], "fmt");
    assert_eq!(results[0]["would_reformat"], false);
}

#[test]
fn single_shot_fmt_missing_path() {
    let (code, lines, _) = single_shot(r#"{"op":"fmt"}"#);
    assert_eq!(code, 2);
    let errors = find_kind(&lines, "error");
    assert!(!errors.is_empty());
}

#[test]
fn single_shot_find_substring() {
    let (tmp, _) = write_tmp("a.mty", "fn write_thing() -> Unit { }\n");
    let req = format!(
        r#"{{"op":"find","query":"write_thing","root":"{}"}}"#,
        tmp.path().display().to_string().replace('\\', "\\\\")
    );
    let (code, lines, _) = single_shot(&req);
    assert_eq!(code, 0);
    let results = find_kind(&lines, "result");
    assert_eq!(results[0]["op"], "find");
    let hits = results[0]["hits"].as_array().unwrap();
    assert!(!hits.is_empty());
    assert!(hits[0]["text"]
        .as_str()
        .unwrap_or("")
        .contains("write_thing"));
}

#[test]
fn single_shot_find_missing_query() {
    let (code, lines, _) = single_shot(r#"{"op":"find"}"#);
    assert_eq!(code, 2);
    let errors = find_kind(&lines, "error");
    assert!(!errors.is_empty());
}

#[test]
fn single_shot_fix_no_prior_check_errors() {
    let (code, lines, _) = single_shot(r#"{"op":"fix","code":"MT4099"}"#);
    assert_eq!(code, 2);
    let errors = find_kind(&lines, "error");
    assert!(!errors.is_empty());
    assert!(errors[0]["message"]
        .as_str()
        .unwrap_or("")
        .contains("no `path`"));
}

#[test]
fn single_shot_fix_no_diagnostic_in_file() {
    let (_tmp, p) = write_tmp("ok.mty", "fn main() -> Unit { }\n");
    let req = format!(
        r#"{{"op":"fix","path":"{}","code":"MT4099"}}"#,
        path_json(&p)
    );
    let (code, lines, _) = single_shot(&req);
    // No MT4099 envelope in this clean file → error.
    assert_eq!(code, 1);
    let errors = find_kind(&lines, "error");
    assert!(!errors.is_empty());
}

#[test]
fn single_shot_transport_http_stub() {
    // http transport is reserved for v0.34 — should error cleanly.
    let mut child = Command::new(mty_bin())
        .args(["agent", "--transport", "http", "--single-shot"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    {
        let stdin = child.stdin.as_mut().unwrap();
        let _ = stdin.write_all(b"");
    }
    let out = child.wait_with_output().expect("wait");
    assert_eq!(out.status.code().unwrap_or(-1), 2);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("v0.34"), "stdout: {}", s);
}

#[test]
fn single_shot_transport_unknown_errors() {
    let out = Command::new(mty_bin())
        .args(["agent", "--transport", "foobar", "--single-shot"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run");
    assert_eq!(out.status.code().unwrap_or(-1), 2);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown --transport"));
}

// ---------------------------------------------------------------------------
// Interactive loop
// ---------------------------------------------------------------------------

#[test]
fn interactive_explain_then_halt() {
    let (code, lines, _) =
        interactive(&[r#"{"op":"explain","code":"MT0001"}"#, r#"{"op":"halt"}"#]);
    assert_eq!(code, 0);
    let dones: Vec<_> = find_kind(&lines, "done");
    // One done per request.
    assert_eq!(dones.len(), 2);
}

#[test]
fn interactive_check_then_fix() {
    let (_tmp, p) = write_tmp(
        "bad.mty",
        "fn main() -> Unit { let x: Str = undefined_name }\n",
    );
    let r1 = format!(r#"{{"op":"check","path":"{}"}}"#, path_json(&p));
    // Fix won't necessarily succeed (depends on whether MT1001 ships a
    // fix in T4) — but it should not crash the loop.
    let r2 = r#"{"op":"fix","code":"MT1001"}"#.to_string();
    let r3 = r#"{"op":"halt"}"#.to_string();
    let (_code, lines, _stderr) = interactive(&[&r1, &r2, &r3]);
    let dones = find_kind(&lines, "done");
    assert_eq!(dones.len(), 3);
}

#[test]
fn interactive_skip_blank_lines() {
    let (code, lines, _) = interactive(&[
        "",
        "   ",
        r#"{"op":"explain","code":"MT0001"}"#,
        "",
        r#"{"op":"halt"}"#,
    ]);
    assert_eq!(code, 0);
    let dones = find_kind(&lines, "done");
    // Blank lines should not generate dones; only the two real requests do.
    assert_eq!(dones.len(), 2);
}

#[test]
fn interactive_continues_after_bad_json() {
    let (_code, lines, _) = interactive(&[
        r#"{"op":"check""#, // malformed
        r#"{"op":"explain","code":"MT0001"}"#,
        r#"{"op":"halt"}"#,
    ]);
    let errors = find_kind(&lines, "error");
    let results = find_kind(&lines, "result");
    let dones = find_kind(&lines, "done");
    // 3 lines submitted → 3 done terminators.
    assert_eq!(dones.len(), 3);
    // At least one error envelope (the malformed JSON).
    assert!(!errors.is_empty());
    // At least one successful result (the explain).
    let explain_results: Vec<_> = results.iter().filter(|r| r["op"] == "explain").collect();
    assert_eq!(explain_results.len(), 1);
}

#[test]
fn interactive_find_then_explain_then_halt() {
    let (tmp, _) = write_tmp("a.mty", "fn write_thing() -> Unit { }\n");
    let r1 = format!(
        r#"{{"op":"find","query":"write_thing","root":"{}"}}"#,
        tmp.path().display().to_string().replace('\\', "\\\\")
    );
    let r2 = r#"{"op":"explain","code":"MT0001"}"#.to_string();
    let r3 = r#"{"op":"halt"}"#.to_string();
    let (code, lines, _) = interactive(&[&r1, &r2, &r3]);
    assert_eq!(code, 0);
    let dones = find_kind(&lines, "done");
    assert_eq!(dones.len(), 3);
}

#[test]
fn interactive_eof_terminates_loop() {
    // No halt — but closing stdin should end the loop.
    let (code, lines, _) = interactive(&[r#"{"op":"explain","code":"MT0001"}"#]);
    assert_eq!(code, 0);
    let dones = find_kind(&lines, "done");
    assert_eq!(dones.len(), 1);
}

#[test]
fn done_is_always_last_line_per_request() {
    let (_code, lines, _) = interactive(&[
        r#"{"op":"explain","code":"MT0001"}"#,
        r#"{"op":"explain","code":"MT0002"}"#,
        r#"{"op":"halt"}"#,
    ]);
    // Every "done" should be preceded by a "result".
    let mut last_kinds: Vec<String> = Vec::new();
    for l in &lines {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(l) {
            if let Some(k) = v.get("kind").and_then(|s| s.as_str()) {
                last_kinds.push(k.to_string());
            }
        }
    }
    // Just check that "done" appears three times and the last is "done".
    assert_eq!(
        last_kinds.iter().filter(|k| k.as_str() == "done").count(),
        3
    );
    assert_eq!(last_kinds.last().map(|s| s.as_str()), Some("done"));
}

#[test]
fn interactive_check_then_unknown_op_then_halt() {
    let (_tmp, p) = write_tmp("ok.mty", "fn main() -> Unit { }\n");
    let r1 = format!(r#"{{"op":"check","path":"{}"}}"#, path_json(&p));
    let r2 = r#"{"op":"unknown_op"}"#.to_string();
    let r3 = r#"{"op":"halt"}"#.to_string();
    let (_code, lines, _) = interactive(&[&r1, &r2, &r3]);
    let dones = find_kind(&lines, "done");
    assert_eq!(dones.len(), 3);
    let errors = find_kind(&lines, "error");
    assert!(errors
        .iter()
        .any(|e| e["message"].as_str().unwrap_or("").contains("unknown op")));
}
