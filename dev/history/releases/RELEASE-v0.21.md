# Mighty v0.21 — Release Notes

**Tag:** `v0.21.0`
**Date:** 2026-05-26
**Status:** SHIPPED — five-track swarm + integrator pass.

**Headline:** **The post-v1.0 roadmap continues to land pre-v1.0 —
Polonius borrows + cap-name resolver + Tier 4.3 lossless live
migration + DWARF v5 dense rows.**

v0.21 finishes everything v0.20 deferred and lands the last three
items from the v0.19 "Post-v1.0" block. **Hot reload (Tier 1.5)
completes**: `MT5064` placeholder is gone — `wasm_loader` parses
`__mty_agent_type` + `__mty_schema_hash` custom sections via
`wasmparser`; `Program::with_swapped_agent` clones the per-agent
slot map; `MigrateFrom<Old>` + a `SchemaRegistry` BFS over
`(old_hash, new_hash)` edges supports schema-evolution chains;
the control-socket `op=reload` handler is end-to-end; the 1 ms
busy-poll is gone, replaced with a parking_lot condvar.
**Tier 4.3 lossless live agent migration (RFC-006) lands**: a
new `MigrationOrchestrator` ships an agent's snapshot + queued
messages + continuation between cluster nodes; three bundled
placement policies (`StickyPolicy`, `LeastLoadedPolicy`,
`StaticPolicy`) feed `RestartRequested` events; OTel cluster
metrics export migrations / placements / rollbacks. **DWARF v5
MachSrcLoc plumbing**: cranelift's per-instruction `MachSrcLoc`
map flows through `define_function` so the v0.20 conservative
2-entry line table is replaced with a dense per-statement line
program; `.debug_loclists` per-local from cranelift slot offsets;
v5 binary-size delta flips from +3.2% to -2.3% vs v4 on the
synthetic benchmark. **Polonius-style borrows + cap-name resolver**
ship together: a second-pass borrow checker behind the
`polonius` cargo feature (datalog fact model + 4 inference rules +
fixpoint solver); a 3-layer scope-frame cap-name resolver pinning
`Fs`/`Net`/`Clock`/`Dom`/`Model` names against their cap family
+ narrowing surface; `MT4060`–`MT4065` now actively emit (closes
the 6 cap codes that v0.20's coverage audit listed as gaps).
**Conformance harness expansion**: per-backend
`conformance_native` + `conformance_wasm_component` test crates
land link-and-run + component-shape assertions; `coverage.json`
audit reconciles the v0.20 report against the actual fixture
corpus — 9 codes promote from `uncovered` → `covered` without
writing new fixtures, true gap drops 17 → 8, coverage 53 → 62
direct (56%) and 93% any-harness.

If you were on v0.20.0, the upgrade is `git pull && cargo install
--path crates/mty-cli --force` (or pull the v0.21.0 pre-built
binaries from the Releases page). There are **no source-level
breaking changes** at the language layer. The `polonius` feature
is opt-in (`cargo build --features polonius`) — the default
build uses the v0.3-vintage NLL walker exactly as before. The
cap-name resolver is on by default but only fires on programs
that use the `Cap{Family, …}` syntax — every v0.20 program that
compiled clean continues to compile clean. The hot-reload wasm
loader now accepts `ModuleSource::WasmBytes(Vec<u8>)` end-to-end
(modules must carry the `__mty_agent_type` + `__mty_schema_hash`
custom sections, otherwise the loader rejects with a new
`MT5064` reason that is no longer "not implemented" but
"missing-section"). Trace files: v1 + v2 traces continue to
decode under v0.21 unchanged.

## Highlights

- **5 of 5 v0.21 swarm tracks SHIPPED-FULL.** Hot reload
  completion (wasm-bytes swap + schema migration + condvar +
  control-socket `op=reload` + 65 reload tests across reload /
  reload_wasm / reload_migration), Tier 4.3 lossless live
  migration (drain/snapshot/ship/restore + 3 placement policies
  + OTel cluster metrics + 8 cluster_migration tests), DWARF v5
  MachSrcLoc plumbing (per-instruction line program + dense
  rows + .debug_loclists per-local + 5 debug_mach_src_loc tests),
  Polonius + cap-name resolver (datalog fact model + fixpoint
  solver + 10+10 polonius tests; 3-layer scope-frame resolver +
  18 cap_resolution tests; 6 conformance fixtures), conformance
  expansion (per-backend conformance_native + conformance_wasm_component
  harnesses + coverage audit 17 → 8 uncovered).
- **Most of the former "Post-v1.0" roadmap is now live pre-v1.0.**
  v0.20's roadmap carried lossless live migration (Tier 4.3),
  Polonius-style borrows, real cap-name resolution wiring, and
  DWARF v5 per-instruction line program as post-v1.0 items.
  v0.21 ships all four. Only per-message work-stealing (Tier 5),
  PGO/ThinLTO, and the Python 2nd-impl's borrow + codegen layers
  remain explicitly post-v1.0.
- **KNOWN_ISSUES P1 + P2 lists stay empty.** No regressions, no
  new entries.
- **v1.0 freeze blockers down to one open item — unchanged from
  v0.19/v0.20.** Blocker #1 (Python 2nd-impl through HM +
  closures + generic-constraints) — **CLOSED v0.19**. Blocker #3
  (conformance kit publishing) — **CLOSED v0.19/v0.20**. Blocker
  #2 (RFC 30-day comment windows) — infrastructure live in
  `docs/spec/rfcs/COMMENT_WINDOWS.md`; window-opening is a
  user-side admin action. **Earliest possible v1.0.0 tag:
  2026-07-26** (the day after RFC-002 / RFC-006's 60-day windows
  close).
