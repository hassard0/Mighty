# Mighty v0.20 — Release Notes

**Tag:** `v0.20.0`
**Date:** 2026-05-26
**Status:** SHIPPED — five-track swarm + integrator pass.

**Headline:** **The full post-v1.0 roadmap is now live pre-v1.0 —
hot reload, cluster mTLS+supervisor, DWARF v5, byte-identical
replay all landed.**

v0.20 collapses the entire `### Post-v1.0` block from the v0.19
roadmap into shipping code. Hot reload (Tier 1.5 of
`docs/internals/agent-features-roadmap.md`) lands with a `Resumable`
trait + the swap pipeline (pause → drain → snapshot → schema-check
→ restore → resume) + a `mty reload` CLI. Cluster security
hardening (Tier 4.2) lands with mutual-TLS + CN-bound node identity
+ a `ClusterSupervisor` with 3 restart strategies + a per-child
circuit breaker. DWARF v5 lands as an opt-in (`MTY_DWARF5=1`)
parallel emission path next to the existing v4 default. The
v0.19-prepared `ReplayValue` migration finishes: the recorder hot
path now emits `ReplayPayload::Values` directly, so byte-identical
replay is the live semantic, not just the v0.19 capability. The
conformance kit's four placeholder categories from v0.19 are
populated (122 → 140 cases) and the kit is now built + attached by
`.github/workflows/release.yml` on every tag push.

If you were on v0.19.0, the upgrade is `git pull && cargo install
--path crates/mty-cli --force` (or pull the v0.20.0 pre-built
binaries from the Releases page). There are **no source-level
breaking changes** at the language layer. Toolchain consumers that
construct `ClusterConfig` directly continue to do so unchanged;
the new mTLS path is an opt-in `ClusterMesh::from_config_mtls(cfg)`
constructor. Replay trace files: v1 + v2 traces continue to decode
under v0.20; new v0.20 recordings always carry structural
`ReplayPayload::Values` payloads on the in-process send / ask
paths (cluster routing still speaks the `Vec<u8>` wire envelope by
transport contract, because the receiver structurally decodes on
the other side of the mesh).

## Highlights

- **5 of 5 v0.20 swarm tracks SHIPPED-FULL.** Hot reload (Tier 1.5
  with `Resumable` + swap pipeline + `mty reload` CLI + 24 new
  tests + MT5060–MT5069 diagnostic band), cluster mTLS + Tier 4.2
  supervisor (CN-bound identity + 3 restart strategies + circuit
  breaker + 13 new tests), DWARF v5 + per-instruction line program
  (opt-in via `MTY_DWARF5=1` + 5 new integration tests), strict-
  equality replay payloads (v0.18 hot-path migrated from `Opaque`
  to `Values` + 5 strict-equality tests + spec cross-ref polish),
  conformance corpus expansion (kit 92 K → 108 K, 122 → 140 cases,
  `coverage.json`, kit now auto-built + attached on every release
  tag).
- **The entire former "Post-v1.0" roadmap is live pre-v1.0.** v0.19
  carried four roadmap items as post-v1.0: live agent migration
  precursor (hot reload), cluster supervisors + mTLS hardening,
  DWARF v5, and a few smaller items. v0.20 ships all three of the
  big ones. Only Tier 4.3 lossless live migration, per-message
  work-stealing (Tier 5), Polonius borrows, cap-name resolution,
  PGO/ThinLTO, and the Python 2nd-impl's borrow + codegen layers
  remain explicitly post-v1.0.
- **KNOWN_ISSUES P1 + P2 lists stay empty.** No regressions, no new
  entries. The v0.19 closes (#1 / #2 / #3 / #4 / #5 / #7) hold;
  #6 stays not-a-defect.
- **v1.0 freeze blockers down to one open item — unchanged from
  v0.19.** Blocker #1 (Python 2nd-impl through HM + closures +
  generic-constraints) — **CLOSED**. Blocker #3 (conformance kit
  publishing) — **CLOSED**. Blocker #2 (RFC 30-day comment
  windows) — infrastructure live in
  `docs/spec/rfcs/COMMENT_WINDOWS.md`; the actual window-opening
  is a user-side admin action. **Earliest possible v1.0.0 tag:
  2026-07-26** (the day after RFC-002 / RFC-006's 60-day windows
  close).
