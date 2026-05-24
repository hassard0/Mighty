# `std.http`

Real HTTP/1.1 client + server via [`hyper`](https://github.com/hyperium/hyper)
1.x and [`hyper-util`](https://github.com/hyperium/hyper-util).

Supersedes the slice-7 minimal in-memory server (`sdust-runtime::http`),
which is still re-exported for backwards compatibility with existing
runtime tests.

## Client

```sd
fn get(url: Str) -> Response!HttpErr
fn post(url: Str, body: Bytes) -> Response!HttpErr
```

In Rust:

```rust
pub async fn get(url: &str) -> Result<Response, HttpErr>;
pub async fn post(url: &str, body: Vec<u8>) -> Result<Response, HttpErr>;
```

`Response` carries `status: u16`, `body: Vec<u8>`, and the response
`headers: Vec<(String, String)>`. `Response::body_str()` is a
convenience for UTF-8 bodies.

## Server

```sd
fn serve(addr: Str, handler: agent) -> Unit!HttpErr
```

In Rust:

```rust
pub async fn serve(addr: &str, handler: Handler)
    -> Result<(SocketAddr, JoinHandle<()>), HttpErr>;
```

`Handler` is an `Arc<dyn Fn(Request) -> impl Future<Output = Response>>`.
The accept loop runs on a dedicated tokio task; the returned `JoinHandle`
can be `abort()`ed to shut down. Binding `127.0.0.1:0` lets the OS
pick a port — the returned `SocketAddr` carries the actual port.

## Example: echo server

```rust
use sdust_stdlib::http::{serve, Handler, Request, Response};
use std::sync::Arc;

let h: Handler = Arc::new(|req: Request| Box::pin(async move {
    Response {
        status: 200,
        body: req.body,
        headers: vec![("content-type".into(), "text/plain".into())],
    }
}));
let (addr, _task) = serve("127.0.0.1:0", h).await?;
println!("listening on {addr}");
```

## Known limits (v0.2)

- **HTTPS client**: only `http://` URLs are accepted; `https://`
  errors with `HttpErr::Url`. v0.3 will wire `hyper-rustls` via
  `std.tls`.
- **HTTP/2 server**: `hyper` supports HTTP/2, but the v0.2 server
  builder uses `http1::Builder` only. HTTP/2 + ALPN is a v0.3 task.

## See also

- [`std.tls`](tls.md) for the TLS primitives.
- [STDLIB_V0_2_NOTES.md](../../../STDLIB_V0_2_NOTES.md) for the
  HTTPS + HTTP/2 roadmap.
