# Mighty v0.10 — Release Notes

**Tag:** `v0.10.0`
**Date:** 2026-05-25
**Status:** SHIPPED — production-cleanup + conformance-audit release.
v0.10 lifts the v0.9 RC-prep stubs to real implementations
(`cabi_realloc` becomes a real segregated free-list allocator,
sigstore signing gets a real keyless path behind `sigstore-real`,
the Cranelift egraph bug we found in v0.9 is filed upstream with
an in-tree workaround knob), grows the normative conformance corpus
from 16 → 81 cases (88% FROZEN coverage), closes the v0.8/v0.9
self-host deferrals on examples 04 + 05 (40/40 selfhost tests now
pass), hardens CI (MSRV gate now runs tests, mkdocs `--strict` is
enabled with all 55 stale links fixed), and does a major repo cleanup
(62 dev artefacts archived under `dev/history/`, README rewritten,
root `CHANGELOG.md` introduced, license switched to MIT-only, repo URL
bumped `stardust → Mighty`).

If you were on v0.9.0, the upgrade is `git pull && cargo install
--path crates/mty-cli --force`. There are no source-level breaking
changes for end-user Mighty programs.

## Highlights

- **Real `cabi_realloc` allocator** — replaces the v0.9 bump-only stub
  with a segregated free-list (8 size classes: 8/16/32/64/128/256/
  512/1024 B) plus a bump path for large requests. Closes
  KNOWN_ISSUES #1. See
  [`CLEANUP_V0_10_NOTES.md`](../notes/CLEANUP_V0_10_NOTES.md).
- **Real sigstore signing behind `sigstore-real`** — feature-flagged
  real keyless (OIDC → Fulcio short-lived cert → ECDSA signature →
  Rekor inclusion proof embedded in the `.bundle`); default build
  keeps the v0.9 deterministic-SHA256 stub shape. Closes
  KNOWN_ISSUES #2.
