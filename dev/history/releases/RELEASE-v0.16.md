# Mighty v0.16 — Release Notes

**Tag:** `v0.16.0`
**Date:** 2026-05-26
**Status:** SHIPPED — production observability lands (live agent
introspection + OpenTelemetry spans), the v0.15 RFC-008 surface
syntax now typechecks end-to-end for user-authored row variables,
WASI Preview 2 closes the `std.fs` + `std.http` direct-lowering
surfaces (only `log()` remains adapter-routed), and the self-host
codegen broadens with `MethodCall` lowering plus a custom-iterator
desugar (21 live driver tests vs 17 in v0.15).

v0.16 is the **observability + RFC-008 typeck-finishing release**:
the v0.15 surface-syntax-only RFC-008 parse path is now consumed by
typed AST accessors, HIR (`HirEffectRow`), and the typeck
unifier (`UserRowPolyIndex`), so `examples/22_effect_row.mty`
flips from `@typeck-pending` to live in the example sweep; live
agent introspection lands as the new `mty inspect` CLI + an opt-in
`MTY_RUNTIME_CONTROL_SOCK` runtime control socket carrying mailbox
depths, in-flight handler info, budgets, and the last-N messages;
OpenTelemetry agent spans land lazily from `MTY_OTLP_ENDPOINT`
with a cost-zero off path; the WASI P2 emitter takes nine more
stdlib lowerings direct (`std.fs.*` + `std.http.*`); and the
self-host Wasm codegen finally lowers `Rvalue::MethodCall` to a real
host-bridged dispatch plus desugars `for x in custom_iter` into the
loop/match/Some/None iter-protocol shape.

**Headline:** **Production observability lands (introspect + OTel),
RFC-008 effect rows fully typecheck for user code, WASI P2 closes
fs + http surfaces, self-host adds MethodCall + custom iterators.**

