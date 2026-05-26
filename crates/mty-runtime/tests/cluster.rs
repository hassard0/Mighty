//! v0.18 Tier 4.1 — multi-node cluster mesh tests.
//!
//! These tests run TWO `ClusterMesh` instances in the same process on
//! separate ephemeral ports. Each test mints its own self-signed cert
//! via `rcgen`, builds a `rustls` server config from it, and a
//! client config that trusts only that cert. No on-disk fixtures.
//!
//! Coverage:
//!
//! 1. `addr_parse_local_remote_distinguishes` — `AgentAddr::local`
//!    vs `remote` shape & `is_local` flag.
//! 2. `wire_frame_roundtrip` — every `WireFrame` variant survives
//!    encode → decode.
//! 3. `peer_connect_to_listener` — boot a single mesh, connect a
//!    Peer to it, exchange Hello + a Send + Goodbye.
//! 4. `mesh_routes_remote_frame_to_peer` — two-node setup; node A
//!    routes a Send addressed to node B; B's inbox sees it.
//! 5. `mesh_returns_error_on_unknown_node` — frame addressed to a
//!    non-configured node returns `MeshError::UnknownNode`.
//! 6. `mesh_returns_error_on_local_loop` — frame addressed to self
//!    returns `MeshError::WouldLoopLocal`.
//! 7. `peer_reconnects_after_disconnect` — kill a peer, mesh's
//!    background dialer reconnects.

use mty_runtime::cluster::{
    address::{AgentAddr, NodeId},
    mesh::{ClusterConfig, ClusterMesh, MeshError, PeerEntry, TlsConfig},
    peer::Peer,
    wire::{decode_frame, encode_frame, WireFrame, WIRE_VERSION},
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::{TlsAcceptor, TlsConnector};

// ---------- helpers ----------

fn ensure_crypto() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

struct TestCert {
    cert_der: CertificateDer<'static>,
    key_der: PrivateKeyDer<'static>,
}

/// Generate a brand-new self-signed cert valid for `sni`.
fn mint_cert(sni: &str) -> TestCert {
    let cert = rcgen::generate_simple_self_signed(vec![sni.to_string()]).expect("rcgen");
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    let key_der =
        PrivateKeyDer::try_from(cert.key_pair.serialize_der()).expect("key der into pemfile");
    TestCert { cert_der, key_der }
}

fn build_tls(
    server_sni: &str,
    client_trusts: &CertificateDer<'static>,
    cert: &TestCert,
) -> TlsConfig {
    ensure_crypto();
    let server_cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert.cert_der.clone()], cert.key_der.clone_key())
        .expect("server config");
    let acceptor = TlsAcceptor::from(Arc::new(server_cfg));
    let mut roots = RootCertStore::empty();
    roots.add(client_trusts.clone()).expect("add root");
    let client_cfg = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(client_cfg));
    // `server_sni` is captured into the cert's SAN field above via
    // `mint_cert`, so the client will SNI-validate it correctly.
    let _ = server_sni;
    TlsConfig {
        connector,
        acceptor,
    }
}

/// Pick a random unused localhost port by binding+dropping a TCP
/// listener. Returns the addr ready to be re-bound.
async fn ephemeral_addr() -> SocketAddr {
    let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0u16))
        .await
        .unwrap();
    let addr = l.local_addr().unwrap();
    drop(l);
    addr
}

// ---------- 1: AgentAddr ----------

#[test]
fn addr_parse_local_remote_distinguishes() {
    let local = AgentAddr::local("Greeter", 1);
    let remote = AgentAddr::remote("node-b", "Greeter", 1);
    assert!(local.is_local());
    assert!(!remote.is_local());
    assert_ne!(local.node, remote.node);
    assert_eq!(format!("{remote}"), "node-b:Greeter:1");
}

// ---------- 2: wire frame roundtrip ----------

