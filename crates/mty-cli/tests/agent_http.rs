//! v0.35 T2 — integration tests for `mty agent --transport http`.
//!
//! These spin up `mty agent --transport http --listen 127.0.0.1:0`,
//! discover the bound port from the stderr banner, and drive the three
//! documented endpoints via a tiny hand-rolled HTTP/1.1 client. We
//! deliberately don't pull `reqwest` into dev-dependencies (it'd add
//! rustls + ring transitively just for these tests) — the wire
//! traffic is simple enough that a 30-line client is clearer.

// HTTP transport runs on every OS. The harness here reads the
// "HTTP listening on..." banner from the agent's stderr to discover
// the port chosen by the kernel; we keep a generous 10-second
// deadline so Windows CI's slower process startup never flakes.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn mty_bin() -> &'static str {
    env!("CARGO_BIN_EXE_mty")
}

/// Spawn `mty agent --transport http --listen 127.0.0.1:0`. Returns
/// the child handle + the bound `host:port` parsed from the startup
/// banner.
fn spawn_http_agent(extra_args: &[&str]) -> (Child, String) {
    let mut cmd = Command::new(mty_bin());
    cmd.args(["agent", "--transport", "http", "--listen", "127.0.0.1:0"]);
    for a in extra_args {
        cmd.arg(a);
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mty agent http");

    // Read stderr until we see the "HTTP listening on http://<addr>/..."
    // banner so we know which port we got.
    let stderr = child.stderr.take().expect("stderr");
    let mut reader = BufReader::new(stderr);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut addr: Option<String> = None;
    while Instant::now() < deadline {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        if let Some(rest) = line.split_once("listening on http://") {
            let tail = rest.1;
            // tail starts with `<addr>/v1/agent...`. Strip at the
            // first `/`.
            if let Some(end) = tail.find('/') {
                addr = Some(tail[..end].to_string());
                break;
            }
        }
    }
    let addr = addr.expect("never saw HTTP listening banner");
    (child, addr)
}

/// Tiny HTTP/1.1 client: send one request, read the whole response.
/// Returns `(status, headers_lower, body)`.
fn http_request(
    addr: &str,
    method: &str,
    path: &str,
    extra_headers: &[(&str, &str)],
    body: &[u8],
) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let mut stream = TcpStream::connect(addr).expect("connect");
    let mut req = String::new();
    req.push_str(&format!("{} {} HTTP/1.1\r\n", method, path));
    req.push_str(&format!("Host: {}\r\n", addr));
    req.push_str("Connection: close\r\n");
    req.push_str(&format!("Content-Length: {}\r\n", body.len()));
    for (k, v) in extra_headers {
        req.push_str(&format!("{}: {}\r\n", k, v));
    }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes()).expect("write req");
    if !body.is_empty() {
        stream.write_all(body).expect("write body");
    }
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read resp");
    // Split headers / body on the first CRLF CRLF.
    let split = (0..raw.len().saturating_sub(3))
        .find(|i| &raw[*i..*i + 4] == b"\r\n\r\n")
        .expect("no header terminator");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let body_bytes = raw[split + 4..].to_vec();
    let mut lines = head.lines();
    let status_line = lines.next().expect("status line");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .expect("status code")
        .parse()
        .expect("parse status");
    let mut headers = Vec::new();
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    (status, headers, body_bytes)
}

