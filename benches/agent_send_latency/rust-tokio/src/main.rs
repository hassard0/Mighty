//! Rust + Tokio comparator. Same shape as the Stardust mailbox:
//! one-shot `send` on a bounded mpsc, drain on the same task.
//!
//! Usage: `cargo run --release -- 1000` (iterations).

use std::time::Instant;
use tokio::sync::mpsc;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let iters: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let (tx, mut rx) = mpsc::channel::<u64>(8);
        let t0 = Instant::now();
        tx.send(1).await.unwrap();
        let _r = rx.recv().await.unwrap();
        samples.push(t0.elapsed().as_nanos());
    }
    samples.sort();
    let pick = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
    println!(
        "rust_tokio_agent_send_latency: median={:.3} ms  p95={:.3} ms  p99={:.3} ms",
        (pick(0.50) as f64) / 1.0e6,
        (pick(0.95) as f64) / 1.0e6,
        (pick(0.99) as f64) / 1.0e6,
    );
}