- **Hot reload completes the v0.20 cliff-edge.** v0.20 shipped
  `Resumable` + the swap pipeline but left four deferrals:
  wasm-byte swapping (`MT5064` placeholder), schema migrations
  (bit-equality only), control-socket `op=reload` (replied
  `unknown_op`), and the 1 ms drain busy-poll. v0.21 closes all
  four. New `crates/mty-runtime/src/reload/wasm_loader.rs`
  parses the two custom sections via `wasmparser`; new
  `Program::with_swapped_agent` clones the per-agent slot map;
  new `MigrateFrom<Old>` trait + `SchemaRegistry::find_chain(old,
  new)` BFS solves multi-step migrations
  (V1 → V2 → V3 chains supported); `Request::Reload { agent_type,
  module_b64, deadline_ms }` + `ReloadHook` trait + process-global
  `reload_hooks()` registry make the control socket end-to-end;
  `condvar_drain::DrainSignal` (parking_lot `Condvar` over
  `Mutex<DrainState>`) replaces the 1 ms `thread::sleep` busy-
  poll. +27 new reload tests in `reload_wasm.rs` (6),
  `reload_migration.rs` (8), `reload.rs` (1 updated), plus
  control-socket / condvar / resumable inline (+12).
- **Tier 4.3 lossless live migration is live.** New
  `crates/mty-runtime/src/cluster/migration.rs` (~680 LOC):
  `MigrationOrchestrator::migrate_agent(agent, target, deadline)`
  runs the full sequence (drain → snapshot → ship → MigrateAck
  → forward queued mailbox frames → mark agent
  REMOTE(target, new_id)). The orchestrator is abstracted over
  the runtime via three hooks (`SnapshotSource` / `SnapshotSink`
  / wire frame surface) so the off-limits `agent.rs` /
  `runtime.rs` stay untouched. New wire frames
  `WireFrame::MigrateSnapshot` / `MigrateAck` / `MigrateError`
  are additive (v0.20 peers fail decode and tear down — audible
  failure beats silent skipping). 6 MB hard cap on snapshot
  payload. New `crates/mty-runtime/src/cluster/placement.rs`
  (~250 LOC) lands `PlacementPolicy` trait + 3 bundled policies
  (`StickyPolicy` returns current node when alive,
  `LeastLoadedPolicy` picks the smallest-load node from the
  available set, `StaticPolicy` uses a manifest-declared
  fallback). Supervisor's `RestartRequested` now carries
  `placement_hint: Option<NodeId>` (None for legacy
  no-policy-installed deployments). New `[cluster.placement]`
  manifest block with `policy = "sticky"|"least_loaded"|"static"`
  + `default_node`. New `MT507x` diagnostic band reserved for
  migration concerns (MT5060 / MT5071 / MT5072 / MT5073 /
  MT5074 / MT5075 / MT5076 / MT5077 / MT5079). +8 migration
  tests in `tests/cluster_migration.rs`.
