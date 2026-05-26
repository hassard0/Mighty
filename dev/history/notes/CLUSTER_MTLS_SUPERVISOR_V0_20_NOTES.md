# Cluster mTLS + Tier 4.2 supervisor — design notes (v0.20)

Roadmap reference: `docs/internals/agent-features-roadmap.md` Tier 4.2
"distributed supervisors + mTLS hardening." Public-facing arch:
`docs/internals/cluster.md` (mTLS + cluster supervisor sections).

## What landed in this slice

| Component                                       | File                                             | Status     |
| ----------------------------------------------- | ------------------------------------------------ | ---------- |
| `ClusterTlsConfig` + acceptor/connector builder | `cluster/tls.rs`                                 | landed     |
| Cert CN extractor + `verify_peer_identity`      | `cluster/tls.rs`                                 | landed     |
| `Peer::connect_mtls` + `server_handshake_mtls`  | `cluster/peer.rs`                                | landed     |
| `ClusterMesh::from_config_mtls` + `register_supervisor` | `cluster/mesh.rs`                        | landed     |
| `ClusterSupervisor` + 3 strategies              | `cluster/supervisor.rs`                          | landed     |
| Circuit breaker (per-child, sliding window)     | `cluster/supervisor.rs`                          | landed     |
| Mesh → supervisor disconnect notify             | `cluster/mesh.rs::notify_node_disconnect`        | landed     |
| Cross-node fail-over (placement)                | —                                                | **v0.21+** |
| Lossless live migration (Tier 4.3)              | —                                                | **v0.21+** |
| SPIFFE/SAN-URI identity                         | —                                                | **v0.21+** |
| `[cluster.tls].require_client_cert` manifest    | (parsed but not wired through `mty-pkg`)         | follow-up  |

## Design choices

### CN-based identity vs SPIFFE

The cluster already has a meaningful operator-readable identifier:
the `node_id` string in `mighty.toml`. Putting it straight in the
cert's Subject CN gives us:

- One identity vocabulary instead of two (the operator doesn't have
  to invent a trust-domain URI).
- A 5-line DER walker — no extra crate dependency, no extra parsing
  surface. The walker lives in `cluster/tls.rs` and walks the
  top-level TLVs inside `TBSCertificate` looking for the *second*
  Name (issuer is first, subject is second). Self-signed certs (one
  Name only) fall through to the single CN they encode.
- A clean upgrade path: `cert_node_id` is the single chokepoint;
  the day an operator asks for SPIFFE we add a `cert_node_id_spiffe`
  variant and a `ClusterTlsConfig::identity_strategy` knob.

SPIFFE IDs (`spiffe://trust-domain/node`) are technically nicer —
they nest under SAN URI, survive issuer changes, and slot directly
into Istio / Linkerd / friends — but they pull in a second identity
vocabulary and force every cluster to pick a trust domain. The
recurring lesson from the cluster work so far: ship the simpler
thing, leave the door open. Defer to v0.21+.

### Why a hand-rolled DER walker (instead of `x509-cert`)?

`x509-cert` IS in the dep graph transitively (via `sigstore`). I
considered promoting it to a direct dep but the use case is one
function (`extract_cn_from_der`) and ~50 lines of straightforward
TLV walking, so a dep + a dozen API calls is more code than the
inline walker. The walker has 4 unit tests pinning behaviour
(simple CN, dashy CN, identity-mismatch error, SAN-only fallback).
A future SPIFFE-aware extractor would happily reuse the same walker.

### mTLS opt-in via constructor, not field

The first iteration added `require_mtls: bool` to `ClusterConfig`
as a struct field. That would break every existing call site (the
v0.18 / v0.19 tests construct `ClusterConfig` via full struct-
literal syntax). Adding `Default` doesn't help because struct-
literal requires every field.

Final shape: `ClusterMesh::from_config_mtls(cfg)` is a separate
constructor that flips an internal `require_mtls: bool` flag.
`ClusterConfig` shape is unchanged → zero breakage for v0.18 /
v0.19 callers, and the v0.20 mTLS path is opt-in with one method.

### Cluster supervisor — events vs callbacks