- **Hot reload is live for state-only swaps.** New `Resumable`
  trait carries a `SCHEMA_HASH: u64` const + a default
  ciborium-backed `to_snapshot` / `from_snapshot` pair. The swap
  pipeline (`reload::swap`) pauses dispatch via `ReloadGate`,
  drains any in-flight handler against `--deadline-ms`, snapshots
  state, schema-checks the new module's hash against the live
  agent's, restores into a fresh `AgentRuntimeState`, and resumes
  the gate — mailbox preserved across the boundary. `mty reload
  <agent-type> --from new.wasm` is the CLI seam; supports
  `--dry-run`, `--deadline-ms`, `--sock`, `--json`. State-only
  reload (`ModuleSource::SameProgram`) is wired end-to-end; raw
  wasm reload (`ModuleSource::WasmBytes(_)`) is rejected with the
  new diagnostic `MT5064` until v0.21 lands `Program::with_swapped_agent`.
  Diagnostic band MT5060–MT5069 is reserved for reload concerns.
  +24 tests across `crates/mty-runtime/tests/reload.rs` (9),
  `reload/resumable.rs` (7), `reload/swap.rs` (5), and
  `crates/mty-cli/src/cmd/reload.rs` (3).
- **Cluster mTLS + CN-bound node identity.** New `cluster/tls.rs`
  builds rustls accept / connect configs from cert + key + CA-
  bundle paths and pins a hand-rolled `extract_cn_from_der` (~50
  LOC TLV walker, no extra dep — `x509-cert` was already
  transitively present via sigstore, but a single function isn't
  worth the dep promotion). `verify_peer_identity(node_id,
  cert_der)` rejects connections whose cert CN doesn't match the
  configured `node_id`. mTLS is opt-in via a new
  `ClusterMesh::from_config_mtls(cfg)` constructor — `ClusterConfig`
  shape is unchanged, so every v0.18 / v0.19 caller continues to
  compile.
- **Cluster supervisor (Tier 4.2) with 3 restart strategies +
  circuit breaker.** New `cluster/supervisor.rs` lands a
  `ClusterSupervisor` with per-child state machine, three restart
  strategies (`OneForOne`, `RestForOne`, `OneForAll`), and a
  per-child circuit breaker (sliding window of failures; trips
  open on `failures_per_window >= threshold`, half-open after
  `cooldown`, closes on next success). The supervisor emits events
  on a bounded `SUPERVISOR_EVENT_CAPACITY = 256` channel rather
  than invoking restart logic synchronously — the caller (or
  placement service, in v0.21) picks placement. Mesh disconnect
  notifications mark children `:noproc` via a new
  `notify_node_disconnect` hook. +13 tests across
  `cluster_mtls` (5), `cluster_supervisor` (6), and inline cert
  walker tests (4 in `cluster/tls.rs`).
- **DWARF v5 + per-instruction line program (opt-in).** New
  `crates/mty-debuginfo/src/dwarf5.rs` (~330 LOC) emits a v5
  `.debug_info` + `.debug_line` + `.debug_str` + `.debug_line_str`
  + `.debug_abbrev` quintuple alongside the existing v4 default.
  `mty-codegen-cranelift/src/debug.rs` gains a `build_dwarf_dispatch`
  that switches on `MTY_DWARF5=1` (env var rather than Cargo feature
  to keep the toggle at build time without rebuilding the workspace).
  v5 wins (denser opcode-table line rows, cross-CU string sharing
  via `.debug_line_str`) are dormant until cranelift `MachSrcLoc`
  plumbing lands per-instruction rows — the *capacity* is there
  (`Dwarf5Builder::add_function` writes one row per line-table
  entry, defensively skipping non-monotonic addresses); the
  conservative 2-entry table from v0.2 keeps the default v4
  emission deterministic. +5 integration tests in
  `crates/mty-debuginfo/tests/dwarf5.rs` (header magic, indirect-
  string-table, round-trip, monotonic-drop, file-id-zero re-add).