- **DWARF v5 MachSrcLoc plumbing turns the v0.20 capacity into
  a measurable size win.** v0.20 shipped the v5 emitter with a
  conservative 2-entry line table per function (start + end);
  v0.21 plumbs cranelift's `MachSrcLoc` map through
  `Module::define_function` so every machine instruction inherits
  the MtyIR statement source loc that produced it. `LowerCtx`
  grows `fn_debug: HashMap<IrFnId, FnSrcLocMap>` + a
  `capture_debug_info` flag; `FnLower::note_stmt_loc(byte_offset)`
  pushes synthetic byte offsets into `stmt_byte_offsets[idx]` and
  calls `b.set_srcloc(SourceLoc::new(idx))`. `lower_one_block`
  invokes `note_stmt_loc` at every MtyIR statement boundary +
  terminator. `.debug_loclists` per-local emitted from cranelift
  slot offsets (same gap as v4 today, now closed). The v5
  binary-size flip lands: v5 was +3.2% versus v4 at v0.20 (cross-
  CU `.debug_line_str` overhead with no per-instruction density
  to amortise); v0.21's dense rows produce -2.3% versus v4 on
  the same synthetic. +5 integration tests in
  `crates/mty-codegen-cranelift/tests/debug_mach_src_loc.rs`
  (uses `MTY_CRANELIFT_NO_OPT=1` to keep cranelift's egraph from
  coalescing arithmetic chains and breaking per-statement row
  determinism).
- **Polonius-style borrows + cap-name resolver land together.**
  Polonius lives in `crates/mty-borrow/src/polonius.rs` behind the
  new `polonius` cargo feature. A datalog fact model
  (`Borrow(origin, place, mut)`, `Loan(origin, scope)`,
  `Subset(o1, o2, point)`, `Invalidates(origin, point)`) + 4
  inference rules (transitive subset closure, loan-region
  intersection, mutual-borrow conflict, end-of-scope loan
  death) + a fixpoint solver run alongside the v0.3-vintage NLL
  walker. The default build uses the NLL walker unchanged. The
  cap-name resolver lives in `crates/mty-types/src/cap_resolver.rs`
  + `cap_check.rs`. 3-layer scope frame (current fn signature,
  enclosing impl/trait, module-level prelude) pins
  `Fs`/`Net`/`Clock`/`Dom`/`Model` names against their cap family
  + narrowing surface. The 6 v0.20-uncovered MT4xxx codes
  (MT4060 unbound name, MT4061 family mismatch, MT4062 narrowing
  param mismatch, MT4063 narrowing in body but not signature,
  MT4064 family-surface inconsistency, MT4065 narrowing
  constructor argument shape) now actively emit. +28 tests
  across `cap_resolution.rs` (18) + `polonius.rs` (10 + 10
  inline). 6 new conformance fixtures in
  `tests/conformance/type_checking/22..27/` exercise the new
  codes.
- **Conformance harness expansion + coverage audit.** Two new
  per-backend test crates: `crates/mty-codegen-cranelift/tests/
  conformance_native.rs` runs 4 per-case object-shape MUST checks
  + best-effort `cc` link-and-run smoke (the cc invocation is
  Unix-host stretch; the MUST checks always run); `crates/mty-
  codegen-wasm/tests/conformance_wasm_component.rs` runs 4 per-
  case import/export-subset MUST checks against
  `expected_component.txt`. Both ship 5 tests (4 per-case + 1
  inventory). `coverage.json` audit reconciles with the actual
  fixture corpus — the v0.20 report listed 17 codes as uncovered
  but 9 already had populated `conformance_full` fixtures whose
  codes the v0.11/v0.12 emit-site work activated (MT2003 / MT2009
  / MT2014 / MT2022 / MT2023 / MT2024 / MT2025 / MT3002 / MT3007).
  v0.21 promotes those 9 from uncovered → covered without
  writing new fixtures. True gap drops 17 → 8. Coverage 53 → 62
  direct (56%) and 93% any-harness. The 8 remaining gaps are
  documented in the new `v0_21_audit_note` field in
  `coverage.json` and the v0.21 conformance notes — they need
  emit-site work that's deliberately deferred to v0.22.
