# Changelog

All notable changes to Mighty are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

For the full per-release notes, see
[`dev/history/releases/`](dev/history/releases/).

## [Unreleased]

- v0.20-RC1 candidates: cross-RFC spec wording normalisation;
  RFC comment-window monitoring (8 open windows; earliest close
  2026-06-09 RFC-005, latest 2026-07-25 RFC-002 / RFC-006); strict-equality
  replay payloads (migrate v0.18 hot-path recording sites from
  `Opaque(format!("{:?}", args))` to
  `Values(args.iter().map(RuntimeValueLike::to_replay_value).collect())`);
  cluster security hardening (mutual-TLS client-cert verification by
  node id; cluster supervisors Tier 4.2 deferred from v0.19);
  populate the four placeholder conformance categories
  (`deterministic_replay/`, `formatter_idempotence/`, `native_abi/`,
  `wasm_component/`); diagnostic-code coverage report
  (`tests/conformance/coverage.json`); `mty conform <kit.tar.gz>`
  implementer-CLI shim; sigstore inclusion-proof crypto verify on
  `fetch`; MT0001 funnel split (MT0002/MT0003/MT0010/MT0011/MT0012/
  MT0020/MT0021/MT0030); `mty-pkg` cross-file resolution; parametric
  newtypes for self-host arena ids; self-host codegen broadening
  (real LEB128 in Mighty, arena drops at scope exit, agent backend);
  Windows named-pipe introspect backend; full `TokenStream` marshalling.

## [0.19.0] - 2026-05-26

**The last minor before v1.0-RC — Blockers #1 + #3 closed, every
KNOWN_ISSUES P1/P2 cleared, full cluster routing + byte-identical
replay land.** v0.19 closes two of the three v1.0-freeze blockers
(#1 Python 2nd-impl through HM + closures + generic-constraints with
+37 new tests; #3 normative conformance kit + spec doc +
`scripts/build-conformance-kit.sh`) and ships the tracking
infrastructure for the third (#2 RFC comment-window tracking via
`docs/spec/rfcs/COMMENT_WINDOWS.md`; the actual window-opening is a
user-driven admin action). The replay subsystem grows a **byte-identical
re-execution** mode on wire-format v2: `ReplayPayload::Values` carries
a structural mirror of the IR `Value` type (13 variants), `ReplayDriver`
re-runs the original program against the trace and diffs each event
byte-for-byte, `mty replay --byte-identical --program <path>` is the
CLI seam, v0.18 (`version=1`) traces decode transparently via the
`V1TraceFile` back-compat shim, +24 tests in
`crates/mty-runtime/tests/replay_byte_identical.rs` + unit-test files.
**Cluster routing wires into the Runtime hot path** (Tier 4.1
follow-up): `Runtime::with_cluster(SharedRouter)` +
`send_addr(AgentAddr, …)` + `ask_addr(AgentAddr, …)` consult the
router; a new `CorrelationTable` (`cluster/correlation.rs`) demuxes
inbound `Reply` / `Error` frames into oneshot receivers; a reply-demux
task peels reply frames off the mesh inbox before the runtime sees
them; peer-disconnect fan-out cleanly fails every in-flight ask to
that node (`MT5032`); a `[cluster]` / `[[cluster.peers]]` /
`[cluster.tls]` manifest parser lands in `mty-driver/src/manifest.rs`;
+8 integration tests in `tests/cluster_routing.rs`. **HIR lowerer
reads every row var**: `EffectClause::row_var_names()` (new AST
iterator) chains the three source positions in order;
`lower_effect_clause` collects every var into a fully-populated
`Vec<HirRowVar>`; the v0.15 first-only `row_var_name()` accessor is
`#[deprecated(since = "0.19.0", …)]`; +14 tests; `examples/24_multi_row_full.mty`
typechecks. **Paper-cuts cleared**: KNOWN_ISSUES #4 (`clippy-strict`
required) re-verified, KNOWN_ISSUES #5 (`mkdocs --strict`) re-verified,
KNOWN_ISSUES #7 (`--no-default-features` example sweep) added to the
`test-minimal` job; the vendored `wasi_snapshot_preview1.*.wasm`
bytes are deleted (~125 KB removed) in favour of caller-supplied
bytes via `AdapterEmbed::new(AdapterKind, Vec<u8>)`. **All
KNOWN_ISSUES P1/P2 entries are now closed.** The release workflow
that first fired on v0.15.0 continues to ship `mty` binaries for
Linux / macOS arm64 / Windows on every `v*` tag push (Intel macOS
dropped in v0.18). **1378 Rust + 311 Python + 122 conformance + 23
selfhost-driver = 1834 tests passing** (+121 vs v0.18), 0 failing,
2 ignored (`capability_checking/03_narrow_to_ro`,
`supervisor_restart/02_escalate` — both pending the cap-name
resolver wiring + escalation-chain serialisation rework, both post-v1.0
backlog). One new internals doc page lands
(`docs/internals/conformance.md`); `docs/reference/README.md` rewrites
from stub to full landing page; `mkdocs.yml` nav extended with the
new pages + a top-level **RFCs** section; `mkdocs build --strict`
passes locally. **Earliest possible v1.0.0 tag: 2026-07-26.**
[Release notes](dev/history/releases/RELEASE-v0.19.md).