- **Strict-equality replay payloads (v0.18 hot-path completed).**
  The v0.19 work shipped `ReplayValue` + `RuntimeValueLike` but
  the v0.18 hot-path recording sites still emitted
  `Opaque(format!("{:?}", args))`. v0.20 migrates the two in-
  process send callsites (`Runtime::send`, `Runtime::ask`) to
  `encode_payload_for_trace_structural(&[Value]) -> ReplayPayload`,
  so fresh recordings carry structural payloads by default. The
  `ReplayDriver`'s strict structural equality arm is now the live
  replay semantic; the `Opaque ≈ Opaque` loose-equality arm
  becomes a backwards-compat fallback that never fires for fresh
  recordings. Cluster routing paths continue to use the byte
  envelope (transport contract; receiver decodes structurally on
  the other side of the mesh). +5 strict-equality tests in
  `crates/mty-runtime/tests/replay_strict_equality.rs`. Spec cross-
  reference polish: 7 broken internal anchor refs fixed across
  `docs/spec/v1.0-rc.md` (em-dash slugs collapse to single hyphens
  in `python-markdown`'s `toc.slugify`) + 1 stale RFC cross-ref
  removed in `docs/spec/rfcs/RFC-008-effect-rows.md`.
- **Conformance corpus expansion (placeholders → real fixtures).**
  v0.19 shipped the kit with 4 empty placeholder categories
  (`deterministic_replay/`, `formatter_idempotence/`, `native_abi/`,
  `wasm_component/`); v0.20 backfills all 4 (5 + 5 + 4 + 4 = +18
  cases). New `tests/conformance/coverage.json` makes diagnostic-
  code coverage machine-readable (53 covered / 42 auxiliary / 17
  uncovered; the uncovered set is unchanged from v0.11 and is
  hand-off documented). `.github/workflows/release.yml` grows a
  `conformance-kit` job that runs in parallel with `build`,
  shell-execs `scripts/build-conformance-kit.sh <tag>`, and adds
  the resulting `mty-conformance-kit-<version>.tar.gz` to the
  release's `files:` list. The kit now ships alongside the binaries
  on every tagged release. Kit grows ~92 K → ~108 K.
- **All gates green, test count grows 1378 → 1433 Rust** (+55 from
  hot reload (24), cluster mTLS+supervisor (13), DWARF v5 (5),
  strict-equality replay (5), plus per-track inline unit-test
  adds). Python steady at **311** (no change — v0.19's HM +
  closures + generic-constraints was the freeze-prep peak;
  borrow + codegen stay post-v1.0). Conformance **140 cases**
  (was 122 at v0.19; the 4 placeholder categories now populated).
  Self-host driver still at **23**. **1907 combined** (+73 vs
  v0.19's 1834).
- **Conformance kit auto-publishes alongside binaries.** Tagging
  `v0.20.0` triggers `release.yml`, which now builds + uploads
  `mty-conformance-kit-v0.20.0.tar.gz` (~108 K) next to the three
  binaries (Linux x86_64, macOS arm64, Windows x86_64). The
  v0.15.0-introduced binary matrix is unchanged.

## What's new

### Hot reload — `Resumable` + swap pipeline + `mty reload` CLI

Agent-features-roadmap Tier 1.5 lands. The reload module is
strictly additive — no existing runtime hot path consumes it; the
gate is only consulted by code that opts in. Agents that don't
`impl Resumable` cost the same as a v0.19 build.

- **`Resumable` trait.** New `crates/mty-runtime/src/reload/resumable.rs`.
  Carries `const SCHEMA_HASH: u64` + default `to_snapshot(&self) ->
  Vec<u8>` / `from_snapshot(bytes) -> Self` via `ciborium`
  (already a workspace dep through the cluster wire). Users who
  want a different codec override both methods. New
  `compute_schema_hash(fields: &[(&str, &str)]) -> u64` helper
  (FNV-1a, order-insensitive) for agents that derive their schema
  hash from a struct shape. A future v0.21 derive macro will emit
  the const from the struct shape automatically.
- **Swap pipeline.** New `crates/mty-runtime/src/reload/swap.rs`.
  `ReloadGate` arbitrates the pause/busy state — handlers check
  `gate.dispatchable()` before dispatching, the swap path calls
  `gate.acquire_swap()` and waits for `gate.mark_idle()` (busy-
  polled with 1 ms sleep — a v0.21 follow-up replaces with a
  condvar). The runner walks pause → drain (against
  `--deadline-ms`) → snapshot → schema check → restore → resume
  and reports via `ReloadReport`. Failures map to the
  `ReloadError` enum (one variant per `MT506x` diagnostic).
- **`ModuleSource`.** Two variants: `SameProgram` (state-only
  restart, wired end-to-end) and `WasmBytes(Vec<u8>)` (rejected
  with `MT5064` until v0.21). The CLI accepts both via `--from
  <path>`; passing a `.wasm` file errors clean today.
- **CLI.** `mty reload <agent-type> --from <path>` (with
  `--dry-run`, `--deadline-ms`, `--sock`, `--json`). The wire
  shape matches the v0.16 `mty inspect` control-socket contract;
  the runtime listener handler in
  `crates/mty-runtime/src/control_socket.rs` is a v0.21 follow-up
  (the CLI ships ahead of the runtime listener so the wire
  contract is testable in isolation).
- **Diagnostic band MT5060–MT5069 reserved for reload.**
  - `MT5060` IncompatibleSchema — new module's hash doesn't match.
  - `MT5061` AgentNotFound — no live agent matching the request.
  - `MT5062` DrainDeadline — handler didn't return inside `--deadline-ms`.
  - `MT5063` Snapshot — snapshot encode / decode failed.
  - `MT5064` WasmReloadNotImplemented — `WasmBytes` rejected (v0.20).
  - `MT5069` Internal — runtime catch-all.
- **Tests (+24).** `tests/reload.rs` (9 integration: compatible
  swap, incompatible-schema reject, drain wait, deadline trip,
  mailbox preservation, raw-wasm reject, dry-run, pure-data
  helpers) + inline (`reload/resumable.rs` 7, `reload/swap.rs` 5,
  `cmd/reload.rs` 3 — base64, JSON escape, pretty-print).

See [`HOT_RELOAD_V0_20_NOTES.md`](../notes/HOT_RELOAD_V0_20_NOTES.md).

### Cluster mTLS + Tier 4.2 supervisor

Agent-features-roadmap Tier 4.2 lands the security-hardening half
of "distributed supervisors + mTLS hardening." Cross-node fail-over
(Tier 4.2 placement) and lossless live migration (Tier 4.3) stay
deferred — the supervisor emits `RestartRequested` events but
carries no "place me here instead" hint until a `PlacementPolicy`
abstraction lands in v0.21.

- **mTLS configuration.** New `cluster/tls.rs`. `ClusterTlsConfig`
  carries cert + key + CA bundle paths; `build_acceptor` /
  `build_connector` return `Arc<rustls::ServerConfig>` /
  `Arc<rustls::ClientConfig>`. The connector pins
  `verify_peer_identity(node_id, cert_der)` as a custom
  `ServerCertVerifier`-driven post-handshake check, so a peer
  presenting a valid CA-chained cert with the wrong CN is rejected
  even if everything else verifies.
- **CN extraction.** `extract_cn_from_der(cert_der) ->
  Result<String, TlsError>` walks the top-level TLVs inside
  `TBSCertificate` looking for the *second* `Name` (issuer is
  first, subject second). Self-signed certs with a single Name fall
  through. A future SPIFFE-aware extractor reuses the walker. 4
  unit tests pin behaviour (simple CN, dashy CN, mismatch error,
  SAN-only fallback).
- **mTLS opt-in.** `ClusterMesh::from_config_mtls(cfg)` is a new
  constructor that flips an internal `require_mtls: bool` flag —
  `ClusterConfig` shape unchanged, so v0.18 / v0.19 struct-literal
  callers compile clean.
- **`ClusterSupervisor`.** New `cluster/supervisor.rs`. Per-child
  state machine (Running / Restarting / NoProc / Backoff). Three
  restart strategies match the runtime supervisor surface
  (`OneForOne` restarts only the failed child; `RestForOne`
  restarts the failed child + everything started after it;
  `OneForAll` restarts every child). Per-child circuit breaker:
  sliding window of failures, trips open at
  `failures_per_window >= threshold`, half-open after
  `cooldown_ms`, closes on next success. When tripped open, the
  supervisor refuses to plan a restart and emits
  `CircuitBreakerTripped` instead.
- **Event-driven, not callback-driven.** Restart decisions are
  emitted on a bounded `SUPERVISOR_EVENT_CAPACITY = 256` channel
  rather than invoked synchronously — the caller (placement
  service in v0.21) decides what "restart" means. Tests observe
  via `try_next_event`; channel full is bad-not-corrupting (the
  authoritative state lives in `children`, re-pollable via
  `state_of`).
- **Mesh → supervisor disconnect notify.**
  `ClusterMesh::register_supervisor(Arc<dyn SupervisorHook>)` lets
  the mesh notify the supervisor when a peer drops. The hook
  marks affected children `:noproc` (idempotent across repeated
  disconnect notifications on the same `node_id`).
  `SupervisorHook` is hand-rolled `Future + Send` for one async
  method — no `async-trait` dep needed.
- **Tests (+13).** `tests/cluster_mtls.rs` (5: cert + key load,
  identity verify accept, identity verify reject on bad CN, SAN
  fallback, opt-in flag) + `tests/cluster_supervisor.rs` (6:
  one-for-one, rest-for-one, one-for-all, circuit-breaker trip,
  circuit-breaker recovery, NoProc on disconnect) + inline 4
  cert-walker tests.

See [`CLUSTER_MTLS_SUPERVISOR_V0_20_NOTES.md`](../notes/CLUSTER_MTLS_SUPERVISOR_V0_20_NOTES.md).

### DWARF v5 + per-instruction line program (opt-in)

A parallel emission path next to the v4 default, gated on
`MTY_DWARF5=1` (env var rather than Cargo feature — feature
unification across the workspace would invalidate caches for the
v4 path on every test).

- **New builder.** `crates/mty-debuginfo/src/dwarf5.rs` (~330
  LOC). Emits the v5 `.debug_info` + `.debug_line` +
  `.debug_str` + `.debug_line_str` + `.debug_abbrev` quintuple.
  Uses `gimli::write::Dwarf::new_5()` + `LineProgram::new_5()`.
  The v5 `LineProgram::new` automatically inserts the comp_file
  at index 0 but doesn't return its id (`FileId::new` is
  `pub(crate)`); `add_file` is idempotent on the key, so calling
  it a second time returns the existing `FileId(0)` — verified by
  walking the v5 output through `gimli::read`.
- **Codegen dispatch.** `mty-codegen-cranelift/src/debug.rs`
  gains `build_dwarf_dispatch(...)` + `dwarf5_enabled() ->
  bool` (`std::env::var("MTY_DWARF5") == Ok("1")`). The
  `mty-codegen-cranelift/src/object.rs` call-site flips from
  `build_dwarf_for` to `build_dwarf_dispatch` (1 line change).
  Default build (`MTY_DWARF5` unset) is byte-identical to v0.19.
- **Defensive monotonic-address handling.**
  `LineProgram::generate_row` has a `debug_assert` that
  address_offsets within a sequence are monotonically increasing.
  v4 doesn't guard this — a malformed `FunctionDebugInfo` would
  panic in debug builds. v5 silently skips out-of-order entries
  (covered by `dwarf5::tests::drops_out_of_order_rows`). v4 was
  deliberately not backported to avoid changing observable v4
  behaviour.
- **Honest performance note.** Today's single-CU workload makes
  v5 slightly *larger* than v4 (+3.2% on a 16-fn × 32-row × 4-
  local synthetic) because `.debug_line_str` carries fixed
  overhead (24 bytes) regardless of cross-string sharing. The v5
  wins kick in when (a) many CUs share path strings — dormant
  until the build splits per-module / merges cross-package, or
  (b) per-instruction line rows exceed per-block — dormant until
  cranelift `MachSrcLoc` plumbing lands the dense rows. The v5
  *capacity* for both wins is wired; the *enablement* is a
  follow-up.
- **Tests (+5).** `crates/mty-debuginfo/tests/dwarf5.rs` —
  header magic, indirect string table, full round-trip parse,
  out-of-order row drop, file-id-zero re-add trick.

See [`DWARF5_V0_20_NOTES.md`](../notes/DWARF5_V0_20_NOTES.md).

### Strict-equality replay payloads + spec cross-ref polish

v0.19 landed `ReplayValue` + `RuntimeValueLike` + a `ReplayDriver`
that already supported `byte_identical(true)` — but the v0.18 hot-
path recording sites still emitted `Opaque(format!("{:?}", args))`,
so the strict-equality arm never fired for fresh recordings. v0.20
finishes the migration.

- **Hot-path sites migrated.** `Runtime::send` and `Runtime::ask`
  (the two in-process send callsites) now call a new
  `encode_payload_for_trace_structural(&[Value]) -> ReplayPayload`
  helper that walks the slice via `encode_values_payload` (already
  shipped in v0.19, unused until now) and wraps the result in
  `ReplayPayload::Values`. The recorder side calls
  `record_message_sent_payload` (also v0.19, unused until now). The
  legacy `record_message_sent(..., Vec<u8>)` API stays — it's still
  the right shape for the cluster routing path (`route_send` /
  `route_ask`), which speaks opaque bytes by transport contract.
- **Other sites unaffected.** `with_recorder(...)` callsites in
  `agent.rs`, `host_std.rs`, and `budget.rs` cover IO / clock /
  random / handler-dispatch / budget / exit events — none of those
  carry user-typed payloads, so they were structural-by-construction
  in v0.19 and are unchanged.
- **Hot-path benchmark.** `benches/runtime_send.rs` shows no
  measurable delta with `MTY_RECORD_TRACE` unset (the
  `recording_enabled()` short-circuit fires before any allocation).
  With recording on, allocations are roughly N + 1 for an N-arg
  payload versus the old 1 large `format!` string — slightly more
  on count, considerably less on bytes for non-trivial payloads.
- **Tests (+5).** `crates/mty-runtime/tests/replay_strict_equality.rs`:
  two-agent zero-mismatch under `byte_identical(true)`; multi-typed
  args exact match (`[Int(7,I64), Int(35,I64)]`); structural-payload
  disk round-trip (record → write → decode → re-encode → re-decode);
  3-agent chain no-fallback (zero non-empty Opaque); legacy-Opaque
  write-side regression guard (mixed-arm trace survives encode +
  decode).
- **Spec cross-reference polish.** 7 broken internal anchor refs
  fixed across `docs/spec/v1.0-rc.md`:

  | Broken anchor                                          | Fixed anchor                                   |
  | ------------------------------------------------------ | ---------------------------------------------- |
  | `#appendix-a--v10-scope-frozen--open-matrix`           | `#appendix-a-v10-scope-frozen-open-matrix`     |
  | `#appendix-b--backwards-compatibility-policy` (×3)     | `#appendix-b-backwards-compatibility-policy`   |
  | `#appendix-c--cross-reference-map-amendment--spec-section` | `#appendix-c-cross-reference-map-amendment-spec-section` |
  | `#255-deterministic-mode`                              | `#255-deterministic-mode-a35-a39`              |
  | `#116--propagation-a7`                                 | `#116-propagation-a7`                          |

  The python-markdown `toc.slugify(text, '-')` algorithm collapses
  runs of non-word characters into a *single* hyphen, so headings
  with em-dashes or inline code spans never produced the double-
  hyphen slugs the table-of-contents was written against. Fixed
  mechanically — Python audit script round-trips every heading
  through `slugify` and diffs against every `](#...)` reference.
  Final audit: 0 broken anchors in `v1.0-rc.md` (was 7 of 78).
  Bonus: `docs/spec/rfcs/RFC-008-effect-rows.md` had one stale
  cross-RFC reference ("deferred to **RFC-009**" in an Open
  Question about effect handlers; RFC-009 is *Set-of-Scopes Macro
  Hygiene*, not effect handlers) — replaced with "deferred to a
  future RFC."

See [`REPLAY_STRICT_V0_20_NOTES.md`](../notes/REPLAY_STRICT_V0_20_NOTES.md).

### Conformance corpus expansion + kit-in-release

v0.19 published the conformance kit with 122 cases / 24 categories
— 20 populated + 4 placeholder. v0.20 fills the placeholders.

- **`deterministic_replay/` (+5).** `01_pure_program` (empty trace
  invariant), `02_clock_read` (ClockRead per host observation),
  `03_random_seq` (RandomRead per host observation),
  `04_send_message` (Spawn / MessageSent / MessageHandled ordering),
  `05_replay_roundtrip` (byte-identical record→replay invariant).
  `expected_trace.txt` per case describes the recorder's expected
  output; the trace-shape assertion lives in
  `crates/mty-runtime/tests/replay_*.rs`.
- **`formatter_idempotence/` (+5).** `01_canonical_struct`,
  `02_canonical_match`, `03_canonical_effect_clause` (multi-row-var
  preserved — the v0.19 lowerer fix), `04_canonical_comments`,
  `05_canonical_macro`. Each ships `input.mty` + `canonical.mty`;
  byte-equivalence assertion lives in `crates/mty-fmt/tests/`.
- **`native_abi/` (+4).** `01_export_main`, `02_string_return`
  (cabi_realloc convention), `03_struct_return` (by-value),
  `04_callback`. Each ships a `harness.c` linkable against the
  emitted object with an `expected_harness_exit.txt`. The
  link-and-run assertion is wired in
  `crates/mty-codegen-cranelift/tests/` (or v0.21 follow-up; the
  fixtures are kit-ready today).
- **`wasm_component/` (+4).** `01_minimal_component`,
  `02_wasi_p2_log` (direct `wasi:cli/stdout` + `wasi:io/streams`),
  `03_wasi_p2_fs` (direct `wasi:filesystem/types` + `preopens`),
  `04_user_wit` (`--wit world.wit` → custom export).
  `expected_component.txt` per case describes the import/export
  list the emitted component MUST carry. Component-shape assertion
  lives in `crates/mty-codegen-wasm/tests/`.
- **`tests/conformance/coverage.json`.** New machine-readable
  diagnostic-code coverage report. Shape: `version` + per-category
  `case_count` + `diagnostic_codes` + a global `covered` /
  `auxiliary` / `uncovered` split. v0.20 numbers: 53 covered (48%
  of registered MTxxxx) / 42 auxiliary (38%) / 17 uncovered (15%).
  The 17 uncovered codes are the unchanged v0.11 hand-off set —
  they need crate-source emit-site work, which is excluded from
  this slice's scope.
- **`.github/workflows/release.yml` gains a `conformance-kit`
  job.** Runs in parallel with `build`. Shell-execs
  `scripts/build-conformance-kit.sh <tag>` and uploads the
  resulting tarball as a workflow artifact. The `release` job's
  `needs:` list now includes `conformance-kit`; its `files:` list
  now includes `out/mty-conformance-kit-*.tar.gz`. The build
  matrix, binary packaging, and the existing 3 binary targets
  (Linux x86_64, macOS arm64, Windows x86_64) are unchanged.
- **`conformance_full.rs` per-category floor assertions.** Added
  for the 4 newly-populated categories (≥5 / ≥5 / ≥4 / ≥4). The
  pre-existing ≥70 cases overall floor stays. Catches the
  regression where a future agent accidentally `rm -rf`s a
  category directory.

Kit size: ~108 K (was ~92 K at v0.19), 140 cases (was 122 at
v0.19, +18).

See [`CONFORMANCE_V0_20_NOTES.md`](../notes/CONFORMANCE_V0_20_NOTES.md).

## Documentation polish

- **New page: [`docs/internals/hot-reload.md`](../../../docs/internals/hot-reload.md).**
  Hot-reload architecture, the `Resumable` trait surface, the swap
  pipeline state machine, the diagnostic band, the v0.21
  follow-ups (wasm reload, schema-evolution migrations, multi-
  version rolling restart, control-socket reload handler, condvar
  drain wake-up). Linked from `mkdocs.yml` Internals nav between
  Introspect and Replay.
- **New page: [`docs/reference/cli/mty-reload.md`](../../../docs/reference/cli/mty-reload.md).**
  CLI usage for `mty reload`: synopsis, options, JSON output
  shape, exit codes, examples. Linked from `mkdocs.yml`
  Reference → CLI nav after `mty-replay`.
- **Extended page: `docs/internals/cluster.md`.** New sections
  for the mTLS configuration surface, CN-bound identity model,
  the `ClusterSupervisor` lifecycle, restart strategies, circuit
  breaker semantics. Diagrams describe the mesh → supervisor
  disconnect-notify flow.
- **Extended page: `docs/internals/debug-info.md`.** New DWARF v5
  section with opt-in instructions (`MTY_DWARF5=1`), v4-vs-v5
  diffs, tooling matrix (gdb, lldb, llvm-dwarfdump, addr2line
  versions tested), binary-size measurements + the honest
  "today's wins are dormant" caveat.
- **`docs/spec/v1.0-rc.md` cross-reference cleanup.** 7 broken
  internal anchor refs fixed (slug double-hyphen → single-hyphen);
  audited via a Python script that diffs `slugify` output against
  every `](#...)` reference. 0 broken anchors after.
- **`docs/spec/rfcs/RFC-008-effect-rows.md` stale RFC-009 ref
  removed.** "deferred to **RFC-009**" → "deferred to a future
  RFC" (RFC-009 is Set-of-Scopes Macro Hygiene, not effect
  handlers).
