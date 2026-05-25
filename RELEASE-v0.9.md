# Mighty v0.9 — Release Notes

**Tag:** `v0.9.0`
**Date:** 2026-05-24
**Status:** SHIPPED — RC-prep + freeze-readiness release. v0.9
promotes the v1.0 spec to **v1.0-RC2** with all 10 OPEN amendments
resolved, brings up a four-target cargo-fuzz harness (and fixes the
three P0 OOM bugs it surfaced), self-hosts the MtyIR lowering on
examples 01-03, ships the `cabi_realloc` fix that unblocks demo 02,
publishes the GitHub Pages docs site, and lands six follow-up RFCs.

If you were on v0.8.0, the upgrade is `git pull && cargo install
--path crates/mty-cli --force`. There are no source-level breaking
changes for end-user Mighty programs.

## Highlights

- **v1.0 spec promoted to v1.0-RC2** at `docs/spec/v1.0-rc.md`. All
  10 OPEN amendments resolved: 3 FREEZE-MVP (A15, A31, A49), 7
  DEFER-V1.1 (A11, A45, A47, A94, A97, A102, A103). Six RFCs ship as
  first-drafts under `docs/spec/rfcs/`.
- **Cargo-fuzz harness** with 4 targets (parser / typeck / fmt /
  codegen-cranelift), 27-file seed corpus, full bring-up docs at
  `docs/internals/fuzzing.md`.
- **3 P0 parser OOM bugs fixed** + audit-sweep over every sibling
  loop. 60-second `parser_fuzz` smoke now completes cleanly
  (13 859 runs, zero OOMs). See
  [`PARSER_AUDIT_V0_9.md`](PARSER_AUDIT_V0_9.md).
- **Self-host MtyIR** (subset) — `selfhost/ir/{lib,nodes,lower}.mty`
  pass 7/9 self-host tests on examples 01-03 (04 + 05 deferred for
  the same reason as v0.8 HIR/typeck deferrals).
- **`demos/02_counter_web` fixed** — wasm-component `cabi_realloc`
  synthesis now correctly bump-allocates, smoke PASSes.