fn kill(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn body_str(body: &[u8]) -> String {
    String::from_utf8_lossy(body).into_owned()
}

// ===========================================================================
// /v1/agent/version
// ===========================================================================

#[test]
fn version_endpoint_returns_protocol_and_mty_version() {
    let (mut child, addr) = spawn_http_agent(&[]);
    let (status, headers, body) = http_request(&addr, "GET", "/v1/agent/version", &[], b"");
    assert_eq!(status, 200);
    assert!(headers
        .iter()
        .any(|(k, v)| k == "content-type" && v.contains("application/json")));
    let v: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(v["agent_protocol"], "1.0");
    assert!(v["mty_version"].as_str().unwrap().contains('.'));
    kill(&mut child);
}

#[test]
fn version_endpoint_post_returns_404() {
    let (mut child, addr) = spawn_http_agent(&[]);
    let (status, _h, _b) = http_request(&addr, "POST", "/v1/agent/version", &[], b"");
    assert_eq!(status, 404);
    kill(&mut child);
}

// ===========================================================================
// /v1/agent — single-request endpoint
// ===========================================================================

#[test]
fn post_agent_explain_known_code() {
    let (mut child, addr) = spawn_http_agent(&[]);
    let (status, headers, body) = http_request(
        &addr,
        "POST",
        "/v1/agent",
        &[],
        br#"{"op":"explain","code":"MT0001"}"#,
    );
    assert_eq!(status, 200);
    assert!(headers
        .iter()
        .any(|(k, v)| k == "content-type" && v.contains("ndjson")));
    let s = body_str(&body);
    let lines: Vec<&str> = s.lines().collect();
    let last: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    assert_eq!(last["kind"], "done");
    assert_eq!(last["exit_code"], 0);
    kill(&mut child);
}

#[test]
fn post_agent_unknown_op_returns_done_2() {
    let (mut child, addr) = spawn_http_agent(&[]);
    let (status, _h, body) =
        http_request(&addr, "POST", "/v1/agent", &[], br#"{"op":"frobnicate"}"#);
    assert_eq!(status, 200);
    let s = body_str(&body);
    let lines: Vec<&str> = s.lines().collect();
    let last: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    assert_eq!(last["kind"], "done");
    assert_eq!(last["exit_code"], 2);
    // And an error frame before it.
    let has_err = lines.iter().any(|l| {
        serde_json::from_str::<serde_json::Value>(l)
            .ok()
            .and_then(|v| {
                v.get("kind")
                    .and_then(|k| k.as_str())
                    .map(|k| k.to_string())
            })
            == Some("error".to_string())
    });
    assert!(has_err);
    kill(&mut child);
}

#[test]
fn post_agent_malformed_json_done_2() {
    let (mut child, addr) = spawn_http_agent(&[]);
    let (status, _h, body) = http_request(&addr, "POST", "/v1/agent", &[], br#"{"op":"check""#);
    assert_eq!(status, 200);
    let s = body_str(&body);
    assert!(s.contains("malformed JSON"));
    let last: serde_json::Value = serde_json::from_str(s.lines().last().unwrap()).unwrap();
    assert_eq!(last["kind"], "done");
    assert_eq!(last["exit_code"], 2);
    kill(&mut child);
}

#[test]
fn post_agent_halt_returns_done_0() {
    let (mut child, addr) = spawn_http_agent(&[]);
    let (status, _h, body) = http_request(&addr, "POST", "/v1/agent", &[], br#"{"op":"halt"}"#);
    assert_eq!(status, 200);
    let s = body_str(&body);
    let last: serde_json::Value = serde_json::from_str(s.lines().last().unwrap()).unwrap();
    assert_eq!(last["kind"], "done");
    assert_eq!(last["exit_code"], 0);
    kill(&mut child);
}

#[test]
fn post_agent_empty_body_done_2() {
    let (mut child, addr) = spawn_http_agent(&[]);
    let (status, _h, body) = http_request(&addr, "POST", "/v1/agent", &[], b"");
    assert_eq!(status, 200);
    let s = body_str(&body);
    assert!(s.contains("empty request"));
    kill(&mut child);
}

#[test]
fn post_agent_explain_bad_code_done_2() {
    let (mut child, addr) = spawn_http_agent(&[]);
    let (_status, _h, body) = http_request(
        &addr,
        "POST",
        "/v1/agent",
        &[],
        br#"{"op":"explain","code":"garbage"}"#,
    );
    let s = body_str(&body);
    let last: serde_json::Value = serde_json::from_str(s.lines().last().unwrap()).unwrap();
    assert_eq!(last["exit_code"], 2);
    kill(&mut child);
}

// ===========================================================================
// /v1/agent/batch — NDJSON in, NDJSON out
// ===========================================================================

#[test]
fn batch_runs_each_request_in_order() {
    let (mut child, addr) = spawn_http_agent(&[]);
    let batch = "{\"op\":\"explain\",\"code\":\"MT0001\"}\n{\"op\":\"halt\"}\n";
    let (status, _h, body) = http_request(&addr, "POST", "/v1/agent/batch", &[], batch.as_bytes());
    assert_eq!(status, 200);
    let s = body_str(&body);
    let dones: Vec<_> = s
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v.get("kind").and_then(|k| k.as_str()) == Some("done"))
        .collect();
    // Two `done` lines, one per request.
    assert_eq!(dones.len(), 2);
    kill(&mut child);
}

#[test]
fn batch_empty_body_yields_empty_response() {
    let (mut child, addr) = spawn_http_agent(&[]);
    let (status, _h, body) = http_request(&addr, "POST", "/v1/agent/batch", &[], b"");
    assert_eq!(status, 200);
    assert!(body.is_empty());
    kill(&mut child);
}

#[test]
fn batch_skips_blank_lines() {
    let (mut child, addr) = spawn_http_agent(&[]);
    let (status, _h, body) = http_request(
        &addr,
        "POST",
        "/v1/agent/batch",
        &[],
        b"\n{\"op\":\"halt\"}\n\n",
    );
    assert_eq!(status, 200);
    let s = body_str(&body);
    let dones: Vec<_> = s
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v.get("kind").and_then(|k| k.as_str()) == Some("done"))
        .collect();
    assert_eq!(dones.len(), 1);
    kill(&mut child);
}

#[test]
fn batch_session_state_persists_within_one_request() {
    // Each connection gets a fresh session, but within /v1/agent/batch
    // a `check` and `fix` share state. We can't easily test fix here
    // (it requires a buggy program), but we can prove that two
    // `explain` calls both produce results.
    let (mut child, addr) = spawn_http_agent(&[]);
    let batch =
        "{\"op\":\"explain\",\"code\":\"MT0001\"}\n{\"op\":\"explain\",\"code\":\"MT0001\"}\n";
    let (_status, _h, body) = http_request(&addr, "POST", "/v1/agent/batch", &[], batch.as_bytes());
    let s = body_str(&body);
    let results: Vec<_> = s
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v.get("kind").and_then(|k| k.as_str()) == Some("result"))
        .collect();
    assert_eq!(results.len(), 2);
    kill(&mut child);
}

// ===========================================================================
// Routing + method handling
// ===========================================================================

#[test]
fn unknown_path_returns_404() {
    let (mut child, addr) = spawn_http_agent(&[]);
    let (status, _h, _b) = http_request(&addr, "GET", "/nope", &[], b"");
    assert_eq!(status, 404);
    kill(&mut child);
}

#[test]
fn get_agent_returns_404() {
    let (mut child, addr) = spawn_http_agent(&[]);
    let (status, _h, _b) = http_request(&addr, "GET", "/v1/agent", &[], b"");
    assert_eq!(status, 404);
    kill(&mut child);
}

#[test]
fn put_agent_returns_404() {
    let (mut child, addr) = spawn_http_agent(&[]);
    let (status, _h, _b) = http_request(&addr, "PUT", "/v1/agent", &[], b"{}");
    assert_eq!(status, 404);
    kill(&mut child);
}

// ===========================================================================
// Bearer-token auth
// ===========================================================================

#[test]
fn no_auth_header_when_required_returns_401() {
    let (mut child, addr) = spawn_http_agent(&["--auth-token", "s3cret"]);
    let (status, headers, body) =
        http_request(&addr, "POST", "/v1/agent", &[], br#"{"op":"halt"}"#);
    assert_eq!(status, 401);
    let s = body_str(&body);
    assert!(s.contains("unauthorized"));
    assert!(headers
        .iter()
        .any(|(k, v)| k == "www-authenticate" && v.contains("Bearer")));
    kill(&mut child);
}

#[test]
fn wrong_token_returns_401() {
    let (mut child, addr) = spawn_http_agent(&["--auth-token", "s3cret"]);
    let (status, _h, _b) = http_request(
        &addr,
        "POST",
        "/v1/agent",
        &[("Authorization", "Bearer wrong")],
        br#"{"op":"halt"}"#,
    );
    assert_eq!(status, 401);
    kill(&mut child);
}

#[test]
fn correct_token_allows_request() {
    let (mut child, addr) = spawn_http_agent(&["--auth-token", "s3cret"]);
    let (status, _h, _b) = http_request(
        &addr,
        "POST",
        "/v1/agent",
        &[("Authorization", "Bearer s3cret")],
        br#"{"op":"halt"}"#,
    );
    assert_eq!(status, 200);
    kill(&mut child);
}

#[test]
fn auth_scheme_is_case_insensitive() {
    let (mut child, addr) = spawn_http_agent(&["--auth-token", "tok"]);
    let (status, _h, _b) = http_request(
        &addr,
        "POST",
        "/v1/agent",
        &[("Authorization", "bearer tok")],
        br#"{"op":"halt"}"#,
    );
    assert_eq!(status, 200);
    kill(&mut child);
}

#[test]
fn no_token_means_no_auth_required() {
    let (mut child, addr) = spawn_http_agent(&[]);
    let (status, _h, _b) = http_request(&addr, "POST", "/v1/agent", &[], br#"{"op":"halt"}"#);
    assert_eq!(status, 200);
    kill(&mut child);
}

#[test]
fn auth_blocks_version_too() {
    let (mut child, addr) = spawn_http_agent(&["--auth-token", "x"]);
    let (status, _h, _b) = http_request(&addr, "GET", "/v1/agent/version", &[], b"");
    assert_eq!(status, 401);
    kill(&mut child);
}

#[test]
fn auth_allows_version_with_token() {
    let (mut child, addr) = spawn_http_agent(&["--auth-token", "x"]);
    let (status, _h, body) = http_request(
        &addr,
        "GET",
        "/v1/agent/version",
        &[("Authorization", "Bearer x")],
        b"",
    );
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["agent_protocol"], "1.0");
    kill(&mut child);
}

// ===========================================================================
// Recorder over HTTP
// ===========================================================================

#[test]
fn http_recorder_writes_pairs_to_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("rec.ndjson");
    let path_str = path.to_string_lossy().into_owned();
    let (mut child, addr) = spawn_http_agent(&["--record", &path_str]);
    let _ = http_request(&addr, "POST", "/v1/agent", &[], br#"{"op":"halt"}"#);
    // Give the recorder a moment to flush before we kill the child.
    std::thread::sleep(Duration::from_millis(100));
    kill(&mut child);

    let body = std::fs::read_to_string(&path).expect("rec file");
    assert!(
        !body.is_empty(),
        "recorder produced empty file; child died early"
    );
    let line = body.lines().next().expect("at least one line");
    let v: serde_json::Value = serde_json::from_str(line).unwrap();
    assert_eq!(v["request"], r#"{"op":"halt"}"#);
    assert!(v["response"]
        .as_str()
        .unwrap_or("")
        .contains("\"kind\":\"done\""));
}