- **`mkdocs build --strict` passes locally.** No ERROR or WARNING
  lines (residual INFO lines are unrecognised-relative-link notes
  for intentional repo-root references like `../../demos/` that
  mkdocs declines to resolve; unchanged from v0.19).

## Integration fixes (this tag commit)

- **`crates/mty-runtime/tests/replay_strict_equality.rs`:**
  collapsed-match clippy lint fixed (the cluster-mTLS swarm agent
  noted this was pre-existing and out of its slice scope; the
  integrator owns cross-cut fixes). The double `match` is now a
  single `matches!` with an `if` guard.
- **`crates/mty-runtime/src/reload/resumable.rs`:** unusual-byte-
  groupings clippy lint fixed on the doc-string schema-hash const
  (`0xa1b2c3d4e5f6_0001` → `0xa1b2_c3d4_e5f6_0001`). Pre-existing
  from the hot-reload swarm agent's slice; integrator-owned
  cross-cut.
- **`crates/mty-cli/src/cmd/reload.rs`:** manual-div-ceil clippy
  lint fixed in the tiny self-contained base64 encoder
  (`(bytes.len() + 2) / 3 * 4` → `bytes.len().div_ceil(3) * 4`).
  Same pre-existing-clean-up category.
- **`mkdocs.yml` nav extended** with the new `docs/internals/hot-
  reload.md` page (Internals → Hot Reload, between Introspect and
  Replay) and the new `docs/reference/cli/mty-reload.md` page
  (Reference → CLI → `mty reload`, after `mty replay`).

