//! v0.21 Tier 4.3 — lossless live agent migration tests.
//!
//! Coverage (>= 6 tests):
//!
//! 1. `migrate_simple_agent_between_two_nodes` — happy path, 2 nodes,
//!    agent on A, migrate to B, state preserved, addr rewrites on
//!    source.
//! 2. `migrate_with_queued_messages` — 3 messages enqueued during the
//!    drain → ack window are forwarded to the target after restore.
//! 3. `migrate_with_incompatible_schema_rejected` — target advertises
//!    a different schema_hash; source sees a `MigrationError::Rejected`
//!    and rolls back.
//! 4. `migrate_target_offline_fails_clean` — target unreachable
//!    (never connected); source returns `TargetUnreachable` and never
//!    calls the source-side drain hook.
//! 5. `placement_sticky_keeps_agent_on_source_when_alive` — sticky
//!    policy returns `current_node` when it's in `available_nodes`.
//! 6. `placement_least_loaded_distributes` — 5 children spread; the
//!    next placement picks the smallest load.
//! 7. `migration_metrics_track_bytes_and_counts` — Prometheus-shaped
//!    counter exposure verified after a full migration round-trip.
//! 8. `migrate_same_node_rejected` — defensive: migrating to the
//!    local node fails with `SameNode`.

use mty_runtime::cluster::{
    address::{AgentAddr, NodeId},
    mesh::{ClusterConfig, ClusterMesh, PeerEntry, TlsConfig},
    migration::{
        AgentSnapshot, MigrationError, MigrationOrchestrator, MigrationResult, QueuedMessage,
        SnapshotSink, SnapshotSource,
    },
    placement::{LeastLoadedPolicy, PlacementContext, PlacementPolicy, StickyPolicy},
    supervisor::{
        ChildSpec, ClusterSupervisor, RestartPolicy, RestartStrategy,
    },
    WireFrame,
};
use parking_lot::Mutex as PlMutex;
use rustls::pki_types::CertificateDer;
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_rustls::{TlsAcceptor, TlsConnector};

// ---------- TLS + mesh helpers (mirror tests/cluster_routing.rs) ----------

fn ensure_crypto() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

struct TestCert {
    cert_der: CertificateDer<'static>,
    key_der: rustls::pki_types::PrivateKeyDer<'static>,
}

fn mint_cert(sni: &str) -> TestCert {
    let cert = rcgen::generate_simple_self_signed(vec![sni.to_string()]).expect("rcgen");
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    let key_der =
        rustls::pki_types::PrivateKeyDer::try_from(cert.key_pair.serialize_der()).expect("key der");
    TestCert { cert_der, key_der }
}

fn build_tls(our: &TestCert, their: &TestCert) -> TlsConfig {
    ensure_crypto();
    let server_cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![our.cert_der.clone()], our.key_der.clone_key())
        .expect("server cfg");
    let mut roots = RootCertStore::empty();
    roots.add(their.cert_der.clone()).expect("trust");
    let client_cfg = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConfig {
        acceptor: TlsAcceptor::from(Arc::new(server_cfg)),
        connector: TlsConnector::from(Arc::new(client_cfg)),
    }
}

async fn ephemeral_addr() -> SocketAddr {
    let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0u16))
        .await
        .unwrap();
    let addr = l.local_addr().unwrap();
    drop(l);
    addr
}

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

/// Two cluster meshes A and B, mutually trusted. A dials B.
async fn two_meshes() -> (Arc<ClusterMesh>, Arc<ClusterMesh>) {
    let cert_a = mint_cert("node-a.test");
    let cert_b = mint_cert("node-b.test");
    let tls_a = build_tls(&cert_a, &cert_b);
    let tls_b = build_tls(&cert_b, &cert_a);
    let addr_b = ephemeral_addr().await;
    let mesh_b = ClusterMesh::from_config(ClusterConfig {
        node_id: NodeId::new("node-b"),
        listen_addr: Some(addr_b),
        peers: vec![],
        tls: tls_b,
    })
    .await
    .expect("mesh b");
    let mesh_a = ClusterMesh::from_config(ClusterConfig {
        node_id: NodeId::new("node-a"),
        listen_addr: None,
        peers: vec![PeerEntry {
            node_id: NodeId::new("node-b"),
            addr: addr_b,
            server_name: Some("node-b.test".into()),
        }],
        tls: tls_a,
    })
    .await
    .expect("mesh a");
    let connected = wait_until(Duration::from_secs(5), || {
        mesh_a.has_peer(&NodeId::new("node-b"))
    })
    .await;
    assert!(connected, "A did not connect to B");
    (mesh_a, mesh_b)
}

