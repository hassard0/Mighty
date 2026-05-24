//! Minimal std.http server (HTTP/1.1 GET only, slice-7 MVP).

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub path: String,
}

pub fn parse_request_line(line: &[u8]) -> Option<Request> {
    let s = std::str::from_utf8(line).ok()?;
    let s = s.trim_end_matches(['\r', '\n']);
    let mut parts = s.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let version = parts.next()?;
    if !version.starts_with("HTTP/") {
        return None;
    }
    Some(Request { method, path })
}

/// Bind on 127.0.0.1:0, return (task handle, allocated port). The
/// handler is invoked once per request; its return value becomes the
/// response (status, body).
pub async fn serve_in_memory<F>(handler: F) -> (tokio::task::JoinHandle<()>, u16)
where
    F: Fn(Request) -> (u16, String) + Send + Sync + 'static + Clone,
{
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind 127.0.0.1:0");
    let port = listener.local_addr().expect("local_addr").port();
    let handle = tokio::spawn(async move {
        loop {
            let (mut sock, _) = match listener.accept().await {
                Ok(x) => x,
                Err(_) => return,
            };
            let h = handler.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = match sock.read(&mut buf).await {
                    Ok(n) if n > 0 => n,
                    _ => return,
                };
                let first_line_end = buf[..n].iter().position(|&b| b == b'\n').unwrap_or(n);
                let req = match parse_request_line(&buf[..=first_line_end.min(n - 1)]) {
                    Some(r) => r,
                    None => {
                        let _ = sock
                            .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
                            .await;
                        return;
                    }
                };
                let (status, body) = h(req);
                let resp = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    (handle, port)
}
