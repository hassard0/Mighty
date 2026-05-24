use mty_runtime::http::{parse_request_line, serve_in_memory};

#[test]
fn parse_get_root() {
    let r = parse_request_line(b"GET / HTTP/1.1\r\n").unwrap();
    assert_eq!(r.method, "GET");
    assert_eq!(r.path, "/");
}

#[test]
fn parse_with_query() {
    let r = parse_request_line(b"GET /search?q=hello HTTP/1.1\r\n").unwrap();
    assert_eq!(r.method, "GET");
    assert_eq!(r.path, "/search?q=hello");
}

#[test]
fn parse_rejects_bad() {
    assert!(parse_request_line(b"INVALID").is_none());
    assert!(parse_request_line(b"GET / FOO/1.0").is_none());
}

#[tokio::test]
async fn serve_and_get_localhost() {
    let (handle, port) = serve_in_memory(|_req| (200, "hello".to_string())).await;
    let body = simple_get(port).await;
    handle.abort();
    assert!(body.contains("hello"), "body: {body}");
    assert!(body.starts_with("HTTP/1.1 200"));
}

async fn simple_get(port: u16) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    s.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), s.read_to_end(&mut buf)).await;
    String::from_utf8_lossy(&buf).into_owned()
}
