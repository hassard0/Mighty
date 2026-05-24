//! Rust + Tokio mailbox throughput: 1 producer + 1 consumer, 10 000
//! msgs per iter, 30 iters by default.

use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let iters: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    const N: usize = 10_000;
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let (tx, mut rx) = mpsc::channel::<u64>(N);
        let tx = Arc::new(tx);
        let prod = {
            let tx = tx.clone();
            tokio::spawn(async move {
                for i in 0..N {
                    tx.send(i as u64).await.unwrap();
                }
            })
        };
        let t0 = Instant::now();
        for _ in 0..N {
            let _r = rx.recv().await.unwrap();
        }
        let d = t0.elapsed().as_nanos();
        prod.await.unwrap();
        samples.push(d);
    }
    samples.sort();
    let pick = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
    println!(
        "rust_tokio_mailbox_throughput: median={:.3} ms  p95={:.3} ms  p99={:.3} ms  ({} msgs/iter)",
        (pick(0.50) as f64) / 1.0e6,
        (pick(0.95) as f64) / 1.0e6,
        (pick(0.99) as f64) / 1.0e6,
        N,
    );
}