// ---------- mock source/sink hooks ----------

/// Holds the agent state the source has snapshotted + the queued
/// messages collected during the drain → ack window.
#[derive(Default)]
struct SourceState {
    /// Agent state: maps `agent_id` → opaque bytes (the test treats
    /// state as plain bytes).
    states: HashMap<u64, Vec<u8>>,
    /// Drain calls observed.
    drain_calls: u32,
    rollback_calls: u32,
    finalize_calls: u32,
    /// Messages queued during the migration window, keyed by agent
    /// address.
    queued: HashMap<AgentAddr, Vec<QueuedMessage>>,
    /// Agent type for every known agent.
    agent_type: HashMap<u64, String>,
    /// Schema-hash the source reports for every agent.
    schema_hash: u64,
    /// If true, the source pretends to fail the drain.
    fail_drain: bool,
}

struct MockSource {
    inner: PlMutex<SourceState>,
}

impl MockSource {
    fn new(schema_hash: u64) -> Arc<Self> {
        Arc::new(Self {
            inner: PlMutex::new(SourceState {
                schema_hash,
                ..Default::default()
            }),
        })
    }

    fn install_agent(&self, id: u64, ty: &str, state: Vec<u8>) {
        let mut g = self.inner.lock();
        g.states.insert(id, state);
        g.agent_type.insert(id, ty.into());
    }

    fn enqueue_message(&self, addr: &AgentAddr, from: AgentAddr, msg: &str, bytes: Vec<u8>) {
        self.inner
            .lock()
            .queued
            .entry(addr.clone())
            .or_default()
            .push(QueuedMessage {
                from,
                msg: msg.into(),
                msg_bytes: bytes,
            });
    }

    fn drain_calls(&self) -> u32 {
        self.inner.lock().drain_calls
    }
    fn rollback_calls(&self) -> u32 {
        self.inner.lock().rollback_calls
    }
    fn finalize_calls(&self) -> u32 {
        self.inner.lock().finalize_calls
    }
}

impl SnapshotSource for MockSource {
    fn drain_and_snapshot<'a>(
        &'a self,
        agent: &'a AgentAddr,
    ) -> Pin<Box<dyn std::future::Future<Output = MigrationResult<AgentSnapshot>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut g = self.inner.lock();
            g.drain_calls += 1;
            if g.fail_drain {
                return Err(MigrationError::Internal("forced fail".into()));
            }
            let state = g
                .states
                .get(&agent.agent_id)
                .cloned()
                .ok_or_else(|| MigrationError::AgentNotFound(agent.clone()))?;
            let ty = g
                .agent_type
                .get(&agent.agent_id)
                .cloned()
                .unwrap_or_else(|| agent.agent_type.clone());
            let hash = g.schema_hash;
            Ok(AgentSnapshot {
                agent_type: ty,
                schema_hash: hash,
                state,
            })
        })
    }

    fn drain_queued_messages<'a>(
        &'a self,
        agent: &'a AgentAddr,
    ) -> Pin<
        Box<dyn std::future::Future<Output = MigrationResult<Vec<QueuedMessage>>> + Send + 'a>,
    > {
        Box::pin(async move {
            let mut g = self.inner.lock();
            Ok(g.queued.remove(agent).unwrap_or_default())
        })
    }

    fn finalize_migrated<'a>(
        &'a self,
        _agent: &'a AgentAddr,
        _new_addr: &'a AgentAddr,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            self.inner.lock().finalize_calls += 1;
        })
    }

    fn rollback<'a>(
        &'a self,
        _agent: &'a AgentAddr,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            self.inner.lock().rollback_calls += 1;
        })
    }
}

#[derive(Default)]
struct SinkState {
    restored: HashMap<u64, Vec<u8>>,
    /// Schema-hash the sink expects to receive.
    expected_hash: u64,
    /// Whether the sink should reject any restore call.
    reject: bool,
    next_id: AtomicU64,
}

