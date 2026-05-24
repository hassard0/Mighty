//! v0.5 dogfood Gap-1 — process-wide HTTP server registry that backs
//! the `std.http.serve` host bridge.
//!
//! ## Why a registry?
//!
//! The SIR interpreter's dispatcher signature is
//! `(path, method, args) -> Value`. It has no `&mut Interp`, so it
//! cannot directly invoke an agent handler when a request lands. We
//! solve the impedance mismatch in two layers:
//!
//! 1. **This module** owns a tokio runtime + an `Arc<Mutex<…>>` map
//!    of `(handle_id, sender)` pairs. `start_blocking(addr)` binds a
//!    TCP socket, spawns the accept loop, and returns
//!    `(handle_id, bound_addr)`. The accept loop hands every
//!    incoming `Request` to the *currently installed*
//!    [`AgentDispatch`] callback (see [`install_agent_dispatch`]).
//! 2. **The runtime** installs an `AgentDispatch` that knows how to
//!    look up the owning agent, post a synthetic `Request` ask, and
//!    marshal the agent's reply back as an HTTP response. Until that
//!    runtime hook lands (post-v0.5), this module ships a default
//!    dispatcher that returns `200 OK` with a deterministic body
//!    so the bound-socket smoke tests still pass.
//!
//! The crate is intentionally tokio-driven (not the v0.4 single-shot
//! `block_on` shape used by `http_get`) so a single server thread can
//! handle many concurrent connections.

use crate::http::{Request, Response};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::oneshot;

/// Synchronous agent-dispatch callback. The runtime installs a real
/// implementation via [`install_agent_dispatch`]; the default is a
/// 200 OK echo so dogfood smoke tests pass even without a runtime
/// integration.
pub type AgentDispatch = Arc<dyn Fn(Request) -> Response + Send + Sync>;

struct ServerEntry {
    bound: SocketAddr,
    shutdown_tx: oneshot::Sender<()>,
}

struct Registry {
    next_handle: AtomicU64,
    entries: Mutex<HashMap<u64, ServerEntry>>,
    dispatch: Mutex<AgentDispatch>,
    rt: tokio::runtime::Runtime,
}

fn default_dispatch() -> AgentDispatch {
    Arc::new(|req: Request| {
        // Default: deterministic 200 echo. The body includes the
        // method and path so the v0.5 smoke test can assert a real
        // roundtrip even without a runtime integration.
        let body = format!(
            "{{\"method\":\"{}\",\"path\":\"{}\",\"status\":\"ok\"}}",
            req.method, req.path
        );
        Response {
            status: 200,
            body: body.into_bytes(),
            headers: vec![("content-type".into(), "application/json".into())],
        }
    })
}

fn registry() -> &'static Registry {
    static R: OnceLock<Registry> = OnceLock::new();
    R.get_or_init(|| {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("http_server: build tokio runtime");
        Registry {
            next_handle: AtomicU64::new(1),
            entries: Mutex::new(HashMap::new()),
            dispatch: Mutex::new(default_dispatch()),
            rt,
        }
    })
}

/// Replace the process-wide agent dispatcher. The runtime calls this
/// once per process at startup with a closure that posts incoming
/// requests into the owning agent's mailbox.
pub fn install_agent_dispatch(d: AgentDispatch) {
    let r = registry();
    let mut g = r.dispatch.lock().expect("dispatch poisoned");
    *g = d;
}

/// Bind `addr`, spawn the accept loop, and return
/// `(handle_id, bound_socket_addr)`. Blocks until the TCP listener is
/// bound and ready to accept; the accept loop runs in the background
/// tokio runtime until [`shutdown`] is called with the same handle.
pub fn start_blocking(addr: &str) -> Result<(u64, SocketAddr), String> {
    use tokio::net::TcpListener;
    let r = registry();
    let addr_owned = addr.to_string();
    // Bind on the runtime thread so we block until it's ready and
    // get back the real bound port.
    let listener = r.rt.block_on(async move {
        TcpListener::bind(&addr_owned)
            .await
            .map_err(|e| format!("bind {}: {}", addr_owned, e))
    })?;
    let bound = listener.local_addr().map_err(|e| e.to_string())?;

    let (tx, rx) = oneshot::channel::<()>();
    let dispatcher = {
        let g = r.dispatch.lock().expect("dispatch poisoned");
        g.clone()
    };
    r.rt.spawn(accept_loop(listener, dispatcher, rx));

    let handle_id = r.next_handle.fetch_add(1, Ordering::Relaxed);
    {
        let mut g = r.entries.lock().expect("entries poisoned");
        g.insert(
            handle_id,
            ServerEntry {
                bound,
                shutdown_tx: tx,
            },
        );
    }
    Ok((handle_id, bound))
}

