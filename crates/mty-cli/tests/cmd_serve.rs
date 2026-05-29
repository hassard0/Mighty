#![cfg(feature = "host-toolchain")]
//! v0.23 Track C — `mty serve` integration tests.
//!
//! Spawns `mty serve --port 0` as a child process pointed at a
//! scaffolded web-game package, then hits the resulting HTTP server
//! with [`ureq`].
//!
//! v0.37 T6 — switched from a hand-rolled raw-TCP `http_get()` to
//! `ureq::get(...)`. The previous implementation needed an explicit
//! `ConnectionReset && !raw.is_empty()` carve-out to work on GHA Ubuntu
//! runners (see commit 19e2163 in v0.36.1) because Linux servers
//! sometimes close the socket with RST before the client has finished
//! draining the response. ureq folds that case into a clean EOF so the
//! workaround is gone.
//!
//! We pin the bound port via `--port` (port 0 would be ideal but
//! `mty serve` doesn't expose the OS-assigned port back out yet).
//! Each test picks a high random-ish port to avoid collisions on
//! shared CI hosts; a clash retries once on a different port.
//!
//! See `dev/history/notes/MTY_SERVE_V0_23_NOTES.md`.

use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn mty(cwd: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_mty"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run mty");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn fresh_tmpdir(label: &str) -> std::path::PathBuf {
    let mut d = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    d.push(format!("mty-cli-serve-test-{label}-{nanos}"));
    std::fs::create_dir_all(&d).expect("create tmpdir");
    d
}

/// Pick a port that's most likely free. We bind a transient
/// `TcpListener` on `127.0.0.1:0`, let the OS pick a free dynamic
/// port, then drop the listener and return the port number. This
/// is racy (another process could grab the port between drop and
/// `mty serve --port` binding) but vastly less racy than the
/// time-based hashing we used to do — that flaked under parallel
/// `cargo test --workspace` runs because two seeds + a fast clock
/// would collide deterministically. The `seed` arg is retained for
/// API compatibility but unused.
fn pick_port(_seed: u16) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1:0");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);
    port
}

/// Spawn `mty serve --port <port>` against `pkg_root`. Returns the
/// child + bound port.
fn spawn_serve(pkg_root: &std::path::Path, port: u16, watch: bool) -> Child {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mty"));
    cmd.current_dir(pkg_root)
        .arg("serve")
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if watch {
        cmd.arg("--watch");
    }
    cmd.spawn().expect("spawn mty serve")
}

/// Block until `127.0.0.1:port` accepts a connection, or `deadline`
/// elapses.
fn wait_for_listen(port: u16, deadline: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < deadline {
        if TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            Duration::from_millis(200),
        )
        .is_ok()
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Bundled response object — keeps the test surface a 3-tuple while
/// hiding ureq's typed response from per-test code.
struct HttpResp {
    status: u16,
    content_type: Option<String>,
    body: Vec<u8>,
}

/// Blocking HTTP GET via [`ureq`]. ureq closes the connection cleanly
/// regardless of how the server shuts down (RST vs FIN), which is the
/// whole reason this exists — the v0.36.1 hand-rolled `read_to_end`
/// path needed a `ConnectionReset` carve-out to work on GHA Ubuntu.
fn http_get(port: u16, path: &str) -> HttpResp {
    let url = format!("http://127.0.0.1:{port}{path}");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(2))
        .timeout(Duration::from_secs(5))
        .build();
    let resp = match agent.get(&url).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => panic!("ureq GET {url} failed: {e:?}"),
    };
    let status = resp.status();
    let content_type = resp.header("content-type").map(|s| s.to_string());
    let mut body = Vec::new();
    resp.into_reader()
        .read_to_end(&mut body)
        .expect("read body");
    HttpResp {
        status,
        content_type,
        body,
    }
}

/// Scaffold a fresh web-game package and return its root.
fn scaffold_web_game(label: &str) -> std::path::PathBuf {
    let dir = fresh_tmpdir(label);
    let (code, stdout, stderr) = mty(&dir, &["new", "--template", "web-game", label]);
    assert_eq!(code, 0, "scaffold failed: stdout={stdout} stderr={stderr}");
    dir.join(label)
}

/// Pre-build the wasm so `mty serve`'s startup time is dominated by
/// the listener bind, not the (slow) backend pass. This keeps each
/// test ~well under the CI 60s timeout.
fn prebuild(pkg: &std::path::Path) {
    let (code, stdout, stderr) = mty(pkg, &["build", "--target", "wasm32-web", "src/main.mty"]);
    assert_eq!(code, 0, "prebuild failed: {stdout}\n{stderr}");
}