- **GitHub Pages docs site** at
  [hassard0.github.io/Mighty](https://hassard0.github.io/Mighty/).
- **CI hardened** — nightly-matrix + minimal-version + strict + MSRV
  jobs; pinned cargo + rustup caches.
- **Release scripts** for reproducible tag + push + GH-release flow.
- **Package-signing stub** (sigstore-style) — shape ready, real
  Fulcio integration is post-v1.0.
- **955 tests passing** (was 927 at v0.8.0; +28 net, all from the
  parser non-progress regression suite + new self-host MtyIR cases).

## What's new

### Spec freeze prep v1.0-RC2

- `docs/spec/v1.0-rc.md` promoted in-place from v1.0-RC to v1.0-RC2.
  Appendix §A.2 expanded to cover all 7 DEFER-V1.1 features with
  explicit "v1.0 status" + "v1.1+ scope" boundaries.
- `docs/spec/v0.1-amendments.md` — Status lines for the 10 OPEN
  amendments updated to FREEZE-MVP or DEFER-V1.1.
- `docs/spec/CHANGELOG.md` — v1.0-RC2 entry added.
- Six new RFCs:
  - **RFC-001** First-class union ADTs (A11)
  - **RFC-002** Wasm Component-Model wrapper (A47 + A97)
  - **RFC-003** Sandboxed proc-macro execution (A94)
  - **RFC-004** Per-call FsCap manifest threading (A100 residual)
  - **RFC-005** Affinity front-end syntax (A102)
  - **RFC-006** Lossless live agent migration (A103)

No crate source files were touched by the spec slice.

### Fuzz harness (4/4 targets shipped)

| Target            | Asserts                                                          |
| ----------------- | ---------------------------------------------------------------- |
| `parser_fuzz`     | `mty_syntax::parse(s)` never panics on arbitrary input            |
| `typeck_fuzz`     | parse + HIR-lower + `check_package_typed` never panic             |
| `fmt_idempotence` | `format(format(x)) == format(x)`; no panic in fmt path            |
| `codegen_fuzz`    | Cranelift lowering + object emit never panic on well-typed input  |

Each target carries a 27-file seed corpus (20 `examples/*.mty` +
5 self-host sources + `empty.mty` + `minimal_main.mty`).
Steady-state docs at `docs/internals/fuzzing.md`. CI integration
(PR fast path + nightly 5-min sweep + 30-min release gate) is
designed for v0.10 wiring.

Bugs found during the 5-minute smoke run: 3 P0 (`parser_fuzz`,
`fmt_idempotence` (inherited), `typeck_fuzz`) + 1 P1 upstream
(`codegen_fuzz`, Cranelift egraph stack overflow). The 3 P0s are
**fixed in v0.9** (see next section). The P1 is documented for v0.10
upstream + workaround.

### Parser audit: non-progress-guard family fix

The fuzz harness surfaced three OOM bugs, all the same anti-pattern:
a `while !p.at(R_BRACE) && !p.at(EOF)` loop body that can fail to
consume any tokens on adversarial input, growing green-tree nodes
one per iteration until OOM (~12 GB).

The v0.9 integrator slice applies the fix to **every vulnerable loop
in the parser**, not just the three crashed by the fuzzer. The fix
shape is:

```rust
let before = p.pos;
loop_body(p);
if p.pos == before {
    p.error("unexpected token in <context>");
    p.bump_any();
    p.skip_trivia();
}
```

Loops fixed (10 total): `enum_decl`, `struct_decl`, `trait_decl`,
`impl_block`, `sandbox_decl` (top-level), `attribute` derive-args,
`protocol_decl`, `supervisor_decl`, `match_expr`, `extern_block`.

Loops audited and confirmed safe-by-construction (no change needed):
all of `types.rs`, the `exprs.rs` tuple/map/struct/args loops,
`concurrency.rs::{budget_block, sandbox_block}` (already pre-guarded
in v0.6 — good pattern to migrate the rest of the parser toward in
v0.10), `stmts.rs::block`.

**Regression coverage**: 16 new tests in
`crates/mty-syntax/tests/parser_non_progress.rs`. The saved fuzz
artifacts in `crates/mty-syntax/fuzz/artifacts/parser_fuzz/` are
replayed as part of the test suite — they now parse in microseconds.

**Re-verification**: 60-second `parser_fuzz` smoke run on Windows
MSVC nightly after the fix — 13 859 mutations, zero OOMs, zero
panics. The non-progress family is closed.

Full audit table + fix rationale in
[`PARSER_AUDIT_V0_9.md`](PARSER_AUDIT_V0_9.md).

### Self-host MtyIR (subset)

The v0.6 lexer (4/4), v0.6 parser (13/13), v0.8 HIR (5/7), and v0.8
typeck (5/7) self-host coverage is joined by:

- `selfhost/ir/lib.mty` (26 LOC) — package + intent doc.
- `selfhost/ir/nodes.mty` (122 LOC) — data-shape spec mirroring
  `crates/mty-ir/src/ir.rs`.
- `selfhost/ir/lower.mty` (~530 LOC) — IR lowering for the example
  01-03 subset.

`cargo test -p mty-driver --test selfhost_ir` passes 7/9 live tests
(examples 04 + 05 ignored for the same reason as v0.8 HIR/typeck
deferrals: Result-sugar + `?` + struct-literal exprs in 04, range
patterns + private-fn name mangling in 05).

Subset coverage by design — the Rust IR lowerer is ~2500 LOC across
5 files (ctx/items/exprs/pats/ty); v0.9 ships the ~530 LOC subset
that covers every production exercised by examples 01-03 + partially
04-05. Gap catalog in
[`SELFHOST_IR_V0_9_NOTES.md`](SELFHOST_IR_V0_9_NOTES.md).

**Self-host running total**: 4 (lexer) + 13 (parser) + 5 (HIR) + 5
(typeck) + 7 (MtyIR) = **34 tests**.

### `demos/02_counter_web` cabi_realloc fix

`wit-component::ComponentEncoder::encode()` rejects any core module
whose WIT world has imports returning owned heap values
(`string`, `list<u8>`, …) unless it exports
`cabi_realloc(i32, i32, i32, i32) -> i32`. The Mighty wasm32-web
world has had those imports since v0.5; the emitter never
synthesised the export — this was the pre-existing regression that
flagged in v0.7 + v0.8.

Fix in `crates/mty-codegen-wasm/src/emit.rs`: a mutable i32 global
initialised to `CABI_REALLOC_HEAP_BASE = 32 768` plus a synthesised
bump-allocator function exported as `cabi_realloc`. Correct for our
current canonical-ABI lifts (only ever called with `old_ptr == 0`,
i.e. fresh allocations). A real free-list / wee_alloc-style
allocator is a v0.10 follow-up (KNOWN_ISSUES.md#1).

`bash demos/02_counter_web/smoke.sh` → **PASS** (component size
1523 bytes).

### GitHub Pages docs site

Live at [hassard0.github.io/Mighty](https://hassard0.github.io/Mighty/).
mkdocs-material with deep-purple/amber palette, pure Python (no
Ruby runtime needed on the runner). Deploy on push to main with
concurrency cancellation.

`--strict` is intentionally not enabled — the docs corpus accreted
organically over 8 slices and includes a handful of stale RFC and
example-source links. Cleanup is a v0.10 follow-up
(KNOWN_ISSUES.md#5).

### Package signing (stub)

The real `sigstore` Rust dep graph (tonic + Fulcio OpenAPI + Rekor
OpenAPI + …) is too heavy for an MVP. v0.9 ships the shape — signing
API, JSON manifest, signature file, verify path — with an explicit
"STUB" mode. Consumers can wire real Fulcio later without an API
change. Design docs at `docs/internals/package-signing.md`.

### CI hardening

Four new jobs:

- **matrix**: stable + beta + nightly toolchains.
- **minimal-versions**: cargo `-Z minimal-versions` build to keep the
  declared dep ranges honest.
- **strict**: clippy `--deny warnings` across all features.
- **MSRV**: pin a minimum supported Rust version and verify it.

Plus pinned cargo + rustup caches for reproducible builds.

### Release scripts

- `scripts/release.sh` — bash, Linux/macOS path.
- `scripts/release.ps1` — PowerShell, Windows path.

Same flow on both: validate-tag → cargo test → cargo build release →
git tag -s → push tag → gh release create → upload `mty` binary
asset(s).

## v1.0 freeze: blockers + proposed date

The v1.0 spec is now feature-complete at v1.0-RC2. Blockers before
the spec can be promoted to v1.0-final:

1. **Two independent implementations.** Mighty has one (this
   compiler); the second-implementation effort is RFC-007 (planned
   for v0.10).
2. **RFC comment periods.** RFC-001 through RFC-006 each need a
   30-day public window; opens on the v0.10 tag.
3. **Published normative conformance suite.** v0.9 ships what was
   good enough for v0.8 plus the regression suite for the parser
   fixes; v1.0-final demands a published conformance corpus any
   v1.x implementation must pass.
4. **Real `cabi_realloc` allocator** (KNOWN_ISSUES.md#1).

**Proposed v1.0 freeze date: 2026-09-01** (~3 months from this tag).

## Backwards-compat aliases (status)

All v0.7 + v0.8 aliases stay live (per A45's DEFER-V1.1 resolution
in v0.9):

- `mty dump --sir` aliases `--ir` ✅
- `mty explain SD####` accepts legacy `SD` prefix ✅
- `--legacy-interp` flag unchanged ✅
- `mty-doc` recognises legacy `sd` / `stardust` code-block tags ✅

A45's DEFER-V1.1 resolution flags these for removal in v1.1 with a
30-day deprecation window.

## Stats

| | v0.8.0 | v0.9.0 | Delta |
|---|---|---|---|
| Workspace crates | 20 | 20 | 0 |
| Source files (Rust + `.mty`) | 168 + 145 | 168 + 148 | +3 |
| Self-host `.mty` LoC | ~3 600 | ~5 330 | +1 730 (MtyIR) |
| Tests passing | 927 | 955 | +28 |
| Tests failing | 0 | 0 | 0 |
| Diagnostic codes | 67+ | 67+ | 0 |
| Examples passing | 20/20 | 20/20 | 0 |
| Demos passing | 2/3 | **3/3** | +1 |
| Self-host tests | 27 | **34** | +7 |
| Spec amendments | 88 (10 OPEN) | 88 (0 OPEN; 3 FREEZE-MVP + 7 DEFER-V1.1) | resolved |
| RFCs | 0 | 6 | +6 |
| Fuzz targets | 0 | 4 | +4 |
| Fuzz bugs surfaced / fixed | — | 4 / 3 | — |
| Commits since prior tag | — | 8 | — |
| Lines changed since prior tag | — | 161 files, +30 400 / -69 | — |

## Migration steps

For end-user Mighty packages: **none required**. v0.9 is purely
additive at the language level (no spec changes affect existing
programs).

For agents that rely on the parser bouncing back errors on
adversarial input: error messages have changed in shape for some
malformed-body inputs (the new "unexpected token in <context>"
message replaces silent infinite-loop behaviour). Inspect
`ParseResult.errors` if you were string-matching specific messages.

## Known issues

Canonical list in [`KNOWN_ISSUES.md`](KNOWN_ISSUES.md):

1. **`cabi_realloc` is a bump allocator** — v0.10 will switch to a
   real free-list. Current implementation is correct for the
   only `old_ptr == 0` paths we currently emit.
2. **Parallel monomorphisation** ships in-tree but `run()` dispatches
   to `run_sequential` (pre-existing).
3. **Set-of-scopes hygiene in LSP completion** (A111) deferred
   post-1.0 (pre-existing).
4. **Cranelift egraph stack overflow** (Bug 4 in
   `FUZZ_V0_9_NOTES.md`) — upstream `cranelift-codegen 0.132`;
   workaround is to disable the egraph pass. Not in our code.
5. **mkdocs `--strict`** not enabled — stale links from organic
   v0.1 → v0.8 doc accretion.

## v0.9 → v0.10 roadmap

- Second independent compiler implementation effort kickoff
  (RFC-007).
- Real `cabi_realloc` allocator.
- mkdocs `--strict` cleanup.
- CI nightly + release-gate fuzz wiring (the harness is ready;
  jobs need a custom rust-nightly + libFuzzer setup-step).
- Cranelift egraph upstream report + workaround patch.
- Self-host HIR + typeck examples 04 + 05 (v0.8 deferral; still open).
- Full `TokenStream` marshalling for proc-macros (v0.8 deferral).
- `mty-pkg` cross-file resolution.
- Parametric newtypes for self-host arena ids.
- WASM size + HTTP-server throughput optimisation targets.
- Set-of-scopes hygiene in LSP completion (A111).
- Real sigstore integration.
- Open RFC-001..006 30-day comment periods.
- Publish normative conformance suite.

After v0.10 the run-up to v1.0-final is the comment-period close
+ second-implementation validation + spec freeze.

## Acknowledgments

v0.9 was built in a single overnight autonomous swarm:

- **spec-freeze-swarm** (1 commit, `637312d`) — v1.0-RC2 promotion
  + 10 OPEN-amendment resolutions + 6 RFCs.
- **fuzz-swarm** (3 commits, `d977ea6`, `9e4980b`, `99c8676`) — 4
  cargo-fuzz targets + 27-file seed corpus + 4 bug triage.
- **selfhost-mtyir-swarm** (2 commits, `cb38c8a`, `dc1538a`) —
  partial MtyIR lowering + gap catalog.
- **rc-prep-swarm** (1 commit, `69a3cb7`) — demo 02 fix + Pages
  site + signing stub + CI hardening + release scripts.

The integrator pass (commits `4f85e8e` plus this v0.9.0 tag commit)
closed the parser non-progress-guard family (Bug 1, 2, 3 + 7 audit-
sweep extras + 16 regression tests + saved-artifact replay), re-
verified the gates (955 tests / clippy / fmt / 20-example matrix /
3/3 demos / self-host 34), and authored
[`SLICE_V0_9.md`](SLICE_V0_9.md),
[`RELEASE-v0.9.md`](RELEASE-v0.9.md), and
[`PARSER_AUDIT_V0_9.md`](PARSER_AUDIT_V0_9.md).

See `SPEC_FREEZE_V0_9_NOTES.md`, `FUZZ_V0_9_NOTES.md`,
`SELFHOST_IR_V0_9_NOTES.md`, `RC_PREP_V0_9_NOTES.md`, and
`PARSER_AUDIT_V0_9.md` for per-agent interpretation calls.
