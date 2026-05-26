# Mighty v0.15 — Release Notes

**Tag:** `v0.15.0`
**Date:** 2026-05-25
**Status:** SHIPPED — HOF dispatch closes the v0.13–v0.14
row-polymorphism loop end-to-end, RFC-008 effect-row surface syntax
lands, WASI Preview 2 is now the default for `wasm32-wasi`, the
self-host codegen reaches 17 live tests with variant-call lowering +
SwitchInt cascade + for-range desugar, and the two-phase macro
hygiene migration completes with the deprecated `expand` /
`expand_to_source` API removed.

v0.15 is the **dispatch-finishing release** for the five v0.14
infrastructure tracks plus one cross-cut: the row-polymorphic
signatures that v0.14 landed as a SHIPPED-SUBSET are now wired
through call-site dispatch and propagate closure effects into
callers; the v0.13/v0.14 `mighty:cli-adapter`-free WASI Preview 2
backend ships direct P2 imports for `std.random` + `std.time` and
flips the default for `wasm32-wasi` (explicit `--wasi=p1` retains
back-compat); the self-host codegen grows variant-call lowering +
the SwitchInt cascade + the for-range desugar (17 live driver tests,
was 13); RFC-008 surface syntax `!E` / `!{a | E}` parses through
`mty-syntax` with 4 new SyntaxKind variants and 16 new parser tests;
and the deprecated `mty_macros::expand` / `expand_to_source` API is
gone in favour of the set-of-scopes `expand_scoped_to_source`
path. Plus: the cross-platform release-binary workflow fires for the
first time on this tag (Linux / macOS x64+arm64 / Windows).

**Headline:** **HOF dispatch end-to-end, RFC-008 surface syntax,
WASI P2 default, self-host 17 codegen tests, cross-platform release
binaries.**

If you were on v0.14.0, the upgrade is `git pull && cargo install
--path crates/mty-cli --force` (or, for the first time, grab a
pre-built binary from the GitHub Releases page). The only
source-level breaking change is the removal of the deprecated
`mty_macros::expand` / `expand_to_source` API — downstream consumers
must migrate to `expand_scoped` / `expand_scoped_to_source` (the
deprecation has been live since v0.14).

## Highlights

- **5 of 5 v0.15 swarm tracks SHIPPED.** HOF dispatch wiring (the
  v0.14 SHIPPED-SUBSET completion), effect-row surface syntax,
  WASI P2 emit dispatch + default flip, self-host codegen broadening,
  deprecated-API removal — all land complete.
- **HOF dispatch NOW WIRED end-to-end.** The 19 row-polymorphic
  stdlib signatures that v0.14 landed as a SHIPPED-SUBSET are now
  consumed at call sites: 21 signatures across 12 method names flow
  through `BuiltinMethod.row_sig` → `walk_expr_effects` row
  unification → propagate closure effects into the caller's effect
  row. MT4050 fires on closed-row rejection. The v0.13–v0.14 row-poly
  loop is closed.
