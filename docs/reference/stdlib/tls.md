# `std.tls`

TLS 1.2 + 1.3 client + server via [`rustls`](https://github.com/rustls/rustls)
0.23 and [`tokio-rustls`](https://github.com/rustls/tokio-rustls) 0.26.

## Surface

```sd
fn connect(host: Str, port: U16) -> TlsStream!TlsErr     // client
fn server(cert_path: Path, key_path: Path) -> TlsAcceptor!TlsErr
```

In Rust:

```rust
pub async fn connect(host: &str, port: u16)
    -> Result<TlsStream<TcpStream>, TlsErr>;
pub fn acceptor_from_pem(cert_path: &Path, key_path: &Path)
    -> Result<TlsAcceptor, TlsErr>;
pub async fn accept(acceptor: &TlsAcceptor, tcp: TcpStream)
    -> Result<TlsStream<TcpStream>, TlsErr>;
pub fn client_config_with_root(root: CertificateDer<'static>)
    -> Result<ClientConfig, TlsErr>;
pub fn ensure_crypto_provider();
```

## Crypto provider

`rustls` 0.23 requires a `CryptoProvider` to be installed before any
config is built. `std.tls` ships the `ring` provider and installs it
once-per-process via `ensure_crypto_provider()`; every public entry
point calls it before doing TLS work. The call is idempotent.

## PEM loading

`acceptor_from_pem` accepts:

- A cert PEM file containing one or more `CERTIFICATE` blocks.
- A key PEM file containing PKCS#8 (`PRIVATE KEY`), RSA
  (`RSA PRIVATE KEY`), or SEC1 (`EC PRIVATE KEY`) format — tried in
  that order.

## Example: connect to a TLS endpoint

```rust
use sdust_stdlib::tls;
use tokio::io::AsyncWriteExt;

async fn ping() -> Result<(), Box<dyn std::error::Error>> {
    let mut s = tls::connect("example.com", 443).await?;
    s.write_all(b"GET / HTTP/1.0\r\n\r\n").await?;
    Ok(())
}
```

## Example: serve TLS

```rust
use sdust_stdlib::tls;
use std::path::Path;
use tokio::net::TcpListener;

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let acceptor = tls::acceptor_from_pem(
        Path::new("cert.pem"),
        Path::new("key.pem"),
    )?;
    let lis = TcpListener::bind("0.0.0.0:8443").await?;
    loop {
        let (tcp, _) = lis.accept().await?;
        let mut stream = tls::accept(&acceptor, tcp).await?;
        // ...
        drop(&mut stream);
    }
}
```

## Known limits (v0.2)

- Native root cert loading is stubbed; the client side accepts any
  root explicitly added via `client_config_with_root`. v0.3 will pull
  `rustls-native-certs`.
- No `https://` URL plumbing in `std.http` (use `std.tls` directly to
  hand-roll an HTTPS request today).

## See also

- [`std.http`](http.md) for the HTTP client/server that v0.3 will
  layer on top of `std.tls`.