/// Drop guard that kills the child server even on panic.
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn serve_starts_and_returns_index_html() {
    let pkg = scaffold_web_game("idx");
    prebuild(&pkg);
    let port = pick_port(1);
    let child = spawn_serve(&pkg, port, false);
    let mut guard = ChildGuard(child);

    assert!(
        wait_for_listen(port, Duration::from_secs(15)),
        "mty serve never started listening on :{port}\n--- stderr ---\n{}",
        drain_stderr(&mut guard.0)
    );

    let resp = http_get(port, "/");
    assert_eq!(resp.status, 200, "content-type={:?}", resp.content_type);
    assert!(
        resp.content_type
            .as_deref()
            .unwrap_or("")
            .starts_with("text/html"),
        "content-type wasn't text/html: {:?}",
        resp.content_type
    );
    let body_s = String::from_utf8_lossy(&resp.body);
    assert!(
        body_s.contains("<canvas") && body_s.contains("dom-shim.js"),
        "body didn't look like the web-game template: {body_s}"
    );
}

#[test]
fn serve_serves_wasm() {
    let pkg = scaffold_web_game("wasm");
    prebuild(&pkg);
    let port = pick_port(2);
    let child = spawn_serve(&pkg, port, false);
    let mut guard = ChildGuard(child);

    assert!(
        wait_for_listen(port, Duration::from_secs(15)),
        "mty serve never started: {}",
        drain_stderr(&mut guard.0)
    );

    let resp = http_get(port, "/main.wasm");
    assert_eq!(resp.status, 200, "content-type={:?}", resp.content_type);
    assert_eq!(
        resp.content_type.as_deref(),
        Some("application/wasm"),
        "content-type={:?}",
        resp.content_type
    );
    // Component preamble (\0asm followed by 0x0d for components,
    // 0x01 for core modules). Either is fine; we just want non-empty.
    assert!(
        resp.body.len() > 8,
        "wasm body too short ({} bytes)",
        resp.body.len()
    );
    assert_eq!(&resp.body[..4], b"\0asm", "missing wasm preamble");
}

#[test]
fn serve_serves_static_asset() {
    let pkg = scaffold_web_game("static");
    prebuild(&pkg);
    let port = pick_port(3);
    let child = spawn_serve(&pkg, port, false);
    let mut guard = ChildGuard(child);

    assert!(
        wait_for_listen(port, Duration::from_secs(15)),
        "mty serve never started: {}",
        drain_stderr(&mut guard.0)
    );

    let resp = http_get(port, "/dom-shim.js");
    assert_eq!(resp.status, 200, "content-type={:?}", resp.content_type);
    assert!(
        resp.content_type
            .as_deref()
            .unwrap_or("")
            .starts_with("application/javascript"),
        "content-type wasn't application/javascript: {:?}",
        resp.content_type
    );
    let body_s = String::from_utf8_lossy(&resp.body);
    assert!(
        body_s.contains("function boot") || body_s.contains("loadWasm"),
        "dom-shim.js didn't look right: first 200 chars={}",
        &body_s.chars().take(200).collect::<String>()
    );
}

#[test]
fn serve_404_for_missing_file() {
    let pkg = scaffold_web_game("missing");
    prebuild(&pkg);
    let port = pick_port(4);
    let child = spawn_serve(&pkg, port, false);
    let mut guard = ChildGuard(child);

    assert!(
        wait_for_listen(port, Duration::from_secs(15)),
        "mty serve never started: {}",
        drain_stderr(&mut guard.0)
    );

    let resp = http_get(port, "/no-such-file.txt");
    assert_eq!(resp.status, 404);
}

#[test]
fn serve_fails_outside_a_package() {
    // No `mighty.toml` ⇒ exit 2 with a useful message.
    let dir = fresh_tmpdir("nopkg");
    let port = pick_port(5);
    let out = Command::new(env!("CARGO_BIN_EXE_mty"))
        .current_dir(&dir)
        .arg("serve")
        .arg("--port")
        .arg(port.to_string())
        .output()
        .expect("run mty serve");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("mighty.toml"),
        "expected mighty.toml in stderr: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// v0.23 had a `serve_watch_rebuilds_on_change` test here that was
// `#[ignore]`'d on filesystem-event timing flake. v0.24 Track C
// replaced it with `crates/mty-cli/tests/cmd_serve_watch.rs`, which
// drives the same `rebuild_and_broadcast` path via a hidden
// env-gated HTTP endpoint instead of waiting on `notify` events.
// See `dev/history/notes/SERVE_WATCH_V0_24_NOTES.md`.

fn drain_stderr(child: &mut Child) -> String {
    use std::io::Read;
    let mut s = String::new();
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_string(&mut s);
    }
    s
}