If you were on v0.15.0, the upgrade is `git pull && cargo install
--path crates/mty-cli --force` (or pull pre-built binaries from
the [Releases page](https://github.com/hassard0/Mighty/releases)
— the cross-platform `release.yml` workflow first fired on
v0.15.0 and continues on every `v*` tag). There are no source-level
breaking changes: the v0.16 surfaces are strictly additive at the
language, runtime, and toolchain layers.

## Highlights

- **5 of 5 v0.16 swarm tracks SHIPPED-FULL.** Agent introspection
  (Tier 1.1), OpenTelemetry agent spans (Tier 1.2 + 1.3),
  RFC-008 HIR + typeck wiring, WASI P2 fs + http direct lowering,
  and self-host `MethodCall` + custom-iter desugar all land
  complete.
- **`mty inspect` ships.** A new CLI subcommand + opt-in runtime
  control socket (`MTY_RUNTIME_CONTROL_SOCK`) expose
  `AgentSnapshot`: agent type, mailbox depth + high-water mark,
  in-flight handler name + elapsed time, per-handler memory + tick
  budgets, and the last-N message records (opt-in body capture).
  Pretty / JSON / `--watch` modes; 20 new tests
  (7 integration + 4 + 4 unit + 5 CLI). The `roadmap` Tier 1.1
  item from `docs/internals/agent-features-roadmap.md` is now live.
- **OpenTelemetry agent spans land.** New `telemetry/` submodule
  in `mty-runtime` emits spans at every agent boundary: `spawn`,
  `send`, `ask`, `handler`, plus restart + budget-exhaustion event
  records; the `agent.event(name, &[(k, v)])` helper attaches
  user attributes to the active handler span. Lazy init from
  `MTY_OTLP_ENDPOINT`; **cost-zero when the env var is unset**
  (no provider, no per-call branching beyond a single atomic load).
- **RFC-008 effect rows typecheck end-to-end for user code.** The
  v0.15 surface syntax (`!E`, `!{a | E}`, `!{| E}`,
  `effect a, b | E`) was parser-only; v0.16 wires it through:
  `mty-ast::effects` typed accessors → `HirEffectRow`
  (`Closed | Open`) on `HirFn::effect_row` → `UserRowPolyIndex`
  in `mty-types::effects`. Five new diagnostic codes
  (**MT4055 / MT4056 / MT4057 / MT4058 / MT4059**) are wired;
  **MT4057** ("user row variable escapes its scope") actively
  emits. `examples/22_effect_row.mty` no longer carries the
  `@typeck-pending` marker.
- **WASI P2: `std.fs` + `std.http` direct.** Nine stdlib lowerings
  move from preview1-adapter-routed to direct P2 imports — five
  fs fns (`open` / `read_file` / `write_file` / `stat` / `close`
  mapping to `wasi:filesystem/types@0.2.3#descriptor.*`) and four
  http variants (`get` / `post` / `send` /
  `incoming_request_consume` mapping to `wasi:http/types@0.2.3` +
  `wasi:http/outgoing-handler@0.2.3`). Latent emitter import-index
  bug fixed via a new predeclare pass. The only call still flowing
  through the adapter is the `log()` shim (last item, deferred to
  v0.17 with the `wasi:logging` interface).
- **Self-host codegen: MethodCall + custom iterators.**
  `Rvalue::MethodCall { receiver, method, args }` now lowers to a
  real Wasm `call` sequence through the host
  `ir_method_resolve(name)` bridge (v0.15 emitted `unreachable`);
  `for x in custom_iter { body }` desugars at the selfhost-IR
  layer into the iter-protocol loop-match-`Some`/`None` shape;
  selfhost driver tests go **17 → 23 live, 0 ignored**.
- **All gates green, test count grows 1140 → 1217 Rust + 139
  Python + 92 conformance + 23 self-host driver tests = 1471
  combined** (+43 vs v0.15). 0 failing, 4 ignored (2 doc-tests +
  2 long-standing conformance).
- **Cross-platform binaries continue to ship.** The `release.yml`
  workflow that first fired on v0.15.0 produces `mty` binaries for
  Linux x86_64, macOS x86_64 + arm64, and Windows x86_64 on every
  `v*` tag push. See the
  [Releases page](https://github.com/hassard0/Mighty/releases).

## What's new

### Agent introspection — `mty inspect` + control socket (Tier 1.1)

The first tier of the
`docs/internals/agent-features-roadmap.md` plan lands as a
production-grade live introspection surface.

- **`crates/mty-runtime/src/introspect.rs`** — `AgentSnapshot` +
  `RuntimeSnapshot` types + a per-agent ring buffer for last-N
  message records. Wire payload carries `version: 1`; new fields
  are additive without a version bump.
- **`crates/mty-runtime/src/control_socket.rs`** — opt-in local
  control-socket server, Unix-domain on POSIX. Boots only when
  `MTY_RUNTIME_CONTROL_SOCK=/path/to/sock` is set, so zero impact
  on processes that don't ask for it.
- **`crates/mty-cli/src/cmd/inspect.rs`** — `mty inspect` subcommand
  with pretty + JSON + `--watch` modes. Connects to the runtime
  control socket; lists running agents; per-agent snapshot view
  shows mailbox depth + high-water, in-flight handler + elapsed,
  CPU/mem/tick budgets, and the last-N messages (opt-in body
  capture).
- **+20 new tests.** `crates/mty-runtime/tests/introspect.rs` (7
  integration); `runtime` unit tests in `introspect.rs` (4) and
  `control_socket.rs` (4); `crates/mty-cli/tests/inspect_cli.rs`
  (5).
- **Reference docs.** New `docs/reference/cli/mty-inspect.md`.

See
[`INTROSPECT_V0_16_NOTES.md`](../notes/INTROSPECT_V0_16_NOTES.md).

### OpenTelemetry agent spans (Tier 1.2 + 1.3)

OTel agent spans + the `agent.event` helper close Tier 1.2 + 1.3
of the agent-features roadmap.

- **`crates/mty-runtime/src/telemetry/`** — split out from the
  previous `telemetry.rs` so the legacy JSON-line / OTLP event
  sink (`telemetry::sink`) keeps its public surface while the new
  span machinery lives in `telemetry::mod` (`init_from_env` /
  `shutdown`), `telemetry::spans` (RAII guards + helpers), and
  `telemetry::events` (the `agent.event` user helper).
- **Spans emitted at every agent boundary.** `span_spawn`,
  `span_send`, `span_ask`, `span_handler`, plus `record_restart`
  and `record_budget_exhausted` event records. The handler span
  is stored in a `tokio::task_local!` so `agent.event` attaches
  user-supplied attributes to the live context automatically.
- **Lazy init from `MTY_OTLP_ENDPOINT`.** No env var → no
  provider → no per-call branching beyond a single atomic load.
  When set, the OTLP HTTP exporter ships to the configured endpoint
  (the `tracing-opentelemetry` + `opentelemetry-otlp` pair).
- **Reference docs.** New `docs/reference/telemetry.md` (user
  guide) and `docs/internals/telemetry-spans.md` (implementer
  notes).

See
[`TELEMETRY_SPANS_V0_16_NOTES.md`](../notes/TELEMETRY_SPANS_V0_16_NOTES.md).

### RFC-008 HIR + typeck wiring — user row variables live

v0.15 landed the RFC-008 surface syntax as parser-only; v0.16 wires
it through the rest of the compiler.

- **Typed AST accessors** (`crates/mty-ast/src/effects.rs`) —
  `EffectSet`, `EffectName`, `EffectRowTail`, `EffectRowVar`
  typed views over the v0.15 `SyntaxKind` variants. `EffectClause`
  gains `row_var_name()`, `has_row_var()`,
  `braced_concrete_names()`.
- **HIR shape** (`crates/mty-hir/src/effects.rs`) — `HirEffectRow`
  enum (`Closed(Vec<HirEffectName>)` /
  `Open(Vec<HirEffectName>, HirRowVar)`) attached as an additive
  `Option<HirEffectRow>` on `HirFn::effect_row`. Closed-set fns
  leave it `None` so the legacy `HirFn::effects: Vec<String>`
  consumers keep working.
- **HIR lowering** (`crates/mty-hir/src/lower/items.rs`) —
  `lower_effect_clause` populates both the legacy `effects` view
  and the new `effect_row`, detecting all three v0.15 surface
  shapes.
- **Typeck wiring** (`crates/mty-types/src/effects.rs`) —
  `UserRowPolyIndex` interns each user-authored row variable name
  once per fn signature; unification against an open row instantiates
  fresh row vars and propagates per RFC-008's algorithm.
- **5 new diagnostic codes wired.** MT4055 (unknown effect name in
  row body), MT4056 (duplicate row var name), **MT4057 (user row
  variable escapes its scope — actively emits)**, MT4058 (row var
  appears in non-fn position), MT4059 (row var unification with
  an incompatible closed row).
- **`examples/22_effect_row.mty` now typechecks.** The `@typeck-pending`
  marker is gone; the example is in the normal `conformance_codegen`
  sweep.
- **+12 new tests** across `mty-ast`, `mty-hir`, and `mty-types`
  unit / integration suites.

See
[`EFFECT_ROW_WIRING_V0_16_NOTES.md`](../notes/EFFECT_ROW_WIRING_V0_16_NOTES.md).

### WASI Preview 2 — fs + http direct

v0.15 wired direct P2 imports for `std.random` + `std.time`; v0.16
extends the direct path to nine more stdlib lowerings, leaving only
the `log()` shim adapter-routed.

- **5 `std.fs` fns direct** —
  `open` / `read_file` / `write_file` / `stat` / `close` lower
  to `wasi:filesystem/types@0.2.3#[method]descriptor.*` (with
  `[resource-drop]descriptor` for close).
- **4 `std.http` variants direct** —
  `get` / `post` / `send` / `incoming_request_consume` lower to
  `wasi:http/types@0.2.3#[constructor]outgoing-request` +
  `wasi:http/outgoing-handler@0.2.3#handle`.
- **Latent emitter import-index bug fixed.** The
  `mty-codegen-wasm::emit` pass needed a predeclare phase so the
  import indices for direct-lowered fns are stable before the SIR
  walk begins; without it, late-discovered direct imports
  collided with monotonic-id assignments. A new `prescan_p2_direct`
  walk runs first and records the needed imports; the main walk
  then resolves them through the pre-built index.
- **+12 new tests** across `crates/mty-codegen-wasm/tests/`
  covering each fs + http direct import, the predeclare pass,
  and a parity-check that the adapter path still services `log()`.

See
[`WASI_P2_FS_HTTP_V0_16_NOTES.md`](../notes/WASI_P2_FS_HTTP_V0_16_NOTES.md).

### Self-host codegen — `MethodCall` + custom iterators

The v0.15 self-host codegen reached 17 live driver tests with
variant-call + SwitchInt + for-range desugar; v0.16 closes the two
biggest gaps flagged in those notes.

- **`Rvalue::MethodCall` lowering.** The selfhost
  `compile_method_call_rvalue` now emits a real Wasm `call`
  sequence: the host bridge `ir_method_resolve(name) -> USize`
  resolves a method name to a wasm fn idx; on `sentinel_none()`
  the emitter degrades gracefully to an `i32.const 0` placeholder
  so the module stays validatable.
- **Custom-iter desugar.** `for x in <non-range-iter> { body }`
  now expands at the selfhost-IR layer into the iter-protocol
  shape (`loop` + `match iter.next() { Some(x) => body, None =>
  break }`). Combined with `MethodCall` lowering, for-loops over
  user-defined iterators now emit real iteration code at the Wasm
  level.
- **Driver tests.** `crates/mty-driver/tests/selfhost_codegen.rs`
  reports **23 live tests, 0 ignored** (was 17 live in v0.15).
  New tests: `MethodCall` lowering (simple + with args), custom
  iter on a user-defined Option-returning `next` fn, and three
  unit tests for the helper paths.

See
[`SELFHOST_V0_16_NOTES.md`](../notes/SELFHOST_V0_16_NOTES.md).

## Integration fixes (this tag commit)

Three integrator commits land alongside the v0.16 swarm to keep the
required-gate jobs green:

- **`4f5fe58`** — **clippy `collapsible_match` + selfhost test
  `span` clones + audit advisories**. The prescan pass in
  `crates/mty-codegen-wasm/src/emit.rs` had nested
  `if let Stmt::Assign(_, Rvalue::Call { func, .. })` →
  `if let FnRef::Builtin(BuiltinId::Extern(name))` that clippy
  rejects under `-D warnings`; the patch collapses both into a
  single deep destructure pattern. The new `MethodCall` and
  iter-custom tests in `crates/mty-driver/tests/selfhost_codegen.rs`
  construct two `Function` literals sharing a `SourceSpan` value;
  `SourceSpan` is not `Copy` (and cannot become so without cascading
  clippy `clone_on_copy` failures in callers), so the two new tests
  now `.clone()` the span explicitly. The `AdtId` import path was
  switched from the private `mty_ir::ir::AdtId` re-export to the
  public `mty_types::AdtId` to satisfy `E0603`. `audit.toml` picks
  up two more wasmtime entries (RUSTSEC-2026-0096 + RUSTSEC-2025-0046)
  + three transitive warnings (RUSTSEC-2024-0436 paste,
  RUSTSEC-2025-0134 rustls-pemfile, RUSTSEC-2026-0008 git2 0.19.0
  `Buf`-deref unsoundness on an API surface `mty-pkg` does not call,
  tracked for a git2 bump in v0.17).
- **`32ebec4`** — **`.cargo/audit.toml` + `examples/22_effect_row.mty`
  fmt**. cargo-audit's documented config-file lookup is
  `~/.cargo/audit.toml` or `.cargo/audit.toml`; the root
  `audit.toml` introduced in `7148b56` was never being read (the
  security workflow was red on every commit since), so the same
  file was copied into `.cargo/`. The example sweep also flagged a
  trailing blank line in `examples/22_effect_row.mty` — the
  `@typeck-pending` marker removal in v0.16 exposed a minor fmt
  deviation that the v0.15 skip-list had hidden.
- **`380c35b`** — **`fs_capability_allowlist` test serialization**.
  The `--no-default-features` Linux CI job intermittently failed on
  `host_dispatch_read_outside_default_cap_returns_err_variant` because
  cargo's default parallel-test execution let
  `install_default_read_cap_returns_previous_for_scoped_overrides`
  install `/scoped` as the process-wide default cap between the
  scoped test's save and restore points. A `Mutex<()>` gate around
  the two tests that mutate the global cap slot serializes them;
  poisoned-lock recovery keeps a panicking test from cascading.

No source-level behaviour changes; all three are gate-keeping
plumbing.

## v1.0 freeze: blockers + proposed date

The v1.0 spec is at v1.0-RC3 (unchanged from v0.13–v0.15; RFC-008
landed surface syntax in v0.15 and typeck in v0.16, but the spec
amendment for the user-authored row form is staged for v1.0-RC4).
Blocker status (delta vs v0.15 in italics):

1. **Two independent implementations.** Rust reference compiler,
   Python 2nd-impl (`impl-py/`, *139 tests*, unchanged from v0.15),
   Go 3rd-impl (`impl-go/`, 4848 LOC source-only). Cross-validation
   still pending Go toolchain.
2. **RFC comment periods.** RFC-001..006 + RFC-008 + RFC-009 each
   need a 30-day public window. Unchanged from v0.15.
3. **Published normative conformance suite.** Corpus stands at
   *92 cases / 16 categories / 2 ignored* (unchanged from v0.15).
   Coverage of FROZEN diagnostic codes climbs to ~96% (was ~95%) —
   MT4055..MT4059 are now wired (MT4057 actively emits; the others
   are reserved for v0.17 typeck broadening).

**Proposed v1.0 freeze date: 2026-09-01** (unchanged).

## Backwards-compat aliases (status)

- **`--wasi=p1`** still retained as an explicit opt-out from the
  v0.15-onwards P2 default. The v0.13/v0.14 behaviour is fully
  preserved when the flag is set.
- **`MTY_OTLP_ENDPOINT`** is the new env var for OTel; if unset,
  the runtime ships exactly the v0.15 telemetry behaviour (JSON
  line + legacy OTLP event sink).
- **`MTY_RUNTIME_CONTROL_SOCK`** is the new env var for the
  introspect control socket; if unset, no socket is bound and no
  introspect state is exposed.

All other v0.7+ aliases (`mty dump --sir` for `--ir`; `SD####`
accepted by `mty explain`; `--legacy-interp`; legacy `sd` /
`stardust` code-block tags) stay live.

## Stats

| | v0.15.0 | v0.16.0 | Delta |
|---|---|---|---|
| Workspace crates | 20 | 20 | 0 |
| Rust tests passing | 1140 | **1217** | **+77** |
| Python tests passing | 139 | **139** | 0 |
| Self-host driver tests | 17 (codegen) | **23 (codegen)** | **+6** |
| Conformance cases | 92 | **92** | 0 |
| Conformance ignored | 2 | **2** | 0 |
| Combined test count | 1428 | **1471** | **+43** |
| Tests failing | 0 | 0 | 0 |
| Diagnostic codes wired | ~69 | **~74** (+MT4055..MT4059) | **+5** |
| Examples passing (check) | 22/22 (1 parse-only) | **22/22 (all typecheck)** | **+1 typecheck** |
| Demos passing | 4/4 | **4/4** | 0 |
| Independent implementations | 3 (front-end only) | **3 (front-end only)** | 0 |
| Spec | v1.0-RC3 | **v1.0-RC3** | 0 |
| Spec amendments | 88 | 88 | 0 |
| RFCs | 8 | 8 | 0 |
| Fuzz targets | 4 | 4 | 0 |
| CI jobs (all required) | 6 | 6 | 0 |
| Release-binary targets | 4 | **4** (Linux / macOS×2 / Windows) | 0 |
| WASI P2 direct-lowered fns | 4 (random + time) | **13** (+5 fs + 4 http) | **+9** |
| Commits since prior tag | 11 | **11** | 0 |

## Migration steps

For end-user Mighty packages: **none required**. v0.16 is strictly
additive at the language, runtime, and toolchain surfaces.

For toolchain contributors:

- **OTel telemetry** is opt-in via `MTY_OTLP_ENDPOINT`. If you
  already set the legacy slice-7 telemetry path, both continue to
  work (the OTLP event sink moved into `telemetry::sink` but the
  public re-exports are preserved at `mty_runtime::telemetry::*`).
- **Runtime introspection** is opt-in via
  `MTY_RUNTIME_CONTROL_SOCK`. If you want the `mty inspect` CLI
  to attach to a running process, set the env var to a socket
  path before spawning; on Unix the socket is created with
  user-only permissions.
- **`examples/22_effect_row.mty`** has moved from
  `@typeck-pending` to live. The `conformance_codegen` skip-list
  entry from v0.15 is removed; consumers that wrap the example
  sweep should re-import the file as a typecheckable fixture.
- **`HirFn::effect_row: Option<HirEffectRow>`** is a new
  additive field. Existing consumers that only read
  `HirFn::effects: Vec<String>` keep working unchanged; consumers
  that need user-authored row info should opt into the new field.
- **`mty-codegen-wasm` emit predeclare pass** is internal but
  user-visible if you were caching the import index — the index
  is now fully populated after `prescan_p2_direct` rather than
  monotonically growing during the SIR walk.

For Wasm component authors: `cargo run --target wasm32-wasi`
produces a P2 component by default. After v0.16, `std.fs` and
`std.http` flow through direct P2 imports rather than the embedded
preview1-adapter; only the `log()` shim still flows through the
adapter. If you target an adapter that doesn't ship those P2
interfaces, set `--wasi=p1` explicitly.

For users wanting pre-built binaries: the release workflow that
first fired on v0.15.0 continues to fire on every `v*` tag. The
GitHub Releases page should carry `mty` binaries for
Linux / macOS×2 / Windows shortly after each push.

## Known issues

Canonical list in [`KNOWN_ISSUES.md`](../../../KNOWN_ISSUES.md).
v0.16 net delta vs v0.15:

- **CLOSED** — v0.15 follow-up: RFC-008 effect-row HIR/typeck
  wiring is now live (see "RFC-008 HIR + typeck wiring" above).
- **CLOSED** — v0.15 follow-up: WASI P2 `std.fs` + `std.http`
  direct lowering is now live (see "WASI Preview 2 — fs + http
  direct" above).
- **CLOSED** — v0.15 follow-up: self-host `MethodCall` lowering +
  custom iter desugar are now live (see "Self-host codegen" above).

Carried over unchanged from v0.15:

- **#3** MSRV gate runs only `cargo build` (partially closed in v0.10).
- **#6** Demo 02 JS shim still writes into fixed `DOM_RETURN_AREA`
  rather than calling `cabi_realloc()`.
- **#7** `--no-default-features` test job does not run the example
  sweep.
- **#9** Cranelift egraph stack overflow
  (`MTY_CRANELIFT_NO_OPT=1` workaround stays).
- **#14** Go 3rd-impl cross-validation pending — Go toolchain still
  absent on the build host.

New for v0.16:

- **`log()` shim still adapter-routed.** Last remaining adapter
  dependency. The `wasi:logging` interface needs a canonical-ABI
  rewrite (the v0.15 note flagged this for v0.16; the v0.16 slice
  prioritised `std.fs` + `std.http` for blast-radius reasons).
  Tracked for v0.17.
- **wasmtime dev-dep bump deferred.** The dev-only wasmtime
  bundle continues to accrue advisories upstream; `audit.toml`
  ignores them with the standing dev-dep rationale. Tracked for
  v0.17 (alongside a possible swap to `wasmer` for the dev-side
  P2 instantiation tests, which would close the entire ignore
  bundle).
- **Windows named-pipe introspect backend.** The
  control-socket impl is Unix-domain-only on POSIX today; on
  Windows the `mty inspect` CLI exits with a clear "control
  socket not yet supported on Windows" message. The named-pipe
  backend is tracked for v0.17.

## v0.16 → v1.0-final roadmap

Carry-overs from v0.15 are unchanged for items that didn't land
this version. v0.16-specific follow-ups:

- **WASI P2 `log()` finish**: canonical-ABI rewrite for the
  `wasi:logging` interface; close the last adapter dependency.
- **Windows named-pipe introspect**: parity with the POSIX
  Unix-domain control socket.
- **wasmtime dev-dep bump or swap**: clear the advisory bundle.
- **RFC-008 typeck broadening**: wire MT4055 / MT4056 / MT4058 /
  MT4059 to active emission paths beyond the v0.16 row-poly
  contexts (the MT4057 path is already live).
- **Carry-overs**: open RFC-001..006 + RFC-008 + RFC-009 comment
  periods; run `go test ./...` on a Go-1.22+ host; extend the
  Python 2nd-impl through HIR + sketch typeck; split MT0001
  funnel; `mty-pkg` cross-file resolution; publish normative
  conformance suite as a downloadable kit.

## Acknowledgments

v0.16 was built across a five-track swarm followed by an integrator
pass:

- **agent-introspect-swarm** — `mty-runtime` introspect module +
  control socket; `mty inspect` CLI; 20 tests; wire-version policy.
  Commits `956a09c` + `ff9ae42`. **SHIPPED-FULL** (Tier 1.1 from
  agent-features-roadmap).
- **otel-telemetry-swarm** — `mty-runtime` `telemetry/` submodule
  split + span helpers + `agent.event`; lazy init from
  `MTY_OTLP_ENDPOINT`. Commit `7ad8df5`. **SHIPPED-FULL**
  (Tier 1.2 + 1.3).
- **effect-row-wiring-swarm** — typed AST accessors + `HirEffectRow`
  + `UserRowPolyIndex` + MT4055..MT4059; example 22 typechecks.
  Commits `f583cf6` + `395f809` + `9806206`. **SHIPPED-FULL**.
- **wasi-p2-fs-http-swarm** — 5 fs + 4 http direct lowerings; latent
  emit.rs import-index bug fix via `prescan_p2_direct`. Commit
  `53fbafe`. **SHIPPED-FULL**.
- **selfhost-method-iter-swarm** — `MethodCall` lowering via host
  bridge + custom-iter desugar; 21 bootstrap tests target reached
  (23 driver tests live, 0 ignored). Commit `27455d5`.
  **SHIPPED-FULL**.

Plus orchestrator commits:

- **`7148b56`** — `audit.toml` ignore list for the wasmtime
  dev-dep bundle (extended in the integrator commit for two more
  wasmtime + three transitive entries).
- **`6030117` + `73b7467`** — Pages strict-mode link fixes (mkdocs
  workflow stayed on `build_type=workflow`; the off-tree config
  setting introduced in v0.15 continues to apply).

The integrator pass (this v0.16.0 tag commit) ran the pre-integrator
cleanup (`4f5fe58`: clippy collapse fix + selfhost test span clones
+ audit advisories), then re-verified all gates
(**1217 Rust + 139 Python + 92 conformance + 23 selfhost = 1471
tests passing** / clippy strict / fmt / 22-example matrix / 4/4
demos / 2 conformance ignored) and authored this `RELEASE-v0.16.md`.

See
[`INTROSPECT_V0_16_NOTES.md`](../notes/INTROSPECT_V0_16_NOTES.md),
[`TELEMETRY_SPANS_V0_16_NOTES.md`](../notes/TELEMETRY_SPANS_V0_16_NOTES.md),
[`EFFECT_ROW_WIRING_V0_16_NOTES.md`](../notes/EFFECT_ROW_WIRING_V0_16_NOTES.md),
[`WASI_P2_FS_HTTP_V0_16_NOTES.md`](../notes/WASI_P2_FS_HTTP_V0_16_NOTES.md),
and
[`SELFHOST_V0_16_NOTES.md`](../notes/SELFHOST_V0_16_NOTES.md) for
per-agent interpretation calls.