struct MockSink {
    node: NodeId,
    inner: PlMutex<SinkState>,
    /// Public access for assertions after the migration completes.
    /// Counters live on the inner lock; the atomic is for parallel
    /// id-issuance without a lock acquire on the hot path.
    restored_count: AtomicU64,
}

impl MockSink {
    fn new(node: impl Into<NodeId>, expected_hash: u64) -> Arc<Self> {
        Arc::new(Self {
            node: node.into(),
            inner: PlMutex::new(SinkState {
                expected_hash,
                next_id: AtomicU64::new(1000),
                ..Default::default()
            }),
            restored_count: AtomicU64::new(0),
        })
    }

    fn reject_next(self: &Arc<Self>) {
        self.inner.lock().reject = true;
    }

    fn restored_count(&self) -> u64 {
        self.restored_count.load(Ordering::Relaxed)
    }

    fn restored_state(&self, id: u64) -> Option<Vec<u8>> {
        self.inner.lock().restored.get(&id).cloned()
    }
}

impl SnapshotSink for MockSink {
    fn restore<'a>(
        &'a self,
        snapshot: &'a AgentSnapshot,
        originating_addr: &'a AgentAddr,
    ) -> Pin<Box<dyn std::future::Future<Output = MigrationResult<AgentAddr>> + Send + 'a>> {
        Box::pin(async move {
            let mut g = self.inner.lock();
            if g.reject {
                return Err(MigrationError::IncompatibleSchema {
                    old: snapshot.schema_hash,
                    new: g.expected_hash,
                });
            }
            if snapshot.schema_hash != g.expected_hash {
                return Err(MigrationError::IncompatibleSchema {
                    old: snapshot.schema_hash,
                    new: g.expected_hash,
                });
            }
            let new_id = g.next_id.fetch_add(1, Ordering::Relaxed);
            g.restored.insert(new_id, snapshot.state.clone());
            self.restored_count.fetch_add(1, Ordering::Relaxed);
            Ok(AgentAddr::remote(
                self.node.clone(),
                originating_addr.agent_type.clone(),
                new_id,
            ))
        })
    }
}

/// Spawn an inbound-frame pump for `mesh` that forwards every
/// migration-shaped frame to the orchestrator. Returns a handle that
/// aborts the pump on drop.
fn spawn_migration_pump(
    mesh: Arc<ClusterMesh>,
    orch: Arc<MigrationOrchestrator>,
) -> tokio::task::JoinHandle<()> {
    let mut inbox = mesh.take_inbox().expect("inbox taken twice");
    tokio::spawn(async move {
        while let Some(env) = inbox.recv().await {
            // Migration frames are consumed by the orchestrator; non-
            // migration frames (Send/Ask in a real runtime) would be
            // routed to the agent layer — we drop them here.
            orch.handle_inbound(env.frame).await;
        }
    })
}

// ---------- 1: happy path — A → B ----------

#[tokio::test]
async fn migrate_simple_agent_between_two_nodes() {
    let (mesh_a, mesh_b) = two_meshes().await;
    let hash = 0xDEAD_BEEF_CAFE_F00D;
    let source = MockSource::new(hash);
    let sink = MockSink::new(NodeId::new("node-b"), hash);
    source.install_agent(42, "Counter", b"state-original".to_vec());

    let orch_a = MigrationOrchestrator::new(mesh_a.clone()).with_source(source.clone());
    let orch_b = MigrationOrchestrator::new(mesh_b.clone()).with_sink(sink.clone());

    let _pump_a = spawn_migration_pump(mesh_a.clone(), orch_a.clone());
    let _pump_b = spawn_migration_pump(mesh_b.clone(), orch_b.clone());

    let agent = AgentAddr::remote("node-a", "Counter", 42);
    let report = orch_a
        .migrate_agent(agent.clone(), NodeId::new("node-b"), 5_000)
        .await
        .expect("migrate");

    assert_eq!(report.source.as_str(), "node-a");
    assert_eq!(report.target.as_str(), "node-b");
    assert_eq!(report.agent, agent);
    assert_eq!(report.new_addr.node.as_str(), "node-b");
    assert_eq!(report.new_addr.agent_type, "Counter");
    assert!(report.new_addr.agent_id >= 1000);
    assert_eq!(report.state_bytes, b"state-original".len());
    assert_eq!(report.forwarded_messages, 0);

    // Target restored the state once.
    assert_eq!(sink.restored_count(), 1);
    assert_eq!(
        sink.restored_state(report.new_addr.agent_id).as_deref(),
        Some(b"state-original".as_ref())
    );

    // Source finalize fired exactly once; no rollback.
    assert_eq!(source.drain_calls(), 1);
    assert_eq!(source.finalize_calls(), 1);
    assert_eq!(source.rollback_calls(), 0);

    // Routing table now rewrites the original addr → new addr.
    let rewrite = orch_a.lookup_rewrite(&agent).expect("rewrite");
    assert_eq!(rewrite, report.new_addr);

    Arc::clone(&mesh_a).shutdown().await;
    Arc::clone(&mesh_b).shutdown().await;
}

