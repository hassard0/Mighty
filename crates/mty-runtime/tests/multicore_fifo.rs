//! v0.6 conformance: an agent's mailbox preserves FIFO order under
//! the multi-worker scheduler. Companion to
//! `tests/conformance/mailbox_ordering/06_multicore_fifo/`.

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
fn multi_worker_preserves_fifo_per_agent() {
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
        let h = rt.spawn_agent("Accumulator", vec![]).await.unwrap();

        // 100 sequential asks. Reply N should be the running sum 1..=N.
        // If the multi-worker scheduler reordered messages, the running
        // total would jump or skip — the assertion would catch it.
        let mut last = 0i128;
        for i in 1..=100u8 {
            let v = rt
                .ask(
                    &h,
                    "Add",
                    vec![Value::Int(1, mty_types::IntKind::I64)],
                    None,
                )
                .await
                .unwrap();
            match v {
                Value::Int(n, _) => {
                    assert_eq!(
                        n,
                        last + 1,
                        "FIFO broken at message {} got {} expected {}",
                        i,
                        n,
                        last + 1
                    );
                    last = n;
                }
                other => panic!("expected Int reply, got {:?}", other),
            }
        }
        assert_eq!(last, 100);
        let _ = rt.shutdown().await;
    });
}