The clippy fixes are the only integrator-side patches in this
slice; every swarm track landed against a clean main and didn't
require deeper cross-cut surgery.

## Verification (rerun locally)

```bash
git checkout v0.20.0

cargo build --workspace                                    # clean
cargo test --workspace                                     # 1433 passing
cargo clippy --workspace --all-targets -- -D warnings      # clean
cargo fmt --all -- --check                                  # clean
cargo audit --deny warnings                                 # clean (rsa ignored)

cargo test -p mty-driver --test conformance_full           # 1 passing
cargo test -p mty-driver --test conformance_codegen        # 22 passing
cargo test -p mty-driver --test selfhost_codegen           # 23 passing

cd impl-py && python -m pytest tests/ -q && cd ..          # 311 passing, 1 skipped

for d in demos/*/; do bash "$d/smoke.sh"; done             # 4/4 PASS
```

## v1.0 freeze gate status after v0.20

| Blocker                                       | Status   | Notes                                                                 |
|-----------------------------------------------|----------|-----------------------------------------------------------------------|
| #1 Second independent compiler implementation | **CLOSED** | Python 2nd-impl through HM + closures + generic-constraints. 311 tests, 23/23 examples typeck clean. Borrow + codegen stay post-v1.0. |
| #2 RFC 30-day comment windows                 | **Infra shipped — user action pending** | `COMMENT_WINDOWS.md` is the master tracker. User must open the 8 GitHub Discussions threads. Earliest close: 2026-06-09 (RFC-005). Latest close: 2026-07-25 (RFC-002 / RFC-006). |
| #3 Published normative conformance suite      | **CLOSED** | `scripts/build-conformance-kit.sh` builds a ~108 K tarball; 140 cases / 24 categories; auto-attached to every tagged release; `docs/spec/conformance.md` is the normative doc; `docs/internals/conformance.md` is the implementer companion. |

