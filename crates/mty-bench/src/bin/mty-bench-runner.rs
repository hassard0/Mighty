//! `mty-bench-runner` — orchestrate the v0.6 benchmark categories and
//! emit a JSON summary that the doc scripts can ingest.
//!
//! For each category, we run the **Mighty impl in-process** (no
//! criterion harness — that's what `cargo bench -p mty-bench` is for)
//! and record raw timings. The runner is intentionally tiny so the
//! same binary works in CI's lightweight mode, where running the full
//! criterion suite is overkill.
//!
//! Usage:
//!
//! ```text
//! mty-bench-runner --category parse_throughput --iters 50
//! mty-bench-runner --all --iters 30 --out target/bench-results.json
//! ```

use clap::{Parser, ValueEnum};
use mty_bench::{
    fixtures::mty_10kloc,
    metrics::{mean, percentiles},
};
use serde::Serialize;
use std::io::Write;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Category {
    ParseThroughput,
    AgentSendLatency,
    MailboxThroughput,
    HttpServerThroughput,
    CompileToNative,
    WasmSize,
}

#[derive(Parser, Debug)]
#[command(name = "mty-bench-runner", version)]
struct Args {
    /// Run a single category.
    #[arg(long, value_enum, conflicts_with = "all")]
    category: Option<Category>,
    /// Run every category in turn.
    #[arg(long)]
    all: bool,
    /// Iterations per category (sample count).
    #[arg(long, default_value_t = 25)]
    iters: usize,
    /// Optional JSON output path.
    #[arg(long)]
    out: Option<std::path::PathBuf>,
}

#[derive(Debug, Serialize)]
struct Sample {
    category: String,
    iters: usize,
    median_ns: u128,
    mean_ns: u128,
    p95_ns: u128,
    p99_ns: u128,
    extra: Option<serde_json::Value>,
}

fn main() {
    let args = Args::parse();
    let cats: Vec<Category> = if args.all {
        vec![
            Category::ParseThroughput,
            Category::AgentSendLatency,
            Category::MailboxThroughput,
            Category::HttpServerThroughput,
            Category::CompileToNative,
            Category::WasmSize,
        ]
    } else if let Some(c) = args.category {
        vec![c]
    } else {
        eprintln!("must pass --category or --all");
        std::process::exit(2);
    };

    let mut results = Vec::new();
    for cat in cats {
        let s = run_one(cat, args.iters);
        println!(
            "{:<22} median={:>10.3} ms  p95={:>10.3} ms  p99={:>10.3} ms",
            s.category,
            (s.median_ns as f64) / 1.0e6,
            (s.p95_ns as f64) / 1.0e6,
            (s.p99_ns as f64) / 1.0e6,
        );
        results.push(s);
    }

    if let Some(out) = args.out {
        let json = serde_json::to_string_pretty(&results).expect("serialise");
        let mut f = std::fs::File::create(&out).expect("open out");
        writeln!(f, "{json}").expect("write json");
        println!("wrote {} samples to {}", results.len(), out.display());
    }
}

fn run_one(cat: Category, iters: usize) -> Sample {
    match cat {
        Category::ParseThroughput => run_parse(iters),
        Category::AgentSendLatency => run_agent_send(iters),
        Category::MailboxThroughput => run_mailbox(iters),
        Category::HttpServerThroughput => run_http(iters),
        Category::CompileToNative => run_compile(iters.clamp(1, 3)),
        Category::WasmSize => run_wasm_size(),
    }
}

fn finish(name: &str, samples: Vec<Duration>, extra: Option<serde_json::Value>) -> Sample {
    let mut s = samples;
    let m = mean(&s);
    let (p50, p95, p99) = percentiles(&mut s);
    Sample {
        category: name.into(),
        iters: s.len(),
        median_ns: p50.as_nanos(),
        mean_ns: m.as_nanos(),
        p95_ns: p95.as_nanos(),
        p99_ns: p99.as_nanos(),
        extra,
    }
}

fn run_parse(iters: usize) -> Sample {
    let src = mty_10kloc();
    let bytes = src.len();
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let s = src.clone();
        let t0 = Instant::now();
        let parsed = mty_driver::parse_source(s, "synth.mty".into());
        std::hint::black_box(&parsed);
        samples.push(t0.elapsed());
    }
    finish(
        "parse_throughput",
        samples,
        Some(serde_json::json!({ "bytes": bytes })),
    )
}

