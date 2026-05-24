//! v0.6 cross-worker send: an agent pinned to worker A receives a
//! message sent from a context running on worker B.

use sdust_runtime::RuntimeBuilder;
use sdust_sir::interp::value::Value;
use std::sync::Arc;

fn compile(src: &str) -> Arc<sdust_sir::sir::Program> {
    use sdust_driver::pipeline::{lower, lower_to_sir, parse_source, type_and_borrow_check};
    let parsed = parse_source(src.to_string(), "test.sd".to_string());
    let (pkg, _diags) = lower(&parsed);
    let _ = type_and_borrow_check(&pkg);
    let (prog, _diags) = lower_to_sir(&pkg);
    Arc::new(prog)
}

#[test]
fn agent_on_worker_b_receives_send_from_worker_a() {
    let src = r#"
protocol P { Ping() -> Str }
agent Echo: P {
  on Ping() -> "pong"
}
fn main() { () }
"#;
    let prog = compile(src);
    let rt = RuntimeBuilder::new().workers(4).build(prog);
    let driver = rt.scheduler.rt.clone();
    driver.block_on(async {
        // Spawn several agents — round-robin pins them to different
        // workers. With 4 workers and 4 agents we get one per worker.
        let mut handles = Vec::new();
        for _ in 0..4 {
            handles.push(rt.spawn_agent("Echo", vec![]).await.unwrap());
        }
        // Verify the routes hit different workers (at least 2).
        let mut workers_used = std::collections::HashSet::new();
        for h in &handles {
            if let Some(route) = rt.scheduler.route(h.id.0) {
                workers_used.insert(route.worker);
            }
        }
        assert!(
            workers_used.len() >= 2,
            "expected agents to spread across workers, got {:?}",
            workers_used
        );
        // Now ask each — all should reply "pong" even though the call
        // originates from the driver runtime (a separate runtime
        // entirely from the worker runtimes hosting the agent loops).
        for h in &handles {
            let r = rt.ask(h, "Ping", vec![], None).await.unwrap();
            match r {
                Value::Str(s) => assert_eq!(s, "pong"),
                other => panic!("expected Str, got {:?}", other),
            }
        }
        let _ = rt.shutdown().await;
    });
}

#[test]
fn single_worker_mode_still_works() {
    // workers(1) should reproduce v0.5 single-thread behavior.
    let src = r#"
protocol P { Ping() -> Str }
agent Echo: P {
  on Ping() -> "pong"
}
fn main() { () }
"#;
    let prog = compile(src);
    let rt = RuntimeBuilder::new().workers(1).build(prog);
    let driver = rt.scheduler.rt.clone();
    driver.block_on(async {
        let h = rt.spawn_agent("Echo", vec![]).await.unwrap();
        let r = rt.ask(&h, "Ping", vec![], None).await.unwrap();
        assert!(matches!(r, Value::Str(s) if s == "pong"));
        let _ = rt.shutdown().await;
    });
}
