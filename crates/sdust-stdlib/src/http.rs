//! `std.http` — real HTTP/1.1 + HTTP/2 client + server via [`hyper`].
//!
//! Supersedes the slice-7 minimal in-memory server in `sdust-runtime::http`.
//! That module is still re-exported by the runtime for backwards
//! compatibility with existing tests; new agent code should use this
//! one.
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
pub async fn get(url: &str) -> Result<Response, HttpErr> {
    request(Method::GET, url, Vec::new()).await
}

/// Issue an HTTP POST with `body`.
pub async fn post(url: &str, body: Vec<u8>) -> Result<Response, HttpErr> {
    request(Method::POST, url, body).await
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
        let (stream, _peer) = match listener.accept().await {
            Ok(x) => x,
            Err(_) => return,
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
}
