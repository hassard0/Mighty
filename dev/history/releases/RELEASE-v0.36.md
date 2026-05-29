# Mighty v0.36 — Release Notes

**Tag:** `v0.36.0`
**Date:** 2026-05-29
**Status:** SHIPPED — fix-it-for-others.

**Headline:** **Mighty v0.36 — fix-it-for-others. Real native binaries
(U8 widening + dynamic log fixed), extern c + manifest static-lib
linking (FFI apps unblocked, IDE work unblocked), String position /
range ops, the long-promised STARDUST→MTY rename with backward
compat, Windows `cli-min` install path, macOS LC_BUILD_VERSION
warnings silenced, and PGO finally re-enabled on 3 platforms.**

The external reviewer in 2026-05's adoption survey called out five
things blocking real-world usage: (1) native codegen silently produced
wrong results on small unsigned types and crashed on dynamic log,
(2) extern c worked for the zero-arg case but the signature matrix was
undocumented and untested, (3) `String` had `find` / `slice` but no
position-finding / range-editing primitives, (4) the
`STARDUST_*` env-var prefix kept surfacing in error messages two
releases after the brand rename, and (5) Windows `cargo install mty`
exploded on a rusqlite C-build that wasn't relevant to the CLI. v0.36
crosses all five items off and brings PGO back online on three of the
five release platforms while we're at it.

Five tracks merge in parallel. The codegen-heavy ones (T1, T2, T5)
touched overlapping files but resolved cleanly under the `ort` merge
strategy. T1's `native_dynamic_log.rs` integration test needed a
two-field follow-up to match T2's new `BuildOptions { extern_libs,
manifest_dir }` shape; documented in the integrator commit.

## Track-by-track

### T1 — Native codegen fixes

Branch `v036-track-codegen`, merged at `c874a96`.

Three independent native-codegen bugs, all reachable through the
sample programs in `examples/39_native_binary.mty`.

**U8 widening (`sextend` → `uextend`).** Cranelift's `sextend`
sign-extends; for `U8 → I32` comparisons of values with the high bit
set (`200u8 == 200`), the LHS got sign-extended to `0xFFFFFFC8`
(`-56`) and the comparison silently flipped. The fix is mechanical:
unsigned source types dispatch to `uextend`, signed to `sextend`. Both
arms now share a tiny helper in `mty-codegen-cranelift/src/lower.rs`
and there's a dedicated test file
(`crates/mty-codegen-cranelift/tests/u8_widening.rs`, 11 cases) that
pins the widening map across all 12 integer types.

**Dynamic log lowering.** Pre-fix, `log(g)` for a non-literal `g: Str`
hit
`CodegenError::Unsupported("non-literal string in log/print")`
because the cranelift lowerer assumed every log argument was a
compile-time `Str` literal pinned to a fixed data symbol. The fix
allocates a 16-byte ptr+len stack slot, stores the runtime `(ptr,
len)` into it, and passes the slot address + 16 to the runtime's
`mty_log_dynamic` shim. Static literals still use the fast path; only
dynamic args pay the stack-slot cost. Tests in
`crates/mty-codegen-cranelift/tests/dynamic_log.rs` (8 cases) +
`crates/mty-driver/tests/native_dynamic_log.rs` (3 e2e cases).

**Hex / binary / octal literals with type suffixes.** The lexer
recognised `0xFF`, `0b1010`, `0o777` but not the suffixed forms
`0xFFu8`, `0b1010_u16`, `0o777i32`. T1 adds the suffix-parsing path
across all 12 integer types and pins type inference end-to-end.
`crates/mty-hir/tests/radix_literals.rs` (32 cases).

**Deferred to v0.37:** the LLVM backend has the same `sextend` /
`uextend` confusion in `crates/mty-codegen-llvm/src/lower.rs`. We
flagged the fix-path but didn't land it in T1 to keep the diff
focused on cranelift (which is the only backend currently shipping
on the release matrix).

+66 tests across the codegen, driver, and HIR crates.

### T2 — extern c matrix + [[extern_lib]] static-lib linking

Branch `v036-track-extern-c`, merged at `e983d82`.

Two parts: (1) lock in the C-ABI signature shapes that actually work,
and (2) let `mighty.toml` describe the libraries to link against.

**Signature matrix.** Eleven rows under `tests/extern_c_matrix/`,
each a paired `app.mty` + `impl.c` that exercises one calling
convention shape: nil/i32 return, two-i32 in, ptr-in (`&i32`),
out-ptr, struct-by-value, struct-by-ptr, return-struct, array-ptr
(`&[i32]`), str-in (`*const u8 + usize`), str-out (caller-owned
buffer), and fn-ptr (callback). Each row compiles the C impl with
`cc`, the mty driver links against it, and runs the binary. All 11
rows pass on the Linux x86_64 reference platform.

**[[extern_lib]] manifest.** `mighty.toml` now accepts:

```toml
[[extern_lib]]
name = "mathlib"
kind = "static"        # or "dynamic"
path = "vendor/libmath.a"