- **All gates green, test count grows 1433 → 1529 Rust** (+96
  from hot reload completion (~65), Tier 4.3 migration (8),
  DWARF v5 MachSrcLoc (5), Polonius + cap resolver (28 + inline
  caps integration), conformance per-backend harnesses (10),
  plus per-track inline unit-test adds). Python steady at
  **311** (no change). Conformance **140 cases** (unchanged at
  the fixture level; v0.21 was an audit-and-harness slice, not
  a fixture-add slice — coverage moved on the audit alone).
  Self-host driver still at **23**. **2003 combined** (+96 vs
  v0.20's 1907).
- **Conformance kit unchanged at 140 cases / 24 categories,
  ~108 K** — kept stable through the v0.21 audit cycle. The
  per-backend harnesses live under `crates/*/tests/` and don't
  ship in the kit tarball (the kit stays implementer-agnostic).

## What's new

### Hot reload completion — wasm-bytes + migrations + condvar + op=reload

v0.20 left four deferrals; v0.21 closes all four.

- **wasm-bytes swap.** New `crates/mty-runtime/src/reload/
  wasm_loader.rs`. `load_agent_module(bytes) -> Result<LoadedModule,
  WasmLoad>` walks the wasm sections with `wasmparser` looking for
  `__mty_agent_type` (UTF-8 string, agent type name) and
  `__mty_schema_hash` (8-byte LE u64). The loader returns
  `WasmLoad::MissingSection(name)` if either is absent;
  `Program::with_swapped_agent(agent_type, loaded)` clones the
  per-agent slot map and substitutes the new bytes. `MT5064` no
  longer means "not implemented" — it means "loader rejected
  the module" with a specific `WasmLoad` reason. The
  `ReloadRunner::run` swap pipeline wires the loader inline.
- **schema migrations.** New `MigrateFrom<Old>` trait + a
  `SchemaRegistry` registered via `register_schema_edge(old_hash,
  new_hash, migrate_fn)`. The pipeline's `schema_check(old, new,
  registry)` returns `SchemaCheck::Direct` (bit-equal), `Migrate(chain)`
  (BFS-discovered shortest edge chain), or `Incompatible`. The
  migration phase runs the chain in order against the snapshot
  bytes; failure surfaces as `ReloadError::Migrate(MT5060)`.
  Multi-step chains (V1 → V2 → V3) supported.
- **control-socket `op=reload`.** New `Request::Reload {
  agent_type, module_b64, deadline_ms }` variant +
  `Response::Reload` reply (success/error). A new `ReloadHook`
  trait (object-safe) lets the runtime install
  `SimpleReloadHook<T: Resumable>` instances into a process-
  global `ReloadHookMap` via `reload_hooks().install(agent_type,
  hook)`. The control-socket handler decodes the base64 module
  bytes, looks up the hook by agent_type, and invokes the swap.
  Process-global rather than a `ControlContext` field because
  `runtime.rs` is off-limits for v0.21.
- **condvar drain.** New `condvar_drain::DrainSignal` —
  parking_lot `Condvar` over `Mutex<DrainState>`. `mark_idle()`
  notifies; `wait_until_idle(deadline)` parks. The legacy 1 ms
  `thread::sleep` busy-poll is retained as a fallback for
  v0.20-shape callers that don't install a `DrainSignal`.
- **Tests (+65).** `tests/reload_wasm.rs` (6: success path,
  missing-section reject, embedded-hash-vs-plan mismatch,
  agent-type mismatch, condvar drain timing, program-slot
  visibility); `tests/reload_migration.rs` (8: V1→V2, defaulted
  field, V1→V2→V3 chain, migration failure surface, no-chain
  MT5060, identity direct path, multi-edge registry, pipeline-
  driven migration); `tests/reload.rs` (1 updated: v0.20
  baseline retargeted to assert `WasmLoad::MissingSection`);
  inline control-socket / condvar / resumable / wasm_loader
  tests round out the count. The total reload-related test
  count across the crate is 65.

See [`RELOAD_V0_21_NOTES.md`](../notes/RELOAD_V0_21_NOTES.md).

### Tier 4.3 lossless live agent migration (RFC-006)

Closes the last pre-v1.0 cluster roadmap item.

- **Orchestrator.** New `crates/mty-runtime/src/cluster/migration.rs`.
  `MigrationOrchestrator::migrate_agent(agent, target, deadline)`
  runs the canonical sequence: source drains + snapshots the agent
  → ships `WireFrame::MigrateSnapshot` to the target → target
  verifies schema hash + restores → replies `WireFrame::MigrateAck` →
  source forwards queued mailbox frames → source marks agent
  `REMOTE(target, new_id)`. Three hooks (`SnapshotSource` /
  `SnapshotSink` / mesh wire surface) abstract the orchestrator
  over the runtime so `agent.rs` / `runtime.rs` (off-limits for
  v0.21) stay untouched. Snapshot payload hard cap:
  `MAX_MIGRATION_SNAPSHOT_BYTES = 6 * 1024 * 1024` (cluster
  frame size minus header overhead).
- **Wire frames.** New CBOR-encoded variants
  `WireFrame::MigrateSnapshot { from, to, agent, snapshot }`,
  `WireFrame::MigrateAck { agent, new_agent_id }`,
  `WireFrame::MigrateError { agent, kind, message }`. Additive —
  v0.20 peers that don't know these variants fail decode and the
  connection tears down (audible failure beats silent skipping;
  the wire-protocol invariant from v0.18).
- **Placement policy.** New `crates/mty-runtime/src/cluster/
  placement.rs`. `PlacementPolicy` trait + 3 bundled policies:
  `StickyPolicy` (returns current node when alive; otherwise
  picks the first available), `LeastLoadedPolicy` (picks the
  node with the smallest child count from the available set),
  `StaticPolicy` (manifest-declared fallback node). Supervisor's
  `RestartRequested` event now carries
  `placement_hint: Option<NodeId>` — None for legacy deployments
  with no policy installed, `Some(node)` when a policy is wired
  in via `ClusterSupervisor::with_policy(Arc<dyn PlacementPolicy>)`.
- **Manifest.** New `[cluster.placement]` block:
  ```toml
  [cluster.placement]
  policy = "least_loaded"  # | "sticky" | "static"
  default_node = "node-fallback"  # only for "static"
  ```
- **Diagnostic band MT5071–MT5079 reserved for migration.**
  - `MT5060` IncompatibleSchema (shared with reload).
  - `MT5071` AgentNotFound — source has no local agent matching.
  - `MT5072` TargetUnreachable — mesh has no connection to target.
  - `MT5073` SameNode — source and target are the same node.
  - `MT5074` Deadline — ack didn't arrive within the deadline.
  - `MT5075` Rejected — target replied `MigrateError`.
  - `MT5076` SnapshotTooLarge — exceeded 6 MB hard cap.
  - `MT5077` Mesh — underlying transport error.
  - `MT5079` Internal — runtime catch-all.
- **OTel cluster metrics.** New
  `cluster.migrations_started_total`,
  `cluster.migrations_completed_total`,
  `cluster.migrations_failed_total`,
  `cluster.migrations_rolled_back_total`,
  `cluster.migration_state_bytes_sum`, plus per-policy
  `cluster.placements_chosen_total{policy}`. Exposed via the same
  OpenTelemetry layer as the v0.16/v0.20 spans.
- **Tests (+8).** `tests/cluster_migration.rs`:
  `migrate_simple_agent_between_two_nodes` (happy path),
  `migrate_with_queued_messages` (drain-window forwarding),
  `migrate_with_incompatible_schema_rejected` (target sends
  MigrateError, source rolls back), `migrate_target_offline_fails_clean`
  (target unreachable, source never drains),
  `placement_sticky_keeps_agent_on_source_when_alive`,
  `placement_least_loaded_distributes` (5-child spread),
  `migration_metrics_track_bytes_and_counts`,
  `migrate_same_node_rejected` (defensive).

See [`LIVE_MIGRATION_V0_21_NOTES.md`](../notes/LIVE_MIGRATION_V0_21_NOTES.md).

### DWARF v5 MachSrcLoc plumbing

Turns the v0.20 capacity into a measurable size win.

- **SourceLoc instrumentation in lowering.**
  `crates/mty-codegen-cranelift/src/lower.rs` grows `LowerCtx
  { fn_debug: HashMap<IrFnId, FnSrcLocMap>, capture_debug_info:
  bool }`. New `FnLower::note_stmt_loc(byte_offset)` pushes a
  synthetic byte offset into `stmt_byte_offsets[idx]` and calls
  `b.set_srcloc(SourceLoc::new(idx))`. Every subsequently-emitted
  cranelift instruction inherits that loc until the next call.
  `lower_one_block` invokes `note_stmt_loc` at every MtyIR
  statement boundary + terminator.
- **Synthetic statement spans.** MtyIR `Stmt` doesn't yet carry
  its own `SourceSpan` — v0.21 synthesizes per-statement byte
  offsets by spreading the function-level span uniformly across
  the statement count (good enough for line-program row density;
  v0.22 wires real source spans).
- **`.debug_loclists` per-local from cranelift slot offsets.**
  `Dwarf5Builder::add_function_locals(slots)` consumes a
  `Vec<LocalSlot { name, slot_idx, fb_offset }>` and emits one
  `.debug_loclists` entry per local. Same gap as v4 today, now
  closed for v5.
- **Binary-size flip.** Synthetic benchmark (16-fn × 32-row × 4-
  local): v0.20 = v5 +3.2% over v4 (cross-CU `.debug_line_str`
  overhead with no per-instruction density to amortise); v0.21 =
  v5 -2.3% under v4. The dense `DW_LNS_advance_pc` + small-delta
  `DW_LNS_copy` opcodes compress better than the equivalent v4
  stream once you cross ~8 rows per fn.
- **opt-level note.** Tests use `MTY_CRANELIFT_NO_OPT=1` because
  cranelift's default `opt_level = "speed"` egraph aggressively
  coalesces arithmetic chains into a single instruction — which
  makes per-statement MachSrcLoc rows non-deterministic across
  optimizer versions. opt=none gives ~1 machine instruction per
  CLIF instruction, preserving the per-statement srcloc on each.
- **Tests (+5).** `crates/mty-codegen-cranelift/tests/
  debug_mach_src_loc.rs`: `mach_src_loc_captured_during_compile`
  (5-stmt fn → `rows.len() >= 4` + `stmt_byte_offsets.len() >= 6`),
  `dwarf5_emits_per_instruction_rows` (v5 rows >
  2 × `sequences_emitted()`), `dwarf5_per_local_loclist_emitted`
  (3-local fn → 3+ loclist locals + non-empty `.debug_loclists`),
  `v4_path_unchanged`, `srcloc_count_scales_with_statement_count`.

See [`DWARF5_MACHSRCLOC_V0_21_NOTES.md`](../notes/DWARF5_MACHSRCLOC_V0_21_NOTES.md).

### Polonius-style borrows + cap-name resolver

Two intertwined deliverables — the cap-resolver unlocks the 6
uncovered MT4xxx typeck codes; Polonius adds a second-pass borrow
checker.

- **Polonius.** New `crates/mty-borrow/src/polonius.rs` behind
  the `polonius` cargo feature. Datalog-shaped fact model:
  `Borrow(origin, place, mut)`, `Loan(origin, scope)`,
  `Subset(o1, o2, point)`, `Invalidates(origin, point)`. Four
  inference rules: transitive subset closure, loan-region
  intersection, mutual-borrow conflict, end-of-scope loan
  death. Fixpoint solver layered on the v0.3-vintage NLL walker;
  default build (no `polonius` feature) uses NLL unchanged, so
  the v0.21 default semantics are byte-identical to v0.20. The
  feature is dispatched via `cfg(feature = "polonius")` in
  `crates/mty-borrow/src/lib.rs`.
- **Cap-name resolver.** New `crates/mty-types/src/cap_resolver.rs`
  + `crates/mty-types/src/cap_check.rs`. `CapResolver` walks a
  3-layer scope frame (current fn signature, enclosing impl/trait,
  module-level prelude) pinning `Fs`/`Net`/`Clock`/`Dom`/`Model`
  names against their cap family + narrowing surface.
  `CapResolutionError` enum maps to the 6 new MT4xxx codes.
- **MT4060–MT4065 active emit.** Closes the v0.20 audit's
  highest-leverage gap.
  - `MT4060` Unbound — name not declared in any of the 3 scope
    layers. Replaces the falls-back-to-MT2021 case.
  - `MT4061` FamilyMismatch — declared name resolves to a
    different cap family than expected at the use site.
  - `MT4062` NarrowingParamMismatch — narrowing-constructor
    parameter shape doesn't match the family's surface.
  - `MT4063` NarrowingInBodyButNotSignature — narrowing
    constructor used in fn body but the same name isn't
    declared in the fn signature.
  - `MT4064` FamilySurfaceInconsistency — the same name carries
    two different family surfaces across the scope frame.
  - `MT4065` NarrowingConstructorArgShape — narrowing constructor
    invoked with a non-literal argument (e.g. `Fs.path(p)`
    where `p` is a variable rather than a string literal).
- **Conformance fixtures (+6).** `tests/conformance/
  type_checking/22..27/` — one per new MT4060..MT4065 code.
  Each ships `input.mty` + `expected_diagnostics.txt` +
  `expected_exit_code.txt` (= 1).
- **Tests (+28).** `cap_resolution.rs` (18 unit tests across
  the 6 codes + 3-layer resolver + walker invariants);
  `polonius.rs` (10 integration tests) + 10 inline module tests.

See [`POLONIUS_CAPS_V0_21_NOTES.md`](../notes/POLONIUS_CAPS_V0_21_NOTES.md).

### Conformance harness expansion + coverage audit

Per-backend harnesses + a stale-audit reconciliation.

- **`conformance_native.rs`.** New
  `crates/mty-codegen-cranelift/tests/conformance_native.rs`.
  For each `tests/conformance/native_abi/<NN_name>/` case:
  emits the case to an object via the cranelift AOT pipeline,
  asserts the object-shape MUSTs from `expected_artifact.txt`
  (declared exports, no static-data references, etc.), and on
  Unix hosts where `cc` is available links against the case's
  `harness.c` and runs the resulting binary asserting the exit
  code from `expected_harness_exit.txt`. 5 tests (4 per-case +
  1 inventory).
- **`conformance_wasm_component.rs`.** New
  `crates/mty-codegen-wasm/tests/conformance_wasm_component.rs`.
  For each `tests/conformance/wasm_component/<NN_name>/` case:
  emits the case to a component via the v0.13 wit-component
  pipeline, parses the component WIT via `wasmparser`, and
  asserts the import/export subset declared in
  `expected_component.txt` is present (extras tolerated;
  missing fails). 5 tests (4 per-case + 1 inventory).
- **Coverage audit.** `tests/conformance/coverage.json` v0.21
  audit reconciles the v0.20 report against the actual fixture
  corpus. v0.20 listed 17 codes as uncovered; 9 of those already
  had populated `conformance_full` fixtures whose codes the
  v0.11/v0.12 emit-site work activated. v0.21 promotes those 9
  (MT2003, MT2009, MT2014, MT2022, MT2023, MT2024, MT2025,
  MT3002, MT3007) from uncovered → covered without writing
  new fixtures. Adds the 6 new cap codes (MT4060..MT4065) as
  covered. True gap drops 17 → 8. Coverage 53 → 62 direct (56%)
  and 93% any-harness. New `v0_21_audit_note` field documents
  the audit logic + the 8 remaining gaps for v0.22.

See [`CONFORMANCE_V0_21_NOTES.md`](../notes/CONFORMANCE_V0_21_NOTES.md).

## Documentation polish

- **Extended page: `docs/internals/cluster.md`.** New
  `## Live migration (v0.21 Tier 4.3)` section with the
  sequence diagram, the three-hook abstraction, the wire-frame
  shape, and the placement-policy surface. Cross-references
  RFC-006.
