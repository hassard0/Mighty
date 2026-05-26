//! v0.20 cluster-wide supervisor tests (Tier 4.2).
//!
//! The cluster supervisor extends the in-process supervisor tree
//! across the cluster: its children live on remote nodes, and the
//! events it reacts to include "peer disconnected → every child on
//! that node is now `:noproc`". v0.20 ships the supervisor + the
//! three sibling-restart strategies + a per-child circuit breaker.
//! Cross-node fail-over (placing a restart on a different node) is
//! deferred to v0.21.
//!
//! Coverage:
//!
//! 1. `supervisor_marks_children_noproc_on_peer_disconnect` — when
//!    the mesh tells the supervisor "node-b is gone," every child
//!    whose `addr.node == "node-b"` transitions to `ChildState::NoProc`
//!    and the supervisor emits a `NodeDisconnect` event.
//! 2. `one_for_one_restart_strategy` — only the failing child is
//!    restarted; siblings stay running.
//! 3. `one_for_all_restart_strategy` — child fails → restart event
//!    lists every sibling.
//! 4. `rest_for_one_restart_strategy` — child fails → restart event
//!    lists ONLY siblings registered AFTER it (insertion order).
//! 5. `max_restarts_window_circuit_breaker` — once the per-child
//!    restart counter exceeds `max_restarts` within `window_ms`, the
//!    supervisor emits `CircuitBreakerTripped` and stops restarting.
//! 6. `mesh_disconnect_propagates_to_registered_supervisor` — the
//!    `ClusterMesh::notify_node_disconnect` plumbing actually wakes
//!    a registered supervisor (covers the trait-object wiring path).

use mty_runtime::cluster::{
    address::{AgentAddr, NodeId},
    mesh::{ClusterConfig, ClusterMesh, PeerEntry, TlsConfig},
    supervisor::{
        ChildSpec, ChildState, ClusterSupervisor, ExitReason, RestartPolicy, RestartStrategy,
        SupervisorEvent,
    },
};
use rustls::pki_types::CertificateDer;
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_rustls::{TlsAcceptor, TlsConnector};

// ---------- helpers ----------

fn child(node: &str, ty: &str, id: u64) -> AgentAddr {
    AgentAddr::remote(node, ty, id)
}

fn spec(addr: AgentAddr, max: u32, window: u64) -> ChildSpec {
    ChildSpec {
        addr,
        restart: RestartPolicy::Permanent,
        max_restarts: max,
        window_ms: window,
    }
}

/// Drain everything the supervisor has emitted so far. Async because
/// the event channel is async and we want to give in-flight notifies
/// time to land.
async fn drain_events(sup: &ClusterSupervisor) -> Vec<SupervisorEvent> {
    // Pump until empty. We use `next_event` with a tiny timeout so
    // tests are responsive but still tolerant of cross-task latency.
    let mut events = Vec::new();
    loop {
        match tokio::time::timeout(Duration::from_millis(100), sup.next_event()).await {
            Ok(Some(ev)) => events.push(ev),
            Ok(None) => break,
            Err(_) => break, // timed out — channel empty for now
        }
    }
    events
}

// ---------- 1: peer disconnect transitions children to NoProc ----------

#[tokio::test]
async fn supervisor_marks_children_noproc_on_peer_disconnect() {
    let sup = ClusterSupervisor::new(RestartStrategy::OneForOne);
    let b1 = child("node-b", "Worker", 1);
    let b2 = child("node-b", "Worker", 2);
    let c1 = child("node-c", "Worker", 3);
    sup.add_child(spec(b1.clone(), 5, 30_000));
    sup.add_child(spec(b2.clone(), 5, 30_000));
    sup.add_child(spec(c1.clone(), 5, 30_000));

    sup.on_node_disconnect(&NodeId::new("node-b")).await;

    assert_eq!(sup.state_of(&b1), Some(ChildState::NoProc));
    assert_eq!(sup.state_of(&b2), Some(ChildState::NoProc));
    // c1 is on a different node — untouched.
    assert_eq!(sup.state_of(&c1), Some(ChildState::Running));

    let events = drain_events(&sup).await;
    let node_ev = events
        .iter()
        .find(|e| matches!(e, SupervisorEvent::NodeDisconnect { .. }))
        .expect("NodeDisconnect emitted");
    match node_ev {
        SupervisorEvent::NodeDisconnect {
            node,
            lost_children,
        } => {
            assert_eq!(node.as_str(), "node-b");
            assert_eq!(*lost_children, 2);
        }
        _ => unreachable!(),
    }
    // Two RestartRequested events, one per lost child.
    let restarts: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, SupervisorEvent::RestartRequested { .. }))
        .collect();
    assert_eq!(restarts.len(), 2, "one restart event per :noproc child");
}