// ---------- 2: queued messages forward after ack ----------

#[tokio::test]
async fn migrate_with_queued_messages() {
    let (mesh_a, mesh_b) = two_meshes().await;
    let hash = 0xAA;
    let source = MockSource::new(hash);
    let sink = MockSink::new(NodeId::new("node-b"), hash);
    source.install_agent(7, "Q", b"snap".to_vec());

    let agent = AgentAddr::remote("node-a", "Q", 7);
    let sender = AgentAddr::remote("node-a", "Producer", 1);
    // Three messages queued during the migration window.
    source.enqueue_message(&agent, sender.clone(), "msg-1", b"p1".to_vec());
    source.enqueue_message(&agent, sender.clone(), "msg-2", b"p2".to_vec());
    source.enqueue_message(&agent, sender.clone(), "msg-3", b"p3".to_vec());

    let orch_a = MigrationOrchestrator::new(mesh_a.clone()).with_source(source.clone());
    let orch_b = MigrationOrchestrator::new(mesh_b.clone()).with_sink(sink.clone());

    // For mesh B we need to also see forwarded Send frames; the pump
    // would consume them as non-migration → drop. So we drain inbox B
    // manually for this test.
    let _pump_a = spawn_migration_pump(mesh_a.clone(), orch_a.clone());
    let mut inbox_b = mesh_b.take_inbox().expect("inbox b");
    let orch_b_clone = orch_b.clone();
    let forwarded_collector: Arc<PlMutex<Vec<String>>> = Arc::new(PlMutex::new(Vec::new()));
    let coll = forwarded_collector.clone();
    let _pump_b = tokio::spawn(async move {
        while let Some(env) = inbox_b.recv().await {
            let consumed = orch_b_clone.handle_inbound(env.frame.clone()).await;
            if !consumed {
                if let WireFrame::Send { msg, .. } = env.frame {
                    coll.lock().push(msg);
                }
            }
        }
    });

    let report = orch_a
        .migrate_agent(agent.clone(), NodeId::new("node-b"), 5_000)
        .await
        .expect("migrate");
    assert_eq!(report.forwarded_messages, 3);

    // Wait for the forwarded Sends to land on B's inbox.
    let landed = wait_until(Duration::from_secs(5), || {
        forwarded_collector.lock().len() == 3
    })
    .await;
    assert!(landed, "expected 3 forwarded Send frames on B");

    let mut names = forwarded_collector.lock().clone();
    names.sort();
    assert_eq!(names, vec!["msg-1", "msg-2", "msg-3"]);

    Arc::clone(&mesh_a).shutdown().await;
    Arc::clone(&mesh_b).shutdown().await;
}

// ---------- 3: schema hash incompatible ----------