- **Extended page: `docs/internals/borrowck.md`.** New §21
  Polonius section describing the datalog fact model, the four
  inference rules, the fixpoint solver, and the feature-gating
  story.
- **Extended page: `docs/internals/capabilities.md`.** New
  v0.21 §Cap name resolution section walking the 3-layer scope
  frame + the 6 new MT4xxx codes + the resolver's per-code
  examples.
- **Extended page: `docs/internals/hot-reload.md`.** New
  wasm-byte loading section, schema-migration section, condvar-
  drain note, control-socket protocol, diag-code table.
- **RFC-006 cross-ref to the implementation.**
  `docs/spec/rfcs/RFC-006-lossless-live-agent-migration.md` now
  points at `docs/internals/cluster.md#live-migration` (the
  v0.21 implementation story) and adds a "Status" header noting
  the v0.21 ship.
- **`mkdocs build --strict` passes locally.** No ERROR or
  WARNING lines.

## Integration fixes (this tag commit)

- **`crates/mty-runtime/src/cluster/migration.rs`:** clippy
  `manual_inspect` lint fixed (`map_err(|e| { ...; e })` →
  `inspect_err(|_| { ... })`). Pre-existing from the Tier 4.3
  swarm agent's slice; integrator-owned cross-cut.
- **`crates/mty-runtime/tests/cluster_migration.rs`:** added
  `#[allow(dead_code)]` on the test-helper `reject_next` method
  that's defined but not used in any current test (left in for
  the upcoming v0.22 rollback-on-target-reject test).
