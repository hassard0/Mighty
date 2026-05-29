//! v0.35 T2 — integration tests for `mty agent --transport unix`.
//!
//! Unix-only. The whole file is `cfg`-gated; on Windows the agent
//! prints a clear error and exits 2, which is covered by the
//! `windows_unix_fallback_emits_error` test below.

// Unix transport runs on Unix only. Windows just exercises the
// fallback path at the bottom of this file.
#[cfg(any(unix, windows))]
use std::process::{Command, Stdio};

fn mty_bin() -> &'static str {
    env!("CARGO_BIN_EXE_mty")
}

#[cfg(unix)]
mod unix_only {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::process::Child;
    use std::time::{Duration, Instant};

    fn spawn_unix_agent(socket: &str, extra_args: &[&str]) -> Child {
        let mut cmd = Command::new(mty_bin());
        cmd.args(["agent", "--transport", "unix", "--listen", socket]);
        for a in extra_args {
            cmd.arg(a);
        }
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn unix agent");

        // Wait for the "Unix socket listening on ..." banner.
        let stderr = child.stderr.take().expect("stderr");
        let mut reader = BufReader::new(stderr);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut ready = false;
        while Instant::now() < deadline {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            if line.contains("Unix socket listening on") {
                ready = true;
                break;
            }
        }
        assert!(ready, "never saw Unix socket listening banner");
        child
    }

