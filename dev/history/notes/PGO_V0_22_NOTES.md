# PGO + ThinLTO build profile — v0.22 notes

Implementation notes for the v0.22 PGO / ThinLTO build profile work.
For the user-facing how-to see `docs/internals/pgo.md`.

## Scope

- Add a new `release-pgo` cargo profile that inherits from `release`
  and turns up the dials worth paying for in a PGO build:
  `lto = "fat"`, `codegen-units = 1`, `debug = false`, `strip = true`.
- Provide a `scripts/build-pgo.sh` + `scripts/build-pgo.ps1` pair
  that drives the standard rustc PGO two-stage pipeline (generate →
  collect → merge → use) over the `mty-cli` binary.
- Add a tiny `crates/mty-bench/src/bin/mty-bench-pgo.rs` driver used
  during the profile-collection phase.
- Add a manual `.github/workflows/pgo-bench.yml` that runs the
  pipeline and writes a baseline-vs-PGO summary.
- Documentation: `docs/internals/pgo.md` + these notes.

**Out of scope**: gating releases on PGO, touching any crate source,
BOLT (deferred to v0.23).

## Implementation choices

### Profile inherits from `release`, not from scratch

Inheriting keeps the optimisation defaults (`opt-level = 3`,
`debug-assertions = false`, the inherited release-profile `panic`
strategy) consistent with the binary users actually run from
`cargo build --release`. The only deltas are the ones PGO actually
benefits from. Defining it from scratch would make it easy to drift
from the release profile and would obscure the *PGO* contribution
when comparing.

### Single-codegen-unit + fat LTO

`release` already pins `codegen-units = 1`. We keep it that way for
the PGO profile because:

- ThinLTO buckets functions across CGUs based on their summary; with
  a profile in hand, fat LTO is strictly better — it sees the whole
  program.
- `codegen-units = 1` makes the instrumented build's counter layout
  deterministic, which keeps the .profraw shards mergeable across
  reruns even if file ordering changes.

### `-Clinker-plugin-lto`

Added in Phase 4 (optimised rebuild) so the linker can cross-LTO
between rustc bitcode and any LLVM-built static libs in the dep
graph. This is the difference between "PGO-aware Rust" and "PGO-aware
final binary".

### llvm-profdata discovery

The script tries `llvm-profdata` on `PATH` first (developers with
system LLVM) and falls back to the rustup-managed copy at
`$(rustc --print sysroot)/lib/rustlib/<host>/bin/llvm-profdata`,
which is what `rustup component add llvm-tools-preview --toolchain
1.95.0` installs. The Windows variant always uses the toolchain-
bundled one — Windows almost never has a system LLVM on PATH.

If neither is available, the script aborts with the exact
`rustup component add` command, so first-time runs of the script
have a clear path to success.

### Why a `mty-bench-pgo` binary rather than `mty-bench-runner --pgo`

The existing `mty-bench-runner` is a criterion-style sampler with
percentile + JSON output. PGO doesn't care about percentiles; it
cares about exercising the same code paths the production user
exercises, and emitting `.profraw` files. Mixing the two would have
required new flags to suppress the JSON output, the iteration loop,
and the percentile math. Splitting them into a sibling binary keeps
each one tiny and single-purpose.

The PGO binary is **optional** — the bash/PowerShell scripts use it
opportunistically (only if it's been built in the `release-pgo`
target dir). The example sweep + the wasm build alone produce a
serviceable profile.

### `@typeck-pending` marker

The bash script grep-skips examples tagged `@typeck-pending`. As of
v0.21 nothing in `examples/*.mty` carries that marker, but the
pattern is the agreed-upon way to mark examples that aren't yet
typechecker-clean (see SELFHOST_V0_15_NOTES.md). Keeping the
filter in the script means future contributors can introduce gated
examples without breaking the PGO collection step.

### CI workflow gating

`pgo-bench.yml` is `workflow_dispatch:` only. Reasons:

- A full PGO build is ~3-4× the wall-clock of `cargo build --release`
  even with the cargo cache. Running it per-push would dominate the
  CI bill.
- The point of v0.22 is to *measure* PGO, not to commit to it for
  every release. Once we have stable numbers across hosts we can
  consider promoting it to a tag-only workflow.

The workflow uploads three artifacts: `target/mty-pgo` (the
optimised binary), `target/mty-baseline` (the release-profile
baseline used in the measurement), and the merged `.profdata`. The
profile is the most useful artifact for diagnosing under- or
over-fitting if the numbers come back odd.

## Acceptance check

- `cargo build --workspace` clean on default profile.
- `cargo build --profile release-pgo --workspace` clean without any
  RUSTFLAGS (i.e. the profile alone compiles).
- `cargo test --workspace` not regressed (no crate source touched).
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `cargo fmt --all -- --check` clean.

## Measured speedup

Target band stated in the slice spec: **12-20% wall-clock on `mty
check` + `mty build`**. Actual numbers will be populated by the
first CI run of `pgo-bench.yml`.

| host                    | mty check (baseline) | mty check (PGO) | Δ      | source                |
|-------------------------|----------------------|-----------------|--------|-----------------------|
| ubuntu-latest (GH CI)   | TBD                  | TBD             | TBD    | pgo-bench.yml run #1  |
| reference desktop       | TBD                  | TBD             | TBD    | local run             |

The honest expectation is that we land in the lower half of the
target band on `mty check` (it's already heavily branch-predicted)
and the upper half on `mty build` (codegen has more for the
optimiser to chew on). If we land outside the band we'll attach the
`.profdata` and dig into the trace.

## Files touched

- `Cargo.toml` — added the `release-pgo` profile.
- `scripts/build-pgo.sh` — new.
- `scripts/build-pgo.ps1` — new.
- `crates/mty-bench/Cargo.toml` — registered the new `[[bin]]`.
- `crates/mty-bench/src/bin/mty-bench-pgo.rs` — new.
- `.github/workflows/pgo-bench.yml` — new.
- `docs/internals/pgo.md` — new.
- `dev/history/notes/PGO_V0_22_NOTES.md` — this file.

No crate source under `crates/*/src/` was touched.

## v0.23 follow-up: BOLT

BOLT does a second post-link layout pass driven by hardware
counters and typically yields another 3-7% on top of a PGO build.
It's a heavier dependency (LLVM + perf + a static build of BOLT
itself) so the v0.22 slice stops at PGO. The v0.23 integration plan:

1. Add a `release-pgo-bolt` profile that inherits from
   `release-pgo` (no rustc flag change — BOLT is a post-link step).
2. Extend `build-pgo.sh` with a `Phase 6: bolt` that runs `perf
   record` against the PGO binary and feeds the trace into
   `llvm-bolt`.
3. Add a third row to the CI workflow's summary table.

This is captured here so v0.23 doesn't have to re-derive the plan.
