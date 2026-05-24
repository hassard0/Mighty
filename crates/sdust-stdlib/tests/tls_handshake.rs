//! `std.tls` client connects to a localhost-spawned rustls server and
//! completes a handshake. Self-signed cert is generated per-test via
//! `rcgen` and trusted explicitly by the client.

use rustls::pki_types::CertificateDer;
use sdust_stdlib::tls::{
    accept, acceptor_from_pem, client_config_with_root, ensure_crypto_provider,
};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsConnector;

#[tokio::test]
async fn handshake_completes_against_local_server() {
    ensure_crypto_provider();

    // Generate a self-signed cert for "localhost".
    let issued = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let cert_pem = issued.cert.pem();
    let key_pem = issued.key_pair.serialize_pem();
    let cert_der: CertificateDer<'static> = issued.cert.der().clone();

    let tmp = tempfile::tempdir().unwrap();
    let cert_path = tmp.path().join("cert.pem");
    let key_path = tmp.path().join("key.pem");
    std::fs::write(&cert_path, cert_pem).unwrap();
    std::fs::write(&key_path, key_pem).unwrap();

    // Server: bind, accept one TLS conn, echo "ok".
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let acceptor = acceptor_from_pem(&cert_path, &key_path).unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut tls = accept(&acceptor, tcp).await.unwrap();
        tls.write_all(b"ok").await.unwrap();
        tls.shutdown().await.ok();
    });

    // Client: trust the self-signed cert.
    let cfg = client_config_with_root(cert_der).unwrap();
    let connector = TlsConnector::from(Arc::new(cfg));
    let tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let server_name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
    let mut stream = connector.connect(server_name, tcp).await.unwrap();

    let mut buf = vec![];
    stream.read_to_end(&mut buf).await.ok();
    assert_eq!(&buf, b"ok");

    server.await.unwrap();
}

#[test]
fn acceptor_from_missing_files_errors() {
    let r = acceptor_from_pem(
        std::path::Path::new("/nonexistent/cert.pem"),
        std::path::Path::new("/nonexistent/key.pem"),
    );
    assert!(r.is_err());
}