[extern_lib.link_args]
linux = ["-lm"]
macos = ["-framework", "Accelerate"]
windows = ["/DEFAULTLIB:msvcrt"]
```

Paths resolve against the manifest directory, the driver forwards
the platform-appropriate `link_args` to the host linker, and the new
public surfaces (`mty_driver::manifest::ExternLib`,
`mty_driver::build::build_linker_args`,
`mty_driver::BuildOptions::with_extern_libs`) are what the parallel
mighty-ide work depends on for the winit/wgpu extern c integration.

`BuildOptions` gains two required fields:
`extern_libs: Vec<ExternLib>` and `manifest_dir: Option<PathBuf>`.
Default-constructed via `BuildOptions::native` /
`BuildOptions::wasm`; explicit constructors via
`BuildOptions::with_extern_libs(libs, manifest_dir)`.

+35 tests.

### T3 — String position / range ops + char-boundary helpers + MT5080

Branch `v036-track-string-ops`, merged at `ba3c11a`.

Twelve new `String` methods, organised in three groups:

**Position-finding.** `rfind(pat)` (right-to-left search),
`position(pat)` (returns `Option<usize>` byte offset). Both run in
linear time on the UTF-8 bytes; `position` is a thin wrapper over
`find` kept separate so `rfind` and `position` can be wrapped
independently from JS / Python bindings.

**Range editing.** `insert_at(byte_idx, s)`, `remove_range(start..end)`,
`replace_range(start..end, s)`. All three validate that
`byte_idx` / `start` / `end` are char boundaries and raise MT5080
otherwise.

**Char-boundary helpers.** `is_char_boundary(byte_idx)`,
`next_char_boundary(byte_idx)`, `prev_char_boundary(byte_idx)`. These
make MT5080 quickfixes cheap to compute. `char_indices()` returns the
canonical `(byte_offset, char)` iterator.

**MT5080 — Range edit at non-char-boundary.** Diagnostic carries the
byte offset that was the problem and the nearest valid boundary in
the hint (e.g. `byte 3 is inside the 4-byte char 'é'; valid
boundaries are 2 and 6`).

Stdlib touchpoints in `crates/mty-stdlib/src/string.rs` (+536 lines)
and the interpreter codegen in `crates/mty-ir/src/interp/run.rs`
(+152 lines). Docstub coverage in
`crates/mty-stdlib/docs/string.docstub` + examples in
`examples/40_string_editing.mty`.

+44 tests.

### T4 — Stardust → Mighty rename sweep

Branch `v036-track-stardust-sweep`, merged at `8d237ed`.

Categorical rename across six env vars, three identifier surfaces,
and the WIT package id. **Backward compat preserved everywhere.**

**Env vars.** New `MTY_LINKER`, `MTY_OTLP_ENDPOINT`, `MTY_TRACE`,
`MTY_RUNTIME_THREADS`, `MTY_CONF_ONLY`, `MTY_CONF_CASE`. The legacy
`STARDUST_*` spellings continue to resolve via the new
`mty_runtime::env_compat::lookup_env` shim with precedence (1)
`MTY_<KEY>` if set non-empty, (2) `STARDUST_<KEY>` if set non-empty
(emits a one-shot stderr warning per legacy key per process),
(3) `None`. The shim is the single source of truth — every previous
direct `std::env::var("STARDUST_…")` callsite was migrated.

**WIT package id.** `wit_package_id` now emits `mty:component@…`
and `mty:effects@…`; the parser still accepts incoming
`stardust:component@…` imports for one release cycle.

**Cranelift object segment.** Renamed from `b"stardust"` to
`b"mighty"`. Historical readers may still match the legacy bytes via
the `accepted_segment_name` helper.

**OTLP spans.** Renamed `stardust.*` → `mty.*`. Legacy consumers can
opt in to the rename hop by reading `mty.legacy_name` on the span,
which carries the pre-rename string for one release (slated for
removal in v0.40).

**Default registry slug.** `mighty-pkg/registry`. The
`stardust-pkg/registry` slug still resolves through the registry's
HTTP backend.

**Reference count.** Pre-T4: 121 mentions of `stardust` across
`*.rs`/`*.toml`/`*.md`/`*.mty`/`*.sh`. Post-T4: 60 — 32 in
`dev/history/` (intentional release / slice / rebrand history), 5 in
`docs/spec/` (v1.0-rc + amendments + spec CHANGELOG, intentional
history), and 23 in live-code compat paths (`env_compat.rs`,
`object.rs`, `lower.rs`, `wit.rs`, `registry.rs`, `otlp.rs`, +
bench/doc/extract surfaces).

+18 tests (env_compat unit tests + otlp legacy-name carry test +
WIT parser backward-read test).

### T5 — Windows cli-min + macOS LC_BUILD_VERSION + PGO re-enable

Branch `v036-track-infra-papercuts`, merged at `d1fed00`.

Three independent infra papercuts, each reported by a distinct
real-world user.

**Windows `cargo install mty`.** Pre-fix: defaulted to
`--features host-toolchain` which transitively pulled in
`observe-sqlite` (rusqlite C build). Windows users without MSVC saw
`link.exe not found` and were stuck. Fix: split `observe-sqlite` into
its own top-level mty-cli feature; introduce the alias
`cli-min = []` (everything host-toolchain provides, minus the
sqlite build). The supported Windows install line is

```
cargo install mty --no-default-features --features cli-min
```

`mty inspect --cost` correctly reports `observe-sqlite: disabled`
when the feature is off (rather than crashing). FAQ entry updated.

**macOS LC_BUILD_VERSION.** Pre-fix, the cranelift-object Mach-O
emitter defaulted `Darwin(_)` to `PLATFORM_UNKNOWN (0)` with no
versions, and Apple's `ld` warned
`object file has malformed LC_BUILD_VERSION (platform=0...)` on every
link. The fix overrides the default with `PLATFORM_MACOS + minos=11.0
+ sdk=14.0` packed in the nibble layout `loader.h` documents
(`(X << 16) | (Y << 8) | Z`). Honors `MACOSX_DEPLOYMENT_TARGET` (the
same env var rustc reads) and a new `MTY_MACOSX_SDK_VERSION` knob for
users with non-default SDK installs.

**PGO re-enabled on linux-x86_64, darwin-arm64, windows-x86_64.**
v0.33 deferred PGO. v0.35.0 + v0.35.1 tried to re-enable it and hit
two distinct bugs across Release. v0.35.2 disabled it again. v0.36 T5
diagnoses and fixes both:

1. **`linux-x86_64` `LLVM ERROR: Broken module found, module flag
   identifiers must be unique !"CG Profile"`** during Phase 4
   profile-use + LTO link. Root cause: `-Clinker-plugin-lto` and
   PGO's CG-Profile metadata both register a `!"CG Profile"` module
   flag and LLVM's verifier rejects the duplicate. Fix:
   drop `-Clinker-plugin-lto` from Phase 4 (rustc's full LTO via the
   `release-pgo` profile is sufficient; the linker-plugin variant was
   a v0.22-era hold-over).
2. **`darwin-arm64` + `windows-x86_64` `raw=8 vs expected=10` profile
   format mismatch.** Root cause: cached
   `target/release-pgo/{build,deps,incremental,.fingerprint}` from a
   previous run carried `-Cprofile-use` codegen incompatible with
   the fresh profile. Fix: new Phase 0 in
   `scripts/build-pgo.{sh,ps1}` that wipes those four dirs before
   the instrumented build, and `release.yml` cache keys segregate
   PGO vs non-PGO so restore-keys can't cross-contaminate.

`darwin-x86_64` and `linux-aarch64` stay `use_pgo: false` (no
native runner can execute the instrumented binary — rosetta on x86_64
macOS produces wrong-shape profiles, and cross-compiled aarch64
binaries can't run on the x86 build host).

+21 tests across `crates/mty-cli/tests/cargo_features_windows.rs`,
`crates/mty-cli/tests/pgo_scripts.rs`, and the macos LC_BUILD_VERSION
unit suite in `crates/mty-codegen-cranelift/src/object.rs`.

## Integrator notes

- **Conflict zone forecast vs reality.** All 5 tracks auto-merged
  cleanly with the `ort` 3-way merge driver. The expected hot zones
  in `mty-codegen-cranelift/src/object.rs` (T1+T4+T5) and
  `crates/mty-runtime/src/otlp.rs` (T4) resolved without manual
  intervention because the tracks edited distinct hunks.
- **`BuildOptions` field reconciliation.** T2 added required fields
  `extern_libs` + `manifest_dir`. T1's
  `crates/mty-driver/tests/native_dynamic_log.rs` was written before
  T2 landed and constructed `BuildOptions` with the old shape. The
  integrator commit `5cc00aa` adds the two `..::default()`-style
  fields to the T1 test struct literal. All other `BuildOptions`
  construction sites already include the new fields.
- **PGO sanity check.** Ran `bash scripts/build-pgo.sh` on the
  Linux x86_64 reference host (vulcan, 4×V100, rust 1.95.0,
  llvm-tools-preview). Outcome: see "Release verification" below.

## Test counts

- Pre-v0.36: 3017 workspace tests (v0.35.5 baseline)
- Post-v0.36: 3176 workspace tests (vulcan)
- Net delta: +159 tests across the 5 tracks + integrator fix

(The +184 estimate in the integrator brief over-counted some
auto-generated test sub-cases that the harness aggregates under a
single `test result:` line.)

## CI / Release matrix

- 6 CI workflows: build / test (default + minimal features) / clippy
  / fmt / audit / fuzz-smoke
- 5 release-binary platforms: `linux-x86_64`, `linux-aarch64`,
  `darwin-arm64`, `darwin-x86_64`, `windows-x86_64`
- 3 platforms with PGO enabled: `linux-x86_64`, `darwin-arm64`,
  `windows-x86_64`
- 2 platforms without PGO: `linux-aarch64` (cross-compile),
  `darwin-x86_64` (rosetta)

## v0.37 backlog

See `CHANGELOG.md` `[Unreleased]` for the full per-track list.
Headlines:

- **T1 follow-up** — LLVM backend U8 widening fix
- **T2 follow-up** — `cdylib` linkage shape + mty bindgen generator
- **T3 follow-up** — `String::splitn` / `rsplitn` + richer MT5080
  quickfix
- **T4 follow-up** — final docs sweep + OTLP `legacy_name`
  removal hop (v0.40)
- **T5 follow-up** — PGO on `darwin-x86_64` + `linux-aarch64` (needs
  native runners); Docker publish toggle; Homebrew-core PR
- **Cross-cutting** — `SCHEMA_VERSION` crate-root re-export;
  vulcan disk hygiene automation; `mty find` semantic search

## External reviewer checklist

The 2026-05 adoption survey flagged five items. v0.36 closes all five:

- [x] Native codegen silently wrong on small unsigned types (T1)
- [x] extern c matrix undocumented / untested (T2)
- [x] String missing position-finding / range-editing primitives (T3)
- [x] STARDUST_* env-var prefix surfacing two releases post-rename (T4)
- [x] Windows `cargo install mty` exploding on rusqlite C build (T5)

LLVM-backend signedness fix is the one bonus item we identified
mid-T1 and deferred to v0.37 (cranelift is the only backend on the
release matrix today; LLVM is dev-only).
