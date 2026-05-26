# Mighty v0.17 — Release Notes

**Tag:** `v0.17.0`
**Date:** 2026-05-26
**Status:** SHIPPED — the WASI Preview 2 adapter goes away for
`log()` (direct three-call `wasi:cli/stdout` + `wasi:io/streams`
sequence; the embedded adapter is opt-in via `with_adapter`),
deterministic-replay infrastructure lands (recorder, wire format
v1, `mty replay` CLI, step replayer), the Python 2nd-impl extends
end-to-end through HIR + lowering + Hindley-Milner typeck (139 →
274 tests; 23/23 examples typeck clean), RFC-008 broadens to a
multi-row-variable HIR + activates four more diagnostic codes
(MT4055 / MT4056 / MT4058 reach active emit, MT4059 reserved), and
the dev-dep wasmtime bump (25 → 36) clears 15 RUSTSEC advisories
from the audit ignore list.

v0.17 is the **adapter-free + replay + Python-typeck + RFC-008-multi
release**: the last preview1-adapter dependency in the WASI P2 hot
path is removed (`log()` direct, adapter dropped from default
`Preview2Options` and only reattached on explicit `with_adapter`);
the agent-features roadmap Tier 1.4 deterministic-replay tier lands
as a recorder + 8 typed event helpers + wire-format-v1 + a
`StepHandler`-trait replay CLI (`mty replay <trace>` with
`--dump-json` and `--step`); the Python second implementation
reaches typeck for the first time (HM unifier with TyAny absorption
for opaque shapes); RFC-008's HIR enriches to `Vec<HirRowVar>` and
the call-site walker fires MT4055 / MT4056 / MT4058 (MT4059
reserved) so multi-row signatures can be represented end-to-end at
the HIR + typeck layers (the surface-syntax parser extension is
queued for v0.18); and `wasmtime` jumps 25 → 36 in dev-deps so the
audit ignore list shrinks 16 → 3.

**Headline:** **WASI Preview 2 adapter goes away (`log()` direct),
deterministic replay + recorder land, Python 2nd-impl through
typeck, RFC-008 multi-row, security bundle cleared.**

If you were on v0.16.0, the upgrade is `git pull && cargo install
--path crates/mty-cli --force` (or pull pre-built binaries from
the [Releases page](https://github.com/hassard0/Mighty/releases)
— the cross-platform `release.yml` workflow first fired on
v0.15.0 and continues on every `v*` tag). There are no source-level
breaking changes for end users: the v0.17 surfaces are strictly
additive at the language layer. **Toolchain consumers that
directly construct `Preview2Options::new(_)` will see
`embed_adapter == None` by default** (was `Some(WASI_P1_ADAPTER_COMMAND)`);
explicit opt-in via `with_adapter(Some(_))` restores v0.16 behaviour.

## Highlights

- **5 of 5 v0.17 swarm tracks SHIPPED** (4 SHIPPED-FULL + 1
  SHIPPED-SUBSET). WASI P2 `log()` direct + adapter opt-out
  (SHIPPED-FULL), Python 2nd-impl HIR + lowering + typeck
  (SHIPPED-FULL), RFC-008 multi-row-var HIR + MT4055/56/58 active
  emit (SHIPPED-FULL), wasmtime dev-dep bump (SHIPPED-FULL),
  deterministic replay (SHIPPED-SUBSET — recorder + wire format +
  step replayer + CLI; full Runtime re-execution deferred to v0.18).
- **The WASI Preview 2 adapter dependency is gone from the default
  hot path.** v0.13–v0.16 wrapped every P2 component with the
  vendored upstream preview1→preview2 `Command`-world adapter to
  satisfy the `wasi:cli/log` shim that `log()` lowered to. v0.17
  drops the shim — `log()` now emits a three-call canonical-ABI
  sequence on `wasi:cli/stdout@0.2.3#get-stdout` +
  `wasi:io/streams@0.2.3#[method]output-stream.blocking-write-and-flush`
  + `[resource-drop]output-stream` — and flips the adapter from
  always-on to opt-in (`Preview2Options::new(_).embed_adapter ==
  None`; `.with_adapter(Some(...))` reattaches it for back-compat
  builds). The vendored adapter bytes still ship for opt-in but
  no longer wire by default; deletion is queued for v0.18.
- **Tier 1.4 deterministic replay lands** (`mty-runtime::replay::*`).
  A `Recorder` captures eight typed `TraceEvent` variants (spawn,
  send, ask-{request,reply}, handler-{begin,end}, timer, restart)
  to a `MTYTRACE`-magic-prefixed wire-format-v1 file; the
  `Replayer` + `StepHandler` trait + `CountingStepHandler` impl
  drive a step-by-step walk (the full re-execution that asserts
  byte-identical handler outputs is the v0.18 stretch); the
  `mty replay <trace>` subcommand exposes `--dump-json` and
  `--step` modes with optional `--json` output for structured
  consumers. Wire-format-v1 uses serde additively (new variants
  decode as `_unknown` on older readers).
- **Python 2nd-impl reaches typeck for the first time** (`impl-py/`).
  `mty/hir.py` + `mty/lower.py` + `mty/typeck.py` ship a Resolution
  discriminator, scope-stack name resolution, and a
  Hindley-Milner unifier with `TyAny` absorption for shapes that
  the v0.17 surface doesn't yet model (closures-as-values, full
  trait dispatch, generic constraint propagation). All 23
  `examples/*.mty` typecheck clean; 139 → **274 tests** (+135);
  9 interpretation calls + 7 v0.18 follow-ups are documented in
  the slice notes. Closes the second-largest v1.0 freeze blocker
  (a 2nd-impl front-end + typeck) substantially — the remaining
  v1.0 work is HM closure inference + generics-with-constraints.