#[tokio::test]
async fn migrate_with_incompatible_schema_rejected() {
    let (mesh_a, mesh_b) = two_meshes().await;
    let source = MockSource::new(0xAAAA);
    let sink = MockSink::new(NodeId::new("node-b"), 0xBBBB); // mismatched
    source.install_agent(99, "Mismatched", b"_".to_vec());

    let orch_a = MigrationOrchestrator::new(mesh_a.clone()).with_source(source.clone());
    let orch_b = MigrationOrchestrator::new(mesh_b.clone()).with_sink(sink.clone());

    let _pump_a = spawn_migration_pump(mesh_a.clone(), orch_a.clone());
    let _pump_b = spawn_migration_pump(mesh_b.clone(), orch_b.clone());

    let agent = AgentAddr::remote("node-a", "Mismatched", 99);
    let err = orch_a
        .migrate_agent(agent.clone(), NodeId::new("node-b"), 5_000)
        .await
        .unwrap_err();
    match &err {
        MigrationError::Rejected { kind, .. } => {
            assert_eq!(kind, "schema_incompatible");
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
    // Rollback was invoked exactly once.
    assert_eq!(source.rollback_calls(), 1);
    assert_eq!(source.finalize_calls(), 0);
    // No routing rewrite installed.
    assert!(orch_a.lookup_rewrite(&agent).is_none());

    Arc::clone(&mesh_a).shutdown().await;
    Arc::clone(&mesh_b).shutdown().await;
}

// ---------- 4: target offline — fail clean ----------

#[tokio::test]
async fn migrate_target_offline_fails_clean() {
    // Build a solo mesh A; no peers.
    let cert_a = mint_cert("node-a.test");
    let cert_b = mint_cert("node-b.test");
    let tls_a = build_tls(&cert_a, &cert_b);
    let mesh_a = ClusterMesh::from_config(ClusterConfig {
        node_id: NodeId::new("node-a"),
        listen_addr: None,
        peers: vec![],
        tls: tls_a,
    })
    .await
    .expect("mesh a");
    let source = MockSource::new(0xC0FFEE);
    source.install_agent(1, "T", b"x".to_vec());
    let orch_a = MigrationOrchestrator::new(mesh_a.clone()).with_source(source.clone());

    let agent = AgentAddr::remote("node-a", "T", 1);
    let err = orch_a
        .migrate_agent(agent, NodeId::new("node-unknown"), 100)
        .await
        .unwrap_err();
    assert!(matches!(err, MigrationError::TargetUnreachable(_)));
    // We failed *before* draining anything.
    assert_eq!(source.drain_calls(), 0);
    assert_eq!(source.rollback_calls(), 0);

    Arc::clone(&mesh_a).shutdown().await;
}

// ---------- 5: sticky placement keeps source ----------

#[test]
fn placement_sticky_keeps_agent_on_source_when_alive() {
    let ctx = PlacementContext::new([NodeId::new("a"), NodeId::new("b"), NodeId::new("c")])
        .with_current_node(NodeId::new("a"));
    let p = StickyPolicy;
    let spec = ChildSpec {
        addr: AgentAddr::remote("a", "X", 1),
        restart: RestartPolicy::Permanent,
        max_restarts: 5,
        window_ms: 30_000,
    };
    let picked = p.place(&spec, &ctx);
    assert_eq!(picked.as_str(), "a");
}

// ---------- 6: least-loaded distributes ----------

#[test]
fn placement_least_loaded_distributes() {
    let mut counts = HashMap::new();
    counts.insert(NodeId::new("a"), 5);
    counts.insert(NodeId::new("b"), 2);
    counts.insert(NodeId::new("c"), 3);
    let ctx = PlacementContext::new([NodeId::new("a"), NodeId::new("b"), NodeId::new("c")])
        .with_child_counts(counts);
    let p = LeastLoadedPolicy;
    // The spec node ("a", load 5) is *not* the answer — least-loaded
    // wins.
    let spec = ChildSpec {
        addr: AgentAddr::remote("a", "X", 1),
        restart: RestartPolicy::Permanent,
        max_restarts: 5,
        window_ms: 30_000,
    };
    let picked = p.place(&spec, &ctx);
    assert_eq!(picked.as_str(), "b");
}

// ---------- 7: metrics counters ----------

#[tokio::test]
async fn migration_metrics_track_bytes_and_counts() {
    let (mesh_a, mesh_b) = two_meshes().await;
    let hash = 0xFEED;
    let source = MockSource::new(hash);
    let sink = MockSink::new(NodeId::new("node-b"), hash);
    source.install_agent(11, "M", vec![0xABu8; 64]);

    let orch_a = MigrationOrchestrator::new(mesh_a.clone()).with_source(source.clone());
    let orch_b = MigrationOrchestrator::new(mesh_b.clone()).with_sink(sink.clone());
    let _pump_a = spawn_migration_pump(mesh_a.clone(), orch_a.clone());
    let _pump_b = spawn_migration_pump(mesh_b.clone(), orch_b.clone());

    let _ = orch_a
        .migrate_agent(
            AgentAddr::remote("node-a", "M", 11),
            NodeId::new("node-b"),
            5_000,
        )
        .await
        .expect("migrate");

    let snap = orch_a.metrics().snapshot();
    assert_eq!(snap.migrations_started, 1);
    assert_eq!(snap.migrations_completed, 1);
    assert_eq!(snap.migrations_failed, 0);
    assert_eq!(snap.migrations_rolled_back, 0);
    assert_eq!(snap.bytes_shipped_total, 64);

    Arc::clone(&mesh_a).shutdown().await;
    Arc::clone(&mesh_b).shutdown().await;
}

// ---------- 8: same-node target rejected up-front ----------

#[tokio::test]
async fn migrate_same_node_rejected() {
    let cert_a = mint_cert("node-a.test");
    let cert_b = mint_cert("node-b.test");
    let tls_a = build_tls(&cert_a, &cert_b);
    let mesh_a = ClusterMesh::from_config(ClusterConfig {
        node_id: NodeId::new("node-a"),
        listen_addr: None,
        peers: vec![],
        tls: tls_a,
    })
    .await
    .expect("mesh a");
    let source = MockSource::new(0x1);
    source.install_agent(1, "X", b"_".to_vec());
    let orch_a = MigrationOrchestrator::new(mesh_a.clone()).with_source(source.clone());

    let err = orch_a
        .migrate_agent(
            AgentAddr::remote("node-a", "X", 1),
            NodeId::new("node-a"),
            100,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, MigrationError::SameNode(_)));
    Arc::clone(&mesh_a).shutdown().await;
}

// ---------- 9: supervisor with placement policy emits hint ----------

#[tokio::test]
async fn supervisor_emits_placement_hint_when_policy_installed() {
    let sup = ClusterSupervisor::new(RestartStrategy::OneForOne);
    sup.set_available_nodes([
        NodeId::new("node-a"),
        NodeId::new("node-b"),
        NodeId::new("node-c"),
    ]);
    sup.set_placement_policy(Arc::new(LeastLoadedPolicy));

    // Stack 3 children on node-a, 1 on node-b, 0 on node-c. When a
    // node-a child fails, least-loaded picks node-c.
    for id in 1u64..=3 {
        sup.add_child(ChildSpec {
            addr: AgentAddr::remote("node-a", "X", id),
            restart: RestartPolicy::Permanent,
            max_restarts: 5,
            window_ms: 30_000,
        });
    }
    sup.add_child(ChildSpec {
        addr: AgentAddr::remote("node-b", "X", 4),
        restart: RestartPolicy::Permanent,
        max_restarts: 5,
        window_ms: 30_000,
    });
    let failed = AgentAddr::remote("node-a", "X", 2);
    sup.on_child_exit(failed.clone(), mty_runtime::cluster::supervisor::ExitReason::Crashed("boom".into()))
        .await;

    let ev = sup.try_next_event().expect("event");
    match ev {
        mty_runtime::cluster::supervisor::SupervisorEvent::RestartRequested {
            child,
            placement_hint,
            ..
        } => {
            assert_eq!(child, failed);
            // node-a has 3 (incl the failing one), node-b has 1,
            // node-c has 0 → least-loaded picks node-c.
            assert_eq!(
                placement_hint.as_ref().map(|n| n.as_str()),
                Some("node-c")
            );
        }
        other => panic!("expected RestartRequested, got {other:?}"),
    }
    assert_eq!(sup.placement_policy_name(), "least-loaded");
    assert_eq!(sup.available_node_count(), 3);
}

// Keep helpers in-scope for future tests.
#[allow(dead_code)]
async fn _unused() {
    let _: Option<PeerEntry> = None;
    let _: Option<TcpListener> = None;
    let _: Option<Ipv4Addr> = None;
    let _: Option<SocketAddr> = None;
}