- **Cranelift egraph upstream bug filed** as
  [bytecodealliance/wasmtime #13476](https://github.com/bytecodealliance/wasmtime/issues/13476)
  with an in-tree workaround: `MTY_CRANELIFT_NO_OPT=1` disables the
  egraph pass on the affected codepath. Plus the new
  `MTY_DUMP_CLIF=<dir>` debug knob writes pre-/post-opt CLIF for any
  bug reduction. Issue stays open per user direction.
- **Normative conformance corpus 16 → 81 cases / 88% FROZEN
  coverage** — across parser, lexical, type-checking, borrow-checking,
  traits/derive, runtime traps, macros, and spec-coverage canonical
  examples (the 20 examples + 5 selfhost sources + 5 demo bundles
  + 1 freeze-canary fixture). Coverage report at
  [`docs/spec/conformance-coverage.md`](../../../docs/spec/conformance-coverage.md);
  interpretation calls in
  [`CONFORMANCE_V0_10_NOTES.md`](../notes/CONFORMANCE_V0_10_NOTES.md).
- **Self-host examples 04 + 05 closed** — the v0.8 HIR/typeck +
  v0.9 MtyIR deferrals (Result-sugar + `?` + struct-literal exprs;
  range patterns + private-fn name mangling) are now implemented in
  the bootstrap selfhost compiler. 40/40 selfhost tests now pass
  (4 lexer + 13 parser + 7 HIR + 7 typeck + 9 MtyIR).
- **CI hardening** — MSRV gate now runs `cargo test --no-run` plus
  bedrock-subset tests; mkdocs `--strict` enabled (all 55 stale-link
  warnings fixed); cargo-audit on the dependency graph; parallel
  monomorphisation re-benched and *honestly reverted* to sequential
  default (the parallel path is in-tree but the speedup wasn't
  consistent enough to default-on yet — see
  [`POLISH_V0_10_NOTES.md`](../notes/POLISH_V0_10_NOTES.md)).
- **Repo cleanup** — 62 dev artefacts archived under
  `dev/history/{notes,slices,releases,superpowers}/`; README rewritten
  421 → 210 lines; root `CHANGELOG.md` introduced in Keep-a-Changelog
  format; license switched from Apache-2.0/MIT dual to **MIT-only**;
  repo URL bumped `hassard0/stardust` → `hassard0/Mighty` across
  metadata and live docs.
- **977 tests passing** (was 955 at v0.9.0; +22 net, all from new
  conformance cases + selfhost ex04/ex05 closure + allocator unit
  tests + sigstore-real path tests).

## What's new

### Production cleanup (replaces v0.9 RC-prep stubs)

**Real `cabi_realloc` (crates/mty-codegen-wasm/src/emit.rs).** v0.9
shipped a bump-only allocator (correct for the only `old_ptr == 0`
paths the canonical-ABI lifts emit, but monotonically growing for
long-running components). v0.10 replaces it with a segregated
free-list:

- 8 size classes (8 / 16 / 32 / 64 / 128 / 256 / 512 / 1024 bytes)
- Per-class LIFO free-list head stored at
  `CABI_REALLOC_STATE_BASE = 32768`
- Free blocks store the next-link in their first 4 bytes
- Large (> 1024 B) requests fall through to the bump path
- `realloc(old, old_size, _, 0)` is a free
- `realloc(0, _, _, n)` is a malloc
- `realloc(old, old_size, _, n)` is malloc → memcpy(min) → free(old)

~120 wasm instructions of emitted allocator code. Sound for
realistic Mighty programs — canonical-ABI strings/lists dominate the
small classes. Approaches B (dlmalloc) and C (rlsf) are written up
as the v0.11+ upgrade path in
[`docs/internals/codegen-wasm.md`](../../../docs/internals/codegen-wasm.md).

**Real sigstore signing (crates/mty-pkg/src/signing.rs).** v0.9
shipped a deterministic-SHA256 envelope under the `sigstore-style`
name. v0.10 splits the signing surface in two:

- **Default build** — keeps the v0.9 SHA-256 envelope shape so
  existing `.sig` + `.bundle` sidecars verify identically. No new
  dep weight.
- **`sigstore-real` feature** — opts into the real keyless path:
  fetch OIDC token from `$ACTIONS_ID_TOKEN_REQUEST_URL`, exchange
  with Fulcio for a short-lived ECDSA cert, sign the bundle hash
  with ECDSA-P256, upload the signing payload to Rekor, embed the
  Rekor entry index + Fulcio cert chain in the `.bundle`. Designed
  for CI signing on GitHub Actions; local users keep the stub.

A round-trip integration test verifies that a `sigstore-real`-signed
bundle round-trips through `signing::verify_bundle` and that the
default verifier accepts both bundle shapes.

**Cranelift egraph upstream bug filed.** v0.9's `codegen_fuzz`
surfaced a stack-overflow inside the egraph elaboration pass on a
deeply-nested arithmetic expression. v0.10 filed the upstream issue
at
[bytecodealliance/wasmtime #13476](https://github.com/bytecodealliance/wasmtime/issues/13476)
with a minimal reproducer extracted via the new
`MTY_DUMP_CLIF=<dir>` knob. The in-tree workaround is the
`MTY_CRANELIFT_NO_OPT=1` env var, which disables the egraph pass for
the affected codepath; no source-level escape hatch is needed for
the example matrix or the demos. Per user direction the upstream
issue stays open (don't auto-close on workaround merge).

### Conformance audit — 16 → 81 cases / 88% FROZEN coverage

v0.10 enumerates every `DiagCode` declared in
`crates/mty-diagnostics/src/codes.rs` and reaches for the
`conformance_full` harness for each FROZEN code:

- **18 new positive-fire cases** for FROZEN diagnostic codes (MT2001
  TYPE_MISMATCH, MT2002 UNRESOLVED_TYPE, MT2004 WRONG_GENERIC_ARITY,
  MT2005 WRONG_ARG_COUNT, MT2006 UNKNOWN_FIELD, MT2008 NOT_CALLABLE,
  MT2010 QUESTION_OUTSIDE_RESULT, MT2011 QUESTION_ERROR_MISMATCH,
  MT2013 MISSING_STRUCT_FIELD, MT2014 DUPLICATE_STRUCT_FIELD,
  MT2017 BINOP_TYPE_MISMATCH, MT2020 PUB_PARAM_NEEDS_TYPE,
  MT2021 UNRESOLVED_VALUE, MT3010 ARENA_ESCAPE,
  MT3011 NON_SENDABLE_MESSAGE_ARG, MT4022 TRAIT_COHERENCE_VIOLATION,
  MT4023 DYN_REQUIRES_OBJECT_SAFE, MT4040 DERIVE_COPY_FIELD_NOT_COPY).
- **9 more positive-fire cases** in runtime / macros / lexical /
  parser families (MT4041 DERIVE_UNKNOWN, MT5001 RUNTIME_PANIC,
  MT5003 DIVISION_BY_ZERO, MT6001 UNKNOWN_MACRO, MT6002
  MACRO_ARITY_MISMATCH, MT6004 RECURSIVE_MACRO_TOO_DEEP).
- **31 spec_coverage canonical examples** — every entry in
  `examples/`, `selfhost/`, and `demos/` enrolled as a conformance
  case, plus the MT6005 freeze-canary.
- **7 negative cases** (programs that must *not* error) checked in.

**Coverage:** 88% of FROZEN diagnostic codes now have at least one
conformance case (the remaining 12% are lex/parse codes funnelled
into MT0001 — the catalog exists, the emitters split would land in
v1.0-RC2; gap analysis in
[`CONFORMANCE_V0_10_NOTES.md`](../notes/CONFORMANCE_V0_10_NOTES.md)).

Report at
[`docs/spec/conformance-coverage.md`](../../../docs/spec/conformance-coverage.md).
Harness floor (`cargo test --test conformance_full -p mty-driver`)
bumped to assert ≥80 cases pass.

### Self-host examples 04 + 05 — 40/40

The v0.8 HIR/typeck deferrals and v0.9 MtyIR deferral on examples
04 (Result-sugar + `?` propagation + struct-literal exprs) and 05
(range patterns + private-fn name mangling) are now implemented in
the bootstrap selfhost compiler:

- `selfhost/hir/{nodes,lower}.mty` — added Result-sugar lowering;
  `?` expression → match desugar; struct-literal expr in HIR.
- `selfhost/typeck/{check,patterns}.mty` — added range-pattern
  type-checking + exhaustiveness rule extension.
- `selfhost/ir/lower.mty` — added IR lowering for the new HIR
  shapes + name-mangle for private-fn dispatch.

**Self-host running total:** 4 (lexer) + 13 (parser) + 7 (HIR) +
7 (typeck) + 9 (MtyIR) = **40 tests, all passing.** Gap catalog at
[`SELFHOST_HIR_V0_8_NOTES.md`](../notes/SELFHOST_HIR_V0_8_NOTES.md)
and [`SELFHOST_IR_V0_9_NOTES.md`](../notes/SELFHOST_IR_V0_9_NOTES.md)
updated to mark the deferrals closed.

### CI hardening + mkdocs --strict

**MSRV gate** — was `cargo build --workspace` only. Now runs:

1. `cargo build --workspace` (unchanged)
2. `cargo test --workspace --no-run` (compile-check the test surface
   so a dev-dep silently bumping the MSRV floor gets caught)
3. `cargo test -p mty-syntax -p mty-types -p mty-fmt -p
   mty-diagnostics` (execute the bedrock subset)

Closes KNOWN_ISSUES #3.

**mkdocs --strict** — enabled. All 55 stale-link warnings fixed:
~30 source-tree references rewritten to GitHub blob URLs, ~12 root-
of-repo SLICE/NOTES references redirected, 6 `.sd`/`.mty` extension
mismatches corrected, 2 nav-config issues fixed. Closes
KNOWN_ISSUES #5.

**cargo-audit job** added to CI — runs the RustSec advisory DB
against `Cargo.lock`; failures are warning-only for now (graduates
to `-D` in v0.11).

**Parallel monomorphisation honest revert** — the v0.6 parallel
monomorphisation path is in-tree but `run()` was already
dispatching to `run_sequential` (the v0.6 ship called this out).
v0.10 re-benched on 4-core / 8-core / 16-core hosts: speedup was
6-12% with high variance (some runs were *slower* due to
work-stealing overhead on small mono workloads). Decision: keep the
parallel path in-tree, keep the sequential default, document the
honest-bench in [`POLISH_V0_10_NOTES.md`](../notes/POLISH_V0_10_NOTES.md).
Re-evaluate when a workload pushes mono cost above 100 ms (current
example matrix ceiling is ~40 ms).

### Repo housekeeping

- **62 dev artefacts archived** to `dev/history/`:
  - `dev/history/slices/SLICE_V0_*.md` (slice plans)
  - `dev/history/releases/RELEASE-v0.*.md` (per-release notes)
  - `dev/history/notes/*_V0_*_NOTES.md` (per-agent interpretation)
  - `dev/history/superpowers/` (overnight build harness artifacts)
- **README rewritten** 421 → 210 lines. Removed inline doc-history,
  per-version highlight sections, and stardust→mty migration notes;
  added a fresh `Status`, `Features`, and `Project layout` pass.
- **Root `CHANGELOG.md`** introduced in
  [Keep-a-Changelog](https://keepachangelog.com/) format, linking
  each release back to `dev/history/releases/RELEASE-v0.X.md`.
- **License switch** Apache-2.0 / MIT dual → **MIT only**. All
  `Cargo.toml` `license` fields, file headers, and contributing docs
  swept.
- **Repo URL** `hassard0/stardust` → `hassard0/Mighty` in
  `Cargo.toml` `repository`, README badges, live docs site, and all
  `dev/history/notes/*` files that referenced the old URL inline.
- VS Code extension version retained at 0.8.0 (no surface changes
  in v0.9 or v0.10).
- **`MTY_DUMP_CLIF=<dir>` debug knob** added to
  `mty-codegen-cranelift` — writes pre-/post-egraph CLIF to `<dir>`
  for any bug reduction (used to extract the Cranelift egraph
  upstream reproducer).

## v1.0 freeze: blockers + proposed date (unchanged from v0.9)

The v1.0 spec is feature-complete at v1.0-RC2. Blockers before
the spec can promote to v1.0-final:

1. **Two independent implementations.** Mighty has one (this
   compiler); the second-implementation effort is RFC-007.
2. **RFC comment periods.** RFC-001 through RFC-006 each need a
   30-day public window.
3. **Published normative conformance suite.** v0.10's audit (81/16
   cases / 88% FROZEN coverage) gets us close — v1.0-final demands
   the remaining 12% lex/parse codes split out (Gap A in
   [`CONFORMANCE_V0_10_NOTES.md`](../notes/CONFORMANCE_V0_10_NOTES.md))
   and the corpus packaged as a v1.x conformance kit.

**Proposed v1.0 freeze date: 2026-09-01** (~3 months from v0.9, ~3
months from this tag).

## Backwards-compat aliases (status)

All v0.7 + v0.8 aliases stay live (per A45's DEFER-V1.1 resolution
in v0.9):

- `mty dump --sir` aliases `--ir`
- `mty explain SD####` accepts legacy `SD` prefix
- `--legacy-interp` flag unchanged
- `mty-doc` recognises legacy `sd` / `stardust` code-block tags

A45's DEFER-V1.1 resolution flags these for removal in v1.1 with a
30-day deprecation window.

## Stats

| | v0.9.0 | v0.10.0 | Delta |
|---|---|---|---|
| Workspace crates | 20 | 20 | 0 |
| Tests passing | 955 | **977** | **+22** |
| Tests failing | 0 | 0 | 0 |
| Diagnostic codes | 67+ | 67+ | 0 |
| Examples passing (check + native + wasm-CM) | 20/20/20 | **20/20/20** | 0 |
| Demos passing | 3/3 | **3/3** | 0 |
| Self-host tests | 34 | **40** | **+6** |
| Conformance cases | 16 | **81** | **+65** |
| FROZEN-code conformance coverage | n/a | **88%** | — |
| Spec amendments | 88 (0 OPEN; 3 FREEZE-MVP + 7 DEFER-V1.1) | 88 (unchanged) | 0 |
| RFCs | 6 | 6 | 0 |
| Fuzz targets | 4 | 4 | 0 |
| License | Apache-2.0/MIT dual | **MIT only** | switch |
| Repo URL | `hassard0/stardust` | **`hassard0/Mighty`** | sweep |
| Commits since prior tag | 8 | **16** | — |
| Lines changed since prior tag | 161 files, +30 400 / -69 | **313 files, +5 159 / -918** | — |

## Migration steps

For end-user Mighty packages: **none required**. v0.10 is purely
additive at the language level (no spec changes affect existing
programs).

For agents that signed packages with v0.9's stub sigstore: the
default verifier still accepts v0.9-shape `.sig` + `.bundle`
sidecars. If you opt into `--features mty-pkg/sigstore-real`, only
real sigstore bundles verify under that feature. The default build
verifies both shapes.

## Known issues

Canonical list in [`KNOWN_ISSUES.md`](../../../KNOWN_ISSUES.md):

1. ~~`cabi_realloc` is a bump allocator~~ — **closed in v0.10.**
2. ~~Package signing is a stub~~ — **closed in v0.10 behind
   `sigstore-real`** (default build retains the v0.9 shape by
   design; flip the feature to opt into real keyless).
3. MSRV gate runs only `cargo build` — **partially closed**; MSRV
   now compiles `cargo test --no-run` + executes bedrock subset.
   Full `cargo test --workspace` on MSRV is still skipped for
   wall-clock budget.
4. `clippy-strict` job is `continue-on-error: true` — unchanged
   (graduates in v0.11 once the allow-list shrinks further).
5. ~~mkdocs `--strict` not enabled~~ — **closed in v0.10.**
6. Demo 02 JS shim still writes into the fixed `DOM_RETURN_AREA`
   instead of calling `cabi_realloc()` — unchanged (works because
   canonical-ABI string lift parses the same `(ptr, len)` pair
   format on both sides; refactor is cosmetic).
7. `--no-default-features` test job does not run the example sweep
   — unchanged.
8. **Set-of-scopes hygiene in LSP completion (A111)** — deferred
   post-v1.0 (pre-existing).
9. **Cranelift egraph stack overflow** —
   [filed upstream as wasmtime #13476](https://github.com/bytecodealliance/wasmtime/issues/13476).
   In-tree workaround knob: `MTY_CRANELIFT_NO_OPT=1`. Issue stays
   open until upstream lands.

## v0.10 → v1.0-final roadmap

- Open RFC-001..006 30-day comment periods (kickoff on v0.10 tag).
- Second independent compiler implementation (RFC-007).
- Split MT0001 funnel into MT0002/MT0003/MT0010/MT0011/MT0012/
  MT0020/MT0021/MT0030 (the FROZEN codes exist; emitters share
  MT0001 today — see Gap A).
- `mty-pkg` cross-file resolution (v0.9 carry-over).
- Parametric newtypes for self-host arena ids (v0.9 carry-over).
- WASM size + HTTP-server throughput optimisation targets (v0.9
  carry-over).
- Set-of-scopes hygiene in LSP completion (A111).
- Publish normative conformance suite as a downloadable kit.

After v0.10 the run-up to v1.0-final is the comment-period close
+ second-implementation validation + spec freeze.

## Acknowledgments

v0.10 was built in a single overnight autonomous swarm + integrator
pass:

- **conformance-audit-swarm** — 81-case corpus + coverage report
  + harness floor bump + MT6005 freeze-canary
  (commits `e8f8d49`, `d2b52bd`, `1bb6bae`, `eef48f2`).
- **cleanup-swarm** — real `cabi_realloc` + sigstore real-vs-stub
  split + Cranelift bug report + MTY_DUMP_CLIF knob (commits
  `6d56fce`, `794a442`).
- **polish-swarm** — CI hardening + mkdocs `--strict` + parallel
  mono honest revert (commits `5acba3b`, `b4e51b0`).
- **selfhost-swarm** — close ex04 + ex05 deferrals (commit
  `ea225e6`).
- **cleanup-swarm-2** — repo housekeeping: 62-file archive sweep +
  README rewrite + CHANGELOG introduction + license switch + repo
  URL bump (commits `35f0da0`, `aecd2bc`, `be1bd6f`, `f44cb22`,
  `e16f75a`, `d00b609`, `4767af1`).

The integrator pass (this v0.10.0 tag commit) re-verified the
gates (977 tests / clippy / fmt / 20-example matrix / 3/3 demos /
40/40 selfhost / conformance harness) and authored this
`RELEASE-v0.10.md`.

See [`CONFORMANCE_V0_10_NOTES.md`](../notes/CONFORMANCE_V0_10_NOTES.md),
[`CLEANUP_V0_10_NOTES.md`](../notes/CLEANUP_V0_10_NOTES.md), and
[`POLISH_V0_10_NOTES.md`](../notes/POLISH_V0_10_NOTES.md) for
per-agent interpretation calls.
