# v0.10 Polish notes — CI hardening agent

Status as of overnight v0.10 build. This file records the interpretation
calls the CI-hardening swarm agent made and the residue work that
follows from them. The scope split is documented in the swarm-agent
brief; this file covers Tasks 1-5 of that brief.

## Task 1 — MSRV testing

**Decision: bedrock subset.**

The MSRV gate previously ran `cargo build --workspace` only. v0.10
extends it to:

1. `cargo build --workspace` (unchanged — keeps non-test failures
   producing the original symbol names).
2. `cargo test --workspace --no-run` — compile-checks the test surface,
   which is where dev-deps silently bump the rust-version floor.
3. `cargo test -p mty-syntax -p mty-types -p mty-fmt -p mty-diagnostics`
   — actually executes the bedrock crates' tests. These four crates
   have no GPU / network / fs deps, run in well under a minute, and
   cover the most MSRV-sensitive surface (syntax, types, diagnostics).

Why not full `cargo test --workspace` on MSRV?

- Wall-clock budget. The cross-platform `test` matrix already does it
  on stable for all three OSes; repeating it on MSRV doubles CI cost
  for vanishingly small extra coverage.
- The crates we excluded (codegen, runtime, driver) pull in heavy
  dev-deps (criterion, insta, fixture corpora) that have historically
  been the first to drop MSRV support, but those are caught by the
  `--no-run` step above without paying the runtime cost.

Future: if a dev-dep update makes `cargo test --no-run` succeed on
MSRV but a non-bedrock crate's *runtime* tests fail there, we can
add the specific crate to the executed list. The bedrock subset is
the floor, not the ceiling.

## Task 2 — mkdocs --strict

**Decision: enabled with all warnings fixed.**

The v0.9 docs build emitted 55 strict-mode warnings:

- **Source-tree references** (~30 warnings). Internals docs linked to
  `../../crates/<crate>/src/<file>.rs`, which mkdocs can't resolve
  because the source tree isn't in `docs_dir`. Fix: rewrote every such
  link to `https://github.com/hassard0/Mighty/blob/main/crates/...`
  so they render as live source-tree links on the Pages site.
- **Stale root-of-repo notes** (~12 warnings). Files like `SLICE1.md`,
  `DEMOS_V0_4_NOTES.md`, `STDLIB_V0_2_NOTES.md`, `REBRAND_NOTES.md`
  exist in the repo root but live outside `docs_dir`. Same fix —
  rewrote to GitHub blob URLs.
- **Extension mismatches** (~6 warnings). `docs/tour/13-unsafe.md`
  and `docs/tour/11-budgets.md` linked to `examples/*.sd`; the
  repo's example files were renamed to `.mty` in the v0.7 rebrand
  but the docs were never updated. Fixed inline.
- **Nav-config issues** (2 warnings). `mkdocs.yml` listed `demos/
  README.md` (the file is named `index.md`) and `superpowers/README.
  md` (only `plans/` and `specs/` subdirs exist). Fixed the demos
  entry; dropped the superpowers entry — those are working artifacts,
  not user-facing docs, and mkdocs still picks them up via the auto
  walk.
- **Anchor-not-found informationals**. mkdocs 1.6 prints these as
  INFO (not WARNING) so they don't gate strict mode. They mostly come
  from forward-reference anchors in the v1.0-RC spec that haven't
  landed yet; deferred to whoever lands the spec changes.

Workflow change: `.github/workflows/pages.yml` now runs `mkdocs
build --strict`. Future PRs that introduce a broken link will fail
the Pages job.

## Task 3 — parallel monomorphization regression

**Decision: sequential stays the default; parallel stays opt-in,
fully documented.**

Re-ran the v0.8 bench, added two new fixtures (`xlarge_1024g` and
`large_256g_fat`) to probe whether the regression scales away or
holds up at extreme sizes. The numbers (Windows host, 4-worker
fan-out):