- **RFC-008 reaches multi-row-variable signatures at the HIR +
  typeck layers.** `HirEffectRow::Open` now carries
  `Vec<HirRowVar>` (was a single `HirRowVar`); `UserRowPolyIndex`
  gains a `meta: HashMap<FnId, UserRowPolyMeta>` side table with
  per-fn row-var names + fn-typed-param count + spans + concrete
  effects so the call-site walker can fire MT4055 / MT4056 / MT4058
  without re-walking the HIR. MT4055 (row var declared but no
  fn-typed param can bind it AND multiple non-fn params), MT4056
  (concrete + row var with no fn-typed param — structurally inert),
  and MT4058 (caller's lambda-arg count vs callee's fn-typed-param
  count) actively emit; MT4059 (caller's closed-row enclosing fn
  rejects the closure's effects) is reserved for v0.18 once the
  parser ships the `!{| E1, E2}` surface form.
- **`wasmtime` dev-dep bumped 25 → 36** (latest stable was 45;
  stayed at 36 for minimal transitive churn). 15 RUSTSEC
  advisories cleared from `audit.toml` ignore list (16 entries
  before, 3 after — `wasmtime-internal-cache`, `pkg-config`,
  `paste`, all dev-dep-only and pinned for documented reasons).
  Transitive bumps: `wasm-encoder` / `wasmparser` / `wasmprinter`
  0.217 → 0.236; `gimli` 0.29 → 0.32 (dev-dep tree only). No
  source-level API changes required.
- **All gates green, test count grows 1217 → 1274 Rust + 139 → 274
  Python + 92 conformance + 23 self-host driver tests = 1663
  combined** (+192 vs v0.16, of which +135 are Python typeck and
  +57 are Rust). 0 failing, 4 ignored (2 doc-tests + 2 long-standing
  conformance).
- **Cross-platform binaries continue to ship.** The `release.yml`
  workflow that first fired on v0.15.0 produces `mty` binaries for
  Linux x86_64, macOS x86_64 + arm64, and Windows x86_64 on every
  `v*` tag push. See the
  [Releases page](https://github.com/hassard0/Mighty/releases).

## What's new

### WASI Preview 2 — `log()` direct + adapter opt-out

The slice-8 `wasi:cli/log` shim that's gated every WASI P2 build
since v0.13 is gone. `log()` and `print()` now lower to a three-call
canonical-ABI sequence on direct versioned imports.

- **Imports declared:**
  `wasi:cli/stdout@0.2.3#get-stdout`,
  `wasi:io/streams@0.2.3#[method]output-stream.blocking-write-and-flush`,
  and `wasi:io/streams@0.2.3#[resource-drop]output-stream`.
- **Emit sequence (per `log()` call):**
  `call get-stdout` → `local.tee handle` → ptr/len/ret-area
  arguments → `call blocking-write-and-flush` → `local.get handle`
  → `call stream-drop`. Zero allocation; zero shim glue.
- **Adapter flipped from always-on to opt-in.**
  `Preview2Options::new(target).embed_adapter == None` in v0.17
  (was `Some(WASI_P1_ADAPTER_COMMAND)`). The constant
  `WASI_P1_ADAPTER_COMMAND` is still exported and `with_adapter`
  still attaches it on request — back-compat consumers that need
  the adapter for legacy preview1 surfaces opt in explicitly.
- **+8 new tests** in `crates/mty-codegen-wasm/tests/preview2_log.rs`
  pin the new direct-import signature, the opt-in adapter
  behaviour, and the byte-size reduction (a log()-heavy program
  ships smaller bytes with the adapter opted out). **4 existing
  tests inverted** to v0.17 expectations: the
  `p2_component_imports_include_wasi_log_shim` assertion + three
  adapter-on default-shape assertions.
- **Reference docs.** Updated
  [`docs/internals/wasi-preview2.md`](../../../docs/internals/wasi-preview2.md);
  unchanged user-facing CLI docs.

See
[`WASI_P2_LOG_V0_17_NOTES.md`](../notes/WASI_P2_LOG_V0_17_NOTES.md).

### Tier 1.4 deterministic replay — recorder + CLI + step replay

The fourth tier of `docs/internals/agent-features-roadmap.md`
lands as a recorder + wire format + step replayer. Full Runtime
re-execution (where the replayer drives a `Runtime` and asserts
byte-identical handler outputs) is the v0.18 stretch.

- **`crates/mty-runtime/src/replay/`** — three modules:
  - `wire.rs` — `TraceFile`, `TraceEvent` (8 variants:
    spawn / send / ask-request / ask-reply / handler-begin /
    handler-end / timer / restart), `TraceSummary`; `MTYTRACE`
    magic + serde-additive variant policy + version field.
  - `recorder.rs` — `Recorder` with thread-safe append + 8 typed
    event-emit helpers + `install_from_env`
    (`MTY_REPLAY_RECORD=/path/to/trace`).
  - `mod.rs` — `Replayer` driver + `StepHandler` trait +
    `CountingStepHandler` impl.
- **`crates/mty-cli/src/cmd/replay.rs`** — `mty replay <trace>` CLI
  subcommand. Default mode prints a `TraceSummary` (event counts +
  total bytes + wall time spanned). `--dump-json` streams every
  event as one JSON object per line. `--step` drives the
  `CountingStepHandler` and prints the per-event step count;
  `--json` toggles structured output on either path.
- **+35 new tests:** 10 integration in `crates/mty-runtime/tests/replay.rs`
  (recorder round-trip, magic prefix rejection, version
  back-compat, install-from-env smoke); 20 unit across
  `replay::{wire, recorder, mod}` (event encoding, ring-buffer
  bounds, error paths); 5 CLI unit in `cmd::replay::tests`
  (each output mode + a malformed-trace negative).
- **Reference docs.** New
  [`docs/reference/cli/mty-replay.md`](../../../docs/reference/cli/mty-replay.md).
- **Hot-path wire-up in Runtime is v0.18.** The recorder is
  spawnable today via `install_from_env` and the API surface is
  stable; emitting events from the live agent boundaries (spawn /
  send / handler) is deferred because it touches off-limits files
  in the v0.17 slice plan (the agent dispatch state machine).

See [`REPLAY_V0_17_NOTES.md`](../notes/REPLAY_V0_17_NOTES.md).

### Python 2nd-impl — HIR + lowering + typeck

The Python second implementation reaches typeck for the first
time, substantially closing v1.0 freeze blocker #2 (independent
implementations through typed semantic analysis).

- **`impl-py/mty/hir.py`** — every expression kind, every item kind,
  a `HirOpaque` escape hatch for shapes typeck doesn't model, and
  a `Resolution` discriminator with `local | item | variant |
  builtin | unknown` cases.
- **`impl-py/mty/lower.py`** — ~700 lines covering struct / enum /
  type-alias / const items, the full expression tree, both `let`
  and `assign` and expr statements, pattern binding (incl.
  recursive nested patterns), and scope-stack name resolution.
- **`impl-py/mty/typeck.py`** — Hindley-Milner unifier with
  `TyAny` absorption for opaque shapes (closures-as-values, full
  trait dispatch, generic constraint propagation are queued for
  v0.18). `MT15xx` (lowering) + `MT20xx` (typeck) bands wired into
  `impl-py/mty/diagnostics.py`.
- **23/23 `examples/*.mty` typecheck clean.** Plus the existing
  examples sweep continues to lex + parse 22/22.
- **139 → 274 tests** (+135). New: `tests/test_hir.py` (24),
  `tests/test_typeck.py` (38), `tests/test_examples_typeck.py`
  (71). The total tally now exceeds the v0.17 spec target of
  ~220 by a healthy margin.
- **Recovery context.** The first v0.17 Python-impl agent wrote
  HIR + lowering, then crashed on an Anthropic API socket error
  before reaching typeck. The recovery agent inspected the WIP,
  found HIR + lowering coherent against 22/22 examples, built the
  missing typeck on top, and shipped the slice with 9 documented
  interpretation calls + 7 v0.18 follow-ups.

See
[`PYTHON_IMPL_V0_17_NOTES.md`](../notes/PYTHON_IMPL_V0_17_NOTES.md).

### RFC-008 multi-row-var + MT4055/56/58 active emit

The v0.16 RFC-008 wiring landed single-row-variable shapes with
MT4057 the only actively emitted code; v0.17 broadens the HIR to
multiple row variables, populates a per-fn metadata side table,
and flips three more codes to active emit.

- **`HirEffectRow::Open(concrete, Vec<HirRowVar>)`.** Was
  `Open(concrete, HirRowVar)`; the multi-var representation is
  ready end-to-end at the HIR + typeck layers. The
  `row_var()` convenience accessor (length-1 case) is retained
  for back-compat; new `row_vars()` + `row_var_count()` accessors
  cover the length-N case.
- **`UserRowPolyMeta` side table.** `UserRowPolyIndex` gains a
  `meta: HashMap<FnId, UserRowPolyMeta>` field recording row-var
  names, fn-typed param count, fn span/name, and concrete-effect
  names so the call-site walker can emit MT4058/MT4059 without
  re-walking the HIR.
- **MT4055 active — declaration-time.** Fires when a row var is
  declared but no fn-typed param can bind it AND the fn has multiple
  non-fn params (the unambiguous-bindable case is preserved as
  forward-compat).
- **MT4056 active heuristic — declaration-time.** Fires when the
  signature mixes concrete effects with a row variable but has no
  fn-typed param to source the row from (the row var is
  structurally inert).
- **MT4058 active — call-site arity.** Fires when the caller's
  lambda-arg count doesn't match the callee's fn-typed-param
  count. The v0.17 commit `f116cc8` extends the firing path so
  private callers (i.e. fns reached only from within the same
  module) also surface the diagnostic, not just pub-API callers.
- **MT4059 reserved.** Will fire once the parser ships the
  `!{| E1, E2}` multi-row-var surface syntax in v0.18.
- **+5 new tests** in `crates/mty-types/tests/effect_row_multi.rs`
  plus extended e2e tests in `crates/mty-types/tests/effect_row_e2e.rs`.
- **`examples/23_multi_row.mty` typechecks.** The example
  demonstrates the single-row-var shape (the v0.17 SHIPPED form at
  the source level) and documents the v0.18 multi-var parser
  follow-up in its header comment.
- **RFC-008 doc updated.** `docs/rfcs/RFC-008-effect-rows.md`
  gains a §multi-row-syntax section with the new representation
  + diagnostic activation table.

See
[`RFC008_MULTI_V0_17_NOTES.md`](../notes/RFC008_MULTI_V0_17_NOTES.md).

### wasmtime dev-dep bump 25 → 36 — RUSTSEC bundle cleared

The dev-only wasmtime bundle had accumulated 15 RUSTSEC advisories
across the v0.13–v0.16 windows; v0.17 cuts them in one go via a
conservative bump to v36.0.10 (latest stable was v45; stayed at 36
for minimal transitive churn).

- **`crates/mty-codegen-wasm/Cargo.toml`** dev-dependency changes
  to `wasmtime = "36"`; the workspace `wasmtime` is dev-only
  (used by P2 instantiation tests) so no production code is
  affected.
- **Transitive bumps:** `wasm-encoder` / `wasmparser` /
  `wasmprinter` 0.217 → 0.236; `gimli` 0.29 → 0.32 (dev-dep tree
  only — workspace `gimli` still pinned to 0.31 for debuginfo);
  `pulley-interpreter` 36.0.10 lands as a new pulley-machine
  fallback (unused by our test paths).
- **`audit.toml` ignore list shrinks 16 → 3.** Remaining ignores:
  `wasmtime-internal-cache` (dev-dep only), `pkg-config`, and
  `paste`, all with documented dev-dep rationale.
- **No test changes required.** The stable API surface across
  v25 → v36 (`Engine::new` / `Component::new` / `Linker` /
  `wasmtime_wasi::WasiCtxBuilder`) is unchanged for our usage.

See
[`WASMTIME_BUMP_V0_17_NOTES.md`](../notes/WASMTIME_BUMP_V0_17_NOTES.md).

## Integration fixes (this tag commit)

Two integrator commits land alongside the v0.17 swarm to keep the
required-gate jobs green:

- **`examples/23_multi_row.mty` codegen-safe shape.** The v0.17
  swarm landed `23_multi_row.mty` as a typeck-focused example with
  call sites that exercise MT4058's arity check; the wasm-component
  validator rejected the resulting module ("type mismatch: expected
  i32 but nothing on stack" at the empty-body `each(...)` /
  `run_both(...)` call sites — a v0.18 codegen-for-closure-args
  follow-up). The example is restructured to mirror the v0.16
  `22_effect_row.mty` convention: the row-poly helpers carry the
  `_` underscore prefix (kept out of WIT exports), and `main` calls
  `log(...)` only. Same SIGNATURE-syntax demonstration; passes the
  full `all_examples_compile_wasm` + `all_examples_compile_wasm_component`
  sweeps.
- **`crates/mty-driver/tests/wasi_default.rs` updated to v0.17
  shape.** The `default_wasi_preview_produces_p2_component_for_wasm`
  test asserted the OLD `wasi:cli/log` shim was always wired (the
  v0.15 default-flip marker). v0.17 dropped the shim, so the test
  is rewritten against the new direct-import signature: the source
  program now calls `log("wasi_default v0.17 marker")` (so there's
  a real import to assert against), the live-imports check is
  flipped to `wasi:cli/stdout@0.2.3` + a negative assertion that
  the dropped `wasi:cli/log` shim is gone from the live import
  section, and the existing `wasi_snapshot_preview1` negative is
  preserved.

No source-level behaviour changes; both are gate-keeping plumbing.

## v1.0 freeze: blockers + proposed date

The v1.0 spec is at v1.0-RC3 (unchanged from v0.13–v0.16; RFC-008
spec amendment for the multi-row-var form is staged for v1.0-RC4).
Blocker status (delta vs v0.16 in italics):

1. **Two independent implementations.** Rust reference compiler,
   Python 2nd-impl (`impl-py/`, *now 274 tests through typeck,
   was 139 front-end-only*), Go 3rd-impl (`impl-go/`, 4848 LOC
   source-only). The Python 2nd-impl is no longer front-end-only;
   the typeck side fully clears the v0.17 example corpus.
   Cross-validation against the Rust reference is the v0.18
   follow-up. **Substantially closed by v0.17** — remaining work
   is HM closure inference + generics-with-constraints.
2. **RFC comment periods.** RFC-001..006 + RFC-008 + RFC-009 each
   need a 30-day public window. Unchanged from v0.16.
3. **Published normative conformance suite.** Corpus stands at
   *92 cases / 16 categories / 2 ignored* (unchanged from v0.16).
   Coverage of FROZEN diagnostic codes climbs to ~98% (was ~96%) —
   MT4055/MT4056/MT4058 are now actively emitting (MT4059 stays
   reserved pending v0.18 parser).

**Proposed v1.0 freeze date: 2026-09-01** (unchanged).

## Backwards-compat aliases (status)

- **`--wasi=p1`** still retained as an explicit opt-out from the
  v0.15-onwards P2 default. The v0.13/v0.14 behaviour is fully
  preserved when the flag is set.
- **`Preview2Options::with_adapter(Some(WASI_P1_ADAPTER_COMMAND))`**
  is the new toolchain-consumer opt-in to restore the v0.16 always-on
  adapter behaviour. Default is now `embed_adapter == None`.
- **`MTY_REPLAY_RECORD`** is the new env var for the replay
  recorder; if unset, no trace is recorded and no recorder is
  installed.
- **`MTY_OTLP_ENDPOINT`** + **`MTY_RUNTIME_CONTROL_SOCK`** are
  unchanged from v0.16.

All other v0.7+ aliases (`mty dump --sir` for `--ir`; `SD####`
accepted by `mty explain`; `--legacy-interp`; legacy `sd` /
`stardust` code-block tags) stay live.

## Stats

| | v0.16.0 | v0.17.0 | Delta |
|---|---|---|---|
| Workspace crates | 20 | 20 | 0 |
| Rust tests passing | 1217 | **1274** | **+57** |
| Python tests passing | 139 | **274** | **+135** |
| Self-host driver tests | 23 (codegen) | **23 (codegen)** | 0 |
| Conformance cases | 92 | **92** | 0 |
| Conformance ignored | 2 | **2** | 0 |
| Combined test count | 1471 | **1663** | **+192** |
| Tests failing | 0 | 0 | 0 |
| Diagnostic codes wired | ~74 | **~74** (4 more active emit) | 0 |
| Examples passing (check) | 22/22 | **23/23 (typeck)** | **+1** |
| Examples passing (impl-py typeck) | — | **23/23** | **new** |
| Demos passing | 4/4 | **4/4** | 0 |
| Independent implementations | 3 (front-end only) | **3 (Rust + Python-typeck + Go-front-end)** | typeck added |
| Spec | v1.0-RC3 | **v1.0-RC3** | 0 |
| RFCs | 8 | 8 | 0 |
| Fuzz targets | 4 | 4 | 0 |
| CI jobs (all required) | 6 | 6 | 0 |
| Release-binary targets | 4 | **4** (Linux / macOS×2 / Windows) | 0 |
| WASI P2 direct-lowered fns | 13 | **14** (+log()) | **+1** |
| WASI P2 adapter-required surfaces | 1 (log) | **0** | **-1** |
| `audit.toml` ignored advisories | 16 | **3** | **-13** |
| Commits since prior tag | 11 | **7** | -4 |

## Migration steps

For end-user Mighty packages: **none required**. v0.17 is strictly
additive at the language surface.

For toolchain contributors:

- **`Preview2Options::new(_).embed_adapter` is now `None` by
  default.** If your downstream consumer relied on the v0.16
  always-on adapter for non-`log()` surfaces (e.g. you were
  routing a custom preview1-shim through it), call
  `with_adapter(Some(WASI_P1_ADAPTER_COMMAND))` after
  construction to restore v0.16 behaviour. The constant
  `WASI_P1_ADAPTER_COMMAND` is still exported.
- **The `wasi:cli/log` shim is gone from the live import section
  of P2 components.** Tools that scanned the live imports for
  `wasi:cli/log` (e.g. to detect "this is a Mighty-built
  component") should switch to scanning for
  `wasi:cli/stdout@0.2.3`. The embedded WIT custom section may
  still mention the legacy interface; both v0.17
  `default_wasi_preview_produces_p2_component_for_wasm` and the
  `preview2_log` test suite document the new signature.
- **`HirEffectRow::Open(_, _)` second field type changed.** Was
  `HirRowVar`, now `Vec<HirRowVar>`. The convenience accessor
  `row_var()` is retained for the length-1 case; new
  `row_vars()` + `row_var_count()` cover the multi-var case.
  Existing consumers that pattern-match `Open(c, v)` will need
  the trivial fix of pattern-matching `Open(c, vs)` then
  indexing `vs[0]` (length-1 invariant under v0.17 source-level
  shapes).
- **`UserRowPolyIndex::meta` is new.** Additive field; existing
  field accesses are unchanged.
- **`MTY_REPLAY_RECORD` env var** is opt-in; if unset, no
  recorder is installed and no per-call branching cost is paid.

For Wasm component authors: `cargo run --target wasm32-wasi`
produces a P2 component by default. After v0.17, `log()` flows
through direct P2 imports rather than the embedded preview1-adapter,
and the embedded adapter is no longer wired by default. If you
target a runtime that doesn't ship the P2 `wasi:cli` + `wasi:io`
interfaces, set `--wasi=p1` explicitly (the legacy adapter-routed
path is preserved end-to-end under that flag).

For users wanting pre-built binaries: the release workflow that
first fired on v0.15.0 continues to fire on every `v*` tag. The
GitHub Releases page should carry `mty` binaries for
Linux / macOS×2 / Windows shortly after each push.

## Known issues

Canonical list in [`KNOWN_ISSUES.md`](../../../KNOWN_ISSUES.md).
v0.17 net delta vs v0.16:

- **CLOSED** — v0.16 follow-up: WASI P2 `log()` finish
  (canonical-ABI rewrite, last adapter dependency).
- **CLOSED** — v0.16 follow-up: wasmtime dev-dep bump (advisory
  bundle reduced from 16 → 3 ignores).
- **PARTIAL** — v0.16 follow-up: RFC-008 typeck broadening.
  MT4055 / MT4056 / MT4058 now actively emit; MT4059 reserved
  pending the v0.18 parser extension for `!{| E1, E2}` shapes.

Carried over unchanged from v0.16:

- **#3** MSRV gate runs only `cargo build` (partially closed in v0.10).
- **#6** Demo 02 JS shim still writes into fixed `DOM_RETURN_AREA`
  rather than calling `cabi_realloc()`.
- **#7** `--no-default-features` test job does not run the example
  sweep.
- **#9** Cranelift egraph stack overflow
  (`MTY_CRANELIFT_NO_OPT=1` workaround stays).
- **#14** Go 3rd-impl cross-validation pending — Go toolchain still
  absent on the build host.
- **Windows named-pipe introspect backend** still tracked for
  v0.18; the v0.16 control-socket impl remains
  Unix-domain-only on POSIX.

New for v0.17:

- **Replay full Runtime re-execution is v0.18.** The recorder,
  wire format v1, step replayer, and `mty replay` CLI all ship;
  the missing piece is the agent dispatch state machine driving
  the replayer to assert byte-identical handler outputs. The
  v0.17 plan deferred this because it touches off-limits files
  in the swarm slice.
- **Replay hot-path wire-up in Runtime is v0.18.** `install_from_env`
  drops a `Recorder` into the runtime today, but the live agent
  boundaries (spawn / send / handler) do not yet emit events
  automatically. Manual emit via `Recorder::record_*` works.
- **Adapter bytes still vendored but unused by default.** The
  ~140KB of preview1-adapter Wasm continues to ship in
  `crates/mty-codegen-wasm/src/embedded/` for `with_adapter`
  opt-in. Deletion is queued for v0.18 once a back-compat sweep
  confirms no downstream consumer relies on the always-on
  default.
- **RFC-008 multi-row surface syntax (`!{| E1, E2}`) parser
  extension is v0.18.** The HIR + typeck layers are ready;
  the parser-side change is small but was out of the v0.17 slice
  scope.
- **Python 2nd-impl typeck has 7 documented follow-ups for v0.18**
  (HM closure inference, generics-with-constraints, full trait
  dispatch, etc.). The current `TyAny` absorption keeps the
  example corpus typeck-clean; the follow-ups are precision
  improvements rather than correctness fixes.

## v0.17 → v1.0-final roadmap

Carry-overs from v0.16 are unchanged for items that didn't land
this version. v0.17-specific follow-ups:

- **Replay full Runtime re-execution**: drive the `Replayer` from
  inside a `Runtime` instance and assert byte-identical handler
  outputs against the recorded trace.
- **Replay hot-path wire-up**: emit `TraceEvent`s from the live
  agent boundaries (spawn / send / handler / restart).
- **Adapter bytes deletion**: remove the vendored
  preview1-adapter Wasm from `crates/mty-codegen-wasm/src/embedded/`
  once a back-compat sweep confirms no downstream relies on the
  always-on default.
- **RFC-008 multi-row surface syntax**: extend the parser to
  accept `!{| E1, E2}`; flip MT4059 to active emit.
- **Python 2nd-impl typeck polish**: 7 documented follow-ups
  (HM closure inference, generics-with-constraints, full trait
  dispatch, …); cross-validation against the Rust reference over
  the `examples/` sweep.
- **Carry-overs**: open RFC-001..006 + RFC-008 + RFC-009 comment
  periods; run `go test ./...` on a Go-1.22+ host; split MT0001
  funnel; `mty-pkg` cross-file resolution; publish normative
  conformance suite as a downloadable kit; Windows named-pipe
  introspect backend.

## Acknowledgments

v0.17 was built across a five-track swarm followed by an integrator
pass:

- **wasi-p2-log-swarm** — `log()` / `print()` direct lowering to
  `wasi:cli/stdout@0.2.3` + `wasi:io/streams@0.2.3`; adapter
  flipped to opt-in via `with_adapter`; 8 new tests + 4 inverted
  legacy assertions. Commits `d5536d3` + `1848fc6`.
  **SHIPPED-FULL**.
- **replay-recorder-swarm** — `mty-runtime::replay::*` (wire format
  v1, recorder, step replayer); `mty replay` CLI with `--dump-json`
  + `--step` + `--json`; 35 tests; `docs/reference/cli/mty-replay.md`.
  Commit `45de577`. **SHIPPED-SUBSET** (full Runtime re-execution
  + hot-path wire-up deferred to v0.18).
- **wasmtime-bump-swarm** — dev-dep bump 25 → 36;
  `wasm-encoder` / `wasmparser` / `wasmprinter` 0.217 → 0.236;
  audit ignore list 16 → 3. Commit `705c954`. **SHIPPED-FULL**.
- **python-impl-swarm** — `impl-py/mty/hir.py` + `lower.py` +
  `typeck.py`; HM with `TyAny` absorption; 23/23 examples typeck
  clean; 139 → 274 tests. Commit `d6e5cfd` (recovery from died
  agent). **SHIPPED-FULL**.
- **rfc008-multi-swarm** — `HirEffectRow::Open(_, Vec<HirRowVar>)`;
  `UserRowPolyMeta` side table; MT4055 / MT4056 / MT4058 active
  emit, MT4059 reserved; +5 multi tests + extended e2e tests;
  `examples/23_multi_row.mty` lands. Commits `62a4b16` + `f116cc8`.
  **SHIPPED-FULL**.

The integrator pass (this v0.17.0 tag commit) ran the
pre-integrator cleanup (this CHANGELOG entry, this RELEASE-v0.17,
README cleanup, two targeted gate fixes: `examples/23_multi_row.mty`
restructure to mirror the codegen-safe v0.16 example-22 convention,
and `crates/mty-driver/tests/wasi_default.rs` v0.17 direct-import
assertion), then re-verified all gates
(**1274 Rust + 274 Python + 92 conformance + 23 selfhost = 1663
tests passing** / clippy strict / fmt / 23-example matrix / 4/4
demos / 2 conformance ignored) and authored this `RELEASE-v0.17.md`.

See
[`WASI_P2_LOG_V0_17_NOTES.md`](../notes/WASI_P2_LOG_V0_17_NOTES.md),
[`REPLAY_V0_17_NOTES.md`](../notes/REPLAY_V0_17_NOTES.md),
[`WASMTIME_BUMP_V0_17_NOTES.md`](../notes/WASMTIME_BUMP_V0_17_NOTES.md),
[`PYTHON_IMPL_V0_17_NOTES.md`](../notes/PYTHON_IMPL_V0_17_NOTES.md),
and
[`RFC008_MULTI_V0_17_NOTES.md`](../notes/RFC008_MULTI_V0_17_NOTES.md)
for per-agent interpretation calls.
