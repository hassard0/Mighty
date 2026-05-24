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
fn example_07_echo() {
    let prog = compile(include_str!("../../../examples/07_agent_echo.sd"));
    let rt = RuntimeBuilder::new().build(prog);
    let rt_arc = rt.scheduler.rt.clone();
    rt_arc.block_on(async {
        let h = rt.spawn_agent("Echoer", vec![]).await.unwrap();
        let reply = rt
            .ask(&h, "Ping", vec![Value::Str("hi".into())], None)
            .await
            .unwrap();
        match reply {
            Value::Str(s) => assert_eq!(s, "hi", "got back: {:?}", s),
            other => panic!("expected Str, got {:?}", other),
        }
        let _ = rt.shutdown().await;
    });
}

#[test]
fn example_08_counter() {
    let prog = compile(include_str!("../../../examples/08_agent_state.sd"));
    let rt = RuntimeBuilder::new().build(prog);
    let rt_arc = rt.scheduler.rt.clone();
    rt_arc.block_on(async {
        let h = rt.spawn_agent("Counter", vec![]).await.unwrap();
        let r1 = rt.ask(&h, "Inc", vec![], None).await.unwrap();
        let r2 = rt.ask(&h, "Inc", vec![], None).await.unwrap();
        let r3 = rt.ask(&h, "Inc", vec![], None).await.unwrap();
        // Slice-7: state mutation through (*self).fN now works via the
        // deref-of-ref write path, so the three asks should yield 1, 2, 3.
        for (n, v) in [(1_i128, r1), (2, r2), (3, r3)] {
            let i = v.as_int().expect("int reply");
            assert_eq!(i, n, "expected counter to read {n} got {:?}", i);
        }
        let _ = rt.shutdown().await;
    });
}

#[test]
fn deadline_short_circuits_on_unknown_handler() {
    // Sending an unknown message: the agent loop catches the
    // HandlerNotFound and the reply sender drops, returning an error.
    let src = r#"
protocol P { X() -> Str }
agent A: P { on X() -> "hi" }
fn main() { () }
"#;
    let prog = compile(src);
    let rt = RuntimeBuilder::new().build(prog);
    let rt_arc = rt.scheduler.rt.clone();
    rt_arc.block_on(async {
        let h = rt.spawn_agent("A", vec![]).await.unwrap();
        let r = rt
            .ask(&h, "Y", vec![], Some(std::time::Duration::from_millis(200)))
            .await;
        // Either: HandlerNotFound bubbles back as an error reply, or
        // the deadline fires. Both are valid slice-7 behaviours.
        assert!(r.is_err(), "expected error, got {:?}", r);
        let _ = rt.shutdown().await;
    });
}

#[test]
fn deadline_succeeds_for_fast_handler() {
    let prog = compile(include_str!("../../../examples/07_agent_echo.sd"));
    let rt = RuntimeBuilder::new().build(prog);
    let rt_arc = rt.scheduler.rt.clone();
    rt_arc.block_on(async {
        let h = rt.spawn_agent("Echoer", vec![]).await.unwrap();
        let reply = rt
            .ask(
                &h,
                "Ping",
                vec![Value::Str("fast".into())],
                Some(std::time::Duration::from_secs(2)),
            )
            .await
            .unwrap();
        assert!(matches!(reply, Value::Str(_)));
        let _ = rt.shutdown().await;
    });
}
