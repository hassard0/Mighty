//! Integration tests for v0.16 introspection (Tier 1.1).
//!
//! These exercise the snapshot/agent-state machinery + the control
//! socket. Compiled programs come through `mty_driver::pipeline` like
//! every other runtime integration test in this crate.

use mty_runtime::control_socket::{ControlContext, Request, Response};
use mty_runtime::introspect::{snapshot_runtime, AgentIntrospectState, SNAPSHOT_WIRE_VERSION};
use mty_runtime::{AgentId, IntrospectMap, RuntimeBuilder};
use std::sync::Arc;

fn compile(src: &str) -> Arc<mty_ir::ir::Program> {
    use mty_driver::pipeline::{lower, lower_to_sir, parse_source, type_and_borrow_check};
    let parsed = parse_source(src.to_string(), "test.mty".to_string());
    let (pkg, _diags) = lower(&parsed);
    let _ = type_and_borrow_check(&pkg);
    let (prog, _diags) = lower_to_sir(&pkg);
    Arc::new(prog)
}

const ECHO_SRC: &str = r#"
protocol P { Ping() -> Str }
agent Echo: P {
  on Ping() -> "pong"
}
fn main() { () }
"#;

#[test]
fn snapshot_includes_live_agent_with_correct_type() {
    let prog = compile(ECHO_SRC);
    let rt = RuntimeBuilder::new().workers(1).build(prog);
    let driver = rt.scheduler.rt.clone();
    driver.block_on(async {
        let _h = rt.spawn_agent("Echo", vec![]).await.unwrap();
        // Snapshot pre-ask: agent should be live + idle.
        let snap = snapshot_runtime(&rt.registry, &rt.introspect, rt.scheduler.worker_count());
        assert_eq!(snap.version, SNAPSHOT_WIRE_VERSION);
        assert_eq!(snap.agents.len(), 1);
        assert_eq!(snap.agents[0].agent_type, "Echo");
        // The agent has no in-flight handler at this exact point.
        assert!(snap.agents[0].in_flight_handler.is_none());
        let _ = rt.shutdown().await;
    });
}

#[test]
fn snapshot_disabled_without_env() {
    // No env var -> sock_path_from_env() returns None.
    std::env::remove_var(mty_runtime::CONTROL_SOCK_ENV);
    assert!(mty_runtime::sock_path_from_env().is_none());
}

#[test]
fn agent_id_lookup_works() {
    let prog = compile(ECHO_SRC);
    let rt = RuntimeBuilder::new().workers(2).build(prog);
    let driver = rt.scheduler.rt.clone();
    driver.block_on(async {
        let h1 = rt.spawn_agent("Echo", vec![]).await.unwrap();
        let h2 = rt.spawn_agent("Echo", vec![]).await.unwrap();
        assert_ne!(h1.id.0, h2.id.0);
        let ctx = ControlContext {
            registry: rt.registry.clone(),
            introspect: rt.introspect.clone(),
            worker_count: rt.scheduler.worker_count(),
        };
        match ctx.handle(Request::SnapshotAgent { id: h1.id.0 }) {
            Response::Agent(a) => assert_eq!(a.agent_id, h1.id.0),
            other => panic!("expected Agent, got {:?}", other),
        }
        match ctx.handle(Request::SnapshotAgent { id: 99999 }) {
            Response::Error { error, .. } => assert_eq!(error, "not_found"),
            other => panic!("expected Error, got {:?}", other),
        }
        let _ = rt.shutdown().await;
    });
}

#[test]
fn list_op_enumerates_live_agents() {
    let prog = compile(ECHO_SRC);
    let rt = RuntimeBuilder::new().workers(1).build(prog);
    let driver = rt.scheduler.rt.clone();
    driver.block_on(async {
        let _h1 = rt.spawn_agent("Echo", vec![]).await.unwrap();
        let _h2 = rt.spawn_agent("Echo", vec![]).await.unwrap();
        let ctx = ControlContext {
            registry: rt.registry.clone(),
            introspect: rt.introspect.clone(),
            worker_count: rt.scheduler.worker_count(),
        };
        match ctx.handle(Request::List) {
            Response::List { agents } => {
                assert_eq!(agents.len(), 2);
                assert!(agents.iter().all(|e| e.agent_type == "Echo"));
            }
            other => panic!("expected List, got {:?}", other),
        }
        let _ = rt.shutdown().await;
    });
}