/// Send a shutdown signal to the server identified by `handle_id`. The
/// accept loop drains in-flight connections then exits. Returns `true`
/// if the handle existed, `false` otherwise.
pub fn shutdown(handle_id: u64) -> bool {
    let r = registry();
    let entry = {
        let mut g = r.entries.lock().expect("entries poisoned");
        g.remove(&handle_id)
    };
    match entry {
        Some(e) => {
            let _ = e.shutdown_tx.send(());
            true
        }
        None => false,
    }
}

/// Look up the bound socket address for a live handle. Useful when the
/// caller passed `:0` and wants to discover the OS-assigned port.
pub fn bound_addr(handle_id: u64) -> Option<SocketAddr> {
    let r = registry();
    let g = r.entries.lock().expect("entries poisoned");
    g.get(&handle_id).map(|e| e.bound)
}

async fn accept_loop(
    listener: tokio::net::TcpListener,
    dispatcher: AgentDispatch,
    mut shutdown_rx: oneshot::Receiver<()>,
) {
    use http_body_util::{BodyExt, Full};
    use hyper::body::{Bytes, Incoming};
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper::{Request as HReq, Response as HResp};
    use hyper_util::rt::TokioIo;
    use std::convert::Infallible;

    loop {
        tokio::select! {
            _ = &mut shutdown_rx => return,
            r = listener.accept() => {
                let (stream, _peer) = match r {
                    Ok(x) => x,
                    Err(_) => return,
                };
                let d = dispatcher.clone();
                tokio::spawn(async move {
                    let io = TokioIo::new(stream);
                    let svc = service_fn(move |req: HReq<Incoming>| {
                        let d = d.clone();
                        async move {
                            let method = req.method().to_string();
                            let path = req
                                .uri()
                                .path_and_query()
                                .map(|p| p.as_str().to_string())
                                .unwrap_or_default();
                            let headers: Vec<(String, String)> = req
                                .headers()
                                .iter()
                                .map(|(k, v)| (
                                    k.as_str().to_string(),
                                    v.to_str().unwrap_or("").to_string(),
                                ))
                                .collect();
                            let body = req
                                .into_body()
                                .collect()
                                .await
                                .map(|c| c.to_bytes().to_vec())
                                .unwrap_or_default();
                            let star_req = Request {
                                method,
                                path,
                                body,
                                headers,
                            };
                            let resp = d(star_req);
                            let mut builder = HResp::builder().status(resp.status);
                            for (k, v) in &resp.headers {
                                builder = builder.header(k, v);
                            }
                            let h = builder
                                .body(Full::new(Bytes::from(resp.body)))
                                .unwrap_or_else(|_| {
                                    HResp::builder()
                                        .status(500)
                                        .body(Full::new(Bytes::from_static(b"")))
                                        .expect("500 always builds")
                                });
                            Ok::<_, Infallible>(h)
                        }
                    });
                    let _ = http1::Builder::new().serve_connection(io, svc).await;
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_and_shutdown_roundtrip() {
        let (h, bound) = start_blocking("127.0.0.1:0").expect("bind");
        assert!(bound.port() != 0);
        assert!(shutdown(h));
        // Second shutdown is a no-op.
        assert!(!shutdown(h));
    }

    #[test]
    fn bound_addr_returns_some_when_live() {
        let (h, bound) = start_blocking("127.0.0.1:0").expect("bind");
        assert_eq!(bound_addr(h), Some(bound));
        shutdown(h);
        assert!(bound_addr(h).is_none());
    }
}