// ---------- 2: OneForOne — only the failing child restarts ----------

#[tokio::test]
async fn one_for_one_restart_strategy() {
    let sup = ClusterSupervisor::new(RestartStrategy::OneForOne);
    let a = child("n", "A", 1);
    let b = child("n", "B", 2);
    let c = child("n", "C", 3);
    sup.add_children([
        spec(a.clone(), 5, 30_000),
        spec(b.clone(), 5, 30_000),
        spec(c.clone(), 5, 30_000),
    ]);

    sup.on_child_exit(b.clone(), ExitReason::Crashed("boom".into()))
        .await;

    let events = drain_events(&sup).await;
    let restart = events
        .iter()
        .find(|e| matches!(e, SupervisorEvent::RestartRequested { .. }))
        .expect("RestartRequested emitted");
    match restart {
        SupervisorEvent::RestartRequested {
            child, siblings, ..
        } => {
            assert_eq!(*child, b);
            assert!(siblings.is_empty(), "OneForOne must not wake siblings");
        }
        _ => unreachable!(),
    }
    // A and C are still Running; B is Restarting.
    assert_eq!(sup.state_of(&a), Some(ChildState::Running));
    assert_eq!(sup.state_of(&b), Some(ChildState::Restarting));
    assert_eq!(sup.state_of(&c), Some(ChildState::Running));
}

// ---------- 3: OneForAll — child + every sibling restart ----------

#[tokio::test]
async fn one_for_all_restart_strategy() {
    let sup = ClusterSupervisor::new(RestartStrategy::OneForAll);
    let a = child("n", "A", 1);
    let b = child("n", "B", 2);
    let c = child("n", "C", 3);
    sup.add_children([
        spec(a.clone(), 5, 30_000),
        spec(b.clone(), 5, 30_000),
        spec(c.clone(), 5, 30_000),
    ]);

    sup.on_child_exit(b.clone(), ExitReason::Crashed("oops".into()))
        .await;

    let events = drain_events(&sup).await;
    let restart = events
        .iter()
        .find(|e| matches!(e, SupervisorEvent::RestartRequested { .. }))
        .expect("RestartRequested emitted");
    match restart {
        SupervisorEvent::RestartRequested {
            child, siblings, ..
        } => {
            assert_eq!(*child, b);
            // siblings = {a, c} in some order.
            let mut sibs = siblings.clone();
            sibs.sort_by_key(|x| x.agent_id);
            assert_eq!(sibs, vec![a.clone(), c.clone()]);
        }
        _ => unreachable!(),
    }
    // All three are in Restarting now.
    assert_eq!(sup.state_of(&a), Some(ChildState::Restarting));
    assert_eq!(sup.state_of(&b), Some(ChildState::Restarting));
    assert_eq!(sup.state_of(&c), Some(ChildState::Restarting));
}

// ---------- 4: RestForOne — child + later-inserted siblings ----------

#[tokio::test]
async fn rest_for_one_restart_strategy() {
    let sup = ClusterSupervisor::new(RestartStrategy::RestForOne);
    let a = child("n", "A", 1);
    let b = child("n", "B", 2);
    let c = child("n", "C", 3);
    let d = child("n", "D", 4);
    // Inserted in order a, b, c, d. Failing c should wake d only.
    sup.add_children([
        spec(a.clone(), 5, 30_000),
        spec(b.clone(), 5, 30_000),
        spec(c.clone(), 5, 30_000),
        spec(d.clone(), 5, 30_000),
    ]);

    sup.on_child_exit(c.clone(), ExitReason::Crashed("rip".into()))
        .await;

    let events = drain_events(&sup).await;
    let restart = events
        .iter()
        .find(|e| matches!(e, SupervisorEvent::RestartRequested { .. }))
        .expect("RestartRequested emitted");
    match restart {
        SupervisorEvent::RestartRequested {
            child, siblings, ..
        } => {
            assert_eq!(*child, c);
            assert_eq!(siblings, &vec![d.clone()], "only later siblings");
        }
        _ => unreachable!(),
    }
    // a, b untouched; c restarting; d restarting.
    assert_eq!(sup.state_of(&a), Some(ChildState::Running));
    assert_eq!(sup.state_of(&b), Some(ChildState::Running));
    assert_eq!(sup.state_of(&c), Some(ChildState::Restarting));
    assert_eq!(sup.state_of(&d), Some(ChildState::Restarting));
}

