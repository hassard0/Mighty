//! v0.5 dogfood Gap-1 — end-to-end real-socket roundtrip going
//! through the runtime crate (via the `http::serve` API in
//! `mty_runtime::http`). The companion test in `mty-stdlib`
//! exercises the high-level `std.http.serve` dispatcher; this test
//! confirms the older runtime-level surface still works alongside.

use mty_runtime::http::serve_in_memory;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn runtime_http_serve_in_memory_roundtrip() {
    let (handle, port) = serve_in_memory(|req| {
        // Echo the request method+path so a curl-equivalent can
        // assert the server actually saw them.
        let body = format!(
            "{{\"method\":\"{}\",\"path\":\"{}\"}}",
            req.method, req.path
        );
        (200, body)
    })
    .await;

    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    s.write_all(b"GET /v0.5 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("write");
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), s.read_to_end(&mut buf)).await;
    handle.abort();
    let body = String::from_utf8_lossy(&buf);
    assert!(body.starts_with("HTTP/1.1 200"), "body: {body}");
    assert!(body.contains("\"method\":\"GET\""), "body: {body}");
    assert!(body.contains("\"path\":\"/v0.5\""), "body: {body}");
}