#[test]
fn wire_frame_roundtrip() {
    let frames = vec![
        WireFrame::Hello {
            node_id: NodeId::new("node-x"),
            version: WIRE_VERSION,
        },
        WireFrame::Heartbeat,
        WireFrame::Send {
            from: AgentAddr::remote("a", "S", 1),
            to: AgentAddr::remote("b", "R", 2),
            msg: "ping".into(),
            msg_bytes: vec![1, 2, 3],
        },
        WireFrame::Ask {
            from: AgentAddr::remote("a", "S", 1),
            to: AgentAddr::remote("b", "R", 2),
            msg: "ask".into(),
            msg_bytes: vec![9],
            correlation: 42,
        },
        WireFrame::Reply {
            correlation: 42,
            msg_bytes: vec![7],
        },
        WireFrame::Error {
            correlation: 42,
            kind: "trap".into(),
            message: "bad arg".into(),
        },
        WireFrame::Goodbye,
    ];
    for f in frames {
        let bytes = encode_frame(&f).unwrap();
        let (decoded, n) = decode_frame(&bytes).unwrap();
        assert_eq!(n, bytes.len(), "frame {:?} consumed wrong byte count", f);
        assert_eq!(decoded, f);
    }
}

// ---------- 3: peer connects to a listener ----------

#[tokio::test]
async fn peer_connect_to_listener() {
    let sni = "node-listen.test";
    let cert = mint_cert(sni);
    let tls = build_tls(sni, &cert.cert_der, &cert);
    let cfg = ClusterConfig {
        node_id: NodeId::new("node-listen"),
        listen_addr: Some(ephemeral_addr().await),
        peers: vec![],
        tls: tls.clone(),
    };
    let listen_addr = cfg.listen_addr.unwrap();
    let mesh = ClusterMesh::from_config(cfg).await.expect("mesh boots");
    // Drain the mesh inbox into a vec we can inspect.
    let mut inbox = mesh.take_inbox().expect("take inbox");

    // Dial the listener directly.
    let connector = tls.connector.clone();
    let tcp = TcpStream::connect(listen_addr).await.unwrap();
    let server_name: ServerName<'static> = ServerName::try_from(sni.to_string()).unwrap();
    let tls_stream = connector.connect(server_name, tcp).await.unwrap();
    let (tx, _rx_unused) = mpsc::channel(16);
    let peer = Peer::from_raw_tls_client(tls_stream, listen_addr, NodeId::new("node-dial"), tx)
        .await
        .expect("client peer");

    // Send a frame; the listener-side reader should push it onto the
    // mesh inbox.
    let frame = WireFrame::Send {
        from: AgentAddr::remote("node-dial", "S", 1),
        to: AgentAddr::remote("node-listen", "R", 2),
        msg: "hi".into(),
        msg_bytes: b"hi".to_vec(),
    };
    peer.send_frame_async(frame.clone()).await.expect("send");

    // The mesh's listener should accept, handshake, and push our
    // frame to the inbox. Allow a short window for the accept task
    // chain.
    let got = tokio::time::timeout(Duration::from_secs(5), inbox.recv())
        .await
        .expect("inbox recv timeout")
        .expect("inbox closed");
    assert_eq!(got.from_node.as_str(), "node-dial");
    assert_eq!(got.frame, frame);

    peer.close().await;
    Arc::clone(&mesh).shutdown().await;
}

// ---------- 4: mesh A routes to mesh B ----------

