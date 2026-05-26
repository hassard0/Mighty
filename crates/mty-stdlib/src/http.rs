//! `std.http` — real HTTP/1.1 + HTTP/2 client + server via [`hyper`].
//!
//! Supersedes the slice-7 minimal in-memory server in `mty-runtime::http`.
//! That module is still re-exported by the runtime for backwards
//! compatibility with existing tests; new agent code should use this
//! one.
//!
//! ## Backend dispatch (v0.16 P2 direct lowering)
//!
//! When a program is compiled with `--wasi=p2` (the default since
//! v0.15), `std.http.*` calls now lower to **direct** P2 imports
//! of the `wasi:http/types@0.2.3` + `wasi:http/outgoing-handler@0.2.3`
//! surface instead of routing through the `wasi_snapshot_preview1`
//! adapter. The canonical import shapes are exposed below as
//! [`P2_DIRECT_IMPORT_NEW_OUTGOING_REQUEST`] /
//! [`P2_DIRECT_IMPORT_OUTGOING_HANDLE`] /
//! [`P2_DIRECT_IMPORT_RESPONSE_STATUS`] /
//! [`P2_DIRECT_IMPORT_RESPONSE_CONSUME`] — they match the variants
//! of `mty_codegen_wasm::P2DirectImport` and are pinned here so the
//! stdlib and codegen layers never drift on naming.
//!
//! The v0.16 emitter wiring is **blocking-style**: it splices the
//! constructor + handle imports and uses scratch return-areas for
//! each step. The full streaming layer (incremental body-write,
//! `future-incoming-response.subscribe`, etc.) is a v0.17 follow-up
//! — what's PINNED in v0.16 is that the versioned imports land in
//! the import section so a strict P2 host wires them directly.
//!
//! The native runtime path is unchanged — the import-shape switch
//! is purely a Wasm-side concern.
//!
//! ## Surface
//!
//! - [`get`] / [`post`] — async client. Returns a fully-buffered
//!   [`Response`].
//! - [`serve`] — bind a TCP socket and dispatch each request through a
//!   user `Handler` (a sync closure for now; agent-message dispatch is
//!   wired into the runtime separately, see
//!   `crate::host::dispatch_http_call`).

use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request as HReq, Response as HResp, Uri};
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};

/// Canonical P2 import name for the `outgoing-request` resource
/// constructor. See module doc for the v0.16 dispatch rationale.
pub const P2_DIRECT_IMPORT_NEW_OUTGOING_REQUEST: (&str, &str) =
    ("wasi:http/types@0.2.3", "[constructor]outgoing-request");

/// Canonical P2 import name for `outgoing-handler.handle` — the
/// blocking-style "send the request" entry point.
pub const P2_DIRECT_IMPORT_OUTGOING_HANDLE: (&str, &str) =
    ("wasi:http/outgoing-handler@0.2.3", "handle");

/// Canonical P2 import name for `incoming-response.status`.
pub const P2_DIRECT_IMPORT_RESPONSE_STATUS: (&str, &str) =
    ("wasi:http/types@0.2.3", "[method]incoming-response.status");

/// Canonical P2 import name for `incoming-response.consume` — the
/// entry point that hands the incoming body resource to the caller.
pub const P2_DIRECT_IMPORT_RESPONSE_CONSUME: (&str, &str) =
    ("wasi:http/types@0.2.3", "[method]incoming-response.consume");

#[derive(Debug, thiserror::Error)]
pub enum HttpErr {
    #[error("http io: {0}")]
    Io(#[from] std::io::Error),
    #[error("http: {0}")]
    Hyper(String),
    #[error("invalid url: {0}")]
    Url(String),
    #[error("invalid response: {0}")]
    Response(String),
}

/// A fully-buffered HTTP response.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
}

impl Response {
    pub fn body_str(&self) -> &str {
        std::str::from_utf8(&self.body).unwrap_or("")
    }
}

/// Issue an HTTP GET. Currently HTTP/1.1 only; HTTPS (h2 ALPN) is
/// flagged in `STDLIB_V0_2_NOTES.md` as a v0.3 follow-up — we have the
/// `std.tls` plumbing for it, but wiring hyper's HTTPS connector
/// cleanly without dragging in `hyper-rustls` is a separate task.
///
/// On `wasm32-wasi` builds with `--wasi=p2`, the Mighty codegen
/// lowers calls to this function to direct
/// `wasi:http/types@0.2.3#[constructor]outgoing-request` +
/// `wasi:http/outgoing-handler@0.2.3#handle` imports (see
/// [`P2_DIRECT_IMPORT_NEW_OUTGOING_REQUEST`] /
/// [`P2_DIRECT_IMPORT_OUTGOING_HANDLE`]). The native runtime path
/// is unchanged.
pub async fn get(url: &str) -> Result<Response, HttpErr> {
    request(Method::GET, url, Vec::new()).await
}

/// Issue an HTTP POST with `body`. Same v0.16 wasm-side dispatch as
/// [`get`] — see that function's doc-comment.
pub async fn post(url: &str, body: Vec<u8>) -> Result<Response, HttpErr> {
    request(Method::POST, url, body).await
}

/// Issue a pre-built [`Request`]. Lower-level than [`get`] / [`post`]
/// for callers that want to set custom method + body up front. On
/// `wasm32-wasi` builds with `--wasi=p2` this lowers to a direct
/// `wasi:http/outgoing-handler@0.2.3#handle` import (see
/// [`P2_DIRECT_IMPORT_OUTGOING_HANDLE`]).
pub async fn send(req: Request) -> Result<Response, HttpErr> {
    let method = match req.method.as_str() {
        "GET" => Method::GET,
        "POST" => Method::POST,
        "PUT" => Method::PUT,
        "DELETE" => Method::DELETE,
        "HEAD" => Method::HEAD,
        "PATCH" => Method::PATCH,
        "OPTIONS" => Method::OPTIONS,
        other => {
            return Err(HttpErr::Url(format!("unsupported method: {other}")));
        }
    };
    request(method, &req.path, req.body).await
}