OTP-style supervisors invoke the restart logic synchronously. We
inverted that: the supervisor decides what to do (per strategy +
circuit breaker) and emits events on a bounded channel
(`SUPERVISOR_EVENT_CAPACITY = 256`). The caller drains via
`next_event` and decides what "restart" means.

Rationale:

- A cluster restart is not a local function call — it might mean
  "wait for the dead node to reconnect" or "ask the placement
  service to pick a new node." Both are async. Forcing the
  supervisor to await on those would couple it to whatever
  placement strategy the caller picks.
- Events are observable in tests via `try_next_event` — no fake
  callback registry needed.
- Losing an event (channel full) is bad but not corrupting; the
  authoritative state lives in the supervisor's `children` map,
  and the caller can re-poll `state_of` to recover.

### NoProc preservation

When `on_node_disconnect` marks a child `:noproc` and then runs
`plan_restart_locked`, the planner *does NOT* transition the state
back to `Restarting`. NoProc means "currently unreachable; restart
attempts will keep trying" — calling it Restarting would obscure
the diagnostic story ("why is B not running?" → "the planner
thinks it's restarting" → wrong; the node is down).

The caller is responsible for transitioning to `Running` once it
succeeds in placing the child somewhere. Until then, repeated
disconnect notifications on the same `node_id` are idempotent (the
NoProc check in `on_node_disconnect` skips already-NoProc children).

### Supervisor hook trait — hand-rolled `Future + Send`

`SupervisorHook` has one async method (`on_node_disconnect`). We
declare it with a hand-rolled `fn -> Pin<Box<dyn Future + Send>>`
shape rather than pulling in `async-trait` for a single method.
Rust 1.85 (this workspace's `rust-version`) supports `async fn`
in trait bodies, but trait-object dispatch on async-in-trait still
requires the dyn-compat box shuffle — so the hand-rolled signature
is honestly clearer.

## Tier 4.3 (live migration) deferred

The brief left "real cross-node restart (failover) is v0.21." The
v0.20 supervisor knowingly emits a per-child `RestartRequested`
event but does NOT carry any "place me here instead" hint. The
caller's restart machinery picks the placement (try the same node
again? pick a new one? round-robin? lowest-load?). Once a
`PlacementPolicy` abstraction lands, the supervisor will pass a
suggestion in the event and the caller can override.

## Tests + acceptance

- `cargo build --workspace` — clean.
- `cargo test -p mty-runtime --test cluster_mtls` — 5 / 5.
- `cargo test -p mty-runtime --test cluster_supervisor` — 6 / 6.
- `cargo test -p mty-runtime` — no regressions (139 lib tests +
  every prior cluster integration test still passes).
- `cargo clippy -p mty-runtime --lib` — clean.
- `cargo clippy -p mty-runtime --test cluster_mtls --test cluster_supervisor
  --test cluster --test cluster_routing -- -D warnings` — clean.
- `cargo fmt --all -- --check` — clean.

(Two pre-existing clippy lints in `reload/resumable.rs` +
`tests/replay_strict_equality.rs` are out of scope for this slice
— they're owned by another swarm agent.)

## v0.21 follow-ups

1. **Cross-node fail-over.** Add `PlacementPolicy` trait + an event
   field hinting the placement choice; supervisor restart events
   become "spawn `(addr, hint)`" instead of "respawn `addr`".
2. **Manifest wiring.** Plumb `[cluster.tls].require_client_cert` +
   `[cluster.tls].client_ca` through `mty_driver::manifest::ClusterManifest`
   so the operator doesn't have to hand-build `ClusterTlsConfig`.
3. **Supervisor metrics.** Counters for restart_total /
   circuit_breaker_tripped_total / node_disconnect_total. Wire
   into the existing OpenTelemetry layer.
4. **CN normalization.** Currently a cert with CN `"node-a "`
   (trailing space) would mismatch the configured `"node-a"`.
   Trim + case-normalize before comparing, with a one-line guard
   test.
5. **Cert rotation.** The mesh holds `Arc<TlsAcceptor>` and
   `Arc<TlsConnector>` for the process lifetime. A `rotate_tls`
   method that swaps both (without dropping in-flight connections)
   is a small addition once the v0.21 placement work settles.
