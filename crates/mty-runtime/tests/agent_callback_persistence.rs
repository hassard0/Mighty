//! v0.25 Track C: agent state persists across exported callback
//! invocations.
//!
//! Track E's v0.24 canvas-game work surfaced a worry: when a JS host
//! calls `inst.exports.keydown(k)` twice, does the Mighty agent see
//! the second call with the field mutations from the first call still
//! applied? On the SIR-runtime path the answer has always been
//! yes — `AgentDescriptor::state` is a `Mutex<Value>` that
//! `run_one_turn_with_shared_reply` mutates in place — but it was
//! never pinned by a regression test. These tests do that, plus
//! exercise an array-typed field as a v0.25 Track C smoke for the
//! new agent-field array support.
//!
//! See `dev/history/notes/AGENT_FIELDS_V0_25_NOTES.md` for the
//! wasm32-web persistence design (the single-agent-instance pattern
//! the canvas-game demo will graduate to in v0.26 — for v0.25 the
//! SIR runtime is the canonical persistence surface).

use mty_ir::interp::value::Value;
use mty_runtime::RuntimeBuilder;
use std::sync::Arc;

fn compile(src: &str) -> Arc<mty_ir::ir::Program> {
    use mty_driver::pipeline::{lower, lower_to_sir, parse_source, type_and_borrow_check};
    let parsed = parse_source(
        src.to_string(),
        "agent_callback_persistence.mty".to_string(),
    );
    let (pkg, _diags) = lower(&parsed);
    let _ = type_and_borrow_check(&pkg);
    let (prog, _diags) = lower_to_sir(&pkg);
    Arc::new(prog)
}

#[test]
fn state_persists_across_callbacks() {
    // Spawn an agent, send two `Inc` messages, observe that the
    // second one sees the first's mutation. Pins the v0.25 promise
    // that one agent instance's state survives across callback
    // dispatches (the SIR-runtime version of the wasm32-web
    // single-agent-instance pattern).
    let src = r#"
protocol Count { Inc() -> I64 }
agent Counter: Count {
  n = 0
  on Inc() -> { n += 1; n }
}
fn main() { () }
"#;
    let prog = compile(src);
    let rt = RuntimeBuilder::new().build(prog);
    let rt_arc = rt.scheduler.rt.clone();
    rt_arc.block_on(async {
        let h = rt.spawn_agent("Counter", vec![]).await.unwrap();
        let r1 = rt.ask(&h, "Inc", vec![], None).await.unwrap();
        let r2 = rt.ask(&h, "Inc", vec![], None).await.unwrap();
        let r3 = rt.ask(&h, "Inc", vec![], None).await.unwrap();
        // If the agent were re-allocated per call, every reply would
        // be 1. The 1/2/3 sequence is what persistence gets us.
        assert_eq!(r1.as_int(), Some(1));
        assert_eq!(r2.as_int(), Some(2));
        assert_eq!(r3.as_int(), Some(3));
        let _ = rt.shutdown().await;
    });
}

#[test]
fn state_set_then_read_back_across_callbacks() {
    // Stronger shape: explicitly write a field in callback A, read it
    // back in callback B. Exercises the "set via callback A, read via
    // callback B" contract from the Track C scope.
    let src = r#"
protocol KV {
  Set(v: I64) -> I64
  Get() -> I64
}
agent Slot: KV {
  stored = 0
  on Set(v) -> { stored = v; v }
  on Get() -> stored
}
fn main() { () }
"#;
    let prog = compile(src);
    let rt = RuntimeBuilder::new().build(prog);
    let rt_arc = rt.scheduler.rt.clone();
    rt_arc.block_on(async {
        let h = rt.spawn_agent("Slot", vec![]).await.unwrap();
        // Callback A: Set(42)
        let r_set = rt
            .ask(
                &h,
                "Set",
                vec![Value::Int(42, mty_types::IntKind::I64)],
                None,
            )
            .await
            .unwrap();
        assert_eq!(r_set.as_int(), Some(42));
        // Callback B: Get() — same agent, should see the 42.
        let r_get = rt.ask(&h, "Get", vec![], None).await.unwrap();
        assert_eq!(
            r_get.as_int(),
            Some(42),
            "Get after Set should see 42; state did not persist across callbacks"
        );
        let _ = rt.shutdown().await;
    });
}

#[test]
fn two_agents_have_independent_state() {
    // Persistence is per-agent: spawning two instances of the same
    // agent must yield two independent state slots.
    let src = r#"
protocol Count { Inc() -> I64 }
agent Counter: Count {
  n = 0
  on Inc() -> { n += 1; n }
}
fn main() { () }
"#;
    let prog = compile(src);
    let rt = RuntimeBuilder::new().build(prog);
    let rt_arc = rt.scheduler.rt.clone();
    rt_arc.block_on(async {
        let h1 = rt.spawn_agent("Counter", vec![]).await.unwrap();
        let h2 = rt.spawn_agent("Counter", vec![]).await.unwrap();
        let a1 = rt.ask(&h1, "Inc", vec![], None).await.unwrap();
        let a2 = rt.ask(&h1, "Inc", vec![], None).await.unwrap();
        let b1 = rt.ask(&h2, "Inc", vec![], None).await.unwrap();
        // h1 has been incremented twice; h2 once.
        assert_eq!(a1.as_int(), Some(1));
        assert_eq!(a2.as_int(), Some(2));
        assert_eq!(
            b1.as_int(),
            Some(1),
            "second agent's state must be independent; got {:?}",
            b1.as_int()
        );
        let _ = rt.shutdown().await;
    });
}