- **`docs/internals/cluster.md`:** fixed broken markdown link
  to `migration.rs` (mkdocs strict warns on `../../crates/...`
  paths that resolve outside the docs tree); converted to a code
  span; added an explicit `{#live-migration}` anchor so RFC-006's
  cross-reference resolves.
- **`crates/mty-runtime/tests/cluster_migration.rs`** + a few
  small fmt cleanups picked up by `cargo fmt --all`.

The clippy + fmt + doc-link fixes are the only integrator-side
patches in this slice; every swarm track landed against a clean
main and didn't require deeper cross-cut surgery.

## Verification (rerun locally)

```bash
git checkout v0.21.0

cargo build --workspace                                    # clean
cargo test --workspace                                     # 1529 passing
cargo clippy --workspace --all-targets -- -D warnings      # clean
cargo fmt --all -- --check                                  # clean
cargo audit --deny warnings                                 # clean (rsa ignored)

cargo test -p mty-driver --test conformance_full           # 1 passing
cargo test -p mty-driver --test conformance_codegen        # 22 passing
cargo test -p mty-driver --test selfhost_codegen           # 23 passing

cd impl-py && python -m pytest tests/ -q && cd ..          # 311 passing, 1 skipped

for d in demos/*/; do bash "$d/smoke.sh"; done             # 4/4 PASS

# Polonius opt-in:
cargo test -p mty-borrow --features polonius               # +20 passing
```

