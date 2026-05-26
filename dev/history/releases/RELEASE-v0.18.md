# Mighty v0.18 — Release Notes

**Tag:** `v0.18.0`
**Date:** 2026-05-26
**Status:** SHIPPED — five-track swarm + integrator pass.

**Headline:** **v1.0 freeze gates closing fast — KNOWN_ISSUES P1
list cleared (#1, #2, #3), replay end-to-end, distributed agents
land.**

v0.18 is the **freeze-gates** release: every P1 entry on
`KNOWN_ISSUES.md` is now resolved, deterministic replay reaches
runtime end-to-end with 13 instrumentation sites wired into the hot
path, and the agent runtime grows a distributed transport layer
(Tier 4.1 of `docs/internals/agent-features-roadmap.md`). The
spec promotes to **v1.0-RC4** with the RFC-008 multi-row-variable
parser grammar amendment.

If you were on v0.17.0, the upgrade is `git pull && cargo install
--path crates/mty-cli --force` (or pull pre-built binaries from
the [Releases page](https://github.com/hassard0/Mighty/releases) —
the cross-platform `release.yml` workflow first fired on v0.15.0
and continues on every `v*` tag). There are no source-level
breaking changes for end users; v0.18 is strictly additive at the
language layer. Toolchain consumers that depend on the
`mty_codegen_wasm::emit::build_cabi_realloc_body` helper should
move to the new `mty_codegen_wasm::cabi_realloc::*` module surface
(the old function remains as a thin re-export for one cycle).

## Highlights

- **5 of 5 v0.18 swarm tracks SHIPPED-FULL.** `cabi_realloc` real
  free-list allocator (KNOWN_ISSUES #1 closed), real Sigstore
  keyless signing under `sigstore-real` (KNOWN_ISSUES #2 closed),
  Replay Runtime hot-path wire-up (Tier 1.4 end-to-end), mixed-track
  cross-cut (multi-row-var parser surface + MT4059 active + MSRV
  gate hardening — KNOWN_ISSUES #3 closed), and distributed-agents
  single-cluster mesh (Tier 4.1).
- **KNOWN_ISSUES P1 list is now empty.** Items #1 (bump allocator),
  #2 (signing stub), and #3 (MSRV gate runs only `cargo build`) all
  closed in this release. Remaining open items are P2 quality-of-life
  (#4 clippy-strict job `continue-on-error`, #5 `mkdocs build`
  doesn't run `--strict`, #6 demo-02 JS shim, #7 minimal-features
  CI doesn't sweep examples).
- **Replay is end-to-end through the Runtime hot path.** v0.17
  landed the recorder + wire format + `mty replay` CLI but never
  called any `record_*` function from the Runtime — every recorded
  trace was empty save the start event. **v0.18 wires 13
  instrumentation sites**: spawn, send, ask, message-handled, IO
  reads (fs + net), clock reads, random reads, budget exhaustion,
  cancellation, terminal exit, plus flush-on-shutdown. The fast
  path stays at one atomic load + one branch (~3 ns) when no
  recorder is installed.
- **Real Sigstore keyless signing.** The `sigstore-real` cargo
  feature now compiles cleanly and drives the real keyless flow
  against the public-good Sigstore deployment: GitHub Actions OIDC
  → Fulcio short-lived ECDSA-P256 cert (`~/.cache/fulcio.cert` for
  diffability) → Rekor `hashedrekord` transparency-log upload →
  full standard Sigstore Bundle JSON embedded under
  `verificationMaterial.sigstoreBundle` in the `.bundle` envelope.
  `cosign verify-blob` and `rekor-cli` consume the embedded Bundle
  directly without mty-specific tooling. Default builds keep the
  deterministic stub envelope so Windows hosts without NASM still
  ship.
- **Distributed agents — Tier 4.1 single-cluster mesh.** New
  `AgentAddr = node:type:pid` lifts the address space out of one
  process. The `ClusterMesh` owns a `TcpListener` + `tokio-rustls`
  acceptor, a `DashMap<NodeId, Arc<Peer>>` of outbound peers, and
  a central mpsc inbox; every peer's reader task pushes inbound
  frames there. Framed CBOR over TLS (length-prefixed) carries
  every `WireFrame`; ciborium is the codec (serde_cbor is
  unmaintained). `Runtime::send` consults the router in v0.19; the
  transport layer is feature-complete today.
- **`cabi_realloc` real free-list allocator.** Closes KNOWN_ISSUES
  #1. Extracted from `emit.rs` into its own `cabi_realloc.rs`
  module. Segregated free-list with 8 size classes (8B → 1024B,
  powers of 2) + a large bump path; per-class LIFO push/pop;
  32-byte state region at linear-memory `[32768, 32800)`. ~190
  emitted wasm instructions (target was "< 200 if possible"). 17
  dedicated coverage tests across two new test files.
- **RFC-008 multi-row-variable parser surface.** `EFFECT_ROW_TAIL`
  now accepts `(',' RowVar)*` after the leading row var, opening
  the four new accepted shapes (`!{| E1, E2}`, `!{a | E, F}`,
  `effect a | E, F`, `effect a, b | E, F`). The HIR + typeck layers
  were already multi-var-ready since v0.17 (`HirEffectRow::Open`
  carries `Vec<HirRowVar>`); the HIR lowerer still reads only the
  first var (collapses to v0.17-equivalent), with the multi-var
  lowerer queued for v0.19. **MT4059 (caller's closed-row rejects
  closure's effects) flips to active emit** now that the surface
  has the shape to drive it.
- **MSRV gate hardened.** The MSRV job now runs `cargo build
  --workspace --tests` instead of bare `cargo build --workspace`,
  pulling in the full `[dev-dependencies]` graph. Closes
  KNOWN_ISSUES #3.
- **Spec v1.0-RC4.** §9.2 effect-row grammar amended for the
  multi-row tail. No behavioural change for single-row signatures;
  the new shapes are net-new accepted forms.
- **All gates green, test count grows 1274 → 1324 Rust** (+50,
  primarily replay-e2e + cluster + sigstore + cabi_realloc). Python
  steady at 274, conformance at 92 (2 ignored), self-host driver at
  23. **1713 combined** (+50 vs v0.17).
- **Cross-platform binaries continue to ship.** The `release.yml`
  workflow that first fired on v0.15.0 produces `mty` binaries for
  Linux x86_64, macOS x86_64 + arm64, and Windows x86_64 on every
  `v*` tag push.

## What's new

### `cabi_realloc` real free-list allocator (KNOWN_ISSUES #1)

The Component-Model `cabi_realloc` exported from every WASI Preview
2 component, previously a 280-line function buried in
`crates/mty-codegen-wasm/src/emit.rs::build_cabi_realloc_body`, is
extracted into a dedicated module `cabi_realloc.rs` with focused
coverage.

- **Free-list shape (kept from v0.10, formalised in v0.18):**
  segregated free-list with 8 size classes (`8 / 16 / 32 / 64 /
  128 / 256 / 512 / 1024` bytes) + a "large" bump path for requests
  > 1024B. State region at linear-memory `[32768, 32800)` holds one
  i32 free-list head per class; the link in each free block is the
  first 4 bytes (LIFO push/pop).
- **Realloc semantics:** copies `min(old_size, new_size)` bytes
  byte-by-byte and pushes `old` onto its class's free list. In-place
  realloc within the same size class is a v0.19 optimisation.
- **Memory overhead:** 32 bytes of metadata. Linear-memory pages
  zero-init at module load means no startup code clears it.
- **Instruction count:** ~190 wasm instructions (free-only 25 +
  size_class ×3 = 72 + free-list reuse 45 + realloc copy 35 +
  return 2). Inside the "< 200 if possible" budget.
- **New tests:** 8 in `crates/mty-codegen-wasm/tests/cabi_realloc.rs`
  + 9 retained in `cabi_realloc_real.rs` = 17 dedicated coverage
  tests.
- **Follow-ups (v0.19):** see
  [`CABI_REALLOC_V0_18_NOTES.md`](../notes/CABI_REALLOC_V0_18_NOTES.md)
  — per-component allocator tuning, large-path coalescing, true
  in-place realloc when the size class wouldn't change.

See [`CABI_REALLOC_V0_18_NOTES.md`](../notes/CABI_REALLOC_V0_18_NOTES.md).

### Real Sigstore keyless signing (KNOWN_ISSUES #2)

The `mty-pkg/sigstore-real` cargo feature, broken since v0.10's
mode-aware refactor due to API drift, now compiles and drives the
real keyless flow end-to-end.

- **`sign_keyless`** drives sigstore 0.14's
  `bundle::sign::SigningContext::async_production` against the
  public-good Sigstore deployment. The session generates an
  ephemeral ECDSA-P256 keypair, exchanges it (with the OIDC JWT) at
  `https://fulcio.sigstore.dev/api/v1/signingCert` for a ~10-minute
  cert, signs the bundle digest, and uploads the `hashedrekord` to
  `https://rekor.sigstore.dev/api/v1/log/entries`.
- **Bundle embedding.** The full Sigstore Bundle JSON is embedded
  under `verificationMaterial.sigstoreBundle` in the `.bundle`
  envelope (decoupled from sigstore-crate patch bumps; cosign /
  rekor-cli consume it directly).
- **`verify_bundle`** cross-checks the embedded sigstoreBundle's
  `messageDigest` against the recomputed bundle SHA-256 on disk on
  default builds (no `sigstore-real` required for the consumer
  side). Three structural verify tests live in `signing_real.rs`.
- **Network round-trip test** (`keyless_round_trip_via_fulcio_and_rekor`)
  stays `#[ignore]` + `#[cfg(feature = "sigstore-real")]` to keep
  default CI hermetic.
- **NASM constraint** on Windows: `aws-lc-rs` (pulled by
  sigstore's `cert` feature) needs NASM at build time. Documented
  in `docs/internals/package-signing.md`. Linux CI runners (the
  intended sign+publish target) have NASM by default.
- **Follow-ups (v0.19):** see
  [`SIGSTORE_V0_18_NOTES.md`](../notes/SIGSTORE_V0_18_NOTES.md) —
  full cryptographic cert-chain + Rekor inclusion-proof verify on
  `fetch`, device-flow OAuth for local signing, SLSA v1.0
  provenance attestations, CI smoke against the public Sigstore
  trust root.

See [`SIGSTORE_V0_18_NOTES.md`](../notes/SIGSTORE_V0_18_NOTES.md).

### Replay Recorder Runtime hot-path wire-up (Tier 1.4 end-to-end)

v0.17 landed the recorder + wire format + `mty replay` CLI but
never called any `record_*` function from the Runtime — every
trace was empty save the start event. v0.18 wires the recorder
into the actual Runtime hot path so traces capture real agent
execution.

- **13 instrumentation sites wired:**
  - `RuntimeBuilder::build` reads `MTY_RECORD_TRACE` and calls
    `install_from_env`.
  - `Runtime::spawn_agent_with_affinity` → `record_spawn`.
  - `Runtime::send` / `Runtime::ask` → `record_message_sent`.
  - `agent.rs::run_one_turn_with_shared_reply` →
    `record_message_handled` (inner placement guarantees the event
    is recorded before the reply oneshot fires).
  - agent loop budget-exhaust / cancellation / terminal-exit arms
    → `record_budget_exhausted` + `record_exit`.
  - `StdHost::effect_call` routes for `std.time.{now,sleep}` →
    `record_clock_read`, `std.random.*` → `record_random_read`,
    `std.fs.{read,exists,list_dir}` →
    `record_io_read(agent, "fs:<path>", bytes)`,
    `std.http.{get,post}` →
    `record_io_read(agent, "net:<url>", bytes)`.
  - `Runtime::shutdown` + `Runtime::drop` → `flush_to_disk()` +
    `uninstall()`.
- **New helpers:** `replay::with_recorder<F: FnOnce(&Recorder)>` is
  the cheap fast-path; `replay::recording_enabled() -> bool` for
  sites that need to skip expensive payload encoding when off.
  `BudgetTracker::trip(agent_id, breach) -> RuntimeError` and
  `BudgetBreach::trace_reason() -> &'static str` collapse the
  emit-then-error pattern at agent-loop sites.
  `StdHost::with_agent_id(agent_id: u64) -> Self` tags hosts so
  IO/clock/random events carry the right id.
- **Zero overhead when disabled:** `with_recorder` expands to a
  single atomic load + branch. With no recorder installed, ~3 ns
  per call on modern x86 — well under the 5% budget.
  `disabled_when_env_unset` and `empty_path_env_treated_as_unset`
  pin the behaviour.
- **+8 end-to-end tests** in
  `crates/mty-runtime/tests/replay_e2e.rs` plus 2 new unit tests
  for `with_recorder`. All run serially via a `parking_lot::Mutex`
  to avoid global-state races.
- **Payload encoding via `format!("{:?}", args)`**: `Value` does
  not implement `Serialize` (it carries Host references). Opaque
  but human-readable; full byte-identical replay is a v0.19 stretch.
- **Budget-exhaust + Exit duplication:** every dying agent emits
  BOTH events with different `reason` semantics — `BudgetExhausted`
  is the breach kind (`"cpu"` / `"mem"` / `"MT5009"`), `Exit` is
  the agent-loop's terminal reason
  (`"trap:MT5009"` / `"shutdown"` / `"normal"`).
- **New internals doc:** [`docs/internals/replay.md`](../../../docs/internals/replay.md)
  covers the full module shape, wire format, instrumentation site
  table, design choices, and v0.19 follow-ups.

See [`REPLAY_HOTPATH_V0_18_NOTES.md`](../notes/REPLAY_HOTPATH_V0_18_NOTES.md).

### Distributed agents — single-cluster mesh (Tier 4.1)

The fourth tier of `docs/internals/agent-features-roadmap.md`
lifts the agent runtime out of one process: agent addresses gain a
`node` axis and the runtime grows a transport layer that ships
non-local sends as framed CBOR over TLS.

- **New address.** `AgentAddr = node:type:pid` (e.g.
  `node-a:Counter:7`). The in-process truth (`AgentId(u64)`) is
  preserved; cluster mode adds the richer address as a superset.
- **`ClusterMesh`** owns: one `TcpListener` + `tokio-rustls`
  acceptor for inbound, one `DashMap<NodeId, Arc<Peer>>` of
  outbound peers, one central mpsc inbox that every peer's reader
  task pushes into.
- **`Peer`** handles reconnect (exponential backoff) + heartbeats
  (absorbed silently at the reader, not pushed onto the inbox).
- **Wire format.** `WireFrame::{Send, Ask, Reply, Error,
  Heartbeat}` over framed CBOR (length-prefixed; ciborium codec —
  serde_cbor is unmaintained). `Reply` / `Error` don't carry
  destination addresses; they travel back on the same socket the
  `Ask` came in on (correlation-id bookkeeping is v0.19).
- **`ClusterRouter` trait + `SharedRouter` typedef.** The runtime
  gains an optional `Arc<dyn ClusterRouter>` field in v0.19 (a
  one-line addition + mechanical hookup in `Runtime::send`); the
  v0.18 slice keeps the transport layer additive without editing
  off-limits `runtime.rs` paths.
- **TLS.** `rustls`-backed acceptor + dialer; tests mint self-signed
  certs via `rcgen` per-test (no shared mutable state between
  parallel tests).
- **7 integration tests** in `crates/mty-runtime/tests/cluster.rs`
  covering mesh lifecycle, peer reconnect, multi-peer routing, and
  inbox drain.
- **New internals doc:** `docs/internals/cluster.md` (extended from
  the swarm-shipped version with the v0.18 instrumentation tables).

See [`CLUSTER_V0_18_NOTES.md`](../notes/CLUSTER_V0_18_NOTES.md).

### RFC-008 multi-row-var parser + MT4059 + MSRV (cross-cut)

Three otherwise-unrelated cleanups land in one sweep, unified by
the "close v0.17 loose ends" theme.

- **Multi-row-var parser surface.** `EFFECT_ROW_TAIL` now accepts
  `(',' RowVar)*` after the leading row var. The new accepted
  shapes (`!{| E1, E2}`, `!{a | E, F}`, `effect a | E, F`,
  `effect a, b | E, F`) parse cleanly. Trailing comma is rejected
  (`!{| E,}` is a parse error). The keyword form
  (`effect a, b | E, F`) also gains multi-var "for free" via the
  shared `effect_row_tail` path.
- **HIR lowerer note.** The lowerer is still single-row-var; it
  reads the first `EFFECT_ROW_VAR` only. Multi-var source
  signatures parse but collapse to a single row var at the HIR —
  observationally equivalent to v0.17's SHIPPED-SUBSET behaviour
  at typeck. The lowerer broadening is queued for v0.19.
- **MT4059 active.** `multi_row_var_closed_caller_emits_mt4059`
  pins the diagnostic firing through the multi-row-var source-level
  fn signature.
- **MSRV gate hardened.** `.github/workflows/ci.yml::msrv` now runs
  `cargo build --workspace --tests` (was bare `cargo build
  --workspace` + redundant `cargo test --workspace --no-run`).
  Strictly larger compile surface that pulls in
  `[dev-dependencies]`; same wall-clock cost. Closes
  KNOWN_ISSUES #3.
- **Spec promoted to v1.0-RC4.** §9.2's grammar
  (`RowTail ::= '|' RowVar (',' RowVar)*`) and dispatch table
  pick up the new shapes; no behavioural change for single-row
  signatures.

See [`V0_18_CROSSCUT_NOTES.md`](../notes/V0_18_CROSSCUT_NOTES.md).

## Documentation polish

- **New page: [`docs/internals/replay.md`](../../../docs/internals/replay.md).**
  Replaces the gap left by v0.17 (which shipped notes but no
  internals page). Covers module shape, wire format, the 13
  instrumentation sites, design choices, public API surface,
  v0.19 follow-ups.
- **New page: [`docs/internals/agents.md`](../../../docs/internals/agents.md).**
  Cross-cutting agent overview: surface declaration → HIR → typeck
  → borrowck → MtyIR → codegen → runtime descriptor → spawn loop.
  Tables for runtime fields, spawn anatomy, visibility surfaces,
  capability + budget enforcement. Stitches together the existing
  per-piece docs (runtime / scheduler / mailboxes / supervisors /
  budgets / sendable / replay / cluster / introspect /
  telemetry-spans).
- **New page: [`docs/internals/introspect.md`](../../../docs/internals/introspect.md).**
  Promotes the v0.16 `INTROSPECT_V0_16_NOTES.md` material to a
  first-class internals page (control-socket protocol, snapshot
  shape, Windows status, cross-references with replay + telemetry).
- **`docs/internals/README.md` rewritten** with a sectioned
  taxonomy (Frontend / Codegen + runtime / Observability / Tooling
  + ecosystem / Spec details / Roadmap) and an updated
  Where-to-find table covering all 20 crates and the new modules
  (replay hot-path hooks, cluster mesh, sigstore keyless sign,
  cabi_realloc allocator).
- **`mkdocs.yml` nav extended** with the new internals pages plus
  previously-auto-discovered-only pages (`telemetry-spans.md`,
  `cluster.md`, `fuzzing.md`, `agent-features-roadmap.md`).
- **Spec v1.0-RC4 amendment** at §9.2 — multi-row tail grammar +
  five new accepted shapes documented.
- **`mkdocs build --strict` passes locally.** No `ERROR` or
  `WARNING` lines (the residual `INFO` lines are
  unrecognised-relative-link notes for intentional repo-root
  references like `../../demos/` that mkdocs declines to resolve;
  unchanged from v0.17).

## Integration fixes (this tag commit)

None this slice — every swarm track landed against a clean main
without requiring integrator-side patch fixes. The integrator
pass is restricted to:

- This RELEASE-v0.18.md.
- `CHANGELOG.md` Unreleased-section flush.
- README cleanup (test counts, KNOWN_ISSUES P1 status, Features
  bullets for sigstore-real / distributed agents / real
  cabi_realloc / replay end-to-end; Roadmap pruning of
  distributed-agents and live-introspect bullets; Status paragraph
  rewrite).
- Docs quality pass (the three new internals pages, internals
  README rewrite, mkdocs nav extension, spec promotion to RC4).

## v1.0 freeze: blockers + proposed date

The v1.0 spec promotes to **v1.0-RC4** in this release (RFC-008
multi-row tail grammar amendment, §9.2). Blocker status (delta vs
v0.17 in italics):

1. **Two independent implementations.** Rust reference compiler,
   Python 2nd-impl (`impl-py/`, 274 tests through typeck), Go
   3rd-impl (`impl-go/`, 4848 LOC source-only). *Unchanged from
   v0.17* — HM closure inference + generics-with-constraints
   polish for the Python impl is now a v0.19 line item.
2. **RFC comment periods.** RFC-001..006 + RFC-008 + RFC-009 each
   need a 30-day public window. *Unchanged from v0.17* — opening
   the windows is a v0.19 candidate.
3. **Published normative conformance suite.** Corpus stands at
   *92 cases / 16 categories / 2 ignored* (unchanged from v0.17).
   Coverage of FROZEN diagnostic codes climbs to **100%** of the
   RFC-008 band — MT4055 / MT4056 / MT4057 / MT4058 / MT4059 are
   all actively emitting now that the multi-row parser surface
   shipped.

**Proposed v1.0 freeze date: 2026-09-01** (unchanged).

## Backwards-compat aliases (status)

- **`MTY_RECORD_TRACE`** is the active env var for the replay
  recorder. `MTY_REPLAY_RECORD` (the v0.17 name) is no longer
  recognized; the slice notes for v0.17 always used
  `MTY_RECORD_TRACE` — this is a documentation alignment, not a
  rename.
- **`MTY_RUNTIME_CONTROL_SOCK`** + **`MTY_INSPECT_CAPTURE_BODIES`**
  + **`MTY_OTLP_ENDPOINT`** unchanged from v0.16.
- **`Preview2Options::with_adapter(Some(WASI_P1_ADAPTER_COMMAND))`**
  is the toolchain-consumer opt-in to restore the v0.16 always-on
  adapter behaviour (default since v0.17: `embed_adapter == None`).
- **`--wasi=p1`** still retained as an explicit opt-out from the
  v0.15-onwards P2 default.
- **`sigstore-real`** is the new cargo feature on `mty-pkg` that
  drives the real Sigstore keyless flow; absent (default), the
  signing surface keeps the deterministic stub envelope so Windows
  hosts without NASM still build.

All other v0.7+ aliases (`mty dump --sir` for `--ir`; `SD####`
accepted by `mty explain`; `--legacy-interp`; legacy `sd` /
`stardust` code-block tags) stay live.

## Stats

| | v0.17.0 | v0.18.0 | Delta |
|---|---|---|---|
| Workspace crates | 20 | 20 | 0 |
| Rust tests passing | 1274 | **1324** | **+50** |
| Python tests passing | 274 | 274 | 0 |
| Self-host driver tests | 23 (codegen) | **23 (codegen)** | 0 |
| Conformance cases | 92 | **92** | 0 |
| Conformance ignored | 2 | **2** | 0 |
| Combined test count | 1663 | **1713** | **+50** |
| Tests failing | 0 | 0 | 0 |
| Diagnostic codes wired | ~74 | **~74** (MT4059 now active) | 0 |
| Examples passing (check) | 23/23 | **23/23** | 0 |
| Examples passing (impl-py typeck) | 23/23 | **23/23** | 0 |
| Demos passing | 4/4 | **4/4** | 0 |
| Independent implementations | 3 | **3** | 0 |
| Spec | v1.0-RC3 | **v1.0-RC4** | +1 RC |
| RFCs | 8 | 8 | 0 |
| Fuzz targets | 4 | 4 | 0 |
| CI jobs (all required) | 6 | 6 | 0 |
| Release-binary targets | 4 | **4** (Linux / macOS×2 / Windows) | 0 |
| KNOWN_ISSUES P1 open | 3 | **0** | **-3** |
| WASI P2 adapter-required surfaces | 0 | 0 | 0 |
| Internals doc pages | 41 | **44** (+agents, +introspect, +replay) | +3 |
| Commits since prior tag | 7 | **5** | -2 |

## Migration steps

For end-user Mighty packages: **none required**. v0.18 is strictly
additive at the language surface.

For toolchain contributors:

- **`MTY_RECORD_TRACE`** is the canonical env var for the replay
  recorder. The v0.17 release notes mentioned `MTY_REPLAY_RECORD`
  by mistake; the actual env name has always been
  `MTY_RECORD_TRACE`.
- **`mty_codegen_wasm::cabi_realloc::*`** is the new module path
  for the allocator emitter. `mty_codegen_wasm::emit::build_cabi_realloc_body`
  remains as a thin re-export.
- **`HirEffectRow::Open(_, _)`** still carries `Vec<HirRowVar>`
  (since v0.17). Multi-row source signatures now parse but the HIR
  lowerer collapses them to length-1; the lowerer broadening lands
  in v0.19.
- **`sigstore-real`** cargo feature on `mty-pkg`: opt-in for real
  Sigstore keyless sign. Requires NASM on Windows build hosts (for
  the `aws-lc-rs` transitive). Default builds keep the
  deterministic stub envelope.

For Wasm component authors: `cargo run --target wasm32-wasi`
continues to produce a P2 component by default. Components signed
with `mty pkg publish --sign --features sigstore-real` now carry
the full standard Sigstore Bundle JSON in their `.bundle`
envelope; `cosign verify-blob --bundle <bundle>` consumes it
directly.

For users wanting pre-built binaries: the release workflow that
first fired on v0.15.0 continues to fire on every `v*` tag. The
GitHub Releases page should carry `mty` binaries for
Linux / macOS×2 / Windows shortly after each push.

## CI / infrastructure note

GitHub Actions had a documented partial outage during the v0.18
integration window — every CI job since `4cfecda` (v0.18 Tier 4.1
land) failed at the action-download step
(`codeload.github.com/dtolnay/rust-toolchain/...` returning HTTP
404). The CI workflows themselves are unchanged from v0.17.1; the
expected behaviour is that pre-existing green runs catch up once
GitHub's CDN recovers. All gates were verified green locally
before tagging:

- `cargo build --workspace` — clean
- `cargo test --workspace` — 1324 pass / 0 fail / 5 ignored
- `cargo clippy --workspace --all-targets -- -D warnings` — clean
- `cargo fmt --all -- --check` — clean
- `cargo test -p mty-driver --test conformance_full` — 1/1 pass
- `cargo test -p mty-driver --test conformance_codegen` — 22/22 pass
- `cargo test -p mty-driver --test selfhost_codegen` — 23/23 pass
- `cd impl-py && python -m pytest tests/` — 274/274 pass
- `bash demos/*/smoke.sh` — 4/4 pass
- `mkdocs build --strict` — clean (no `ERROR` / `WARNING` lines)

## Known issues

Canonical list in [`KNOWN_ISSUES.md`](../../../KNOWN_ISSUES.md).
v0.18 net delta vs v0.17:

- **CLOSED — P1 #1** `cabi_realloc` is a bump allocator with no
  `free` (extracted to dedicated module + 17 coverage tests).
- **CLOSED — P1 #2** Package signing is a stub (no real OIDC /
  Rekor) (`sigstore-real` feature now drives the real keyless flow
  with full Sigstore Bundle embedding).
- **CLOSED — P1 #3** MSRV gate uses `cargo build`, not `cargo
  test` (now `cargo build --workspace --tests` which compiles every
  test/bench/example target including `[dev-dependencies]`).

Carried over unchanged:

- **#4** `clippy-strict` job is `continue-on-error: true` (P1, will
  flip once allow-list shrinks).
- **#5** `mkdocs build` runs without `--strict` in pages.yml (P2;
  locally `--strict` passes today).
- **#6** Demo 02 `web/index.html` does not yet exercise the new
  realloc (P2).
- **#7** `--no-default-features` test job does not run the example
  sweep (P2).
- **#9** Cranelift egraph stack overflow workaround
  (`MTY_CRANELIFT_NO_OPT=1`).
- **#14** Go 3rd-impl cross-validation pending Go toolchain on the
  build host.
- **Windows named-pipe introspect backend** tracked for v0.19+.

New for v0.18:

- **Replay full byte-identical re-execution is v0.19.** Today the
  step-replayer walks the recorded events via `StepHandler` rather
  than driving a fresh `Runtime` from the seed. Closing this needs
  structural `Value` serialisation (today payloads use
  `format!("{:?}", args)`).
- **HIR multi-row-var lowerer is v0.19.** The parser ships
  `Vec<EFFECT_ROW_VAR>` but the HIR lowerer reads only the first;
  multi-var source signatures parse cleanly and collapse to a
  single row var at the HIR layer.
- **`Runtime::send` cluster routing is v0.19.** The cluster mesh
  transport layer is feature-complete; `Runtime::send` consults
  the router in v0.19 (a one-line addition + mechanical hookup).
- **Cluster correlation table for `Ask`** is v0.19.
- **Mutual-TLS client-cert verification by node id** is v0.19.
- **Sigstore inclusion-proof crypto verify** is v0.19 (today only
  the structural cross-check of the embedded Bundle's
  `messageDigest` against the recomputed bundle SHA-256 runs on
  the default verify path).

## v0.18 → v1.0-final roadmap

Carry-overs from v0.17 are unchanged for items that didn't land
this version. v0.18-specific follow-ups:

- **Replay byte-identical full re-execution**: serialize `Value`
  payloads structurally (not via `Debug`), then re-construct on
  replay to feed `Runtime::send`/`ask` exactly the same args.
- **HIR multi-row-var lowerer**: read every `EFFECT_ROW_VAR` child
  of `EFFECT_ROW_TAIL` and emit a fully-populated
  `Vec<HirRowVar>`.
- **Cluster `Runtime::send` routing**: add
  `Runtime::cluster: Option<SharedRouter>` and consult it in
  `send` / `ask`; `mighty.toml [cluster.peers]` parser.
- **Sigstore inclusion-proof crypto verify** on `fetch`.
- **Carry-overs**: open RFC-001..006 + RFC-008 + RFC-009 comment
  periods; run `go test ./...` on a Go-1.22+ host; split MT0001
  funnel; `mty-pkg` cross-file resolution; publish normative
  conformance suite as a downloadable kit; Windows named-pipe
  introspect backend.

## Acknowledgments

v0.18 was built across a five-track swarm followed by an integrator
pass:

- **cabi-realloc-swarm** — extract free-list allocator from
  `emit.rs` into `cabi_realloc.rs`; +8 coverage tests retaining
  the v0.10 algorithm; ~190 wasm instructions, 32-byte state.
  Commit `790295d`. **SHIPPED-FULL**.
- **sigstore-real-swarm** — wire real Fulcio + Rekor keyless
  signing; full Sigstore Bundle JSON embedding under
  `verificationMaterial.sigstoreBundle`; three structural verify
  tests on default builds; network round-trip test gated under
  `sigstore-real` + `#[ignore]`. Commit `5a4822a`. **SHIPPED-FULL**.
- **replay-hotpath-swarm** — wire 13 instrumentation sites into
  the Runtime hot path (spawn / send / ask / handle / IO / clock
  / random / budget / exit); zero-overhead `with_recorder`
  helper; 8 end-to-end tests; `BudgetTracker::trip` /
  `BudgetBreach::trace_reason` / `StdHost::with_agent_id`
  helpers. Commit `2cd6d96`. **SHIPPED-FULL**.
- **mixed-track-swarm** — multi-row-var parser tail
  (`!{| E1, E2}`); MT4059 active emit; MSRV gate
  `cargo build --workspace --tests`; cross-cut closes
  KNOWN_ISSUES #3. Commit `7cc34b6`. **SHIPPED-FULL**.
- **cluster-mesh-swarm** — `AgentAddr` + framed CBOR-over-TLS
  transport; `ClusterMesh` with multi-peer dialer + listener +
  reconnect + heartbeat absorption; 7 integration tests against
  real TLS sockets. Commit `4cfecda`. **SHIPPED-FULL**.

The integrator pass (this v0.18.0 tag commit) ran the gates
(1324 Rust + 274 Python + 92 conformance + 23 selfhost = 1713
tests passing, clippy strict, fmt, 4/4 demos), authored this
RELEASE-v0.18.md, flushed CHANGELOG.md, cleaned the README
(test counts, KNOWN_ISSUES status, Features bullets for the four
v0.18 surfaces, Roadmap pruning, Status rewrite), executed the
docs quality pass (three new internals pages — `agents.md`,
`introspect.md`, `replay.md`; internals README rewrite; mkdocs
nav extension), and promoted the spec to v1.0-RC4 with the
RFC-008 multi-row tail grammar amendment.

See
[`CABI_REALLOC_V0_18_NOTES.md`](../notes/CABI_REALLOC_V0_18_NOTES.md),
[`SIGSTORE_V0_18_NOTES.md`](../notes/SIGSTORE_V0_18_NOTES.md),
[`REPLAY_HOTPATH_V0_18_NOTES.md`](../notes/REPLAY_HOTPATH_V0_18_NOTES.md),
[`V0_18_CROSSCUT_NOTES.md`](../notes/V0_18_CROSSCUT_NOTES.md), and
[`CLUSTER_V0_18_NOTES.md`](../notes/CLUSTER_V0_18_NOTES.md)
for per-agent interpretation calls.