**Earliest possible v1.0.0 tag: 2026-07-26.** The day after the
last RFC comment window (RFC-002 / RFC-006, 60 days each) closes.

## v0.21-RC1 candidate tracks

v0.21 is the **second v1.0-RC tag**; the v0.21 swarm continues to
focus on polish + freeze monitoring + closing the last roadmap
gaps that surfaced as v0.20 follow-ups.

1. **Wasm hot reload + schema migration.** Land
   `Program::with_swapped_agent(...)` so `ModuleSource::WasmBytes`
   stops returning `MT5064`. Add `Resumable::migrate_from(old)`
   hooks for schema-evolution ranges; multi-version support during
   a rolling cluster restart. Wire the control-socket `op=reload`
   handler in `crates/mty-runtime/src/control_socket.rs` so the
   `mty reload --sock` path is end-to-end. Replace the 1 ms drain
   busy-poll with a condvar wake-up on `mark_idle()`.
2. **Cluster placement + Tier 4.3 live migration.** Add
   `PlacementPolicy` trait so supervisor `RestartRequested` events
   carry a placement hint; supervisor restart events become
   "spawn `(addr, hint)`". Wire `[cluster.tls].require_client_cert`
   + `[cluster.tls].client_ca` through
   `mty_driver::manifest::ClusterManifest`. Add supervisor metrics
   (restart_total / circuit_breaker_tripped_total /
   node_disconnect_total) to the OpenTelemetry layer.
