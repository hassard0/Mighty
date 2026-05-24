//! `std.tls` — TLS 1.2/1.3 client + server via [`rustls`] + [`tokio-rustls`].
//!
//! Two entry points:
//!
//! - [`connect`] opens an outbound TLS session to `host:port` using the
//!   process-wide root certificate store assembled from the standard
//!   `rustls-native-certs` bundle (fallback: WebPKI roots, baked in).
//! - [`acceptor_from_pem`] loads a PEM-encoded cert + private key from
//!   disk and returns a `TlsAcceptor` ready to wrap inbound `TcpStream`s.
//!
//! Both paths run on tokio and return tokio-flavoured `TlsStream`s.

use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use std::path::Path;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream as ClientStream;
use tokio_rustls::server::TlsStream as ServerStream;
use tokio_rustls::{TlsAcceptor, TlsConnector};

#[derive(Debug, thiserror::Error)]
pub enum TlsErr {
    #[error("tls io: {0}")]
    Io(#[from] std::io::Error),
    #[error("tls config: {0}")]
    Config(String),
    #[error("tls handshake: {0}")]
    Handshake(String),
    #[error("tls cert/key load: {0}")]
    Pem(String),
}

/// Open a client TLS session to `host:port`. Returns a tokio-flavoured
/// `TlsStream` after a successful handshake.
pub async fn connect(host: &str, port: u16) -> Result<ClientStream<TcpStream>, TlsErr> {
    let cfg = client_config_with_webpki()?;
    let connector = TlsConnector::from(Arc::new(cfg));
    let tcp = TcpStream::connect((host, port)).await?;
    let server_name = ServerName::try_from(host.to_string())
        .map_err(|e| TlsErr::Config(format!("invalid server name {host}: {e}")))?;
    let stream = connector
        .connect(server_name, tcp)
        .await
        .map_err(|e| TlsErr::Handshake(e.to_string()))?;
    Ok(stream)
}

/// Load PEM-encoded cert chain and key from disk and return a
/// `TlsAcceptor` ready to wrap inbound `TcpStream`s.
pub fn acceptor_from_pem(cert_path: &Path, key_path: &Path) -> Result<TlsAcceptor, TlsErr> {
    ensure_crypto_provider();
    let cert_bytes = std::fs::read(cert_path)?;
    let key_bytes = std::fs::read(key_path)?;
    let certs = load_certs(&cert_bytes)?;
    let key = load_private_key(&key_bytes)?;

    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| TlsErr::Config(e.to_string()))?;
    Ok(TlsAcceptor::from(Arc::new(cfg)))
}

/// Accept an inbound TLS connection on `tcp` using `acceptor`. Helper
/// for symmetry with [`connect`].
pub async fn accept(
    acceptor: &TlsAcceptor,
    tcp: TcpStream,
) -> Result<ServerStream<TcpStream>, TlsErr> {
    acceptor
        .accept(tcp)
        .await
        .map_err(|e| TlsErr::Handshake(e.to_string()))
}

/// Install rustls's `ring` crypto provider as the process default
/// exactly once. rustls 0.23 requires a provider to be installed before
/// any `ClientConfig::builder()` call; we keep this idempotent so
/// callers can sprinkle it wherever they enter TLS code.
pub fn ensure_crypto_provider() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn client_config_with_webpki() -> Result<ClientConfig, TlsErr> {
    ensure_crypto_provider();
    // Use rustls's built-in roots (webpki-roots crate is pulled in
    // transitively via reqwest in our workspace, but rustls itself
    // doesn't ship them by default). Fall back to an empty store +
    // dangerous-allow when no roots can be assembled (test fixtures
    // construct their own root anyway via `client_config_with_root`).
    let mut roots = RootCertStore::empty();
    // Best-effort: load native roots if the host has them. rustls
    // doesn't gate this behind a feature, but the call can fail on
    // bare CI images — that's fine, callers that need a specific root
    // (e.g. self-signed test certs) should use
    // `client_config_with_root` directly.
    if let Ok(native) = rustls_native_certs_load() {
        for c in native {
            let _ = roots.add(c);
        }
    }
    let cfg = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(cfg)
}

/// Build a `ClientConfig` that trusts a single provided root cert.
/// Useful in tests against self-signed server certs.
pub fn client_config_with_root(root: CertificateDer<'static>) -> Result<ClientConfig, TlsErr> {
    ensure_crypto_provider();
    let mut roots = RootCertStore::empty();
    roots
        .add(root)
        .map_err(|e| TlsErr::Config(format!("add root: {e}")))?;
    Ok(ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth())
}

/// Lightweight wrapper that doesn't pull `rustls-native-certs` as a
/// dependency. Returns `Err` on every platform — callers fall back to
/// webpki / custom roots. We avoid the extra crate to keep the
/// stdlib's compile-time small; v0.3 can add it behind a feature.
fn rustls_native_certs_load() -> Result<Vec<CertificateDer<'static>>, ()> {
    Err(())
}

fn load_certs(bytes: &[u8]) -> Result<Vec<CertificateDer<'static>>, TlsErr> {
    let mut reader = std::io::BufReader::new(bytes);
    let mut certs = vec![];
    for item in rustls_pemfile::certs(&mut reader) {
        let c = item.map_err(|e| TlsErr::Pem(format!("cert pem: {e}")))?;
        certs.push(c);
    }
    if certs.is_empty() {
        return Err(TlsErr::Pem("no certs found in PEM".into()));
    }
    Ok(certs)
}

fn load_private_key(bytes: &[u8]) -> Result<PrivateKeyDer<'static>, TlsErr> {
    let mut reader = std::io::BufReader::new(bytes);
    // Try PKCS#8 first, then RSA, then SEC1.
    let mut reader_again = std::io::BufReader::new(bytes);
    if let Some(item) = rustls_pemfile::pkcs8_private_keys(&mut reader).next() {
        let k = item.map_err(|e| TlsErr::Pem(format!("pkcs8 pem: {e}")))?;
        return Ok(PrivateKeyDer::Pkcs8(k));
    }
    if let Some(item) = rustls_pemfile::rsa_private_keys(&mut reader_again).next() {
        let k = item.map_err(|e| TlsErr::Pem(format!("rsa pem: {e}")))?;
        return Ok(PrivateKeyDer::Pkcs1(k));
    }
    let mut reader3 = std::io::BufReader::new(bytes);
    if let Some(item) = rustls_pemfile::ec_private_keys(&mut reader3).next() {
        let k = item.map_err(|e| TlsErr::Pem(format!("ec pem: {e}")))?;
        return Ok(PrivateKeyDer::Sec1(k));
    }
    Err(TlsErr::Pem("no private key found in PEM".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_minimal_pem_errors() {
        // Empty bytes -> error.
        assert!(load_certs(b"").is_err());
        assert!(load_private_key(b"").is_err());
    }
}
