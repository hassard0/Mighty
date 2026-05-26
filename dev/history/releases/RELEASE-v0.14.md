# Mighty v0.14 — Release Notes

**Tag:** `v0.14.0`
**Date:** 2026-05-25
**Status:** SHIPPED — WASI Preview 2 with vendored wasmtime adapter,
self-host codegen reaches example 03, set-of-scopes hygiene now
powers HIR macro resolution, two conformance gaps closed
(KNOWN_ISSUES #11 RESOLVED).

v0.14 is an **integration-and-finishing release** for the four v0.13
infrastructure tracks. The set-of-scopes layer (RFC-009) is now
wired into `mty-hir`; the WASI P2 backend embeds the upstream
wasmtime v32 preview1-adapter plus the full WASI 0.2.3 WIT surface
and ships direct P2 imports for `std.random` and `std.time`; the
self-host codegen grows string pool + ADT layout + pattern lowering
(example 03 now passes); and the typeck Gap-B emit-sites in
KNOWN_ISSUES #11 land (MT2003 + MT2023 with closure history table —
MT2009/2022/2024/2025 were rediscovered to already have emit-sites
from v0.12, so the table is fully accounted for). Row-polymorphism
relaxations land for 19 more stdlib HOFs as a SHIPPED-SUBSET — the
infrastructure and signatures ship, but the call-site dispatch that
consumes them is owned by `mty-types::check.rs` and routes through
`prelude::BuiltinMethod`, which is a separate cross-cut deferred to
v0.15.

**Headline:** **WASI Preview 2 with vendored wasmtime adapter,
self-host codegen reaches example 03, set-of-scopes hygiene now
powers HIR macro resolution, KNOWN_ISSUES #11 closed.**

If you were on v0.13.0, the upgrade is `git pull && cargo install
--path crates/mty-cli --force`. There are no source-level breaking
changes for end-user Mighty programs; the legacy
`mty_macros::expand` / `expand_to_source` API is now
`#[deprecated(since = "0.14.0")]` but still callable — scheduled
removal in v0.15.

## Highlights

- **4 of 5 v0.14 swarm tracks SHIPPED-FULL.** WASI P2 adapter embed
  + direct lowerings, self-host codegen broadening, macro-hygiene
  rewire, conformance Gap B closure all land complete. The fifth
  track (stdlib HOF row-polymorphism) ships its 19 new signatures
  as a usable subset; the call-site dispatch that consumes them is
  flagged for v0.15.
- **KNOWN_ISSUES #11 RESOLVED.** "Six FROZEN typeck codes still
  constructor-only" (MT2003, MT2009, MT2022, MT2023, MT2024,
  MT2025) — v0.14 lands the MT2003 + MT2023 emit-sites and
  rediscovers that MT2009/2022/2024/2025 already had emit-sites
  from v0.12 work; the KNOWN_ISSUES table was stale. Issue closed
  with a per-code closure-history table.
- **Vendored upstream WASI 0.2.3 surface.** `wit/wasi-p2/` now
  mirrors the upstream `wasi:*@0.2.3` interface set verbatim (not
  just the hand-rolled slice from v0.13). The wasmtime v32
  preview1→preview2 adapter binaries (command / reactor / proxy)
  are vendored under `crates/mty-codegen-wasm/wit/adapter/` and
  embedded into the emitted component, so the preview1 imports
  from `emit.rs` get translated at instantiation time and the v0.13
  `mighty:cli-adapter` shim no longer needs to exist.
- **Self-host codegen passes example 03.** `selfhost/codegen/wasm.mty`
  grew from ~400 LOC to ~660 LOC: string pool, ADT bump-alloc
  layout, pattern lowering. The driver test now reports **13 live
  / 0 ignored** (was 6 live + 1 ignored in v0.13). Examples 01 /
  02 / 03 plus the arith / option / pattern / string fixtures all
  compile through Mighty's self-host.