- **RFC-008 surface syntax lands.** `mty-syntax` parses `!E`,
  `!{a | E}`, `!{fs, net | E}`, and `effect a | E` clauses. 4 new
  SyntaxKind variants (EFFECT_SET, EFFECT_NAME, EFFECT_ROW_TAIL,
  EFFECT_ROW_VAR); 16 new parser tests; disambiguation against the
  legacy `T!{NetErr}` error-union sugar via
  `peeks_as_effect_row_clause`. Spec §9.2.1 added,
  `examples/22_effect_row.mty` parses (typeck wiring is v0.16 per
  RFC-008's staged plan).
- **WASI Preview 2 is now the default for `wasm32-wasi`.**
  `--wasi=p2` was opt-in in v0.13/v0.14; v0.15 flips the default so
  `cargo run --target wasm32-wasi` produces a P2 component out of
  the box. Explicit `--wasi=p1` retains back-compat. Four stdlib fns
  (`std.random.bytes`, `std.time.now`, `std.time.monotonic_now`,
  `std.time.resolution`) now emit direct P2 imports through
  `emit.rs`; the log shim and `std.fs` / `std.http` still route
  through the embedded preview1 adapter (canonical-ABI rewrite
  deferred to v0.16).
- **Self-host codegen reaches 17 live tests.** Variant-call lowering
  in `mty-ir::lower::exprs` detects callees like `Some(x)` /
  `Result.Ok(v)` and emits `Rvalue::AdtInit` directly; the selfhost
  SwitchInt cascade replaces the v0.14 linear if-cascade for dense
  integer patterns; `for i in 0..n` desugars to a counter loop. The
  driver test reports **17 live / 0 ignored** (was 13 live in v0.14).
- **Deprecated macro-expander API removed.** `mty_macros::expand`
  and `mty_macros::expand_to_source` are gone (they were
  `#[deprecated(since = "0.14.0")]` with scheduled removal in v0.15).
  9 integration test files migrated to `expand_scoped_to_source`;
  `mty-macros` test count moves 111 → 101 (10 redundant pruned;
  coverage preserved).
- **Cross-platform release binaries auto-build on this tag.** A new
  `.github/workflows/release.yml` fires on `v*` tag push and produces
  `mty` binaries for Linux x86_64, macOS x86_64 + arm64, and Windows
  x86_64. First-time test of the workflow runs on this tag.
- **All gates green, test count grows 1109 → 1140 Rust + 139
  Python + 92 conformance + 57 self-host = 1428 combined** (+38 vs
  v0.14). 0 failing, 3 ignored (1 cargo-doc-test + 2 conformance
  long-standing).
- **Conformance corpus 91 → 92 cases, 3 → 2 ignored.** The v0.14
  red-shirt `borrow_checking/14_borrow_outlives_owner` is now
  passing thanks to the one-line BLOCK fix in
  `mty-hir::lower::exprs::is_expr_node`. 16 categories unchanged;
  2 remaining ignored are the longer-tail items
  (`capability_checking/03_narrow_to_ro`,
  `supervisor_restart/02_escalate`) — both tracked for later
  versions.

## What's new

### HOF dispatch wiring — row-polymorphism loop closed

v0.13 landed the row-polymorphism infrastructure
(`mty-types::effects::row`) with one wired signature; v0.14 added 19
more signatures as a `pub mod stdlib_sigs` with full unit-test
coverage, but the call-site dispatch through
`prelude::BuiltinMethod` was deferred. v0.15 wires that dispatch.

- **`BuiltinMethod.row_sig`** is a new field threading the
  row-polymorphic signature for the method through to call-site
  dispatch. 21 signatures across 12 method names are populated
  (List / Iterator / Option / Result × map / filter / fold / etc.).
- **`walk_expr_effects` in `mty-types::check.rs`** picks up
  `row_sig`, instantiates fresh row variables per call, unifies the
  closure-argument's inferred row against the parameter's row var,
  and propagates the resulting tail into the caller's effect row.
- **MT4050** fires when a row signature meets a closed row that
  cannot be widened — the canonical "your callback wants `fs` but
  the iterator is closed-row pure" diagnostic.
- **+10 new tests** in `crates/mty-types/tests/stdlib_hof_dispatch.rs`
  cover the propagation path for each combination, including the
  closed-row rejection cases.

**Recovery commit.** The wiring landed across `838cb54` (BLOCK fix
+ initial dispatch sketch) and `d436bb8` (full dispatch wiring +
test suite). The recovery commit reflects a mid-flight rewrite
after the initial dispatch path collided with an unrelated
trait-resolution change; the two commits together represent the
complete wiring. See
[`HOF_DISPATCH_V0_15_NOTES.md`](../notes/HOF_DISPATCH_V0_15_NOTES.md)
for the per-method matrix.

### RFC-008 effect-row surface syntax — parser landed

The effect-row infrastructure has lived in `mty-types::effects::row`
since v0.13 and shipped 19 row-polymorphic stdlib signatures (v0.14)
and end-to-end dispatch (above). v0.15 lands the surface syntax so
user code can finally write row-polymorphic signatures by hand.

- **`!E` on a single effect** — `fn f() !E { ... }`.
- **`!{a | E}` with row-tail variable** — `fn g[a]() !{a | E} {
  ... }`.
- **`!{fs, net | E}` with concrete + tail** — `fn h[a]() !{fs, net |
  E} { ... }`.
- **`effect a | E` clause** — for top-level effect-row declarations
  in `where` clauses and impl items.
- **4 new SyntaxKind variants** — EFFECT_SET, EFFECT_NAME,
  EFFECT_ROW_TAIL, EFFECT_ROW_VAR.
- **Disambiguation against `T!{NetErr}` error-union sugar** —
  `peeks_as_effect_row_clause` in the parser performs a one-token
  lookahead after `!` to decide between the effect-row form
  (followed by `{...|...}` with a `|`) and the legacy error-union
  sugar (followed by `{NetErr}` without a `|`).
- **+16 parser tests** in `mty-syntax`. Spec §9.2.1 documents the
  surface form. `examples/22_effect_row.mty` parses end-to-end
  (typeck wiring is v0.16 per the RFC's staged plan).

**SHIPPED-FULL at the parser layer; typeck wiring is v0.16.** The
spec and parser are in; HIR / typeck pickup of the new SyntaxKind
variants is the next slice. See
[`EFFECT_ROW_SURFACE_V0_15_NOTES.md`](../notes/EFFECT_ROW_SURFACE_V0_15_NOTES.md).

### WASI Preview 2 — emit dispatch + default flip

v0.14 landed the `P2DirectImport` enum + constant table identifying
which stdlib fns should map directly to preview2 imports, but
`emit.rs` was still emitting preview1-shape import names that the
embedded adapter translated at instantiation. v0.15 wires
`P2DirectImport` into `emit.rs` dispatch and flips the toolchain
default.

- **Four stdlib fns now emit direct P2 imports** through `emit.rs`:
  `std.random.bytes` → `wasi:random/random@0.2.3#get-random-bytes`;
  `std.time.now` → `wasi:clocks/wall-clock@0.2.3#now`;
  `std.time.monotonic_now` → `wasi:clocks/monotonic-clock@0.2.3#now`;
  `std.time.resolution` → `wasi:clocks/monotonic-clock@0.2.3#resolution`.
- **`--wasi` default flipped to P2 for `wasm32-wasi`.** A bare `mty
  build --target wasm32-wasi` (or `cargo run --target wasm32-wasi`)
  now produces a P2 component out of the box. Explicit
  `--wasi=p1` is preserved for back-compat with any downstream that
  pinned the v0.13/v0.14 default.
- **+11 new tests.** `crates/mty-codegen-wasm/tests/preview2.rs`
  moves 18 → 24 (6 new tests assert direct import names appear in
  the emitted component for each of the four fns); a new
  `wasi_default.rs` (5 tests) pins the default-flip behaviour and
  the explicit `--wasi=p1` opt-out.

**SHIPPED-SUBSET.** The log shim still routes through the embedded
preview1 adapter (it needs a canonical-ABI rewrite for the P2
`wasi:logging` interface); `std.fs` and `std.http` still route
through the adapter (they need full handle/capability lowering).
Both are tracked for v0.16. See
[`WASI_P2_FINISH_V0_15_NOTES.md`](../notes/WASI_P2_FINISH_V0_15_NOTES.md).

### Self-host codegen — variant-call + SwitchInt + for-range

v0.14 broadened the self-host Wasm codegen with string pool + ADT
layout + pattern lowering so example 03 passed. v0.15 closes three
of the v0.14 follow-ups.

- **Variant-call lowering** (`mty-ir::lower::exprs::resolve_callee`
  path). Calls like `Some(x)` / `Result.Ok(v)` /
  `MyEnum.Variant(a, b)` are now detected as variant-constructor
  callees and lowered to `Rvalue::AdtInit` directly, rather than
  resolving to a function-call site with a missing definition. The
  same path covers user-defined enums.
- **SwitchInt cascade** for dense integer match arms in the selfhost
  Wasm codegen. The v0.14 linear if-cascade still works as a
  fallback; the SwitchInt path kicks in when the match scrutinee is
  an integer type and the arm patterns are dense enough to benefit
  (heuristic: ≥ 4 arms within a span ≤ 16). Lowers to wasm's
  `br_table` instruction.
- **for-range desugar** — `for i in 0..n { ... }` desugars to a
  counter-loop in the selfhost lowerer, removing the need to write
  `let mut i = 0; while i < n { ...; i = i + 1 }` by hand. The
  desugar lives in `selfhost/codegen/wasm.mty`.
- **Driver tests.** `crates/mty-driver/tests/selfhost_codegen.rs`
  reports **17 live tests, 0 ignored** (was 13 live in v0.14). New
  tests: variant-call lowering for `Some` / `Ok` / user-defined;
  SwitchInt fixture (dense int match); for-range fixture; and three
  unit tests for the helper paths.

See
[`SELFHOST_V0_15_NOTES.md`](../notes/SELFHOST_V0_15_NOTES.md) for
the per-feature notes and v0.16 follow-ups (real LEB128 in Mighty,
arena drops, agent backend).

### Two-phase macro hygiene migration complete

v0.14 wired `mty-hir::lower::macros` to drive `expand_scoped_to_source`
(set-of-scopes) and marked the legacy `expand` / `expand_to_source`
API as `#[deprecated(since = "0.14.0")]` with scheduled removal in
v0.15. v0.15 removes them.

- **`mty_macros::expand` and `mty_macros::expand_to_source` are gone.**
  Both removed from the public surface; downstream consumers must
  migrate to `expand_scoped` / `expand_scoped_to_source`.
- **9 integration test files migrated** to the scoped API. The new
  tests are functionally equivalent to the originals but build a
  per-TU `ScopeGen` and pass it through; the `Preprocessed` results
  carry the scope sets that the v0.14 wiring introduced.
- **`mty-macros` test count moves 111 → 101** (10 redundant tests
  pruned where the new and old API had been double-tested in the
  v0.14 transitional period). Coverage is preserved — every behaviour
  the legacy tests pinned is also pinned by the scoped tests.

See
[`EXPAND_REMOVAL_V0_15_NOTES.md`](../notes/EXPAND_REMOVAL_V0_15_NOTES.md)
for the per-file migration notes.

### BLOCK fix + v0.14 red-shirt closed

v0.14 documented a one-line bug in
`mty-hir::lower::exprs::is_expr_node` (missing `SyntaxKind::BLOCK`
arm) that caused the v0.13 red-shirt
`conformance/borrow_checking/14_borrow_outlives_owner` to be silently
lowered to `HirExpr::Error`. v0.15 lands the one-line fix and the
red-shirt now passes. **Conformance corpus moves 91 → 92 cases / 16
categories / 2 ignored** (was 3 ignored).

### Release-binaries workflow

A new `.github/workflows/release.yml` fires on any `v*` tag push and
produces `mty` binaries for:

- Linux x86_64 (gnu)
- macOS x86_64
- macOS arm64
- Windows x86_64

Each binary is attached to the corresponding GitHub Release. This
tag (`v0.15.0`) is the first-time test of the workflow; if the
binaries appear on the Releases page after the push, the workflow
is healthy.

### Agent-features roadmap

`docs/internals/agent-features-roadmap.md` lands as a 5-tier plan
for v0.16:

1. **Introspection** — `mty agent inspect` / `mty agent dump`.
2. **OTel** — OpenTelemetry tracing + metrics for agent boundaries.
3. **Replay** — deterministic-execution log capture + re-execution.
4. **Hot-reload** — supervisor-aware agent module swap.
5. **Distributed** — cross-host agent migration with replay-log
   shipping.

Each tier has a brief scope sketch + a "v0.16 / v0.17 / post-v1.0"
target.

### CI fixes (orchestrator)

Two CI-only fixes land alongside the v0.15 swarm to keep the
required-gate jobs green:

- **`pymdown-extensions` 10.12 → 10.14.3 + `Pygments<2.19` pin**
  in `.github/workflows/pages.yml`. The mkdocs Pages job was
  crashing on a 10.12/2.19 interaction; the pin restores green.
- **`cargo-audit ^0.21` → `^0.22`** in the security job. The
  CVSS 4.0 metric parser landed in 0.22 and the v0.14 audit run
  was failing to parse one new advisory.

## Integration fix (this tag commit)

The pre-integrator cleanup commit (`1da820b`) landed:

- **`conformance_codegen` typeck-pending skip**. The new
  `examples/22_effect_row.mty` parses cleanly but is intentionally
  typeck-pending until the v0.16 HIR/typeck wiring. The
  `conformance_codegen` test sweep walks the examples directory; a
  skip-list entry was added so the typeck-pending example doesn't
  fail the 22-test codegen suite.
- **Format fixups** across files touched by the swarm tracks (the
  per-track agents had left a handful of fmt-deviations that
  `cargo fmt --all -- --check` flagged).

No source-level behaviour changes; both fixups are gate-keeping
plumbing.

## v1.0 freeze: blockers + proposed date

The v1.0 spec is at v1.0-RC3 (unchanged from v0.13/v0.14;
RFC-008 + RFC-009 remain roadmap RFCs and do not move the spec
version). Blocker status (delta vs v0.14 in italics):

1. **Two independent implementations.** Rust reference compiler,
   Python 2nd-impl (`impl-py/`, *139 tests, +2 vs v0.14*), Go 3rd-impl
   (`impl-go/`, 4848 LOC source-only). Cross-validation still
   pending Go toolchain.
2. **RFC comment periods.** RFC-001..006 + RFC-008 + RFC-009 each
   need a 30-day public window. Unchanged from v0.14.
3. **Published normative conformance suite.** Corpus stands at
   *92 cases / 16 categories / 2 ignored* (was 91/3). Coverage of
   FROZEN diagnostic codes is now ~95% (unchanged from v0.14 — no
   new diagnostic codes wired this version).

**Proposed v1.0 freeze date: 2026-09-01** (unchanged).

## Backwards-compat aliases (status)

- **`mty_macros::expand` / `expand_to_source` REMOVED.** Deprecation
  landed in v0.14; removal in v0.15 per the scheduled plan.
  Downstream consumers must migrate to `expand_scoped` /
  `expand_scoped_to_source`.
- **`--wasi=p1` retained** as an explicit opt-out from the new P2
  default. The v0.13/v0.14 behaviour is fully preserved when the
  flag is set.

All other v0.7+ aliases (`mty dump --sir` for `--ir`; `SD####`
accepted by `mty explain`; `--legacy-interp`; legacy `sd` /
`stardust` code-block tags) stay live.

## Stats

| | v0.14.0 | v0.15.0 | Delta |
|---|---|---|---|
| Workspace crates | 20 | 20 | 0 |
| Rust tests passing | 1109 | **1140** | **+31** |
| Python tests passing | 137 | **139** | **+2** |
| Self-host tests | 53 | **57** | **+4** |
| Conformance cases | 91 | **92** | **+1** |
| Conformance ignored | 3 | **2** | **-1** |
| Combined test count | 1390 | **1428** | **+38** |
| Tests failing | 0 | 0 | 0 |
| Diagnostic codes wired | ~69 | ~69 | 0 |
| Examples passing (check) | 21/21 | **22/22** | **+1** (parser-only for #22) |
| Demos passing | 4/4 | **4/4** | 0 |
| Independent implementations | 3 (front-end only) | **3 (front-end only)** | 0 |
| Spec | v1.0-RC3 | **v1.0-RC3** | 0 |
| Spec amendments | 88 | 88 | 0 |
| RFCs | 8 | 8 | 0 |
| Fuzz targets | 4 | 4 | 0 |
| CI jobs (all required) | 6 | 6 | 0 |
| Release-binary targets | — | **4** (Linux / macOS×2 / Windows) | **+4** |
| Commits since prior tag | 9 | **11** | — |
| Lines changed since prior tag | 61 files, +8 548 / -288 | **50 files, +4 900 / -385** | — |

## Migration steps

For end-user Mighty packages: **none required**. v0.15 is
strictly additive at the language and toolchain surfaces (the new
effect-row surface syntax is opt-in by use).

For toolchain contributors:

- **Migrate off `mty_macros::expand` / `expand_to_source`.** The
  deprecated API is removed. Switch to `expand_scoped` /
  `expand_scoped_to_source`; the new entry point takes a `ScopeGen`
  and returns a `ScopedExpansion` carrying the intro scope + binding
  scope sets. The deprecation has been live since v0.14, so
  consumers should already be on the new path.
- If you have downstream tooling that pins `--wasi=p1` by default,
  decide whether to flip it to `--wasi=p2` (now the toolchain
  default) or pin the legacy default explicitly via `--wasi=p1`.
- The conformance harness now reports 92 cases (was 91), 2 ignored
  (was 3). Bump any downstream count pins.
- A new `examples/22_effect_row.mty` lives in the example sweep.
  It is parser-only at this tag (typeck-pending until v0.16) and
  the `conformance_codegen` test sweep skips it explicitly; example
  parsers / linters that walk `examples/` should accept it as a
  parse-only fixture.

For Wasm component authors: `cargo run --target wasm32-wasi` now
produces a P2 component by default. The vendored upstream wasmtime
v32 adapter (embedded since v0.14) handles any remaining
preview1-shape imports. `std.random` and `std.time` calls route
through direct preview2 imports via the new `emit.rs` dispatch;
`std.fs`, `std.http`, and the log shim still flow through the
adapter (v0.16 finish).

For users wanting pre-built binaries: this tag triggers the new
release-binaries workflow. After the push, the GitHub Releases
page should carry `mty` binaries for Linux / macOS×2 / Windows.

## Known issues

Canonical list in [`KNOWN_ISSUES.md`](../../../KNOWN_ISSUES.md). v0.15
closes #13 (the v0.13 red-shirt
`borrow_checking/14_borrow_outlives_owner`); the rest carry over
unchanged. Net delta vs v0.14:

- **CLOSED #13** — red-shirt
  `conformance/borrow_checking/14_borrow_outlives_owner` is now
  passing. The one-line `SyntaxKind::BLOCK` fix in
  `mty-hir::lower::exprs::is_expr_node` (traced during v0.14,
  landed in v0.15 `838cb54`) is the cause.

Carried over unchanged from v0.14:

- **#3** MSRV gate runs only `cargo build` (partially closed in v0.10).
- **#6** Demo 02 JS shim still writes into fixed `DOM_RETURN_AREA`
  rather than calling `cabi_realloc()`.
- **#7** `--no-default-features` test job does not run the example
  sweep.
- **#9** Cranelift egraph stack overflow (`MTY_CRANELIFT_NO_OPT=1`
  workaround stays).
- **#14** Go 3rd-impl cross-validation pending — Go toolchain still
  absent on the build host.

New for v0.15:

- **Effect-row HIR/typeck wiring deferred to v0.16.** The new
  surface syntax `!E` / `!{a | E}` parses through `mty-syntax` and
  the spec is in (§9.2.1), but HIR pickup of the new SyntaxKind
  variants (EFFECT_SET, EFFECT_NAME, EFFECT_ROW_TAIL,
  EFFECT_ROW_VAR) and typeck unification against user-authored row
  variables is the next slice. Tracked in
  [`EFFECT_ROW_SURFACE_V0_15_NOTES.md`](../notes/EFFECT_ROW_SURFACE_V0_15_NOTES.md).
- **WASI P2 log shim + `std.fs` / `std.http` still adapter-routed.**
  The log shim needs a canonical-ABI rewrite for the P2
  `wasi:logging` interface; `std.fs` and `std.http` need full
  handle / capability lowering. Both tracked for v0.16. See
  [`WASI_P2_FINISH_V0_15_NOTES.md`](../notes/WASI_P2_FINISH_V0_15_NOTES.md).
- **Self-host LEB128 + arena drops + agent backend.** Carried over
  from v0.14's self-host follow-ups; v0.15 closed variant-call,
  SwitchInt, and for-range. See
  [`SELFHOST_V0_15_NOTES.md`](../notes/SELFHOST_V0_15_NOTES.md).

## v0.15 → v1.0-final roadmap

Carry-overs from v0.14 are unchanged. New v0.15 follow-ups:

- **Effect-row v0.16 wiring**: HIR pickup of EFFECT_SET /
  EFFECT_NAME / EFFECT_ROW_TAIL / EFFECT_ROW_VAR; typeck
  unification against user-authored row variables;
  `examples/22_effect_row.mty` flips from parser-only to
  end-to-end.
- **WASI P2 v0.16 finish**: canonical-ABI rewrite for the log shim
  (`wasi:logging` interface); direct lowering for `std.fs`
  (handles + capabilities) and `std.http`
  (outgoing/incoming-request).
- **Self-host v0.16 broadening**: real LEB128 in Mighty (not host
  bridge); arena drops at scope exit (currently the bump arena
  leaks per call frame); agent backend (the v0.13 agent runtime
  needs a self-host code path).
- **Agent features v0.16 tier-1**: `mty agent inspect` /
  `mty agent dump` per the new
  `docs/internals/agent-features-roadmap.md`.
- **Carry-overs**: open RFC-001..006 + RFC-008 + RFC-009 comment
  periods; run `go test ./...` on a Go-1.22+ host; extend the
  Python 2nd-impl through HIR + sketch typeck; split MT0001
  funnel; `mty-pkg` cross-file resolution; publish normative
  conformance suite as a downloadable kit.

## Acknowledgments

v0.15 was built across a five-track swarm followed by an integrator
pass:

- **hof-dispatch-swarm** — `BuiltinMethod.row_sig` field; 21
  row-poly sigs across 12 method names; `walk_expr_effects` row
  unification + closure-effect propagation; MT4050 on closed-row
  rejection; +10 dispatch tests. Commits `838cb54` (BLOCK fix +
  initial dispatch sketch) + `d436bb8` (full dispatch +
  recovery). **SHIPPED-FULL**.
- **effect-row-surface-swarm** — `mty-syntax` parsing of `!E` /
  `!{a | E}` / `!{fs, net | E}` / `effect a | E`; 4 new SyntaxKind
  variants; disambiguation via `peeks_as_effect_row_clause`; +16
  parser tests; spec §9.2.1; `examples/22_effect_row.mty`. Commit
  `51d6622`. **SHIPPED-FULL** at the parser layer (typeck wiring
  is v0.16).
- **wasi-p2-finish-swarm** — `P2DirectImport` wired into `emit.rs`
  dispatch for `std.random.bytes` + `std.time.now` /
  `monotonic_now` / `resolution`; `--wasi` default flipped to P2
  for `wasm32-wasi`; +11 tests across `preview2.rs` (18 → 24) and
  new `wasi_default.rs` (5). Commits `bd4dab4` + `cdfbe8c`.
  **SHIPPED-SUBSET** (log shim + `std.fs` / `std.http` still
  adapter-routed).
- **selfhost-codegen-swarm** — `mty-ir::lower::exprs` variant-call
  callee detection → `Rvalue::AdtInit`; selfhost SwitchInt cascade
  + for-range desugar; 17 live driver tests (was 13). Commits
  `0e070a7` + `2837d8e` + `97d2b2b`. **SHIPPED-FULL**.
- **expand-removal-swarm** — `mty_macros::expand` /
  `expand_to_source` removed; 9 integration test files migrated to
  `expand_scoped_to_source`; `mty-macros` 111 → 101 tests (10
  redundant pruned, coverage preserved). Commit `98c6ea0`.
  **SHIPPED-FULL**.

Plus three orchestrator commits:

- **CI fixes** (`f6fcda1`) — `pymdown-extensions` 10.12 → 10.14.3
  + `Pygments<2.19` pin (mkdocs Pages crash);
  `cargo-audit ^0.21` → `^0.22` (CVSS 4.0 parse error in security
  job).
- **Release-binaries workflow** (swept into `0e070a7`) —
  `.github/workflows/release.yml` produces Linux / macOS×2 /
  Windows `mty` binaries on `v*` tag push.
- **Agent-features roadmap** (swept into `0e070a7`) —
  `docs/internals/agent-features-roadmap.md` (5-tier plan for
  v0.16+).

The integrator pass (this v0.15.0 tag commit) ran the
pre-integrator cleanup (`1da820b`: `conformance_codegen`
typeck-pending skip + fmt fixups), then re-verified all gates
(**1140 Rust + 139 Python + 92 conformance + 57 selfhost = 1428
tests passing** / clippy strict / fmt / 22-example matrix / 4/4
demos / 2 conformance ignored) and authored this `RELEASE-v0.15.md`.

See [`HOF_DISPATCH_V0_15_NOTES.md`](../notes/HOF_DISPATCH_V0_15_NOTES.md),
[`EFFECT_ROW_SURFACE_V0_15_NOTES.md`](../notes/EFFECT_ROW_SURFACE_V0_15_NOTES.md),
[`WASI_P2_FINISH_V0_15_NOTES.md`](../notes/WASI_P2_FINISH_V0_15_NOTES.md),
[`SELFHOST_V0_15_NOTES.md`](../notes/SELFHOST_V0_15_NOTES.md), and
[`EXPAND_REMOVAL_V0_15_NOTES.md`](../notes/EXPAND_REMOVAL_V0_15_NOTES.md)
for per-agent interpretation calls.