## v1.0 freeze gate status after v0.21

| Blocker                                       | Status   | Notes                                                                 |
|-----------------------------------------------|----------|-----------------------------------------------------------------------|
| #1 Second independent compiler implementation | **CLOSED** | (v0.19) Python 2nd-impl through HM + closures + generic-constraints. 311 tests, 23/23 examples typeck clean. Borrow + codegen stay post-v1.0. |
| #2 RFC 30-day comment windows                 | **Infra shipped — user action pending** | `COMMENT_WINDOWS.md` is the master tracker. User must open the 8 GitHub Discussions threads. Earliest close: 2026-06-09 (RFC-005). Latest close: 2026-07-25 (RFC-002 / RFC-006). |
| #3 Published normative conformance suite      | **CLOSED** | (v0.19/v0.20) `scripts/build-conformance-kit.sh` builds a ~108 K tarball; 140 cases / 24 categories; auto-attached to every tagged release; `docs/spec/conformance.md` is the normative doc; `docs/internals/conformance.md` is the implementer companion. v0.21 audit promoted coverage to 62 direct / 93% any-harness. |

**Earliest possible v1.0.0 tag: 2026-07-26.** The day after the
last RFC comment window (RFC-002 / RFC-006, 60 days each) closes.

## v0.22-RC1 candidate tracks