## [0.18.0] - 2026-05-26

**v1.0 freeze gates closing fast — KNOWN_ISSUES P1 list cleared
(#1, #2, #3), replay end-to-end, distributed agents land.** v0.18
clears every P1 entry on `KNOWN_ISSUES.md`, wires deterministic
replay into the Runtime hot path across 13 instrumentation sites,
and grows the agent runtime a distributed transport layer (Tier 4.1
of `docs/internals/agent-features-roadmap.md`). The spec promotes
to **v1.0-RC4** with the RFC-008 multi-row-variable parser grammar
amendment at §9.2. The `cabi_realloc` real free-list allocator
(KNOWN_ISSUES #1) extracts from inline-in-emit to its own
`cabi_realloc.rs` module (8 size classes, ~190 wasm instructions, 17
dedicated coverage tests); the `mty-pkg/sigstore-real` cargo feature
(KNOWN_ISSUES #2) now compiles and drives the real keyless flow
end-to-end (Fulcio short-lived ECDSA-P256 cert + Rekor
`hashedrekord` upload with full standard Sigstore Bundle JSON
embedded under `verificationMaterial.sigstoreBundle`; `cosign
verify-blob` consumes it directly); the v0.17 replay recorder wires
into `Runtime::{spawn_agent, send, ask, shutdown}`, `agent.rs`'s
inner `run_one_turn_with_shared_reply`, the agent loop's
budget-exhaust / cancellation / terminal-exit arms, and every
`StdHost::effect_call` route for fs / http / time / random (13
sites total, zero overhead when `MTY_RECORD_TRACE` is unset);
`AgentAddr = node:type:pid` + `ClusterMesh` with framed CBOR over
TLS lands the Tier 4.1 transport layer (`Runtime::send` consults
the router in v0.19); the parser tail accepts `(',' RowVar)*` so
the multi-row source forms (`!{| E1, E2}` / `effect a, b | E1, E2`)
parse cleanly and flip MT4059 to active emit; the MSRV gate
(KNOWN_ISSUES #3) hardens to `cargo build --workspace --tests`
which pulls in the full `[dev-dependencies]` graph. The release
workflow that first fired on v0.15.0 continues to ship `mty`
binaries for Linux / macOS×2 / Windows on every `v*` tag push.
**1324 Rust + 274 Python + 92 conformance + 23 selfhost-driver =
1713 tests passing** (+50 vs v0.17), 0 failing, 5 ignored. Three
new internals doc pages land (`agents.md`, `introspect.md`,
`replay.md`); `mkdocs build --strict` passes locally.
[Release notes](dev/history/releases/RELEASE-v0.18.md).

## [0.17.0] - 2026-05-26

**WASI Preview 2 adapter goes away (`log()` direct), deterministic
replay + recorder land, Python 2nd-impl through typeck, RFC-008
multi-row, security bundle cleared.** v0.17 removes the last
preview1-adapter dependency in the WASI P2 hot path: `log()` /
`print()` now lower to a three-call canonical-ABI sequence on
`wasi:cli/stdout@0.2.3#get-stdout` +
`wasi:io/streams@0.2.3#[method]output-stream.blocking-write-and-flush`
+ `[resource-drop]output-stream`, and the embedded adapter flips
from always-on to opt-in (`Preview2Options::new(_).embed_adapter ==
None`; `.with_adapter(Some(WASI_P1_ADAPTER_COMMAND))` reattaches it
for back-compat builds). Tier 1.4 of
`docs/internals/agent-features-roadmap.md` lands as
`crates/mty-runtime/src/replay/{wire, recorder, mod}` (8 typed
`TraceEvent` variants, `MTYTRACE`-magic + serde-additive wire format
v1, `StepHandler` trait + `CountingStepHandler`) and a `mty replay
<trace>` CLI with `--dump-json` + `--step` + `--json` modes; the
full Runtime re-execution and hot-path wire-up are deferred to v0.18.
The Python 2nd-impl (`impl-py/`) reaches typeck for the first time
via `mty/hir.py` + `mty/lower.py` + `mty/typeck.py` (Hindley-Milner
unifier with `TyAny` absorption for shapes the v0.17 surface doesn't
yet model); all 23 `examples/*.mty` typecheck clean and the test
count grows **139 → 274** (+135), substantially closing v1.0
freeze blocker #2. RFC-008's HIR widens to
`HirEffectRow::Open(concrete, Vec<HirRowVar>)`; the
`UserRowPolyMeta` side table feeds the call-site walker so MT4055
(declaration ambiguity), MT4056 (concrete + row var with no fn-typed
param), and MT4058 (call-site arity mismatch) all reach active
emission, with MT4059 reserved for the v0.18 parser ship of
`!{| E1, E2}`. The `wasmtime` dev-dep bumps 25 → 36, clearing 15
RUSTSEC advisories (`audit.toml` ignore list shrinks 16 → 3); no
production code is affected. The release workflow that first fired
on v0.15.0 continues to ship `mty` binaries for Linux / macOS×2 /
Windows on every `v*` tag push. The spec stays at v1.0-RC3.
**1274 Rust + 274 Python + 92 conformance + 23 selfhost-driver =
1663 tests passing** (+192 vs v0.16), 0 failing, 4 ignored.
[Release notes](dev/history/releases/RELEASE-v0.17.md).

## [0.16.0] - 2026-05-26

**Observability + RFC-008 typeck-finishing tier — live agent
introspection (`mty inspect` + control socket), OpenTelemetry agent
spans, user-authored effect rows typecheck end-to-end, WASI Preview 2
fs + http direct, self-host MethodCall + custom iterators.** Tier 1.1
of `docs/internals/agent-features-roadmap.md` lands as
`crates/mty-runtime/src/introspect.rs` + `control_socket.rs` and a
new `mty inspect` CLI (pretty / JSON / `--watch` modes) wired to an
opt-in `MTY_RUNTIME_CONTROL_SOCK` Unix-domain socket; `AgentSnapshot`
exposes agent type, mailbox depth + high-water, in-flight handler +
elapsed, CPU / mem / tick budgets, and the last-N messages (opt-in
body capture) at wire `version: 1` (additive evolution). Tier 1.2 +
1.3 land as a new `telemetry/` submodule under `mty-runtime`:
`span_spawn` / `span_send` / `span_ask` / `span_handler` plus
`record_restart` + `record_budget_exhausted`; the
`agent.event(name, &[(k, v)])` helper attaches user attributes to the
active handler span; lazy init from `MTY_OTLP_ENDPOINT` keeps the
runtime cost-zero when telemetry is disabled. The v0.15 RFC-008
surface syntax is wired through typed AST accessors
(`mty-ast::effects`) → `HirEffectRow` (`Closed | Open`) on
`HirFn::effect_row` → `UserRowPolyIndex` in `mty-types::effects`;
five new diagnostic codes (**MT4055 / MT4056 / MT4057 / MT4058 /
MT4059**) are wired, MT4057 actively emits, and
`examples/22_effect_row.mty` flips from `@typeck-pending` to live in
the example sweep. The WASI P2 emitter takes nine more stdlib
lowerings direct: five `std.fs` fns (`open` / `read_file` /
`write_file` / `stat` / `close`) hit
`wasi:filesystem/types@0.2.3#descriptor.*` and four `std.http`
variants (`get` / `post` / `send` / `incoming_request_consume`) hit
`wasi:http/types@0.2.3` + `wasi:http/outgoing-handler@0.2.3`; a
latent emitter import-index bug is fixed via a new `prescan_p2_direct`
predeclare pass. The self-host Wasm codegen lowers `Rvalue::MethodCall`
through the host `ir_method_resolve(name)` bridge (v0.15 emitted
`unreachable`) and desugars `for x in custom_iter` at the selfhost-IR
layer into the iter-protocol loop-match-`Some`/`None` shape; driver
tests go **17 → 23 live / 0 ignored**. The release workflow that
first fired on v0.15.0 continues to ship `mty` binaries for Linux /
macOS×2 / Windows on every `v*` tag push. The spec stays at v1.0-RC3.
**1217 Rust + 139 Python + 92 conformance + 23 selfhost-driver = 1471
tests passing** (+43 vs v0.15), 0 failing, 4 ignored.
[Release notes](dev/history/releases/RELEASE-v0.16.md).

## [0.15.0] - 2026-05-25

**Dispatch-finishing tier — HOF dispatch end-to-end, RFC-008
surface syntax, WASI P2 default, self-host 17 codegen tests,
cross-platform release binaries.** The 19 row-polymorphic stdlib
signatures that v0.14 landed as a SHIPPED-SUBSET are now wired
through call-site dispatch: a new `BuiltinMethod.row_sig` field
threads 21 sigs across 12 method names into
`walk_expr_effects`, which instantiates fresh row variables per
call and propagates closure effects into the caller (MT4050 fires
on closed-row rejection; +10 dispatch tests). RFC-008 surface
syntax `!E` / `!{a | E}` / `!{fs, net | E}` / `effect a | E`
parses through `mty-syntax` with 4 new SyntaxKind variants
(EFFECT_SET, EFFECT_NAME, EFFECT_ROW_TAIL, EFFECT_ROW_VAR), spec
§9.2.1, +16 parser tests, and `examples/22_effect_row.mty`
(parser-only; HIR/typeck wiring is v0.16). WASI Preview 2 is now
the default for `wasm32-wasi` (explicit `--wasi=p1` retains
back-compat) and four stdlib fns (`std.random.bytes`,
`std.time.now` / `monotonic_now` / `resolution`) emit direct P2
imports through `emit.rs`; the log shim + `std.fs` / `std.http`
still route through the embedded adapter (canonical-ABI rewrite
deferred to v0.16). The self-host Wasm codegen reaches **17 live /
0 ignored** (was 13) with variant-call lowering in
`mty-ir::lower::exprs::resolve_callee` (Some/Ok/MyEnum.Variant →
`Rvalue::AdtInit`), a SwitchInt cascade for dense integer matches,
and `for i in 0..n` desugar. The deprecated
`mty_macros::expand` / `expand_to_source` API is removed (9
integration test files migrated; `mty-macros` 111 → 101 tests, 10
redundant pruned + coverage preserved). The v0.13 red-shirt
`conformance/borrow_checking/14_borrow_outlives_owner` is closed
by the one-line `SyntaxKind::BLOCK` arm in
`mty-hir::lower::exprs::is_expr_node`; conformance corpus moves
**91 → 92 cases / 16 categories / 3 → 2 ignored**. A new
`.github/workflows/release.yml` produces `mty` binaries for Linux /
macOS×2 / Windows on `v*` tag push — first run on this tag. The
spec stays at v1.0-RC3 (RFC-008 + RFC-009 remain roadmap RFCs).
**1140 Rust + 139 Python + 92 conformance + 57 self-host = 1428
tests passing** (+38 vs v0.14), 0 failing, 3 ignored.
[Release notes](dev/history/releases/RELEASE-v0.15.md).

## [0.14.0] - 2026-05-25

**Integration-and-finishing tier — WASI Preview 2 with vendored
wasmtime adapter, self-host codegen reaches example 03, set-of-scopes
hygiene now powers HIR macro resolution, KNOWN_ISSUES #11 closed.**
The WASI Preview 2 backend now embeds the upstream wasmtime v32
preview1→preview2 adapter (command / reactor / proxy under
[`crates/mty-codegen-wasm/wit/adapter/`](crates/mty-codegen-wasm/wit/adapter/))
and ships the full upstream WASI 0.2.3 WIT surface; `std.random` /
`std.time` route through new `P2DirectImport` constants direct to
preview2 origins (`std.fs` / `std.http` direct lowering is v0.15).
The v0.13 internal `mighty:cli-adapter` shim is gone — components
now run unmodified on any preview2 host. The self-host codegen
([`selfhost/codegen/wasm.mty`](selfhost/codegen/wasm.mty)) grew
~400 → ~660 LOC with three new modules
(`string_pool.mty`, `adt_layout.mty`, `pattern.mty`) and the
driver test reports **13 live / 0 ignored** (example 03 passes,
was the v0.13 single ignored). `mty-hir::lower::macros` now drives
`expand_scoped_to_source` (set-of-scopes) rather than the legacy
mangler; the legacy `expand` / `expand_to_source` API stays
callable behind a `#[deprecated(since = "0.14.0")]` shim with
removal scheduled for v0.15. Two FROZEN typeck codes land their
emit-sites (MT2003 at `check_stmt(HirStmt::Let)`, MT2023 at
`resolve_generic_args`); the other four in KNOWN_ISSUES #11
(MT2009 / MT2022 / MT2024 / MT2025) were rediscovered to already
have emit-sites from v0.12 work — issue #11 closed with a per-code
closure-history table. The conformance corpus moves **89 → 91
cases** / 16 categories / 3 ignored (red-shirt
`14_borrow_outlives_owner` traced to a one-line bug in
`mty-hir::lower::exprs::is_expr_node` missing the `BLOCK` arm —
out of v0.14 swarm scope, carried over). Stdlib HOF row-polymorphism
lands 19 more row-polymorphic signatures in a new `pub mod
stdlib_sigs` (+207 LOC) as a SHIPPED-SUBSET — the signatures + 24
tests ship; the call-site dispatch through
`prelude::BuiltinMethod` is v0.15. Integrator carve-out: MT2003
exempts `let mut xs = []` (legitimate idiom — downstream assignments
unify the element type), with a regression test pinning the
behaviour. The spec stays at v1.0-RC3. **1109 Rust + 137 Python +
91 conformance + 53 self-host = 1390 tests passing** (+67 vs
v0.13), 0 failing, 4 ignored.
[Release notes](dev/history/releases/RELEASE-v0.14.md).

## [0.13.0] - 2026-05-25

**Capability tier — end-to-end self-host complete + WASI Preview 2 +
2 new RFCs (effect rows + set-of-scopes hygiene).** The Mighty
compiler front-end + Wasm core-module back-end is now implemented in
Mighty source for the slice-1 subset:
[`selfhost/codegen/wasm.mty`](selfhost/codegen/wasm.mty) (~400 LOC)
closes the bootstrap chain lexer → parser → HIR → typeck → MtyIR →
wasm codegen, with 6/6 live driver tests passing (1 ignored — example
03's generic `Option[T]`). **The self-host milestone called for since
the v0.5 lexer port is reached.** A WASI Preview 2 backend lands
behind `--wasi=p2` (default stays `p1`): new `--world <name>` flag, a
new `[wit]` section in `mighty.toml` for user-supplied WIT, a vendored
`wasi:*@0.2.3` slice covering `cli`/`io`/`clocks`/`filesystem`/`http`/
`random`, example at [`examples/21_wasi_preview2.mty`](examples/21_wasi_preview2.mty),
user-facing matrix at [`docs/reference/wasi.md`](docs/reference/wasi.md).
Two new RFCs land with usable infrastructure: **RFC-008 effect-row
polymorphism** (`!E`, `!{a | E}`, four-case unification, subsumption)
with a 450-LOC row module in `crates/mty-types/src/effects.rs::row`
and a relaxed `stdlib_list_map_sig()`; and **RFC-009 set-of-scopes
macro hygiene** (Flatt-style scope sets) with `scopes.rs` + `hygiene.rs`
+ a new `expand_scoped()` entry point alongside the legacy mangler.
Both ship as **SHIPPED-SUBSET**: infrastructure + tests + first wired
consumer, with v0.14 follow-ups for surface-syntax parsing
(RFC-008) and mty-hir rewire (RFC-009). The spec stays at v1.0-RC3;
the conformance corpus stays at 89 cases / 16 categories / 3 ignored.
**1051 Rust + 137 Python + 89 conformance + 46 self-host = 1323 tests
passing** (+82 vs v0.12), 0 failing, 5 ignored.
[Release notes](dev/history/releases/RELEASE-v0.13.md).

## [0.12.0] - 2026-05-25

**Spec-and-evidence tier — v1.0-RC3 spec released + 4th showcase
demo + conformance Gap B/C/E partial closure + Go 3rd-impl source
landed.** The normative spec advances **v1.0-RC2 → v1.0-RC3**:
operator precedence is promoted to normative §11.1.1 (was deferred
to non-normative `docs/internals/parser.md`); the full reserved
keyword set is enumerated (63 reserved + 4 contextual + 7
reserved-for-future); the 16 Python-impl spec findings from v0.11
are codified in prose (+396 spec lines, no behaviour change). A
fourth runnable showcase lands at [`demos/04_kvstore/`](demos/04_kvstore/)
— a sharded supervised in-memory key-value store (~400 LOC)
exercising agents + protocols + supervisors + restart-on-crash +
`std.http` end-to-end (the first demo whose pitch is the
supervisor restart story). The conformance corpus gains six new
fixtures (typeck 17..20, borrow 13..14) and a real MT3007
`BORROW_OUTLIVES_OWNER` emit-site in `mty-borrow/src/flow.rs`;
the harness now reports **89 cases / 16 categories / 3 ignored**
(one new red-shirt: `borrow_checking/14_borrow_outlives_owner`
needs `pending_borrower` wired through plain assignments —
deferred to v0.13). A Go 3rd-impl lands at
[`impl-go/`](impl-go/): 4848 LOC of lexer + parser + CLI + tests,
built from `docs/spec/v1.0-rc.md` (v1.0-RC3) prose alone, with
zero peeking at `crates/mty-*`, `selfhost/`, or `impl-py/`. The
Go toolchain is not installed on the v0.12 build host so
`go test ./...` has not been run; cross-validation pending v0.13.
**Closes KNOWN_ISSUES #10 (operator precedence not normative) and
#12 (`package`/`export`/`requires` keywords not in §3.3).** **977
Rust + 135 Python + 89 conformance + 40 self-host = 1241 tests
passing**, 0 failing, 3 ignored. [Release notes](dev/history/releases/RELEASE-v0.12.md).

## [0.11.0] - 2026-05-25

**Quality tier — strict-clippy gate green + Python 2nd-impl partial
+ conformance gap closure + UX polish.** The `clippy (strict)` CI
job is now **required** (no more `continue-on-error: true`) and
clean across the whole 20-crate workspace: 2341 pedantic warnings
on baseline → 0 via a workspace-level `[lints.clippy]` allowlist
plus ~30 real fixes. **All six CI jobs now run as required gates.**
An independent Python implementation of the Mighty front-end lands
at [`impl-py/`](impl-py/): pure-Python lexer + parser (~2.5 KLOC)
built from the v1.0-RC2 spec prose alone (no peeking at
`crates/mty-syntax`, `crates/mty-ast`, or `selfhost/`); **135 tests
passing, 20/20 examples lex+parse**. **Real partial credit on v1.0
freeze blocker #1** (two independent implementations). The slice
also surfaced 16 spec findings — biggest: operator precedence is
not in the normative §11 (deferred to `docs/internals/parser.md`)
and needs to be promoted before v1.0 freeze. Normative conformance
corpus grows **88% → 91% FROZEN coverage** (62% → 70% direct), 4 of
8 documented gaps closed with two harness extensions
(warning-severity assertions; per-case `mighty.toml` via `CwdGuard`)
plus 3 new positive-fire cases (MT2012, MT6003, MT6008); the 4
deferred gaps each have a precise crate-source-edit reason recorded.
UX polish: 15 high-traffic MTxxxx codes rewritten to a consistent
Cause/Example/Fix/Spec format, all 16 tour chapters refreshed
(`.sd` → `.mty`, spec links bumped to `v1.0-rc.md`), FAQ extended
12 → 26 entries, getting-started rewritten 187 → 290 lines.
Inherited from post-v0.10.0 `main`: three macOS codegen fixes
(`LC_BUILD_VERSION` on Mach-O objects + cosmetic + CI tolerance for
missing `cc`). **977 Rust tests + 135 Python tests = 1112 total.**
[Release notes](dev/history/releases/RELEASE-v0.11.md).

## [0.10.0] - 2026-05-25

**Production cleanup + conformance audit.** Lifts the v0.9 RC-prep
stubs to real implementations: `cabi_realloc` becomes a segregated
free-list allocator (8 size classes + bump tail), sigstore signing
gets a real keyless path behind the `sigstore-real` feature (default
keeps the v0.9 SHA-256 envelope shape), the Cranelift egraph fuzz
bug is filed upstream as
[wasmtime #13476](https://github.com/bytecodealliance/wasmtime/issues/13476)
with an in-tree `MTY_CRANELIFT_NO_OPT` workaround and a new
`MTY_DUMP_CLIF` debug knob. Conformance corpus grows 16 → 81 cases
(88% FROZEN coverage). Self-host examples 04 + 05 deferrals closed —
**40/40 selfhost tests now pass**. CI hardened: MSRV gate now runs
`cargo test --no-run` + bedrock subset; `mkdocs --strict` enabled
with all 55 stale links fixed; cargo-audit job added; parallel
monomorphisation honestly reverted to sequential default after
re-benching. Major repo cleanup: 62 dev artefacts archived under
`dev/history/`, README rewritten 421 → 210 lines, root
`CHANGELOG.md` introduced, license switched from Apache-2.0/MIT dual
to **MIT-only**, repo URL bumped `hassard0/stardust` →
`hassard0/Mighty`. **977 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.10.md).

## [0.9.0] - 2026-05-24

**RC-prep + freeze-readiness.** Spec promoted to **v1.0-RC2** with all
10 OPEN amendments resolved (3 FREEZE-MVP, 7 DEFER-V1.1) and six
follow-up RFCs drafted (RFC-001..RFC-006). Brought up a four-target
cargo-fuzz harness (parser / typeck / fmt / codegen) with 27-file seed
corpus, fixed three P0 OOM parser bugs the fuzzer surfaced, and did an
audit sweep over every sibling `loop` for the same anti-pattern.
Self-hosted the MtyIR lowering on examples 01-03 (joining the v0.5
lexer, v0.6 parser, v0.8 HIR + minimal typeck — **34 self-host tests
passing**). Fixed `demos/02_counter_web`'s long-standing
`cabi_realloc` regression (3/3 demos passing again). Published the
[GitHub Pages docs site](https://hassard0.github.io/Mighty/), hardened
CI (stable/beta/nightly matrix, minimal-versions, strict, MSRV), shipped
reproducible release scripts, and landed a sigstore-style package
signing stub. **955 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.9.md).

## [0.8.0] - 2026-05-24

**Loose-end closure + self-host HIR + perf + spec v1.0-RC.** Closed 4 of
5 remaining v0.5 loose ends (proc-macro sandboxed execution with
MT6007/MT6008, real per-agent HTTP routing, LSP cross-file workspace
resolve, WIT canonical-ABI return-area for DOM strings). Self-hosted
the HIR + minimal typeck phases (~1.1 KLOC of Mighty source; 5+5 new
self-host tests). Three of four perf optimisations landed (parse +27%,
mailbox +7%, ~800 ns agent-send). Consolidated 88 spec amendments into
**v1.0-RC** at `docs/spec/v1.0-rc.md`. Closed all rebrand residuals
(runtime ABI symbols, DWARF producer, bench fixture). **927 tests
passing.** [Release notes](dev/history/releases/RELEASE-v0.8.md).

## [0.7.0-rebrand] - 2026-05-24

**Stardust → Mighty rename.** Naming-only release: 20 `sdust-*` crates
renamed to `mty-*`, `.sd` → `.mty` source extension, `star.toml`/`star.lock`
→ `mighty.toml`/`mighty.lock`, `SD####` → `MT####` diagnostic codes
(with `SD` aliases preserved for `mty explain`), WIT `stardust:*` →
`mty:*`, VS Code extension repackaged. **0 behavioural deltas — 885
tests pass byte-for-byte against v0.6.0.**
[Release notes](dev/history/releases/RELEASE-v0.7.md).

## [0.6.0] - 2026-05-24

**Multi-core + benchmarks + self-host parser.** Runtime now distributes
work across N OS threads via per-worker tokio runtimes + crossbeam-deque
work-stealing + affinity hints + lightweight migration + per-worker
stats. First honest benchmarks shipped — new `mty-bench` crate covers
six categories with Rust/Go/C++ comparators. Self-host parser subset
(~1930 LOC, 13/13 bootstrap tests, examples 01-05 covered). DOM MtyIR
lowering reaches `emit_dom_call` end-to-end. MT6001-MT6006 macro codes
merged into the central `mty-diagnostics` catalog. Per-call `FsCap`
isolation contract test. **885 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.6.md).

## [0.5.0] - 2026-05-24

**Self-hosting + dogfood completion.** Loops actually terminate via
`break`/`continue`/iterator exhaustion (bounded-fixed-point loop
borrows). Self-host lexer now round-trips byte-for-byte against the
Rust lexer. Five v0.4 dogfood stopgaps replaced with real
implementations (real `std.http.serve` over TCP, Wasm DOM imports as
a 4-method WIT interface, full `Str` method table, MtyIR
mem-budget auto-charge, `FsCap` allowlist process-wide). Macros
completion: `name!(args)` invocation, extended hygiene, cross-file
`pub macro`, proc-macro skeleton, stdlib macros. LSP advanced —
semantic tokens, rename, inlay hints, code actions, signature help,
workspace folders, semantic completion. **839 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.5.md).

## [0.4.0] - 2026-05-24

**Dogfood + ecosystem.** Three end-to-end dogfood demos
(`01_search_api`, `02_counter_web`, `03_extract_tool`) with passing
smoke scripts. Real package registry transport over GitHub Releases
REST with on-disk index cache + sha256 sidecar + deterministic
`.tar.gz` bundles + three new CLI subcommands. Hygienic declarative
macros (MT6001..MT6004 catch unknown/arity/depth/bad-arg). Self-host
lexer subset bootstrap. MtyIR loop terminator fix — loops genuinely
iterate. **692 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.4.md).

## [0.3.0] - 2026-05-25

**Soundness hardening.** Borrow checker grew NLL last-use deactivation
and field-level Places. Type checker grew scope-aware tolerance and
the formal `Sendable` trait (MT3011 at every send/ask site). Runtime
grew cooperative mid-turn cancellation, OTLP wire-format telemetry,
and slab-pool mailbox frames. Closed v0.2 cleanup backlog: stdlib
install, 6/20 wasm-CM gaps, 3 of 5 INTENTIONALLY_IGNORED conformance
cases. **623 tests passing, 20/20 wasm Components.**
[Release notes](dev/history/releases/RELEASE-v0.3.md).

## [0.2.0] - 2026-05-24

**LSP + pkg + doc + DWARF + Wasm CM + stdlib.** Closed every bullet on
the v0.1 deferral list: LSP 3.17 server with VS Code scaffold, package
manager (resolver + lockfile + path/git fetchers + publisher), doc
generator (markdown + HTML + search index), real stdlib (`std.json`,
`std.tls`, `std.http`, `std.fs`, `std.time`, `std.test`) backed by
rustls/hyper/serde_json/tokio, DWARF v4 debug info + wasm source maps,
Wasm Component Model output by default (`wit-component`). 20/20 native
+ 20/20 wasm core-module compilation. **550 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.2.md).

## [0.1.0] - 2026-05-24

**First feature-complete release.** Walked the full spec §31 roadmap
across eight slices: parser → formatter → HIR → type checker → borrow
checker → effects/capabilities/traits → MtyIR + interpreter → runtime
MVP → native (Cranelift JIT + AOT) + Wasm core module codegen. `mty
new` / `check` / `fmt` / `dump` / `run` / `build` / `explain`. 65+
diagnostic codes across MT0xxx..MT8xxx. MSRV Rust 1.85. **376 tests
passing.** [Release notes](dev/history/releases/RELEASE-v0.1.md).

[Unreleased]: https://github.com/hassard0/Mighty/compare/v0.11.0...HEAD
[0.11.0]: https://github.com/hassard0/Mighty/releases/tag/v0.11.0
[0.10.0]: https://github.com/hassard0/Mighty/releases/tag/v0.10.0
[0.9.0]: https://github.com/hassard0/Mighty/releases/tag/v0.9.0
[0.8.0]: https://github.com/hassard0/Mighty/releases/tag/v0.8.0
[0.7.0-rebrand]: https://github.com/hassard0/Mighty/releases/tag/v0.7.0-rebrand
[0.6.0]: https://github.com/hassard0/Mighty/releases/tag/v0.6.0
[0.5.0]: https://github.com/hassard0/Mighty/releases/tag/v0.5.0
[0.4.0]: https://github.com/hassard0/Mighty/releases/tag/v0.4.0
[0.3.0]: https://github.com/hassard0/Mighty/releases/tag/v0.3.0
[0.2.0]: https://github.com/hassard0/Mighty/releases/tag/v0.2.0
[0.1.0]: https://github.com/hassard0/Mighty/releases/tag/v0.1.0
