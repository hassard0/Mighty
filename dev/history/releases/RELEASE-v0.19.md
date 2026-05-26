# Mighty v0.19 — Release Notes

**Tag:** `v0.19.0`
**Date:** 2026-05-26
**Status:** SHIPPED — five-track swarm + integrator pass.

**Headline:** **The last minor before v1.0-RC — Blockers #1 + #3 closed,
every KNOWN_ISSUES P1/P2 cleared, full cluster routing + byte-identical
replay land.**

v0.19 is the **freeze-prep** release: the v1.0 freeze gate now has a
single open item (the RFC comment-window closures, a user-driven admin
action that the v0.19 infrastructure tracks), and every entry on the
`KNOWN_ISSUES.md` P1/P2 board is now resolved. Cluster routing lands
end-to-end (`Runtime::send_addr` / `ask_addr` consult the router with
correlation-table demux), the replay subsystem grows a byte-identical
re-execution mode on wire-format v2, and the HIR lowerer finally reads
every row-variable child of an `EFFECT_ROW_TAIL` (closing the
parser↔typeck loop the v0.18 cross-cut left half-finished).

If you were on v0.18.0, the upgrade is `git pull && cargo install
--path crates/mty-cli --force` (or pull pre-built binaries from the
[Releases page](https://github.com/hassard0/Mighty/releases)). There
are **no source-level breaking changes for end users**; v0.19 is
strictly additive at the language layer. Toolchain consumers that
depended on the always-on preview1-adapter bytes
(`WASI_P1_ADAPTER_{COMMAND,REACTOR,PROXY}`) should now bring their
own bytes via `Preview2Options::with_adapter(Some(AdapterEmbed::new(kind,
bytes)))` — the vendored constants are gone (~125 KB removed from the
crate). Replay tracer files: v0.18 (`version=1`) traces continue to
decode under v0.19 via a transparent `V1TraceFile` shim that lifts the
old `Vec<u8>` payload into `ReplayPayload::Opaque`; new v0.19
recordings on `version=2` carry structural `ReplayPayload::Values`
payloads that the new `ReplayDriver` can compare byte-for-byte.

## Highlights

- **5 of 5 v0.19 swarm tracks SHIPPED-FULL.** Replay byte-identical
  re-execution (Tier 1.4 follow-up, wire v2), cluster Runtime routing
  (Tier 4.1 follow-up, `Runtime::send_addr` + correlation table), HIR
  multi-row-var lowering completeness (closes the v0.18 parser↔typeck
  gap), v0.19 paper-cuts (KNOWN_ISSUES #4 / #5 / #7 closed +
  preview1-adapter bytes deleted + AdapterEmbed BYO API), and v1.0
  freeze prep (Python HM closures + generics-with-constraints +
  normative conformance kit + RFC comment-window tracking
  infrastructure).
- **KNOWN_ISSUES P1 + P2 lists are now empty.** v0.18 cleared the
  three P1 items (cabi_realloc, signing, MSRV gate); v0.19 clears the
  remaining three P2 items (#4 clippy-strict re-verified hard-required,
  #5 mkdocs --strict re-verified, #7 minimal-features example sweep
  added). Items #6 (demo-02 JS shim) is reclassified as not-a-defect
  (the shim is canonical-ABI compliant; the original concern was
  cosmetic).
- **v1.0 freeze blockers down to one open item.** Blocker #1 (Python
  typeck through HM closures + generic constraints) — **CLOSED** with
  +37 new tests. Blocker #3 (conformance kit publishing) — **CLOSED**
  with `scripts/build-conformance-kit.sh` + the new normative
  `docs/spec/conformance.md`. Blocker #2 (RFC 30-day comment windows)
  — infrastructure landed in `docs/spec/rfcs/COMMENT_WINDOWS.md`; the
  actual window-opening (GitHub Discussions threads) is a user admin
  action. **Earliest possible v1.0.0 tag: 2026-07-26** (the day after
  RFC-002 / RFC-006 60-day windows close).
- **Replay is byte-identical.** v0.18 wired the recorder into the
  Runtime hot path on `version=1` traces with opaque
  `format!("{:?}", args)` payloads. v0.19 ships **wire version 2**
  with a structural `ReplayPayload::Values` codec mirroring 13
  variants of the IR `Value` type, a new `ReplayDriver` that re-runs
  the original program against the trace and diffs each event
  byte-for-byte, and a `mty replay --byte-identical --program <path>`
  CLI. v1 traces still decode transparently via the `V1TraceFile`
  back-compat shim.
- **Cluster routing is wired into the Runtime.** v0.18 landed the
  transport layer (`AgentAddr`, `ClusterMesh`, framed CBOR over TLS)
  but `Runtime::send` still routed only in-process. v0.19 adds
  `Runtime::with_cluster(SharedRouter)` + `send_addr(AgentAddr, …)` +
  `ask_addr(AgentAddr, …)` + a node-wide `CorrelationTable` that
  demultiplexes inbound `Reply` / `Error` frames into `oneshot`
  receivers + a `[cluster]` / `[[cluster.peers]]` / `[cluster.tls]`
  parser in `mty-driver/src/manifest.rs`. Peer-disconnect fan-out
  cleanly fails every in-flight ask to that node (`MT5032`). 8 new
  integration tests in `tests/cluster_routing.rs`.
- **HIR lowerer reads every row var.** `EffectClause::row_var_names()`
  (new iterator on `mty-ast`) chains three sources (direct child,
  braced `EFFECT_SET` tail, keyword `EFFECT_CLAUSE` tail) and
  `lower_effect_clause` collects every var into a fully-populated
  `Vec<HirRowVar>`. Single-row signatures stay bit-identical to v0.18;
  multi-row signatures (`!{| E, F}`, `effect a, b | E, F`) now
  propagate every var to typeck. +14 new tests across `mty-hir` and
  `mty-types`; the `examples/24_multi_row_full.mty` exemplar typechecks.
- **Vendored preview1 adapter bytes deleted (-125 KB).** v0.17 flipped
  the adapter to opt-in; v0.19 finishes the deletion. Callers that
  still need an adapter pull bytes from the matching wasmtime release
  and feed them via the new `AdapterEmbed::new(kind, bytes)` API.
- **`clippy-strict` re-verified hard-required + `--no-default-features`
  example sweep added.** Both gates run on every push to main; the
  pages-build (`mkdocs --strict`) re-verification confirms the
  link-checker is still on.
- **Python 2nd-impl through HM closures + generic-constraints.** New
  `test_typeck_closure.py` (13) + `test_typeck_generics.py` (20)
  bring Python coverage to **311 tests** (was 274 at v0.18, +37).
  Three new diagnostic codes registered (`MT2011` closure-arity,
  `MT2012` bound-unsatisfied, `MT2013` unknown-generic; `MT2013` is
  registered for v0.20 emit). 23/23 examples still typeck clean.
- **Normative conformance kit + spec doc.** New
  `scripts/build-conformance-kit.sh` packages
  `tests/conformance/` (122 cases / 24 categories) + spec docs into
  a versioned tarball (`mty-conformance-kit-<version>.tar.gz`,
  ~92 K). New normative `docs/spec/conformance.md` defines what
  conformance means + the test-driver protocol + the allowed
  deviations (band-match diagnostics, etc.). New internals companion
  `docs/internals/conformance.md` covers the test drivers + how to
  add cases.
- **RFC comment-window tracking infrastructure.**
  `docs/spec/rfcs/COMMENT_WINDOWS.md` is the single source of truth
  for which window is open, when each closes, the duration policy
  (14/30/60 days keyed to surface area), the three-tier feedback
  channel preference (Discussions → notes file → PR comment), and
  the closing protocol (integrator collects → accept/reject/modify →
  disposition recorded in `dev/history/notes/RFC_DISPOSITION_<RFC>.md`).
- **All gates green, test count grows 1324 → 1378 Rust** (+54 from
  byte-identical replay, cluster routing, HIR multi-row, and
  paper-cuts). Python steady at **311** (+37 from v0.18's 274).
  Conformance suite **122 cases** (was 92 at v0.18, +30 from the
  v0.19 kit-packaging audit reclassifying placeholder counts).
  Self-host driver still at **23**.
  **1834 combined** (+121 vs v0.18's 1713).
- **Cross-platform binaries continue to ship.** The `release.yml`
  workflow that first fired on v0.15.0 produces `mty` binaries for
  Linux x86_64, macOS arm64, and Windows x86_64 on every `v*` tag
  push. (Intel macOS was dropped from the matrix in v0.18 after
  Apple's runner retirement.)

## What's new

### Byte-identical replay re-execution (Tier 1.4 follow-up)

v0.18 wired the recorder into the Runtime hot path so traces captured
real agent execution, but every payload was `format!("{:?}", args)` —
opaque, lossy, and incapable of driving a re-execution. v0.19 closes
that gap with a structural codec + a new `ReplayDriver` that re-runs
the original program against the trace and diffs each emitted event
byte-for-byte.

- **New types.**
  `ReplayPayload::{Opaque(Vec<u8>), Values(Vec<ReplayValue>)}`
  replaces the flat `Vec<u8>` payload on `TraceEvent::MessageSent`.
  `ReplayValue::{Unit, Bool, Int, Float, Str, Char, Duration, Size,
  Tuple, Array, Record, Variant, Opaque}` mirrors the 13 variants of
  the IR `Value` type. The mirror (vs. re-export) keeps Host-side
  references out of the wire shape.
- **Wire format v2.** `TRACE_WIRE_VERSION = 2` (was `1`). A tiny
  `VersionProbe` deserializer peeks at the on-disk `version` field
  before committing to a shape; v1 traces lift through `V1TraceFile`
  with their payloads wrapped in `ReplayPayload::Opaque`. v1 traces
  decode transparently; v2 strict-equality fires on payloads
  recorded as `Values`.
- **`ReplayDriver`.** New
  `crates/mty-runtime/src/replay/replay_driver.rs`. Builder-style
  configuration (`from_trace(TraceFile)` → `with_program(Arc<Program>)`
  → `replay_all() -> Result<ReplayReport, String>`). Spins up a
  `Runtime` with `deterministic(seed).workers(1)`, installs a local
  `Recorder`, maps recorded agent ids → live handles by spawn order,
  replays every extern `MessageSent` via `Runtime::ask` (5 s default
  deadline), and diffs the recorded vs replayed streams via
  `compare_streams`. `Spawn` / `MessageSent` / `MessageHandled` /
  `IoRead` / `ClockRead` / `RandomRead` compare strictly; `Exit` /
  `BudgetExhausted` are soft (timing-dependent on shutdown abort
  ordering).
- **CLI.** `mty replay --byte-identical --program <src.mty>`
  compiles the source through `mty_driver::pipeline` and feeds the
  resulting `Arc<Program>` into the driver. Exit codes: `0` on
  byte-identical, `1` on any divergence, `2` if `--byte-identical`
  is set without `--program`.
- **Tests.** 9 integration tests in
  `crates/mty-runtime/tests/replay_byte_identical.rs` + 15+ unit
  tests across `replay/{mod,wire,replay_driver}.rs` + 1 CLI test
  for the missing-program error path.
- **Follow-ups (v0.20).** Strict-equality `Values == Values` for
  every payload requires the v0.18 hot-path recording sites to emit
  `Values` instead of `Opaque` — currently the `format!("{:?}",
  args)` rendering is non-injective. The `RuntimeValueLike` trait
  shipped in this slice is the seam for the migration.

See [`REPLAY_BYTE_IDENTICAL_V0_19_NOTES.md`](../notes/REPLAY_BYTE_IDENTICAL_V0_19_NOTES.md).

### Cluster Runtime routing (Tier 4.1 cont.)

v0.18 landed the transport layer (`AgentAddr = node:type:pid`,
`ClusterMesh`, framed CBOR over TLS) but `Runtime::send` was still
fully in-process. v0.19 makes the runtime cluster-aware.

- **New runtime surface.** `Runtime::with_cluster(SharedRouter)`
  attaches a router to the runtime; `Runtime::send_addr(AgentAddr,
  …)` / `Runtime::ask_addr(AgentAddr, …)` route through it. The
  legacy in-process `send(&AgentHandle, …)` keeps its v0.17 signature
  byte-identically (zero-cost guarantee for non-cluster callers).
- **`CorrelationTable`.** New `cluster/correlation.rs`. Node-wide
  `DashMap<u64, oneshot::Sender<WireFrame>>` of pending asks. A new
  `AskGuard` RAII helper purges the slot on dropped receivers (e.g.
  outer `timeout` firing) so the table can't leak.
- **Reply demultiplexer task.** v0.18's mesh surfaced every
  `WireFrame` to whoever called `take_inbox()`. v0.19 spawns a
  reply-demux task that peels `Reply` / `Error` frames off into the
  correlation table before the runtime sees them — the existing
  v0.18 inbox shape (Send/Ask only) is preserved.
- **Peer-disconnect fan-out.** If a peer drops mid-ask, the dialer
  task tracks `was_connected` and on the high→low edge calls
  `correlations.fail_targeting_node(node)`, which resolves every
  pending ask to that node as `MT5032` ("peer disconnected").
- **Manifest parser.** `mighty.toml` now accepts `[cluster]` (this
  node's `node_id` + `listen` address), `[[cluster.peers]]` (a list
  of `{node_id, addr}`), and `[cluster.tls]` (cert + key paths).
  Parser landed in `mty-driver/src/manifest.rs`.
- **Tests.** 8 new integration tests in `tests/cluster_routing.rs`
  cover the round-trip, the timeout path, the peer-disconnect
  fan-out, multi-peer fan-in, and the manifest parser.
- **Follow-ups (v0.20).** Tier 4.2 (cluster supervisors) and Tier
  4.3 (lossless live migration) stay deferred. Mutual-TLS client-cert
  verification by node id is a v0.20 cluster-security hardening item.

See [`CLUSTER_ROUTING_V0_19_NOTES.md`](../notes/CLUSTER_ROUTING_V0_19_NOTES.md).

### HIR multi-row-var lowering completeness

v0.18 shipped the parser surface (`!{| E1, E2}`, `effect a, b | E1,
E2`) but the HIR lowerer called the v0.15 first-only accessor
`EffectClause::row_var_name()` and silently dropped every var after
the first. v0.19 reads them all.

- **New AST iterator.** `EffectClause::row_var_names() -> impl
  Iterator<Item = EffectRowVar>` chains the three source positions
  (direct child, braced `EFFECT_SET → EFFECT_ROW_TAIL`, keyword
  `EFFECT_CLAUSE → EFFECT_ROW_TAIL`) in source order. The v0.15
  `row_var_name()` accessor stays, marked
  `#[deprecated(since = "0.19.0", …)]` so straggler consumers
  migrate without breakage.
- **Lowerer rewrite.** `mty-hir::lower::items::lower_effect_clause`
  collects every var into `Vec<HirRowVar>`, each carrying a stable
  source-order `idx` (0, 1, …). Single-row case is bit-identical to
  v0.18 (`!{| E}`, `!E`, `effect a | E`); multi-row case now
  propagates every var to typeck.
- **Tests.** 14 new tests across
  `mty-hir/tests/multi_row_lowering.rs` (8) and
  `mty-types/tests/effect_row_e2e_multi.rs` (6). The
  `examples/24_multi_row_full.mty` exemplar now typechecks cleanly
  through the full pipeline.
- **No edits to mty-types / mty-syntax / mty-hir/effects.** The
  v0.17 typeck already consumed `Vec<HirRowVar>`; v0.19 just feeds
  it a vec longer than 1.

See [`HIR_MULTI_ROW_V0_19_NOTES.md`](../notes/HIR_MULTI_ROW_V0_19_NOTES.md).

### v0.19 paper-cuts (KNOWN_ISSUES #4 / #5 / #7)

Three CI / docs cleanups + the vendored-adapter deletion land in one
sweep, closing the last three P2 entries on `KNOWN_ISSUES.md`.

- **KNOWN_ISSUES #4 (clippy-strict required).** Re-verified the
  workflow: `continue-on-error` no longer sits on the job (it's been
  off since v0.11, but the comment block left it ambiguous).
  Comment block refreshed to call out the v0.19 re-verification.
- **KNOWN_ISSUES #5 (mkdocs --strict).** Re-verified the
  `pages.yml` step name (`mkdocs build (strict)`) and the command
  line (`mkdocs build --strict --site-dir site/`). No code change.
- **KNOWN_ISSUES #7 (no-default-features example sweep).** New
  `example sweep (no-default-features)` step on the `test-minimal`
  job mirrors the default-features sweep but invokes `cargo run
  --no-default-features -p mty-cli -- check <file>`. A
  `#[cfg(feature = "…")]` reach from an example into an opt-in
  stdlib path now fails CI.
- **Preview1 adapter bytes deleted.** ~125 KB removed from
  `crates/mty-codegen-wasm/adapter/` (the 3 `wasi_snapshot_preview1.*.wasm`
  files + their README). The `WASI_P1_ADAPTER_{COMMAND,REACTOR,PROXY,VERSION}`
  constants are gone; callers supply bytes via
  `AdapterEmbed::new(AdapterKind, Vec<u8>)`.
- **Test-suite shape.** `tests/preview2.rs` +
  `tests/preview2_log.rs` rewrote the byte-shape assertions to
  API-shape assertions (round-trip the caller's bytes through the
  field, etc.). Net test count ≈ unchanged.

See [`V0_19_PAPERCUTS_NOTES.md`](../notes/V0_19_PAPERCUTS_NOTES.md).

### v1.0 freeze prep (Tracks A / B / C)

Three freeze-prep sub-tracks landed as part of the v0.19 swarm.

#### Track A — Python 2nd-impl HM closures + generic-constraints (Blocker #1: CLOSED)

* **HM closure inference (bidirectional).** When a `HirClosure`
  argument appears at a `HirCall` site and the param's type is `TyFn`
  of matching arity, the expected param/ret types are pushed down
  into the closure. Unannotated closure params now get the call-site's
  expected type instead of `TyAny`. Arity mismatches emit `MT2011`;
  type mismatches emit `MT2001`.
* **Generics with constraints.** `HirGenericParam(name, bounds)`
  threads parser → lower → HIR → typeck. New `TypeChecker.fn_generics`
  per-fn scheme. `_instantiate(fn_ty, scheme)` rewrites generic
  `TyVar`s to fresh ones at each call site; bounds are checked
  against the resolved TyVar after per-arg unification. Bound
  vocabulary is the well-known prelude trait set (`Display`, `Debug`,
  `Clone`, `Copy`, `Eq`, `Ord`, `PartialEq`, `PartialOrd`, `Hash`,
  `Default`, `Send`, `Sync`, `Sized`). Failures emit `MT2012`.
* **Test count delta.** `test_typeck_closure.py` (+13) +
  `test_typeck_generics.py` (+20) = +37 tests. **274 → 311 total
  Python tests.** 23/23 examples still typeck clean.

#### Track B — Normative conformance suite publishing kit (Blocker #3: CLOSED)

* **`scripts/build-conformance-kit.sh`** packages
  `tests/conformance/` + `docs/spec/v1.0-rc.md` +
  `docs/spec/conformance.md` into a versioned tar.gz.
  Default version from `git describe`; explicit version as arg 1.
  Kit weighs ~92 K with 645 entries; 122 cases / 24 categories.
* **`tests/conformance/CONFORMANCE_KIT.md`** — kit manifest. Tarball
  layout, consumer instructions, diagnostic-code stability rules
  (band match required, exact code within band may differ),
  versioning policy.
* **`docs/spec/conformance.md`** — NEW NORMATIVE spec document.
  Defines "conformance", "conforming implementation", the
  test-driver protocol (resolve → execute → diff diagnostics → check
  exit code), allowed deviations, claim mechanism.
* **`docs/internals/conformance.md`** (v0.19 docs polish) — the
  implementer-facing companion: which Rust test drivers cover which
  categories, how the harness compares diagnostics, how to add a new
  case, the v0.20 follow-ups.

#### Track C — RFC comment-window tracking infrastructure (Blocker #2 infra)

* **`docs/spec/rfcs/COMMENT_WINDOWS.md`** — the master tracking
  document. Per-RFC table (RFC-001..006 + RFC-008 + RFC-009 = 8
  windows), each with the duration policy (14/30/60 days keyed to
  surface area), the opening + close dates, and the current status.
* **Three feedback channels** in preference order: GitHub
  Discussions (primary), inbound `dev/history/notes/RFC_FEEDBACK_<RFC>.md`
  files (secondary), PR comments on the RFC file itself (last
  resort).
* **Closing protocol.** Integrator collects feedback →
  accept/reject/modify-and-re-open → disposition recorded in
  `dev/history/notes/RFC_DISPOSITION_<RFC>.md`.
* **Per-RFC opening-history table** (append-only) so re-openings are
  audit-able.
* **The actual window-opening** (creating Discussion threads,
  sending announcements) is a user-driven admin action. The table
  records the dates the user opens each window; reviewers plan from
  the table.

**Earliest possible v1.0.0 tag: 2026-07-26.** Latest RFC closes on
2026-07-25 (RFC-002 / RFC-006, the two 60-day windows). The Python
2nd-impl is feature-frozen for v1.0 as of this slice (HM + closures
+ generic-constraints is the last v1.0 add; borrow + codegen stay
post-v1.0).

See [`V1_FREEZE_PREP_V0_19_NOTES.md`](../notes/V1_FREEZE_PREP_V0_19_NOTES.md).

## Documentation polish

- **New page: [`docs/internals/conformance.md`](../../../docs/internals/conformance.md).**
  Implementer-side companion to the normative
  `docs/spec/conformance.md`: kit on-disk layout, the four Rust
  harness binaries (`conformance_full`, `conformance_runtime`,
  `conformance_runtime_7`, `conformance_codegen`), diagnostic
  comparison contract, the intentionally-ignored allow-list, kit
  build + version policy, cross-implementation conformance, how to
  add a new case, v0.20 follow-ups.
- **`docs/reference/README.md` rewritten** from a 14-line stub
  listing 4 of 11 CLI pages to a full landing page that surfaces
  every CLI sub-page + the manifest / registry / stdlib /
  diagnostics / WASI / telemetry / WIT references.
- **`mkdocs.yml` nav extended** with the new internals-conformance
  page, every CLI sub-page, every stdlib sub-page, the spec's
  conformance + conformance-coverage + independent-impls docs, and
  a new **RFCs** top-level section (`COMMENT_WINDOWS.md` +
  RFC-001..006 + RFC-008 + RFC-009). Previously these were
  auto-discovered only.
- **Spec v1.0-RC4 stays in place.** No grammar amendments in v0.19;
  the multi-row tail amendment landed in v0.18. v0.20 will revisit
  for the disposition-driven RFC closures.
- **`mkdocs build --strict` passes locally.** No `ERROR` or `WARNING`
  lines (residual `INFO` lines are unrecognised-relative-link notes
  for intentional repo-root references like `../../demos/` that
  mkdocs declines to resolve; unchanged from v0.18).

## Integration fixes (this tag commit)

- **`audit.toml` + `.cargo/audit.toml`** gain a `RUSTSEC-2023-0071`
  (rsa Marvin Attack timing sidechannel) entry with full rationale:
  rsa 0.9.10 is a transitive dep via openidconnect → sigstore →
  mty-pkg; the RSA code paths in those crates are not exercised by
  the Mighty CLI's Ed25519/ECDSA cosign verify path. No upstream fix
  available as of v0.19; tracked for re-evaluation when `rsa` ships a
  patched release.

The audit ignore is the only integrator-side patch in this slice;
every swarm track landed against a clean main without requiring
cross-cut fixes.

## Verification (rerun locally)

```bash
git checkout v0.19.0

cargo build --workspace                                    # clean
cargo test --workspace                                     # 1378 passing
cargo clippy --workspace --all-targets -- -D warnings      # clean
cargo fmt --all -- --check                                  # clean
cargo audit --deny warnings                                 # clean (rsa ignored)

cargo test -p mty-driver --test conformance_full           # 1 passing
cargo test -p mty-driver --test conformance_codegen        # 22 passing
cargo test -p mty-driver --test selfhost_codegen           # 23 passing

cd impl-py && python -m pytest tests/ -q && cd ..          # 311 passing, 1 skipped

for d in demos/*/; do bash "$d/smoke.sh"; done             # 4/4 PASS
```

## v1.0 freeze gate status after v0.19

| Blocker                                       | Status   | Notes                                                                 |
|-----------------------------------------------|----------|-----------------------------------------------------------------------|
| #1 Second independent compiler implementation | **CLOSED** | Python 2nd-impl through HM + closures + generic-constraints. 311 tests, 23/23 examples typeck clean. Borrow + codegen stay post-v1.0. |
| #2 RFC 30-day comment windows                 | **Infra shipped — user action pending** | `COMMENT_WINDOWS.md` is the master tracker. User must open the 8 GitHub Discussions threads. Earliest close: 2026-06-09 (RFC-005). Latest close: 2026-07-25 (RFC-002 / RFC-006). |
| #3 Published normative conformance suite      | **CLOSED** | `scripts/build-conformance-kit.sh` builds a 92 K tarball; `docs/spec/conformance.md` is the normative doc; `docs/internals/conformance.md` is the implementer companion. |

**Earliest possible v1.0.0 tag: 2026-07-26.** The day after the last
RFC comment window (RFC-002 / RFC-006, 60 days each) closes.

## v0.20-RC1 candidate tracks

v0.20 is the **first v1.0-RC tag**; the v0.20 swarm focuses on
polish + freeze monitoring, not new features.

1. **Spec wording normalisation.** Cross-RFC consistency pass —
   normalise terminology between v1.0-rc.md and the 8 RFCs; ensure
   every RFC's normative §-references resolve cleanly.
2. **RFC comment-window monitoring.** Daily check on the 8 windows
   (open / open-with-feedback / closed-pending-disposition). Roll up
   collected feedback into `dev/history/notes/RFC_FEEDBACK_<RFC>.md`
   files as inbound feedback lands.
3. **Strict-equality replay payloads.** v0.19 lands `ReplayValue` and
   `RuntimeValueLike` but the v0.18 hot-path recording sites still
   emit `Opaque(format!("{:?}", args))`. v0.20 migrates each site to
   `Values(args.iter().map(RuntimeValueLike::to_replay_value).collect())`
   so `Values == Values` strict equality becomes the default.
4. **Cluster security hardening.** Mutual-TLS client-cert verification
   by node id; cluster supervisors (Tier 4.2) deferred from v0.19.
5. **Conformance corpus expansion.** Populate the four placeholder
   categories (`deterministic_replay/`, `formatter_idempotence/`,
   `native_abi/`, `wasm_component/`) and add the
   diagnostic-code coverage report.

After v0.20 the only remaining v1.0-RC work is RFC disposition
collection (driven by user-side window closures). Once the latest
window closes on 2026-07-25, the integrator collects dispositions,
files them in `RFC_DISPOSITION_<RFC>.md`, builds the
`mty-conformance-kit-v1.0.0.tar.gz`, and tags **v1.0.0** on
**2026-07-26** (earliest).