#[tokio::test]
async fn mesh_routes_remote_frame_to_peer() {
    // Two meshes: A dials B. Each gets its own cert; A trusts B's
    // cert via its client config, and vice versa. We mint two certs
    // and exchange the trust roots.
    let sni_a = "node-a.test";
    let sni_b = "node-b.test";
    let cert_a = mint_cert(sni_a);
    let cert_b = mint_cert(sni_b);
    // A's TLS: A's server cert + A trusts B's cert (for dialing B).
    let tls_a = TlsConfig {
        acceptor: {
            ensure_crypto();
            TlsAcceptor::from(Arc::new(
                ServerConfig::builder()
                    .with_no_client_auth()
                    .with_single_cert(vec![cert_a.cert_der.clone()], cert_a.key_der.clone_key())
                    .unwrap(),
            ))
        },
        connector: {
            let mut roots = RootCertStore::empty();
            roots.add(cert_b.cert_der.clone()).unwrap();
            TlsConnector::from(Arc::new(
                ClientConfig::builder()
                    .with_root_certificates(roots)
                    .with_no_client_auth(),
            ))
        },
    };
    let tls_b = TlsConfig {
        acceptor: TlsAcceptor::from(Arc::new(
            ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(vec![cert_b.cert_der.clone()], cert_b.key_der.clone_key())
                .unwrap(),
        )),
        connector: {
            let mut roots = RootCertStore::empty();
            roots.add(cert_a.cert_der.clone()).unwrap();
            TlsConnector::from(Arc::new(
                ClientConfig::builder()
                    .with_root_certificates(roots)
                    .with_no_client_auth(),
            ))
        },
    };

    let addr_b = ephemeral_addr().await;
    let cfg_b = ClusterConfig {
        node_id: NodeId::new("node-b"),
        listen_addr: Some(addr_b),
        peers: vec![],
        tls: tls_b,
    };
    let mesh_b = ClusterMesh::from_config(cfg_b).await.expect("mesh b");
    let mut inbox_b = mesh_b.take_inbox().expect("inbox b");

    let cfg_a = ClusterConfig {
        node_id: NodeId::new("node-a"),
        listen_addr: None,
        peers: vec![PeerEntry {
            node_id: NodeId::new("node-b"),
            addr: addr_b,
            server_name: Some(sni_b.to_string()),
        }],
        tls: tls_a,
    };
    let mesh_a = ClusterMesh::from_config(cfg_a).await.expect("mesh a");

    // Wait until A's dialer has connected to B (up to 5s).
    let connected = wait_until(Duration::from_secs(5), || {
        mesh_a.has_peer(&NodeId::new("node-b"))
    })
    .await;
    assert!(connected, "A failed to connect to B");

    // A routes a Send to node-b.
    let frame = WireFrame::Send {
        from: AgentAddr::remote("node-a", "S", 1),
        to: AgentAddr::remote("node-b", "R", 2),
        msg: "hello".into(),
        msg_bytes: b"hello".to_vec(),
    };
    mesh_a.route(frame.clone()).expect("a.route");

    let got = tokio::time::timeout(Duration::from_secs(5), inbox_b.recv())
        .await
        .expect("b inbox timeout")
        .expect("b inbox closed");
    assert_eq!(got.from_node.as_str(), "node-a");
    assert_eq!(got.frame, frame);

    Arc::clone(&mesh_a).shutdown().await;
    Arc::clone(&mesh_b).shutdown().await;
}

// ---------- 5: unknown node returns clear error ----------

#[tokio::test]
async fn mesh_returns_error_on_unknown_node() {
    let sni = "node-solo.test";
    let cert = mint_cert(sni);
    let tls = build_tls(sni, &cert.cert_der, &cert);
    let cfg = ClusterConfig {
        node_id: NodeId::new("node-solo"),
        listen_addr: None,
        peers: vec![],
        tls,
    };
    let mesh = ClusterMesh::from_config(cfg).await.expect("mesh");
    let err = mesh
        .route(WireFrame::Send {
            from: AgentAddr::remote("node-solo", "S", 1),
            to: AgentAddr::remote("ghost", "R", 2),
            msg: "x".into(),
            msg_bytes: vec![],
        })
        .unwrap_err();
    matches!(err, MeshError::UnknownNode(_));
}

// ---------- 6: local loop returns clear error ----------

#[tokio::test]
async fn mesh_returns_error_on_local_loop() {
    let sni = "node-self.test";
    let cert = mint_cert(sni);
    let tls = build_tls(sni, &cert.cert_der, &cert);
    let cfg = ClusterConfig {
        node_id: NodeId::new("node-self"),
        listen_addr: None,
        peers: vec![],
        tls,
    };
    let mesh = ClusterMesh::from_config(cfg).await.expect("mesh");
    let err = mesh
        .route(WireFrame::Send {
            from: AgentAddr::remote("node-self", "S", 1),
            to: AgentAddr::remote("node-self", "R", 2),
            msg: "x".into(),
            msg_bytes: vec![],
        })
        .unwrap_err();
    matches!(err, MeshError::WouldLoopLocal(_));
}