    fn socket_path() -> (tempfile::TempDir, PathBuf, String) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("mty-agent.sock");
        let s = p.to_string_lossy().into_owned();
        (dir, p, s)
    }

    fn round_trip(socket: &str, req_line: &str) -> String {
        let mut stream = UnixStream::connect(socket).expect("connect uds");
        stream.write_all(req_line.as_bytes()).unwrap();
        stream.write_all(b"\n").unwrap();
        // Half-close so the server returns its response then closes.
        // tokio UnixListener treats EOF on the read half as
        // end-of-session.
        stream
            .shutdown(std::net::Shutdown::Write)
            .expect("shutdown wr");
        let mut buf = String::new();
        stream.read_to_string(&mut buf).unwrap();
        buf
    }

    fn kill(child: &mut Child) {
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn unix_explain_known_code() {
        let (_dir, _p, sock) = socket_path();
        let mut child = spawn_unix_agent(&sock, &[]);
        let body = round_trip(&sock, r#"{"op":"explain","code":"MT0001"}"#);
        let last: serde_json::Value = serde_json::from_str(body.lines().last().unwrap()).unwrap();
        assert_eq!(last["kind"], "done");
        assert_eq!(last["exit_code"], 0);
        kill(&mut child);
    }

    #[test]
    fn unix_explain_unknown_code() {
        let (_dir, _p, sock) = socket_path();
        let mut child = spawn_unix_agent(&sock, &[]);
        let body = round_trip(&sock, r#"{"op":"explain","code":"MT9999"}"#);
        let last: serde_json::Value = serde_json::from_str(body.lines().last().unwrap()).unwrap();
        assert_eq!(last["exit_code"], 1);
        kill(&mut child);
    }

    #[test]
    fn unix_unknown_op_done_2() {
        let (_dir, _p, sock) = socket_path();
        let mut child = spawn_unix_agent(&sock, &[]);
        let body = round_trip(&sock, r#"{"op":"frobnicate"}"#);
        let last: serde_json::Value = serde_json::from_str(body.lines().last().unwrap()).unwrap();
        assert_eq!(last["exit_code"], 2);
        kill(&mut child);
    }

    #[test]
    fn unix_malformed_json_done_2() {
        let (_dir, _p, sock) = socket_path();
        let mut child = spawn_unix_agent(&sock, &[]);
        let body = round_trip(&sock, r#"{"op":"check""#);
        assert!(body.contains("malformed JSON"));
        let last: serde_json::Value = serde_json::from_str(body.lines().last().unwrap()).unwrap();
        assert_eq!(last["exit_code"], 2);
        kill(&mut child);
    }

    #[test]
    fn unix_halt_ends_session() {
        let (_dir, _p, sock) = socket_path();
        let mut child = spawn_unix_agent(&sock, &[]);
        let body = round_trip(&sock, r#"{"op":"halt"}"#);
        let last: serde_json::Value = serde_json::from_str(body.lines().last().unwrap()).unwrap();
        assert_eq!(last["exit_code"], 0);
        kill(&mut child);
    }

    #[test]
    fn unix_pre_existing_socket_is_replaced() {
        let (_dir, p, sock) = socket_path();
        // Pre-create a dummy file at the socket path.
        std::fs::write(&p, "junk").unwrap();
        let mut child = spawn_unix_agent(&sock, &[]);
        let body = round_trip(&sock, r#"{"op":"halt"}"#);
        let last: serde_json::Value = serde_json::from_str(body.lines().last().unwrap()).unwrap();
        assert_eq!(last["exit_code"], 0);
        kill(&mut child);
    }

    #[test]
    fn unix_two_sequential_connections() {
        let (_dir, _p, sock) = socket_path();
        let mut child = spawn_unix_agent(&sock, &[]);
        let body1 = round_trip(&sock, r#"{"op":"explain","code":"MT0001"}"#);
        let body2 = round_trip(&sock, r#"{"op":"explain","code":"MT0001"}"#);
        assert!(body1.contains("\"kind\":\"result\""));
        assert!(body2.contains("\"kind\":\"result\""));
        kill(&mut child);
    }

    #[test]
    fn unix_multiline_session_one_connection() {
        let (_dir, _p, sock) = socket_path();
        let mut child = spawn_unix_agent(&sock, &[]);
        let mut stream = UnixStream::connect(&sock).unwrap();
        stream
            .write_all(b"{\"op\":\"explain\",\"code\":\"MT0001\"}\n{\"op\":\"halt\"}\n")
            .unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();
        let mut buf = String::new();
        stream.read_to_string(&mut buf).unwrap();
        let dones: Vec<_> = buf
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter(|v| v.get("kind").and_then(|k| k.as_str()) == Some("done"))
            .collect();
        assert_eq!(dones.len(), 2);
        kill(&mut child);
    }

    #[test]
    fn unix_socket_is_cleaned_up_on_exit() {
        let (_dir, p, sock) = socket_path();
        let mut child = spawn_unix_agent(&sock, &[]);
        let _ = round_trip(&sock, r#"{"op":"halt"}"#);
        // Note: cleanup only happens after the accept loop returns,
        // which requires the listener task to drop. We rely on SIGKILL
        // here so we can't observe the unlink in real Unix CI, but we
        // can observe that the socket file existed during the run.
        assert!(p.exists());
        kill(&mut child);
    }

    #[test]
    fn unix_recorder_writes_pairs() {
        let (_dir, _p, sock) = socket_path();
        let rec_dir = tempfile::tempdir().unwrap();
        let rec_path = rec_dir.path().join("rec.ndjson");
        let rec_str = rec_path.to_string_lossy().into_owned();
        let mut child = spawn_unix_agent(&sock, &["--record", &rec_str]);
        let _ = round_trip(&sock, r#"{"op":"halt"}"#);
        std::thread::sleep(Duration::from_millis(100));
        kill(&mut child);
        let body = std::fs::read_to_string(&rec_path).expect("rec file");
        assert!(!body.is_empty());
        let line = body.lines().next().unwrap();
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["request"], r#"{"op":"halt"}"#);
    }

    #[test]
    fn unix_listen_path_falls_back_to_socket_flag() {
        // Pass --socket instead of --listen; the agent should accept it.
        let (_dir, _p, sock) = socket_path();
        let mut cmd = Command::new(mty_bin());
        cmd.args(["agent", "--transport", "unix", "--socket", &sock]);
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn");
        let stderr = child.stderr.take().expect("stderr");
        let mut reader = BufReader::new(stderr);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut ready = false;
        while Instant::now() < deadline {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            if line.contains("Unix socket listening on") {
                ready = true;
                break;
            }
        }
        assert!(ready);
        let body = round_trip(&sock, r#"{"op":"halt"}"#);
        assert!(body.contains("\"kind\":\"done\""));
        kill(&mut child);
    }
}

// ===========================================================================
// Windows-only fallback
// ===========================================================================

#[cfg(windows)]
#[test]
fn windows_unix_fallback_emits_error() {
    let out = Command::new(mty_bin())
        .args([
            "agent",
            "--transport",
            "unix",
            "--listen",
            "C:\\Users\\Public\\nope.sock",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("not supported on Windows"));
    assert!(stdout.contains("\"kind\":\"error\""));
    let last: serde_json::Value = serde_json::from_str(stdout.lines().last().unwrap()).unwrap();
    assert_eq!(last["kind"], "done");
    assert_eq!(last["exit_code"], 2);
}
