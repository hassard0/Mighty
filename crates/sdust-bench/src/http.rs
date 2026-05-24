//! Minimal HTTP request generator for the http_server_throughput
//! benchmark. We avoid a real load-generator (wrk / autocannon) because
//! the bench needs to be portable across the same `cargo bench`
//! invocation used by every other category. The internal generator
//! reaches saturation for the in-process echo server we're measuring;
//! external tools belong in `benches/http_server_throughput/run.sh`
//! for cross-language comparison.

use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// One-shot HTTP/1.1 GET. Returns the wall time spent on the round-trip.
/// Returns Err on the first I/O failure.
pub async fn single_get(addr: &str, path: &str) -> std::io::Result<Duration> {
    let t0 = Instant::now();
    let mut sock = TcpStream::connect(addr).await?;
    let req = format!("GET {path} HTTP/1.1\r\nHost: bench\r\nConnection: close\r\n\r\n");
    sock.write_all(req.as_bytes()).await?;
    let mut buf = [0u8; 1024];
    loop {
        let n = sock.read(&mut buf).await?;
        if n == 0 {
            break;
        }
    }
    Ok(t0.elapsed())
}

/// Fire `n` sequential GETs (single-connection-per-request) and return
/// the elapsed wall time. Used by the http_server_throughput benchmark
/// to compute requests/second without spawning a separate process.
pub async fn sequential_get(addr: &str, path: &str, n: usize) -> std::io::Result<Duration> {
    let t0 = Instant::now();
    for _ in 0..n {
        let _ = single_get(addr, path).await?;
    }
    Ok(t0.elapsed())
}