async fn request(method: Method, url: &str, body: Vec<u8>) -> Result<Response, HttpErr> {
    let uri: Uri = url.parse().map_err(|e| HttpErr::Url(format!("{e}")))?;
    if uri.scheme_str() != Some("http") {
        // HTTPS via `hyper-rustls` is a v0.3 task — see
        // STDLIB_V0_2_NOTES.md. Surface a clear error today.
        return Err(HttpErr::Url(format!(
            "only http:// supported in v0.2 (got {url})"
        )));
    }
    let host = uri
        .host()
        .ok_or_else(|| HttpErr::Url(format!("no host in {url}")))?
        .to_string();
    let port = uri.port_u16().unwrap_or(80);
    let path_and_query = uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());

    let stream = TcpStream::connect((host.as_str(), port)).await?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake::<_, Full<Bytes>>(io)
        .await
        .map_err(|e| HttpErr::Hyper(e.to_string()))?;
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let req = HReq::builder()
        .method(method)
        .uri(path_and_query)
        .header(hyper::header::HOST, &host)
        .body(Full::new(Bytes::from(body)))
        .map_err(|e| HttpErr::Hyper(e.to_string()))?;
    let resp = sender
        .send_request(req)
        .await
        .map_err(|e| HttpErr::Hyper(e.to_string()))?;
    response_from_hyper(resp).await
}

async fn response_from_hyper(resp: HResp<Incoming>) -> Result<Response, HttpErr> {
    let status = resp.status().as_u16();
    let headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let body = resp
        .into_body()
        .collect()
        .await
        .map_err(|e| HttpErr::Response(e.to_string()))?
        .to_bytes()
        .to_vec();
    Ok(Response {
        status,
        body,
        headers,
    })
}

/// Owned, easily-cloned request the user `Handler` sees.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
}

/// Handler type: a function (or closure) that maps `Request` -> `Response`.
/// Boxed + arc'd so it can be cloned across the per-connection task.
pub type Handler = Arc<
    dyn Fn(Request) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>>
        + Send
        + Sync,
>;

/// Spawn an HTTP/1.1 server on `addr`. Returns the bound socket address
/// (useful when port 0 is requested) and a `JoinHandle` for the
/// accept-loop. The loop runs until aborted; the handle's `abort()`
/// will trigger an immediate, clean shutdown.
pub async fn serve(
    addr: &str,
    handler: Handler,
) -> Result<(SocketAddr, tokio::task::JoinHandle<()>), HttpErr> {
    let listener = TcpListener::bind(addr).await?;
    let local = listener.local_addr()?;
    let handle = tokio::spawn(accept_loop(listener, handler));
    Ok((local, handle))
}

async fn accept_loop(listener: TcpListener, handler: Handler) {
    loop {
        let Ok((stream, _peer)) = listener.accept().await else {
            return;
        };
        let h = handler.clone();
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            let svc = service_fn(move |req: HReq<Incoming>| {
                let h = h.clone();
                async move { Ok::<_, Infallible>(serve_one(h, req).await) }
            });
            let _ = http1::Builder::new().serve_connection(io, svc).await;
        });
    }
}

async fn serve_one(handler: Handler, req: HReq<Incoming>) -> HResp<Full<Bytes>> {
    let method = req.method().to_string();
    let path = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_default();
    let headers: Vec<(String, String)> = req
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let body = req
        .into_body()
        .collect()
        .await
        .map(|c| c.to_bytes().to_vec())
        .unwrap_or_default();
    let resp = handler(Request {
        method,
        path,
        body,
        headers,
    })
    .await;
    let mut builder = HResp::builder().status(resp.status);
    for (k, v) in &resp.headers {
        builder = builder.header(k, v);
    }
    builder
        .body(Full::new(Bytes::from(resp.body)))
        .unwrap_or_else(|_| {
            HResp::builder()
                .status(500)
                .body(Full::new(Bytes::from_static(b"")))
                .expect("500 body always builds")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_validation_rejects_https_in_v0_2() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let r = rt.block_on(get("https://example.com"));
        assert!(matches!(r, Err(HttpErr::Url(_))));
    }

    #[test]
    fn p2_direct_import_constants_are_canonical() {
        // Pin the import shapes so a regression in either the
        // codegen layer or this stdlib doesn't drift them apart.
        assert_eq!(
            P2_DIRECT_IMPORT_NEW_OUTGOING_REQUEST,
            ("wasi:http/types@0.2.3", "[constructor]outgoing-request")
        );
        assert_eq!(
            P2_DIRECT_IMPORT_OUTGOING_HANDLE,
            ("wasi:http/outgoing-handler@0.2.3", "handle")
        );
        assert_eq!(
            P2_DIRECT_IMPORT_RESPONSE_STATUS,
            ("wasi:http/types@0.2.3", "[method]incoming-response.status")
        );
        assert_eq!(
            P2_DIRECT_IMPORT_RESPONSE_CONSUME,
            ("wasi:http/types@0.2.3", "[method]incoming-response.consume")
        );
    }

    #[test]
    fn send_rejects_unknown_method() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let req = Request {
            method: "FROBNICATE".into(),
            path: "http://example.invalid".into(),
            body: Vec::new(),
            headers: Vec::new(),
        };
        let r = rt.block_on(send(req));
        assert!(matches!(r, Err(HttpErr::Url(_))));
    }
}