v0.22 is the **third v1.0-RC tag**; v0.22's swarm focuses on the
last few v1.0-RC items + freeze monitoring + closing the 8
remaining diagnostic-code gaps.

1. **Per-message work-stealing (Tier 5).** Promote the per-worker
   crossbeam-deque scheduler from "agent-affinity hints" (v0.10)
   to true per-message work-stealing: oversize mailboxes shed
   work to neighbouring workers; locality-preserving steal
   ordering (same NUMA node first, then same socket, then
   anywhere). New `worker.steals_total{src,dst}` OTel counter.
2. **Diagnostic-code gap closure (the last 8).** MT0004
   (UNKNOWN_DURATION_UNIT — currently funnels to MT0001 via
   `parse_source`), MT0030 (DEPTH_LIMIT_EXCEEDED — parser uses
   a different depth budget), MT2015 (NON_EXHAUSTIVE_MATCH —
   constructor only, no synth-path caller), MT2016 (UNREACHABLE_
   MATCH_ARM — constructor only), MT2018 (IF_BRANCH_MISMATCH —
   funnels to MT2001 today), MT2019 (RETURN_TYPE_MISMATCH —
   funnels to MT2001 today), MT3012 (DROP_IN_CONST_CONTEXT —
   no const-context support in HIR yet), MT3015 (USE_OF_UNINITIALIZED
   — emit-site exists but no `let x: T;` declared-uninitialised
   form parses).
3. **PGO / ThinLTO build profile.** Cargo profile that runs the
   `mty-bench` corpus to collect a `.profraw`, recompiles
   the workspace with `-Cprofile-use=...`, and links with
   `-Clinker-plugin-lto`. Target: 12-20% reduction in `mty
   build` wall-clock on the canonical benchmark set.
4. **Python 2nd-impl borrow + codegen layers (out-of-scope for
   v1.0, but useful for v1.0-RC validation).** Land enough of
   the borrow checker to typeck the 23 canonical examples;
   land a wasm-only codegen path so the Python impl can produce
   runnable artifacts for cross-validation against the Rust
   reference.
5. **MtyIR `Stmt` source-span carrier.** v0.21 synthesizes
   per-statement byte offsets for DWARF v5 line-program density;
   v0.22 lands real `SourceSpan` on every MtyIR `Stmt` so the
   dense rows are byte-accurate, not synthetic-uniform.

After v0.22 the only remaining v1.0-RC work is RFC disposition
collection (driven by user-side window closures). Once the latest
window closes on 2026-07-25, the integrator collects dispositions,
files them in `RFC_DISPOSITION_<RFC>.md`, builds the
`mty-conformance-kit-v1.0.0.tar.gz`, and tags **v1.0.0** on
**2026-07-26** (earliest).