3. **DWARF v5 per-instruction line program enablement.** Plumb
   cranelift's `MachSrcLoc` map through `define_function` so
   `function_debug_info` produces a per-instruction `line_table`
   instead of the conservative 2-entry table from v0.2.
   `.debug_loclists` per-local from cranelift slot offsets
   (same gap as v4 today).
4. **Recording overhead benchmark + payload-size cap.** Benchmark
   strict-equality replay recording-on overhead under load. Add a
   per-payload size cap that elides the structural walk when args
   exceed (say) 64 KiB — at that point the Opaque bytes form is
   cheaper because the receiver almost certainly wants to stream-
   decode rather than fully reify the value tree.
5. **Conformance kit harness expansion + diagnostic-code gap
   closure.** Wire the per-backend link-and-run assertions for
   `native_abi/` cases in `crates/mty-codegen-cranelift/tests/`
   and the component-shape assertions for `wasm_component/` cases
   in `crates/mty-codegen-wasm/tests/`. Close some of the 17
   uncovered diagnostic codes via emit-site work in the originating
   crates (highest-leverage targets: the 6 `MT4xxx` typeck codes
   that still depend on the cap-name resolver wiring).

After v0.21 the only remaining v1.0-RC work is RFC disposition
collection (driven by user-side window closures). Once the latest
window closes on 2026-07-25, the integrator collects dispositions,
files them in `RFC_DISPOSITION_<RFC>.md`, builds the
`mty-conformance-kit-v1.0.0.tar.gz`, and tags **v1.0.0** on
**2026-07-26** (earliest).
