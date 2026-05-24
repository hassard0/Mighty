//! Rust + Hyper HTTP/1.1 server comparator.
//!
//! Spins up a tiny hyper server on 127.0.0.1:0 that responds "ok" to
//! every GET, then drives 30 sequential requests through it from the
//! same task. Reports median/p95/p99 of single-request round-trip.

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::Response;
use hyper_util::rt::TokioIo;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn handle(_req: hyper::Request<hyper::body::Incoming>) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    Ok(Response::new(Full::new(Bytes::from_static(b"ok"))))
}

async fn single_get(addr: &str) -> std::io::Result<std::time::Duration> {
    let t0 = Instant::now();
    let mut s = TcpStream::connect(addr).await?;
    s.write_all(b"GET / HTTP/1.1\r\nHost: bench\r\nConnection: close\r\n\r\n").await?;
    let mut buf = [0u8; 1024];
    loop {
        let n = s.read(&mut buf).await?;
        if n == 0 { break; }
    }
    Ok(t0.elapsed())
}

#[tokio::main]
async fn main() {
    let iters: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let srv = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(v) => v, Err(_) => return,
            };
            let io = TokioIo::new(stream);
            tokio::spawn(async move {
                let _ = http1::Builder::new()
                    .serve_connection(io, service_fn(handle))
                    .await;
                let _ = BodyExt::collect; // keep import live
            });
        }
    });
    let addr = format!("127.0.0.1:{port}");
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let d = single_get(&addr).await.unwrap();
        samples.push(d.as_nanos());
    }
    srv.abort();
    samples.sort();
    let pick = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
    println!(
        "rust_hyper_http_server: median={:.3} ms  p95={:.3} ms  p99={:.3} ms",
        (pick(0.50) as f64) / 1.0e6,
        (pick(0.95) as f64) / 1.0e6,
        (pick(0.99) as f64) / 1.0e6,
    );
}