- **Set-of-scopes wired through mty-hir.** `mty-hir::lower::macros`
  switched from the legacy `expand_to_source` to the new
  `expand_scoped_to_source`. Per-translation-unit `ScopeGen` is
  threaded; `Preprocessed::macro_trace` exposes intro + bindings
  per expansion. Legacy `expand()` / `expand_to_source()` are now
  `#[deprecated(since = "0.14.0")]` with scheduled removal in v0.15.
  Closes the LSP completion / hygiene-in-anger gap (KNOWN_ISSUES
  #8).
- **Conformance corpus 89 → 91 cases.** Two new fixtures land
  (`type_checking/03_cannot_infer_type` for MT2003,
  `type_checking/21_generic_arg_kind_mismatch` for MT2023). 16
  categories unchanged; 3 ignored unchanged (red-shirt
  `14_borrow_outlives_owner` traced to a one-line bug in
  `mty-hir::lower::exprs::is_expr_node` — out of v0.14 swarm scope,
  documented and carried over).
- **All gates green, test count grows 1051 → 1109 Rust + 137
  Python + 91 conformance + 53 self-host = 1390 combined** (+67 vs
  v0.13). 0 failing, 4 ignored (3 conformance carryovers + 1
  cargo-doc-test).

## What's new

### WASI Preview 2 — vendored adapter + direct lowerings

The v0.13 P2 backend shipped behind `--wasi=p2` with a hand-rolled
WASI 0.2.3 slice and a small internal `mighty:cli-adapter` shim
that papered over the still-preview1-shape imports from `emit.rs`.
v0.14 closes that gap on both ends.

- **Vendored upstream wasmtime v32 adapter.**
  `crates/mty-codegen-wasm/wit/adapter/` now carries the upstream
  preview1→preview2 adapter binaries:
  `wasi_snapshot_preview1.command.wasm`,
  `wasi_snapshot_preview1.reactor.wasm`,
  `wasi_snapshot_preview1.proxy.wasm`. The P2 emitter embeds the
  adapter when wrapping the core module into a component, so the
  preview1-shape imports emitted by `emit.rs` are translated at
  instantiation time and the component runs unmodified on any
  preview2 host. The v0.13 `mighty:cli-adapter` shim is no longer
  needed.
- **Full upstream WASI 0.2.3 WIT surface.**
  `crates/mty-codegen-wasm/wit/wasi-p2/` now mirrors the upstream
  `wasi:*@0.2.3` interfaces verbatim — not just the hand-rolled
  slice v0.13 shipped. `wasi:cli`, `wasi:io`, `wasi:clocks`,
  `wasi:filesystem`, `wasi:http`, `wasi:random`, `wasi:sockets`
  (and dependencies) are all present in their upstream shape.
- **Direct P2 imports for `std.random` and `std.time`.** A new
  `P2DirectImport` enum + constant table in `preview2.rs` maps
  Mighty stdlib calls to their preview2 origins: `std.random.u64`
  → `wasi:random/random@0.2.3#get-random-u64`; `std.time.now_ms`
  → `wasi:clocks/wall-clock@0.2.3#now`. When emitted under
  `--wasi=p2`, these stdlib calls route directly to the real
  preview2 imports rather than through the preview1 adapter, which
  is more efficient and removes a layer.
- **`std.fs` / `std.http` still adapter-routed.** The direct
  lowering for filesystem and http is more involved (handles,
  capabilities) and lands in v0.15. Until then those calls flow
  through the embedded preview1 adapter.
- **Tests.** +9 new tests in
  `crates/mty-codegen-wasm/tests/preview2.rs` (now 9 → 18) cover
  the adapter embed (component decoding round-trip; `random` /
  `time` import-name assertions; adapter-binary presence assertion;
  P2DirectImport mapping table; absence of the v0.13
  `mighty:cli-adapter` shim).

**v0.15 follow-ups** (per
[`WASI_P2_LOWERINGS_V0_14_NOTES.md`](../notes/WASI_P2_LOWERINGS_V0_14_NOTES.md)):
wire `P2DirectImport` into `emit.rs` dispatch so the existing
preview1-shape import names get rewritten at codegen time (not
just identified); direct lowering for `std.fs` and `std.http`;
flip the toolchain default from `--wasi=p1` to `--wasi=p2`.

### Self-host codegen — string pool + ADT layout + pattern lowering

v0.13 closed the front-end-through-back-end self-host chain for the
slice-1 subset. v0.14 broadens it.

- **`selfhost/codegen/string_pool.mty`** (NEW). Deterministic
  string-literal pool: each literal is interned into the data
  section once, and `string-literal` expressions lower to a
  `(i32.const offset, i32.const len)` pair so the runtime can
  reconstruct a `&str` view without copying.
- **`selfhost/codegen/adt_layout.mty`** (NEW). Bump-allocated ADT
  layout: each variant gets a tag byte + the largest payload
  alignment; constructors lower to `cabi_realloc` + tag-write +
  field-store; field access lowers to tag-check + offset-load.
- **`selfhost/codegen/pattern.mty`** (NEW). Pattern lowering with
  tag-discrimination on the variant byte plus per-field
  destructuring. Wildcards collapse to a single arm; nested
  patterns recurse via the same data-section offsets as
  `adt_layout.mty`.
- **`selfhost/codegen/wasm.mty`** grew from ~400 LOC to ~660 LOC.
  The bootstrap chain (`lexer → parser → HIR → typeck → MtyIR →
  wasm`) now handles example 03 (`Option[T]` over generics) end-to-end.
- **Driver tests.** `crates/mty-driver/tests/selfhost_codegen.rs`
  reports **13 live tests, 0 ignored** (was 6 live + 1 ignored in
  v0.13). New: `selfhost_codegen_example_03`,
  `selfhost_codegen_pattern_fixture`, `selfhost_codegen_string_fixture`,
  `selfhost_codegen_option_fixture`, plus 3 unit tests for the new
  modules.

**v0.15 follow-ups** (per
[`SELFHOST_CODEGEN_V0_14_NOTES.md`](../notes/SELFHOST_CODEGEN_V0_14_NOTES.md)):
variant-call lowering (`Some(x)` / `Result.Ok(x)` as call exprs
rather than just constructors); the SwitchInt cascade for dense
integer patterns (v0.14 uses a linear if-cascade); a real LEB128
implementation in Mighty rather than the host bridge; arena drops
at scope exit (currently the bump arena leaks per call frame).

### Set-of-scopes wired through mty-hir

v0.13 landed the Flatt-style set-of-scopes layer in `mty-macros`
alongside the legacy mangling-based expander. v0.14 flips the
default consumer over.

- **`mty-hir::lower::macros`** switched from
  `mty_macros::expand_to_source` (legacy mangler) to
  `mty_macros::expand_scoped_to_source` (set-of-scopes). A
  per-translation-unit `ScopeGen` is threaded through the lowering
  context so every macro invocation in the unit gets its own
  unforgeable introduction scope.
- **`Preprocessed::macro_trace`** (NEW field). Per expansion the
  preprocessor now exposes `{ intro: ScopeId, bindings:
  Vec<(String, ScopeSet)> }` so downstream consumers (LSP,
  diagnostics) can inspect which scope a binding was introduced
  into. Closes the v0.12 known issue A111
  ("set-of-scopes hygiene in LSP completion").
- **Legacy expander deprecated.** `mty_macros::expand` and
  `mty_macros::expand_to_source` are now
  `#[deprecated(since = "0.14.0", note = "use expand_scoped /
  expand_scoped_to_source")]`. Both still work for v0.14 — removal
  scheduled for v0.15.
- **Tests.** +6 new e2e tests in
  `crates/mty-hir/tests/macro_hygiene_e2e.rs` covering: scope
  introduction at macro-call site, scope set inheritance through
  nested macros, capture-avoidance across macro boundaries, the
  `let`-binding scope-set growth, ambiguity-error propagation
  through HIR, and parity with the v0.13 macro-crate scoped tests.
  `mty-hir` test count moves 37 → 43.

### Stdlib HOF row-polymorphism — 19 new sigs

v0.13 landed the row-polymorphism infrastructure in
`mty-types::effects::row` plus one wired signature
(`stdlib_list_map_sig`). v0.14 lands 19 more row-polymorphic
signatures so the v0.15 call-site dispatch only has to wire dispatch
once.

- **`mty-types/src/effects.rs` extension.** A new `pub mod
  stdlib_sigs` module (+207 LOC) returns row-polymorphic
  signatures for:
  - `List`: `filter`, `fold`, `flat_map`
  - `Iterator`: `map`, `filter`, `fold`, `for_each`, `find`,
    `any`, `all`, `flat_map`, `collect`
  - `Option`: `map`, `and_then`, `or_else`, `filter`
  - `Result`: `map`, `map_err`, `and_then`, `or_else`
- **Tests.** +24 new unit tests in `effects::row_tests::
  stdlib_sig_01..24`. Each test instantiates the signature, picks
  a concrete row, and asserts the result row is the expected union.
  `mty-types` test count moves 67 → 84.

**SHIPPED-SUBSET — wiring blocker.** The actual call-site dispatch
that picks up these signatures runs through `mty-types::check.rs`
and routes through `prelude::BuiltinMethod`, which is owned by a
separate cross-cut (the prelude / method-resolution path). That
wiring is v0.15 scope; the v0.14 ship is the signatures + tests,
ready to be picked up. See
[`STDLIB_HOF_ROWPOLY_V0_14_NOTES.md`](../notes/STDLIB_HOF_ROWPOLY_V0_14_NOTES.md).

### Conformance gap B — KNOWN_ISSUES #11 RESOLVED

KNOWN_ISSUES #11 carried for six versions: "FROZEN typeck codes
MT2003 / MT2009 / MT2022 / MT2023 / MT2024 / MT2025 are
constructor-only with no emit-sites". v0.14 closes it.

- **MT2003 emit-site** (`mty-types::check.rs::check_stmt(HirStmt::
  Let)`). When a `let`-binding has no type annotation AND the
  initializer's synthesised type is an empty container whose
  element/payload type is a free inference variable, fire MT2003
  ("cannot infer type for binding `x`"). Carve-out: `let mut xs =
  []` is a legitimate idiom (downstream assignments will unify the
  element type) and is exempted, matching the v0.7+ stdlib usage
  patterns and the v0.12 `demos/04_kvstore` shape.
- **MT2023 emit-site** (`mty-types::check.rs::resolve_generic_args`).
  When a generic invocation passes an arg-kind that does not match
  the param-kind (lifetime where type expected, or vice-versa),
  fire MT2023 ("generic argument kind mismatch").
- **MT2009 / MT2022 / MT2024 / MT2025** — v0.14 audit rediscovered
  these already had emit-sites from v0.12 work (the KNOWN_ISSUES
  table had drifted). Each one has a fixture in the conformance
  corpus that exercises its emit-site.
- **Closure history table.** KNOWN_ISSUES #11 now carries a
  per-code closure history pointing at the commit that landed each
  emit-site:

  | Code | Closed in | Commit | Emit-site |
  |---|---|---|---|
  | MT2003 | v0.14 | `e5fb928` | `check.rs::check_stmt(HirStmt::Let)` |
  | MT2009 | v0.12 | (rediscovered) | `check.rs::synth_call` |
  | MT2022 | v0.12 | (rediscovered) | `check.rs::resolve_trait_method` |
  | MT2023 | v0.14 | `e5fb928` | `check.rs::resolve_generic_args` |
  | MT2024 | v0.12 | (rediscovered) | `check.rs::check_pat_struct` |
  | MT2025 | v0.12 | (rediscovered) | `borrow.rs::check_move` |

- **Conformance fixtures.** Two new fixtures land:
  `tests/conformance/type_checking/03_cannot_infer_type/` and
  `tests/conformance/type_checking/21_generic_arg_kind_mismatch/`.
  The corpus moves **89 → 91 cases** / 16 categories / 3 ignored.

**Red-shirt `14_borrow_outlives_owner` carry-over.** The v0.13
red-shirt was traced during v0.14 to a one-line bug in
`mty-hir::lower::exprs::is_expr_node` (missing
`SyntaxKind::BLOCK` arm). The inner `{ ... }` parses as
`EXPR_STMT > BLOCK` but the predicate doesn't recognise BLOCK as
an expression node, so the assignment is silently lowered to
`HirExpr::Error` and the borrow walker never sees the reassignment.
The fix lives in `mty-hir`, outside the v0.14 swarm scope; carried
to v0.15. See
[`CONFORMANCE_GAP_V0_14_NOTES.md`](../notes/CONFORMANCE_GAP_V0_14_NOTES.md).

## Integration fix (this tag commit)

While running the full demo sweep, the v0.14 integrator caught a
regression in `demos/04_kvstore`: the new MT2003 emit-site fired on
`let mut new_vals = []` inside the shard `Put` / `Del` handlers,
even though the next statement (`new_vals = new_vals.push(v)`)
unifies the element type. The v0.14 swarm's emit-site treated all
unannotated empty containers identically — but `mut` bindings get
their element type via subsequent assignment, so the eager emit was
wrong for the mutable case.

The integrator landed a one-flag carve-out in
`mty-types::check.rs::check_stmt(HirStmt::Let)`: skip MT2003 when
`mutable: true`. The non-mutable shape `let xs = []` still fires
MT2003 as intended (the conformance fixture
`type_checking/03_cannot_infer_type` validates this). A new
regression test
`mutable_empty_array_does_not_emit_mt2003` lives at
`crates/mty-types/tests/mt2003_cannot_infer.rs`.

## v1.0 freeze: blockers + proposed date

The v1.0 spec is at v1.0-RC3 (unchanged from v0.13). Blocker
status (delta vs v0.13 in italics):

1. **Two independent implementations.** Rust reference compiler,
   Python 2nd-impl (`impl-py/`, 137 tests), Go 3rd-impl
   (`impl-go/`, 4848 LOC source-only). Unchanged from v0.13.
2. **RFC comment periods.** RFC-001..006 + RFC-008 + RFC-009 each
   need a 30-day public window. Unchanged from v0.13.
3. **Published normative conformance suite.** Corpus stands at
   *91 cases / 16 categories / 3 ignored* (was 89). Coverage of
   FROZEN diagnostic codes is now ~95% (was ~92%) thanks to the
   MT2003 + MT2023 landings.

**Proposed v1.0 freeze date: 2026-09-01** (unchanged).

## Backwards-compat aliases (status)

Unchanged from v0.13. The legacy `mty_macros::expand` /
`expand_to_source` API stays callable behind a
`#[deprecated(since = "0.14.0")]` shim — removal scheduled for
v0.15. All other v0.7+ aliases (`mty dump --sir` for `--ir`;
`SD####` accepted by `mty explain`; `--legacy-interp`; legacy
`sd` / `stardust` code-block tags) stay live.

## Stats

| | v0.13.0 | v0.14.0 | Delta |
|---|---|---|---|
| Workspace crates | 20 | 20 | 0 |
| Rust tests passing | 1051 | **1109** | **+58** |
| Python tests passing | 137 | **137** | 0 |
| Self-host tests | 46 | **53** | **+7** |
| Conformance cases | 89 | **91** | **+2** |
| Conformance ignored | 3 | **3** | 0 |
| Combined test count | 1323 | **1390** | **+67** |
| Tests failing | 0 | 0 | 0 |
| Diagnostic codes wired | ~67 | **~69** | **+2** (MT2003 + MT2023) |
| Examples passing (check) | 21/21 | **21/21** | 0 |
| Demos passing | 4/4 | **4/4** | 0 |
| Independent implementations | 3 (front-end only) | **3 (front-end only)** | 0 |
| Spec | v1.0-RC3 | **v1.0-RC3** | 0 |
| Spec amendments | 88 | 88 | 0 |
| RFCs | 8 | 8 | 0 |
| Fuzz targets | 4 | 4 | 0 |
| CI jobs (all required) | 6 | 6 | 0 |
| Commits since prior tag | 5 | **9** | — |
| Lines changed since prior tag | 35 files, +7 151 / -13 | **61 files, +8 548 / -288** | — |

## Migration steps

For end-user Mighty packages: **none required**. v0.14 is strictly
additive at the language and toolchain surfaces.

For toolchain contributors:

- If you have downstream consumers calling `mty_macros::expand` or
  `mty_macros::expand_to_source`, migrate to `expand_scoped` /
  `expand_scoped_to_source` ahead of v0.15. The new entry point
  takes a `ScopeGen` and returns a `ScopedExpansion` carrying the
  intro scope + binding scope sets. The legacy API still works in
  v0.14 behind a deprecation warning.
- The conformance harness now reports 91 cases (was 89). If you
  have a downstream pinning the count, bump it.

For Wasm component authors: `--wasi=p2` is now adapter-embedded by
default. The generated component runs unmodified on any preview2
host; the v0.13 `mighty:cli-adapter` shim is gone. `std.random` and
`std.time` calls route directly to upstream preview2 imports;
`std.fs` and `std.http` still flow through the embedded adapter
(direct lowering is v0.15).

## Known issues

Canonical list in [`KNOWN_ISSUES.md`](../../../KNOWN_ISSUES.md). v0.14
closes #11 (six FROZEN typeck codes — full per-code closure-history
table now lives in the file); the rest carry over unchanged. Net
delta vs v0.13:

- **CLOSED #11** — six FROZEN typeck codes (MT2003 / MT2009 /
  MT2022 / MT2023 / MT2024 / MT2025) now all have emit-sites. v0.14
  landed MT2003 + MT2023 directly; v0.12 rediscovery covered the
  other four. See the closure history table above.
- **CLOSED A111** (LSP completion / set-of-scopes hygiene). The
  mty-hir rewire to `expand_scoped` brings the hygiene model into
  the LSP path. The legacy completion suite continues to pass.

Carried over unchanged from v0.13:

- **#3** MSRV gate runs only `cargo build` (partially closed in v0.10).
- **#6** Demo 02 JS shim still writes into fixed `DOM_RETURN_AREA`
  rather than calling `cabi_realloc()`.
- **#7** `--no-default-features` test job does not run the example
  sweep.
- **#9** Cranelift egraph stack overflow (`MTY_CRANELIFT_NO_OPT=1`
  workaround stays).
- **#13** Red-shirt
  `conformance/borrow_checking/14_borrow_outlives_owner` — traced
  during v0.14 to a one-line bug in
  `mty-hir::lower::exprs::is_expr_node` (missing `BLOCK` arm).
  Carry-over to v0.15.
- **#14** Go 3rd-impl cross-validation pending — Go toolchain still
  absent on the build host.