fn run_agent_send(iters: usize) -> Sample {
    use mty_runtime::mailbox::{Mailbox, MessageFrame, SendPolicy, SmallPayload};
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let dur = rt.block_on(async {
            let mb = Mailbox::new(8, SendPolicy::Block);
            let mut rx = mb.take_receiver().unwrap();
            let t0 = Instant::now();
            let frame = MessageFrame::fire_and_forget("Ping", SmallPayload::Empty);
            mb.send(frame).await.unwrap();
            let _r = rx.recv().await.unwrap();
            t0.elapsed()
        });
        samples.push(dur);
    }
    finish("agent_send_latency", samples, None)
}

fn run_mailbox(iters: usize) -> Sample {
    use mty_runtime::mailbox::{Mailbox, MessageFrame, SendPolicy, SmallPayload};
    const N: usize = 1_000;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let dur = rt.block_on(async {
            let mb = std::sync::Arc::new(Mailbox::new(N, SendPolicy::Block));
            let mut rx = mb.take_receiver().unwrap();
            let prod = {
                let mb = mb.clone();
                tokio::spawn(async move {
                    for _ in 0..N {
                        let f = MessageFrame::fire_and_forget("M", SmallPayload::Empty);
                        mb.send(f).await.unwrap();
                    }
                })
            };
            let t0 = Instant::now();
            for _ in 0..N {
                let _r = rx.recv().await.unwrap();
            }
            let d = t0.elapsed();
            prod.await.unwrap();
            d
        });
        samples.push(dur);
    }
    finish(
        "mailbox_throughput",
        samples,
        Some(serde_json::json!({ "messages_per_iter": N })),
    )
}

fn run_http(iters: usize) -> Sample {
    use mty_runtime::http::serve_in_memory;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()
        .unwrap();
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let dur = rt.block_on(async {
            let (h, port) = serve_in_memory(|_req| (200u16, "ok".to_string())).await;
            let addr = format!("127.0.0.1:{port}");
            let elapsed = mty_bench::http::single_get(&addr, "/").await.unwrap();
            h.abort();
            elapsed
        });
        samples.push(dur);
    }
    finish("http_server_throughput", samples, None)
}

fn run_compile(iters: usize) -> Sample {
    use mty_codegen_cranelift::artifact::BuildMode;
    use mty_codegen_wasm::WasmTarget;
    use mty_driver::{build_wasm, BuildOptions, BuildTarget};
    let src = mty_bench::fixtures::synth_source(100); // ~1 KLOC
    let mut samples = Vec::with_capacity(iters);
    for i in 0..iters {
        let tmp = tempfile::tempdir().unwrap();
        let opts = BuildOptions {
            target: BuildTarget::Wasm(WasmTarget::Wasi),
            mode: BuildMode::Release,
            out_dir: tmp.path().to_path_buf(),
            binary_name: format!("bench_{i}"),
            no_component: true,
        };
        let t0 = Instant::now();
        let _ = build_wasm(
            src.clone(),
            "compile_bench.mty".into(),
            &opts,
            WasmTarget::Wasi,
        );
        samples.push(t0.elapsed());
    }
    finish(
        "compile_to_native",
        samples,
        Some(serde_json::json!({
            "lines": mty_bench::fixtures::synth_source(100).lines().count(),
            "note": "uses wasm core backend as the stable codegen surface; native cranelift link path requires an external linker on Windows"
        })),
    )
}

fn run_wasm_size() -> Sample {
    use mty_codegen_cranelift::artifact::BuildMode;
    use mty_codegen_wasm::WasmTarget;
    use mty_driver::{build_wasm, BuildOptions, BuildOutcome, BuildTarget};
    let src = mty_bench::fixtures::synth_source(50);
    let tmp = tempfile::tempdir().unwrap();
    let opts = BuildOptions {
        target: BuildTarget::Wasm(WasmTarget::Wasi),
        mode: BuildMode::Release,
        out_dir: tmp.path().to_path_buf(),
        binary_name: "wasm_size_bench".into(),
        no_component: true,
    };
    let outcome = build_wasm(src, "wasm_size.mty".into(), &opts, WasmTarget::Wasi);
    let bytes = match outcome {
        BuildOutcome::WasmOk(p) => std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0),
        _ => 0,
    };
    // Size isn't really a latency sample; we record one fake "duration"
    // = bytes so the JSON shape stays uniform and document the special
    // case in `docs/benchmarks/wasm_size.md`.
    let sample = Duration::from_nanos(bytes);
    finish(
        "wasm_size",
        vec![sample],
        Some(serde_json::json!({
            "wasm_bytes": bytes,
            "note": "single-shot size measurement; not a latency"
        })),
    )
}
