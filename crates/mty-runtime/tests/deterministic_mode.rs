//! v0.6: `workers(1) + deterministic(seed)` must reproduce v0.5
//! single-thread behavior byte-for-byte.

use mty_runtime::RuntimeBuilder;
use mty_ir::interp::value::Value;
use std::sync::Arc;

fn compile(src: &str) -> Arc<mty_ir::ir::Program> {
    use mty_driver::pipeline::{lower, lower_to_sir, parse_source, type_and_borrow_check};
    let parsed = parse_source(src.to_string(), "test.mty".to_string());
    let (pkg, _diags) = lower(&parsed);
    let _ = type_and_borrow_check(&pkg);
    let (prog, _diags) = lower_to_sir(&pkg);
    Arc::new(prog)
}

#[test]
fn deterministic_mode_uses_single_worker() {
    let src = r#"
protocol P { Ping() -> I64 }
agent Counter: P {
  n = 0
  on Ping() -> { n += 1; n }
}
fn main() { () }
"#;
    let prog = compile(src);
    let rt = RuntimeBuilder::new()
        .deterministic(42)
        .workers(1)
        .build(prog);
    assert_eq!(rt.scheduler.worker_count(), 1);
    assert!(rt.scheduler.deterministic);

    let driver = rt.scheduler.rt.clone();
    driver.block_on(async {
        let h = rt.spawn_agent("Counter", vec![]).await.unwrap();
        let r1 = rt.ask(&h, "Ping", vec![], None).await.unwrap();
        let r2 = rt.ask(&h, "Ping", vec![], None).await.unwrap();
        let r3 = rt.ask(&h, "Ping", vec![], None).await.unwrap();
        let pairs = [(1_i128, r1), (2, r2), (3, r3)];
        for (n, v) in pairs {
            match v {
                Value::Int(i, _) => assert_eq!(i, n),
                other => panic!("expected Int, got {:?}", other),
            }
        }
        let _ = rt.shutdown().await;
    });
}

#[test]
fn deterministic_mode_has_no_monitor() {
    let src = r#"fn main() { () }"#;
    let prog = compile(src);
    let rt = RuntimeBuilder::new().deterministic(7).build(prog);
    // Deterministic mode = no load monitor, single worker.
    assert!(rt.monitor.is_none());
    assert_eq!(rt.scheduler.worker_count(), 1);
    let driver = rt.scheduler.rt.clone();
    driver.block_on(async {
        let _ = rt.shutdown().await;
    });
}
