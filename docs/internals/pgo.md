# PGO + ThinLTO build profile (v0.22, cargo-pgo since v0.38)

Mighty's `release` build profile already runs ThinLTO with one codegen
unit, which is fine for day-to-day distribution. v0.22 added a second,
heavier profile — **`release-pgo`** — that combines **profile-guided
optimisation** with **fat LTO** for the `mty` binary specifically.

**v0.38 (T1): the CI pipeline now drives the
[`cargo-pgo`](https://github.com/Kobzol/cargo-pgo) crate** rather
than the in-tree shell scripts. cargo-pgo auto-discovers an
`llvm-profdata` that actually matches the active rustc, which closes
a real bug we hit on `aarch64-apple-darwin`: rustc 1.95.0 in that
channel emits raw=8 `.profraw` shards but the bundled `llvm-profdata`
in the same channel expects raw=10. A path-discovery fix in
`scripts/build-pgo.sh` couldn't help — the rustup-bundled tool was
already being picked correctly; the tools themselves were skewed
inside the rustup distribution. cargo-pgo handles that for us by
locating a profdata that the rustc actually wrote the profraws with.

The in-tree shell scripts (`scripts/build-pgo.sh` and
`scripts/build-pgo.ps1`) are still in the repo for **local dev** —
users who don't want to install cargo-pgo can keep using them on
platforms where the within-channel mismatch doesn't bite. CI uses
cargo-pgo; local dev can use either.

This page documents how the pipeline is wired, when to run it, and
what to expect from it.

## Why PGO

PGO records branch / call frequencies from a real run of an
instrumented binary and feeds those counts back into the optimiser
on a second build. The optimiser uses them to:

- inline hot calls and outline cold ones,
- lay basic blocks out for the actual branch direction,
- order functions in the text segment so hot code is page-adjacent,
- speculate on monomorphisations that turn out to be ubiquitous.

For a compiler like `mty` whose hot path is **lots of small,
hard-to-predict branches in the parser / typer / borrow checker /
codegen** the expected win is in the **12-20% wall-clock range** on
`mty check` and `mty build`. The exact number on your host depends on
your representative workload — that's what the profile-collection
phase is for.

ThinLTO is already on in `release`; the `release-pgo` profile
upgrades to **fat LTO** plus `-Clinker-plugin-lto`, which is the
heaviest layout `rustc` supports. Fat-LTO is significantly more
expensive (single-threaded link, no incrementalism) but the PGO
counts give the optimiser much more leverage on it than they do on
ThinLTO.

## Pipeline overview

```
┌─────────────────────────────────────────────────────────────────┐
│ Phase 0: clean target/pgo-profiles                              │
├─────────────────────────────────────────────────────────────────┤
│ Phase 1: instrumented build                                     │
│   RUSTFLAGS=-Cprofile-generate=$PROFDIR                         │
│   cargo +1.95.0 build --profile release-pgo -p mty-cli          │
├─────────────────────────────────────────────────────────────────┤
│ Phase 2: profile collection (writes $PROFDIR/*.profraw)         │
│   target/release-pgo/mty check examples/*.mty                   │
│   target/release-pgo/mty build examples/01_hello.mty            │
│       --target wasm32-wasi                                      │
│   target/release-pgo/mty-bench-pgo --quick   (if present)       │
├─────────────────────────────────────────────────────────────────┤
│ Phase 3: merge .profraw → merged.profdata                       │
│   llvm-profdata merge -o $PROFDIR/merged.profdata               │
│                       $PROFDIR/*.profraw                        │
├─────────────────────────────────────────────────────────────────┤
│ Phase 4: optimised rebuild                                      │
│   RUSTFLAGS=-Cprofile-use=$PROFDIR/merged.profdata              │
│              -Clinker-plugin-lto                                │
│   cargo +1.95.0 build --profile release-pgo -p mty-cli          │
├─────────────────────────────────────────────────────────────────┤
│ Phase 5: copy artifact → target/mty-pgo                         │
└─────────────────────────────────────────────────────────────────┘
```

## How to run it (CI: cargo-pgo)

`.github/workflows/release.yml` runs the cargo-pgo pipeline on the
three native-PGO platforms (linux-x86_64, darwin-arm64,
windows-x86_64). The matrix entries with `use_pgo: true` execute:

```bash
# 1. Install cargo-pgo (pinned to 0.2.9 in release.yml).
cargo install cargo-pgo --version 0.2.9 --locked

# 2. Instrumented build. cargo-pgo manages the rustflags + finds
#    a matching llvm-profdata on its own.
cargo pgo build -- \
  --profile release-pgo \
  --bin mty -p mty-cli \
  --target <triple>

# 3. Training corpus. Sweep `mty check` over examples/*.mty plus
#    one wasm32-wasi build. Profraws land under target/pgo-profiles.
for f in examples/*.mty; do
  target/<triple>/release-pgo/mty check "$f" 2>&1 | tail -2 || true
done

# 4. Optimised rebuild.
cargo pgo optimize build -- \
  --profile release-pgo \
  --bin mty -p mty-cli \
  --target <triple>
```

cargo-pgo writes the final optimised binary to
`target/<triple>/release-pgo/mty`. The release.yml staging step
copies it into a stable `staged/` directory regardless of which
build path produced it.

### Why cargo-pgo (and not the shell scripts) for CI

v0.37 hit a real bug on darwin-arm64: rustc 1.95.0 emitted raw=8
profraw shards but the bundled `llvm-profdata` in the **same**
rustup toolchain channel expected raw=10. The manual scripts
couldn't fix a within-channel mismatch — every path-discovery tweak
still landed on the wrong tool. cargo-pgo solves this because it
discovers an `llvm-profdata` that the rustc actually wrote the
profraws with, not the one rustup happened to install. CI calls the
tool, gets a working build, and stops carrying a per-platform path
heuristic. The Linux CI step
`scripts/tests/test-cargo-pgo-availability.sh` asserts the rustup
toolchain's llvm-tools-preview matches rustc's LLVM major version
so a future skew fails fast.

## How to run it (local dev: shell scripts)

The `scripts/build-pgo.{sh,ps1}` scripts are preserved for local-dev
ergonomics — installing cargo-pgo is a 1-2 minute first-time cost,
and on Linux + Windows the within-channel mismatch isn't a problem
in practice. Use whichever you prefer.

### Linux / macOS

```bash
# One-shot:
./scripts/build-pgo.sh

# Custom profile dir + toolchain:
PROFDIR=/tmp/pgo TOOLCHAIN=1.95.0 ./scripts/build-pgo.sh
```

The script prefers the rustup-managed copy at

```
$(rustc +1.95.0 --print sysroot)/lib/rustlib/<host>/bin/llvm-profdata
```

— that's what `rustup component add llvm-tools-preview --toolchain
1.95.0` installs. If the host tuple doesn't have it, the script
falls back through a small chain — `aarch64-apple-darwin` →
`x86_64-apple-darwin` → a wildcard scan of every `lib/rustlib/*/bin`
directory — and finally a system `llvm-profdata` on `PATH`. The
fallback chain exists because v0.36.1 hit a macOS-14 layout where
`llvm-tools-preview` landed under a non-host tuple's `bin/`; the v0.37
chain re-enabled darwin-arm64 PGO without needing the system LLVM
(see `scripts/tests/test-build-pgo-paths.sh`). The script errors out
with a clear message if nothing matches.

On `aarch64-apple-darwin` the within-channel raw=8/expected=10 skew
will trip the script. If you're on that platform, use `cargo pgo`
locally instead (or stick to a non-PGO `--release` build).

### Windows

```powershell
# One-shot:
./scripts/build-pgo.ps1

# Custom profile dir + toolchain:
./scripts/build-pgo.ps1 -ProfDir C:\tmp\pgo -Toolchain 1.95.0
```

Windows does **not** ship a system `llvm-profdata` by default, so the
PowerShell variant always uses the toolchain-bundled one. Install it
with:

```powershell
rustup component add llvm-tools-preview --toolchain 1.95.0
```

If the component is missing, the script errors with the exact
install command.

## Profile

The `release-pgo` profile is defined in the workspace `Cargo.toml`:

```toml
[profile.release-pgo]
inherits = "release"
lto = "fat"
codegen-units = 1
debug = false
strip = true
```

Two things to note:

- Building with this profile **without** the PGO flags works fine
  (`cargo build --profile release-pgo -p mty-cli`) — you just won't
  see the profile-guided wins.
- `cargo test --profile release-pgo` is supported but very slow
  (fat LTO every link). Use `--release` for normal test runs.

## `mty-bench-pgo`

`crates/mty-bench/src/bin/mty-bench-pgo.rs` is a thin sequencer that
the profile-collection phase calls when present. It runs an
in-process parse sweep over the bundled examples + a synthetic
wasm32-wasi compile. Two modes:

- `--quick` (default): ~tens of seconds; first 12 examples + a 5-unit
  synthetic compile.
- `--full`: walks every example + a 25-unit synthetic compile.
  Roughly 4-5× the wall-clock of `--quick`.

The script tolerates the binary being absent (it's an extra crate
build) — the example sweep + the wasm build are enough on their own
for a representative profile.

## GitHub Actions workflow

`.github/workflows/pgo-bench.yml` runs the full pipeline on
`workflow_dispatch`. It:

1. Installs `llvm-tools-preview`.
2. Builds the baseline `target/release/mty`.
3. Runs `scripts/build-pgo.sh` to produce `target/mty-pgo`.
4. Measures `mty check examples/01_hello.mty` against both binaries
   over N iterations (default 10).
5. Writes the delta as a workflow-summary block and uploads both
   binaries + the merged profile as an artifact.

It is **not** wired into `release.yml` yet — the v0.22 release ships
the manual workflow so we can collect numbers across machines before
deciding whether to gate releases on it.

### Dry-run an existing tag through `release.yml`

`release.yml` accepts a `workflow_dispatch` input so you can rebuild
the binary set for any existing tag without cutting a new one. This
is the recommended way to canary a PGO change (e.g. the v0.37 T4
darwin-arm64 re-enable) before tagging:

```bash
gh workflow run release.yml -f tag=v0.36.1
```

The input defaults to the last known-green tag, so omitting `-f tag`
re-runs the full matrix against it. The workflow will overwrite the
existing release's assets on success — keep that in mind if you're
dry-running against a real public release.

## Why ThinLTO + PGO together

Strictly speaking the `release-pgo` profile pins `lto = "fat"`, not
thin. The pipeline still benefits from the ThinLTO infrastructure
because `-Clinker-plugin-lto` lets the linker cross-LTO between
`rustc`-emitted bitcode and any LLVM-built static libs in the
dependency graph (e.g. `wasm-encoder`'s C dependency surface, when
present). The combination is:

- **PGO** decides *what* the optimiser should inline / outline based
  on real frequencies.
- **Fat LTO** gives the optimiser the *whole* program to work on,
  not one CGU at a time.
- **Linker-plugin LTO** extends that reach across the language
  boundary so the final binary layout is fully informed.

The trade-off is link time: a fat-LTO link of `mty-cli` takes ~30-60
seconds on a modern desktop, vs ~5-10 seconds for the default
ThinLTO release. That's why PGO is not on by default.

## Measured speedup

The CI workflow records measurements on its host (an Ubuntu
`runs-on` runner). Local numbers from the reference desktop are
captured in `dev/history/notes/PGO_V0_22_NOTES.md` as they're
collected.

The acceptance target for v0.22 was **12-20% wall-clock on `mty
check` + `mty build`**. If your local run lands outside that band
(either way) please attach the `target/pgo-profiles/merged.profdata`
to a bug — that's the most useful artifact for diagnosing under- or
over-fit profiles.

## Platform support

| Platform                       | CI PGO (cargo-pgo) | Local (build-pgo.{sh,ps1}) | Notes                                                              |
|--------------------------------|--------------------|----------------------------|--------------------------------------------------------------------|
| Linux x86_64 (ubuntu CI)       | yes                | yes                        | Reference platform; `pgo-bench.yml` + `release.yml` both run here. |
| Linux aarch64                  | no (cross-compile) | yes                        | release.yml falls back to plain `--release` via `cross`.           |
| macOS aarch64 (Apple Silicon)  | yes (v0.38)        | not recommended            | Local shell scripts hit the v0.37 raw=8/expected=10 mismatch.      |
| macOS x86_64 (Intel via cross) | no (cross-compile) | yes                        | release.yml falls back to plain `--release`.                       |
| Windows x86_64                 | yes                | yes                        | Use `build-pgo.ps1` locally; release.yml uses cargo-pgo on CI.     |

The toolchain-bundled `llvm-profdata` is what makes Windows work
without a system LLVM install. If you're cross-compiling, run the
collection step on the *target* triple — the `.profraw` shards are
not portable across CPU architectures.

## v0.23 follow-up: BOLT

The natural next step after PGO is **BOLT** (Binary Optimization and
Layout Tool), which does a second post-link layout pass driven by
hardware counters. BOLT typically squeezes another **3-7%** on top
of a PGO build. It's a heavier dependency (LLVM + perf + a static
build of BOLT itself) so v0.22 deliberately stops at PGO; the BOLT
integration goes in the v0.23 roadmap.

## See also

- `dev/history/notes/PGO_V0_22_NOTES.md` — implementation notes and
  measured numbers.
- `scripts/build-pgo.sh` / `scripts/build-pgo.ps1` — the pipeline.
- `crates/mty-bench/src/bin/mty-bench-pgo.rs` — collection driver.
- `.github/workflows/pgo-bench.yml` — manual measurement workflow.