#[test]
fn introspect_state_high_water_tracks_max() {
    // White-box: snapshot reads live channel-depth + a CAS-tracked
    // high-water. Push 4 enqueues, drain 3, and confirm the
    // high-water sticks at 4 while live depth is 1.
    let st = AgentIntrospectState::default();
    for _ in 0..4 {
        st.note_enqueue();
    }
    for _ in 0..3 {
        st.note_dequeue();
    }
    use std::sync::atomic::Ordering;
    assert_eq!(st.mailbox_depth.load(Ordering::Relaxed), 1);
    assert_eq!(st.mailbox_high_water.load(Ordering::Relaxed), 4);
}

#[test]
fn snapshot_serializes_to_json() {
    let prog = compile(ECHO_SRC);
    let rt = RuntimeBuilder::new().workers(1).build(prog);
    let driver = rt.scheduler.rt.clone();
    driver.block_on(async {
        let _h = rt.spawn_agent("Echo", vec![]).await.unwrap();
        let snap = snapshot_runtime(&rt.registry, &rt.introspect, rt.scheduler.worker_count());
        let json = serde_json::to_string(&snap).expect("snapshot serializes");
        // Round-trip: must parse back into the same shape.
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["version"], 1);
        assert_eq!(v["agents"][0]["agent_type"], "Echo");
        let _ = rt.shutdown().await;
    });
}

#[cfg(unix)]
#[test]
fn control_socket_responds_to_snapshot_op() {
    use mty_runtime::control_socket::spawn_control_socket_at;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let prog = compile(ECHO_SRC);
    let rt = RuntimeBuilder::new().workers(1).build(prog);
    let driver = rt.scheduler.rt.clone();
    // Unique temp path so parallel test runs don't collide.
    let pid = std::process::id();
    let path = format!("/tmp/mty-introspect-test-{pid}.sock");
    let _ = std::fs::remove_file(&path);

    driver.block_on(async {
        let _h = rt.spawn_agent("Echo", vec![]).await.unwrap();
        let ctx = ControlContext {
            registry: rt.registry.clone(),
            introspect: rt.introspect.clone(),
            worker_count: rt.scheduler.worker_count(),
        };
        let handle = spawn_control_socket_at(ctx, tokio::runtime::Handle::current(), &path)
            .expect("control socket spawn");
        // Give the listener a tick to bind. The accept loop is
        // immediately ready once bound, so a 50 ms wait is generous.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Use blocking std-net for the client — keeps the test simple.
        let path2 = path.clone();
        let join = tokio::task::spawn_blocking(move || {
            let mut s = UnixStream::connect(&path2).expect("connect");
            s.write_all(b"{\"op\":\"snapshot\"}\n").expect("write");
            let mut r = BufReader::new(s);
            let mut line = String::new();
            r.read_line(&mut line).expect("read");
            line
        });
        let line = join.await.expect("client task");
        let v: serde_json::Value = serde_json::from_str(line.trim()).expect("decode");
        assert_eq!(v["version"], 1);
        assert!(v["agents"].is_array());
        assert_eq!(v["agents"][0]["agent_type"], "Echo");
        handle.task.abort();
        let _ = std::fs::remove_file(&path);
        let _ = rt.shutdown().await;
    });
}

#[test]
fn map_insert_get_remove_round_trip() {
    let map = IntrospectMap::new();
    let id = AgentId(7);
    let st = Arc::new(AgentIntrospectState::default());
    map.insert(id.0, st.clone());
    assert!(map.get(id.0).is_some());
    assert_eq!(map.len(), 1);
    map.remove(id.0);
    assert!(map.get(id.0).is_none());
    assert!(map.is_empty());
}
