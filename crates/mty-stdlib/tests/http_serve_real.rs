//! v0.5 dogfood Gap-1 integration test — spawn a real socket-bound
//! HTTP server via `mty_stdlib::http_server::start_blocking`, fire
//! a `tokio::net::TcpStream` GET at it, and assert the body that comes
//! back.
//!
//! This is the smoke test that proves the v0.5 `std.http.serve`
//! binding actually opens a TCP listener (versus the v0.4 stopgap
//! where Demo 01 drove handlers from `main()` directly).

use mty_stdlib::http_server::{shutdown, start_blocking};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn raw_get(port: u16, path: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect localhost");
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        path
    );
    s.write_all(req.as_bytes()).await.expect("write");
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), s.read_to_end(&mut buf)).await;
    String::from_utf8_lossy(&buf).into_owned()
}

#[test]
fn server_binds_real_socket_and_default_dispatcher_replies_200() {
    let (handle, bound) = start_blocking("127.0.0.1:0").expect("bind ephemeral");
    assert!(bound.port() != 0, "bound port should be non-zero");

    // Drive a real HTTP/1.1 request at the listener and parse the
    // response body. The default dispatcher emits a JSON echo with
    // the request method + path.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio rt for test client");
    let body = rt.block_on(raw_get(bound.port(), "/health"));

    // Tear down before assertions so a failure doesn't leave the
    // accept loop running.
    assert!(shutdown(handle), "shutdown should succeed on live handle");

    assert!(body.starts_with("HTTP/1.1 200"), "body was: {body}");
    assert!(body.contains("\"method\":\"GET\""), "body: {body}");
    assert!(body.contains("\"path\":\"/health\""), "body: {body}");
    assert!(
        body.contains("\"status\":\"ok\""),
        "default dispatcher should report ok; body: {body}"
    );
}

#[test]
fn multiple_concurrent_servers_each_bind_a_unique_port() {
    let (h1, b1) = start_blocking("127.0.0.1:0").expect("bind 1");
    let (h2, b2) = start_blocking("127.0.0.1:0").expect("bind 2");
    assert_ne!(b1.port(), b2.port(), "ports must differ: {b1} == {b2}");
    assert!(shutdown(h1));
    assert!(shutdown(h2));
}

#[test]
fn shutdown_of_unknown_handle_returns_false() {
    assert!(!shutdown(99_999_999), "unknown handle should be false");
}

#[test]
fn host_dispatch_serve_returns_handle_pipe_addr_string() {
    use mty_ir::interp::value::Value;
    use mty_stdlib::host::dispatch;

    let v = dispatch(
        &["std".into(), "http".into()],
        "serve",
        &[Value::Str("127.0.0.1:0".into())],
    );
    let s = match &v {
        Value::Str(s) => s.clone(),
        other => panic!("expected Str, got {other:?}"),
    };
    let mut parts = s.split('|');
    let handle: u64 = parts
        .next()
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("no handle id in {s:?}"));
    let addr = parts.next().unwrap_or_default();
    assert!(handle > 0, "handle id should be positive; got {handle}");
    assert!(addr.starts_with("127.0.0.1:"), "addr: {addr}");

    // Clean shutdown via the same dispatch path.
    let shut = dispatch(
        &["std".into(), "http".into()],
        "shutdown",
        &[Value::Str(s.clone())],
    );
    assert!(
        matches!(shut, Value::Bool(true)),
        "shutdown returned {shut:?}"
    );
}