- **(NEW v0.13 #15)** WASI P2 `wasi:cli-adapter` shim — **resolved
  in v0.14** by the upstream wasmtime adapter embed. Removed from
  the carry-over list.

New for v0.14:

- **Effect-row stdlib HOF dispatch deferred.** The 19 new
  row-polymorphic signatures land in `mty-types::effects::stdlib_sigs`
  with full test coverage, but the call-site dispatch that consumes
  them (in `mty-types::check.rs` via `prelude::BuiltinMethod`) is
  owned by a separate cross-cut deferred to v0.15. Until then, the
  v0.13 concrete-effect stdlib HOFs remain the wired path. Tracked
  in [`STDLIB_HOF_ROWPOLY_V0_14_NOTES.md`](../notes/STDLIB_HOF_ROWPOLY_V0_14_NOTES.md).
- **Surface syntax for `!E`.** Still unimplemented in `mty-syntax`.
  Tracked for v0.15.

## v0.14 → v1.0-final roadmap

Carry-overs from v0.13 are unchanged. New v0.14 follow-ups:

- **Effect-row v0.15 wiring**: pick up the 19 row-polymorphic
  stdlib signatures via `mty-types::check.rs` call-site dispatch;
  surface-syntax parser for `!E` / `!{a | E}` in `mty-syntax`;
  MT4020-25 diagnostics.
- **WASI P2 v0.15 finish**: wire `P2DirectImport` into `emit.rs`
  dispatch so the preview1-shape import names get rewritten at
  codegen time (not just identified); direct lowering for
  `std.fs` and `std.http`; flip the toolchain default from
  `--wasi=p1` to `--wasi=p2`.
- **Self-host v0.15 broadening**: variant-call lowering (Some/Ok),
  SwitchInt cascade for dense patterns, real LEB128 in Mighty,
  arena drops at scope exit, agent backend.
- **mty-hir lower::exprs::is_expr_node fix**: add the `BLOCK` arm
  so `14_borrow_outlives_owner` flips from red-shirt to passing.
  One-line fix tracked for v0.15.
- **Legacy macro expander removal**: `mty_macros::expand` /
  `expand_to_source` deprecated in v0.14, scheduled removal in
  v0.15.
- **Carry-overs**: open RFC-001..006 + RFC-008 + RFC-009 comment
  periods; run `go test ./...` on a Go-1.22+ host; extend the
  Python 2nd-impl through HIR + sketch typeck; split MT0001
  funnel; `mty-pkg` cross-file resolution; publish normative
  conformance suite as a downloadable kit.

## Acknowledgments

v0.14 was built across a five-track swarm followed by an integrator
pass:

- **stdlib-hof-rowpoly-swarm** — 19 row-polymorphic stdlib signatures
  + 24 new tests + `pub mod stdlib_sigs` in
  `mty-types::effects.rs`. Commit `aaac11a`. **SHIPPED-SUBSET**
  (dispatch wiring deferred to v0.15).
- **macro-hygiene-wiring-swarm** — mty-hir consumes
  `expand_scoped_to_source`; per-TU `ScopeGen`; `macro_trace`
  field; legacy `expand` deprecated; +6 e2e tests. Commit
  `e97f77f`. **SHIPPED-FULL**.
- **selfhost-codegen-swarm** — `string_pool.mty` + `adt_layout.mty`
  + `pattern.mty`; `wasm.mty` grown ~400 → ~660 LOC; 13 live
  driver tests (was 6 live + 1 ignored). Commits `4615ad0` +
  `1fac9b3`. **SHIPPED-FULL**.
- **wasi-p2-lowerings-swarm** — wasmtime v32 preview1→preview2
  adapter embed (command / reactor / proxy); full upstream WASI
  0.2.3 WIT surface; P2DirectImport for `std.random` + `std.time`;
  +9 tests. Commits `47d2ef6` + `23a4a8b`. **SHIPPED-FULL**.
- **conformance-gap-b-swarm** — MT2003 + MT2023 emit-sites;
  rediscovery audit for MT2009/2022/2024/2025; KNOWN_ISSUES #11
  closed with closure-history table; +2 conformance fixtures.
  Commits `e5fb928` + `6711853`. **SHIPPED-FULL**.

The integrator pass (this v0.14.0 tag commit) caught one cross-cut
regression from the conformance-gap-b track — the MT2003 emit-site
fired on `let mut new_vals = []` in `demos/04_kvstore`, breaking
the kvstore smoke. Carve-out landed in
`mty-types::check.rs::check_stmt(HirStmt::Let)` to skip MT2003 on
`mutable: true`. A regression test
`mutable_empty_array_does_not_emit_mt2003` was added. The pass
then re-verified all gates (1109 Rust + 137 Python + 91 conformance
+ 53 selfhost = 1390 tests passing / clippy strict / fmt /
21-example matrix / 4/4 demos / 3 conformance ignored) and authored
this `RELEASE-v0.14.md`.

See [`STDLIB_HOF_ROWPOLY_V0_14_NOTES.md`](../notes/STDLIB_HOF_ROWPOLY_V0_14_NOTES.md),
[`MACRO_HYGIENE_WIRING_V0_14_NOTES.md`](../notes/MACRO_HYGIENE_WIRING_V0_14_NOTES.md),
[`SELFHOST_CODEGEN_V0_14_NOTES.md`](../notes/SELFHOST_CODEGEN_V0_14_NOTES.md),
[`WASI_P2_LOWERINGS_V0_14_NOTES.md`](../notes/WASI_P2_LOWERINGS_V0_14_NOTES.md), and
[`CONFORMANCE_GAP_V0_14_NOTES.md`](../notes/CONFORMANCE_GAP_V0_14_NOTES.md)
for per-agent interpretation calls.
