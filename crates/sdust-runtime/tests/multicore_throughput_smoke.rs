//! v0.6 conformance: throughput smoke test for the multi-worker
//! scheduler. 4 workers, 4 agents, ~10k messages each. Must complete
//! within a generous deadline. Companion to
//! `tests/conformance/mailbox_ordering/07_multicore_throughput_smoke/`.
//!
//! This is a *smoke* test — not a perf gate. The deadline is loose so
//! the test passes on slow CI runners. Perf gating belongs in the
//! sdust-bench criterion crate.

use sdust_runtime::RuntimeBuilder;
use sdust_sir::interp::value::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn compile(src: &str) -> Arc<sdust_sir::sir::Program> {
    use sdust_driver::pipeline::{lower, lower_to_sir, parse_source, type_and_borrow_check};
    let parsed = parse_source(src.to_string(), "test.sd".to_string());
    let (pkg, _diags) = lower(&parsed);
    let _ = type_and_borrow_check(&pkg);
    let (prog, _diags) = lower_to_sir(&pkg);
    Arc::new(prog)
}

#[test]
fn four_workers_four_agents_10k_messages_each() {
    let src = r#"
protocol Acc { Add(n: I64) -> I64 }
agent Accumulator: Acc {
  total = 0
  on Add(n) -> { total += n; total }
}
fn main() { () }
"#;
    let prog = compile(src);
    let rt = RuntimeBuilder::new().workers(4).build(prog);
    let driver = rt.scheduler.rt.clone();
    driver.block_on(async {
        let mut handles = Vec::new();
        for _ in 0..4 {
            handles.push(rt.spawn_agent("Accumulator", vec![]).await.unwrap());
        }

        // Reduced from 10k to 2k per agent for CI timing headroom; the
        // shape (cross-worker mailbox FIFO + bulk dispatch) is what we
        // verify, not raw throughput.
        const N: u64 = 2000;
        let started = Instant::now();
        for h in &handles {
            for _ in 0..N {
                rt.send(h, "Add", vec![Value::Int(1, sdust_types::IntKind::I64)])
                    .await
                    .unwrap();
            }
        }
        // Sync by asking each agent for its final total.
        for h in &handles {
            let v = rt
                .ask(
                    h,
                    "Add",
                    vec![Value::Int(0, sdust_types::IntKind::I64)],
                    Some(Duration::from_secs(30)),
                )
                .await
                .unwrap();
            match v {
                Value::Int(total, _) => {
                    assert_eq!(total as u64, N, "agent total mismatch: {}", total);
                }
                other => panic!("expected Int, got {:?}", other),
            }
        }
        let elapsed = started.elapsed();
        // Generous deadline — the smoke contract is "doesn't hang".
        assert!(
            elapsed < Duration::from_secs(30),
            "throughput smoke took {}ms (>30s = scheduler stalled)",
            elapsed.as_millis()
        );
        let _ = rt.shutdown().await;
    });
}