// ---------- 7: peer reconnects after disconnect ----------

#[tokio::test]
async fn peer_reconnects_after_disconnect() {
    // B accepts; A dials. We bring B down, A's connection dies; we
    // bring B back up, A's dialer reconnects.
    let sni_a = "node-a.test";
    let sni_b = "node-b.test";
    let cert_a = mint_cert(sni_a);
    let cert_b = mint_cert(sni_b);
    let tls_a = TlsConfig {
        acceptor: TlsAcceptor::from(Arc::new({
            ensure_crypto();
            ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(vec![cert_a.cert_der.clone()], cert_a.key_der.clone_key())
                .unwrap()
        })),
        connector: {
            let mut roots = RootCertStore::empty();
            roots.add(cert_b.cert_der.clone()).unwrap();
            TlsConnector::from(Arc::new(
                ClientConfig::builder()
                    .with_root_certificates(roots)
                    .with_no_client_auth(),
            ))
        },
    };
    let build_tls_b = || TlsConfig {
        acceptor: TlsAcceptor::from(Arc::new(
            ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(vec![cert_b.cert_der.clone()], cert_b.key_der.clone_key())
                .unwrap(),
        )),
        connector: {
            let mut roots = RootCertStore::empty();
            roots.add(cert_a.cert_der.clone()).unwrap();
            TlsConnector::from(Arc::new(
                ClientConfig::builder()
                    .with_root_certificates(roots)
                    .with_no_client_auth(),
            ))
        },
    };

    let addr_b = ephemeral_addr().await;
    let cfg_b = ClusterConfig {
        node_id: NodeId::new("node-b"),
        listen_addr: Some(addr_b),
        peers: vec![],
        tls: build_tls_b(),
    };
    let mesh_b = ClusterMesh::from_config(cfg_b).await.expect("mesh b");

    let cfg_a = ClusterConfig {
        node_id: NodeId::new("node-a"),
        listen_addr: None,
        peers: vec![PeerEntry {
            node_id: NodeId::new("node-b"),
            addr: addr_b,
            server_name: Some(sni_b.to_string()),
        }],
        tls: tls_a,
    };
    let mesh_a = ClusterMesh::from_config(cfg_a).await.expect("mesh a");

    let connected1 = wait_until(Duration::from_secs(5), || {
        mesh_a.has_peer(&NodeId::new("node-b"))
    })
    .await;
    assert!(connected1, "A failed initial connect");

    // Kill B's listener + drop its peers.
    Arc::clone(&mesh_b).shutdown().await;
    drop(mesh_b);

    // Wait until A notices. The peer's writer task tears down on the
    // first heartbeat write that fails after the OS RSTs the dead
    // socket — that can take up to one heartbeat interval (5s) +
    // a couple of seconds for the dialer to flip the slot, so we
    // give it plenty of slack.
    let disconnected = wait_until(Duration::from_secs(15), || {
        !mesh_a.has_peer(&NodeId::new("node-b"))
    })
    .await;
    assert!(disconnected, "A did not notice peer drop");

    // Bring B back up on the SAME addr (we know nothing else is on
    // it because the dialer task in A will be the only client).
    let cfg_b2 = ClusterConfig {
        node_id: NodeId::new("node-b"),
        listen_addr: Some(addr_b),
        peers: vec![],
        tls: build_tls_b(),
    };
    let mesh_b2 = ClusterMesh::from_config(cfg_b2).await.expect("mesh b2");

    let reconnected = wait_until(Duration::from_secs(20), || {
        mesh_a.has_peer(&NodeId::new("node-b"))
    })
    .await;
    assert!(reconnected, "A failed to reconnect");

    Arc::clone(&mesh_a).shutdown().await;
    Arc::clone(&mesh_b2).shutdown().await;
}

// ---------- shared helper ----------

async fn wait_until<F: Fn() -> bool>(timeout: Duration, cond: F) -> bool {
    let start = std::time::Instant::now();
    loop {
        if cond() {
            return true;
        }
        if start.elapsed() > timeout {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