| fixture           | sequential | parallel | ratio |
|-------------------|-----------:|---------:|------:|
| small_4g          | 11 µs      | 12 µs    | 1.1x  |
| medium_32g        | 57 µs      | 459 µs   | 8.0x  |
| large_256g        | 377 µs     | 917 µs   | 2.4x  |
| xlarge_1024g      | 1.42 ms    | 1.98 ms  | 1.4x  |
| large_256g_fat    | 4.00 ms    | 4.71 ms  | 1.2x  |

Two findings:

1. The regression is *fundamental, not a scheduler bug*. Per-fn
   `specialize` work is bound by `Function::clone` + a single
   `concretize` walk (~1-2 µs at default width, ~16 µs in the fat
   variant). The chunked partition (each worker batches `ceil(N/W)`
   fns) already minimises spawn overhead, and we still can't recover
   the ~250 µs thread-spawn floor on Windows.
2. The break-even line is around *per-fn work > 1 ms*. That's the
   regime typeck-per-instantiation will live in (HIR walk, unification,
   constraint solve per call-site tuple), but it's well above
   anything mono does today.

Action taken:

- Updated the doc-comment on `Monomorphizer::run` with the v0.10
  numbers and the per-fn-cost break-even model.
- Left `run_parallel` callable so a future driver can flip the
  default once typeck-per-instantiation lands; until then `run()`
  always dispatches to `run_sequential`.
- Did not mark `run_parallel` `#[allow(dead_code)]` — the
  microbench (`typeck_parallel`) and the `parallel_matches_
  sequential` unit test both still exercise it, so it isn't actually
  dead.

## Task 4 — cargo audit security workflow

**Decision: shipped `.github/workflows/security.yml`.**

- Runs on push, pull_request, daily 07:00 UTC cron, and manual
  dispatch.
- Installs `cargo-audit` ^0.21, caches the binary keyed on that
  version pin.
- Invokes `cargo audit --deny warnings`. The default audit already
  fails on any vulnerability; `--deny warnings` additionally
  promotes unmaintained / yanked crates to errors (early-warning
  signals for downstream CVEs).
- If the noise becomes unmanageable, we can drop back to bare
  `cargo audit` and gate with explicit `--deny <kind>` flags.

## Task 5 — coverage report

**Decision: deferred to v0.10.x as an opt-in script.**

The brief flagged this as optional. Not adding a CI job for now:
`cargo llvm-cov` requires a one-off `cargo install` per runner
(~2 min cold), and the workspace's existing CI matrix is already
under time pressure on the MSRV addition above. Flagged as v0.10.x
follow-up — would prefer to ship as `scripts/coverage.sh` + a
manually-triggered workflow so the cron doesn't pay the cost on
every commit.

## v0.10.x follow-ups flagged by this agent

- Coverage script + manual-trigger workflow (Task 5 above).
- Anchor-not-found informationals in `docs/spec/v1.0-rc.md` — would
  need a forward-reference review when the v1.0 RC anchors stabilize.
- The `large_256g_fat` benchmark is currently the closest proxy we
  have to "real typeck work". Once typeck-per-instantiation lands
  (post v0.10), re-run the bench and reconsider whether `run()`
  should dispatch to `run_parallel` for programs above some
  generic-count threshold.

## Files modified by this agent

- `.github/workflows/ci.yml` — MSRV job extended.
- `.github/workflows/pages.yml` — `--strict` enabled.
- `.github/workflows/security.yml` — new, cargo-audit gate.
- `crates/mty-codegen-cranelift/src/mono.rs` — v0.10 doc-comment
  with re-bench numbers + break-even model.
- `crates/mty-codegen-cranelift/benches/typeck_parallel.rs` — added
  `xlarge_1024g` and `large_256g_fat` fixtures.
- `mkdocs.yml` — nav entries fixed.
- `docs/README.md`, `docs/contributing.md`, `docs/faq.md`,
  `docs/demos/index.md`, `docs/tour/11-budgets.md`,
  `docs/tour/13-unsafe.md`, `docs/tour/14-ownership.md`, and
  ~22 files under `docs/internals/`, `docs/reference/`, `docs/spec/`
  — every broken internal link rewritten to a GitHub blob URL or
  fixed inline.