// ---------- 5: circuit breaker trips after max_restarts ----------

#[tokio::test]
async fn max_restarts_window_circuit_breaker() {
    let sup = ClusterSupervisor::new(RestartStrategy::OneForOne);
    let a = child("n", "A", 1);
    // 3 restarts / 30s window. The fourth crash should trip the breaker.
    sup.add_child(spec(a.clone(), 3, 30_000));

    // First three crashes consume the budget.
    for i in 0..3 {
        sup.on_child_exit(a.clone(), ExitReason::Crashed(format!("crash {i}")))
            .await;
    }
    // Fourth crash should trip the circuit breaker.
    sup.on_child_exit(a.clone(), ExitReason::Crashed("crash 4".into()))
        .await;

    let events = drain_events(&sup).await;
    let restart_count = events
        .iter()
        .filter(|e| matches!(e, SupervisorEvent::RestartRequested { child, .. } if child == &a))
        .count();
    let breaker = events
        .iter()
        .find(|e| matches!(e, SupervisorEvent::CircuitBreakerTripped { .. }))
        .expect("circuit breaker tripped");

    assert_eq!(restart_count, 3, "should restart up to max then trip");
    match breaker {
        SupervisorEvent::CircuitBreakerTripped {
            child,
            attempts,
            window_ms,
        } => {
            assert_eq!(*child, a);
            assert_eq!(*attempts, 3);
            assert_eq!(*window_ms, 30_000);
        }
        _ => unreachable!(),
    }
    match sup.state_of(&a).unwrap() {
        ChildState::Dead(why) => assert!(why.contains("circuit breaker"), "got {why}"),
        other => panic!("expected Dead after breaker, got {other:?}"),
    }
}

// ---------- 6: mesh disconnect wakes registered supervisor ----------

#[tokio::test]
async fn mesh_disconnect_propagates_to_registered_supervisor() {
    // This test wires a real `ClusterMesh` to a `ClusterSupervisor` via
    // `register_supervisor` and verifies that `notify_node_disconnect`
    // ends up driving the supervisor's state. We don't actually need
    // peers to disconnect — we call `notify_node_disconnect` directly
    // (which is what the dialer task does internally when it notices
    // a peer is gone).
    let cert = self_signed("node-solo");
    let tls = trivial_tls(&cert);
    let mesh = ClusterMesh::from_config(ClusterConfig {
        node_id: NodeId::new("node-solo"),
        listen_addr: None,
        peers: vec![],
        tls,
    })
    .await
    .expect("mesh");

    let sup = Arc::new(ClusterSupervisor::new(RestartStrategy::OneForOne));
    let target = child("node-friend", "Worker", 1);
    sup.add_child(spec(target.clone(), 5, 30_000));
    mesh.register_supervisor(sup.clone());

    mesh.notify_node_disconnect(&NodeId::new("node-friend"))
        .await;
    assert_eq!(sup.state_of(&target), Some(ChildState::NoProc));

    Arc::clone(&mesh).shutdown().await;
}

// ---------- tiny TLS helper for the mesh-disconnect test ----------

fn ensure_crypto() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn self_signed(
    sni: &str,
) -> (
    CertificateDer<'static>,
    rustls::pki_types::PrivateKeyDer<'static>,
) {
    ensure_crypto();
    let cert = rcgen::generate_simple_self_signed(vec![sni.to_string()]).expect("rcgen");
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    let key_der =
        rustls::pki_types::PrivateKeyDer::try_from(cert.key_pair.serialize_der()).expect("key der");
    (cert_der, key_der)
}

fn trivial_tls(
    (cert_der, key_der): &(
        CertificateDer<'static>,
        rustls::pki_types::PrivateKeyDer<'static>,
    ),
) -> TlsConfig {
    let server_cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der.clone()], key_der.clone_key())
        .expect("server cfg");
    let mut roots = RootCertStore::empty();
    roots.add(cert_der.clone()).expect("add root");
    let client_cfg = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConfig {
        acceptor: TlsAcceptor::from(Arc::new(server_cfg)),
        connector: TlsConnector::from(Arc::new(client_cfg)),
    }
}

// Keep these in-scope so a future test can grow without re-importing.
#[allow(dead_code)]
async fn _unused() {
    let _: Option<TcpListener> = None;
    let _: Option<Ipv4Addr> = None;
    let _: Option<SocketAddr> = None;
    let _: Option<PeerEntry> = None;
}
