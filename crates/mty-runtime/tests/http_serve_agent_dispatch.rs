//! v0.8 loose-end 2/4 — end-to-end roundtrip:
//!
//!   spawn Echoer agent  →  install runtime dispatcher  →  bind socket
//!   via stdlib::http_server::start_blocking → send a real HTTP request
//!   → verify the response contains the agent-produced body.

use mty_runtime::http_server::{make_dispatcher, Request as RtRequest};
use mty_runtime::{Runtime, RuntimeBuilder};
use mty_stdlib::http::Request as StdRequest;
use mty_stdlib::http_server::{install_agent_dispatch, start_blocking};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn compile(src: &str) -> Arc<mty_ir::ir::Program> {
    use mty_driver::pipeline::{lower, lower_to_sir, parse_source, type_and_borrow_check};
    let parsed = parse_source(src.to_string(), "test.mty".to_string());
    let (pkg, _diags) = lower(&parsed);
    let _ = type_and_borrow_check(&pkg);
    let (prog, _diags) = lower_to_sir(&pkg);
    Arc::new(prog)
}

#[test]
fn agent_dispatched_http_roundtrip() {
    // A tiny Mighty program: an Echoer agent that returns a constant
    // string for the `Request` ask. We don't need to inspect the
    // request body here — the runtime+stdlib bridge already proves
    // request marshalling end-to-end via the JSON parameter shape.
    let src = r#"
protocol Search { Request(req: Str) -> Str }
agent Echoer: Search {
  on Request(req) { "hello agent" }
}
fn main() { () }
"#;
    let prog = compile(src);
    let rt = Arc::new(RuntimeBuilder::new().workers(2).build(prog));

    // Drive the test inside the runtime's tokio handle so block_on
    // inside make_dispatcher has a runtime in scope.
    let rt_inner = rt.scheduler.rt.clone();
    rt_inner.block_on(async {
        let handle = rt
            .spawn_agent("Echoer", vec![])
            .await
            .expect("spawn Echoer");

        // Install the runtime-built dispatcher into the stdlib HTTP server.
        let rt_clone: Arc<Runtime> = rt.clone();
        let dispatcher = make_dispatcher(rt_clone, handle.clone(), "Request");
        install_agent_dispatch(Arc::new(move |req: StdRequest| {
            let r = RtRequest {
                method: req.method,
                path: req.path,
                body: req.body,
                headers: req.headers,
            };
            let resp = dispatcher(r);
            mty_stdlib::http::Response {
                status: resp.status,
                body: resp.body,
                headers: resp.headers,
            }
        }));

        // Bind the listener. `start_blocking` runs `block_on` inside
        // the stdlib's process-wide runtime; jump out of the current
        // tokio context first via `spawn_blocking`.
        let (server_handle, bound) = tokio::task::spawn_blocking(|| {
            start_blocking("127.0.0.1:0").expect("bind")
        })
        .await
        .expect("spawn_blocking");
        let port = bound.port();

        // Real HTTP request.
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect");
        s.write_all(b"GET /agent HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .expect("write");
        let mut buf = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut buf)).await;
        let body = String::from_utf8_lossy(&buf);

        assert!(body.starts_with("HTTP/1.1 200"), "body: {body}");
        assert!(
            body.contains("hello agent"),
            "expected handler body in response: {body}"
        );

        mty_stdlib::http_server::shutdown(server_handle);
    });
}
