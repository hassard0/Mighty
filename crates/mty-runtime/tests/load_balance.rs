//! v0.6: a heavily loaded worker's elastic agents get migrated by the
//! load monitor to a lighter worker.

use mty_runtime::scheduler::{Affinity, LoadMonitor};
use mty_runtime::RuntimeBuilder;
use std::sync::Arc;

fn compile(src: &str) -> Arc<mty_ir::ir::Program> {
    use mty_driver::pipeline::{lower, lower_to_sir, parse_source, type_and_borrow_check};
    let parsed = parse_source(src.to_string(), "test.sd".to_string());
    let (pkg, _diags) = lower(&parsed);
    let _ = type_and_borrow_check(&pkg);
    let (prog, _diags) = lower_to_sir(&pkg);
    Arc::new(prog)
}

#[test]
fn overloaded_worker_triggers_migration_suggestion() {
    let src = r#"
protocol P { Ping() -> Str }
agent Echo: P { on Ping() -> "pong" }
fn main() { () }
"#;
    let prog = compile(src);
    let rt = RuntimeBuilder::new().workers(2).build(prog);
    let sched = rt.scheduler.clone();
    let driver = rt.scheduler.rt.clone();
    driver.block_on(async {
        // Pin an elastic agent to worker 0.
        let h = rt
            .spawn_agent_with_affinity("Echo", vec![], Affinity::Elastic)
            .await
            .unwrap();
        // The first elastic spawn lands on worker 0 (round-robin
        // counter starts at 0).
        let r = sched.route(h.id.0).unwrap();
        assert_eq!(r.affinity, Affinity::Elastic);
        let initial_worker = r.worker;

        // Synthetically skew the depth: bump worker `initial_worker`
        // far above the other so the monitor fires.
        for _ in 0..40 {
            sched.submit_to(
                initial_worker,
                mty_runtime::scheduler::SpawnTask {
                    id: 0,
                    affinity: Affinity::Elastic,
                    run: Box::new(|_h| {}),
                },
            );
        }

        let m = LoadMonitor::new(sched.clone());
        // sample_once may return None if workers already drained, so
        // retry a few times.
        let mut suggested = None;
        for _ in 0..20 {
            if let Some(s) = m.sample_once() {
                suggested = Some(s);
                break;
            }
            // Re-skew between attempts so the worker can't drain it
            // faster than we sample.
            for _ in 0..40 {
                sched.submit_to(
                    initial_worker,
                    mty_runtime::scheduler::SpawnTask {
                        id: 0,
                        affinity: Affinity::Elastic,
                        run: Box::new(|_h| {}),
                    },
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        // We just need *a* migration suggestion for the elastic agent
        // on initial_worker → the other worker.
        let (from, to, aid) = suggested.expect("monitor should have suggested a migration");
        assert_eq!(from, initial_worker);
        assert_ne!(to, initial_worker);
        assert_eq!(aid, h.id.0);

        let _ = rt.shutdown().await;
    });
}
