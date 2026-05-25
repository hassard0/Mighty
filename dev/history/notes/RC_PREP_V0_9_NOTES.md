# RC-Prep v0.9 Notes (overnight build, swarm agent 4 of 4)

Captures the interpretation calls + design decisions made during the
v0.9 → v1.0-RC prep work. Cross-reference: `KNOWN_ISSUES.md`,
`docs/internals/package-signing.md`, `.github/workflows/*.yml`.

## Scope (autonomous overnight build)

Six tasks, all in scope:

1. Fix `demos/02_counter_web` `cabi_realloc` regression. **DONE.**
2. GitHub Pages docs site (mkdocs-material). **DONE.**
3. Package signing (sigstore-style stub). **DONE.**
4. CI hardening (matrix + minimal + strict + MSRV + caches). **DONE.**
5. Release script (bash + powershell). **DONE.**
6. KNOWN_ISSUES.md. **DONE.**

## Task 1 — demo 02 cabi_realloc fix

### Root cause

`wit-component::ComponentEncoder::encode()` rejects any core module
whose WIT world contains an import returning an owned heap-allocated
value (`string`, `list<u8>`, `option<string>`, ...) unless the core
module exports a function named `cabi_realloc` with signature
`(i32, i32, i32, i32) -> i32`. The Mighty wasm32-web world has had
those imports since v0.5 (`dom.get-text`, `dom.query`), but the
`mty-codegen-wasm` emitter never emitted a `cabi_realloc` export.

This is the regression that flagged `pre-existing` in v0.7+v0.8 —
the smoke worked on earlier slices because the WIT world did not yet
have the string-returning imports.

### Fix

`crates/mty-codegen-wasm/src/emit.rs`:

- Added a mutable i32 global, initialized to `CABI_REALLOC_HEAP_BASE
  = 32768`, which serves as a bump pointer.
- Added a synthesized `cabi_realloc` function with the canonical
  signature; body performs the classic align-up bump-allocation:
  `bump = (bump + (align-1)) & ~(align-1); ptr = bump; bump += new;`
- Exported the function as `cabi_realloc`.

The canonical-ABI lifts we currently emit only ever call
`cabi_realloc` with `old_ptr == 0` (fresh allocations), so a bump
allocator suffices for v0.9. A real free-list / wee_alloc-style
buddy allocator is a v0.10 follow-up — flagged in `KNOWN_ISSUES.md#1`.

### Verification

- `bash demos/02_counter_web/smoke.sh` → **PASS** (component size
  1523 bytes; previously failed with "module does not export a
  function named `cabi_realloc`").
- `cargo test -p mty-codegen-wasm` → 11/11 unit tests still pass.

## Task 2 — Pages site

- `mkdocs.yml` at repo root: pinned theme to mkdocs-material with
  the deep-purple/amber palette. Pure-Python, no Ruby runtime
  needed on the CI runner.
- `.github/workflows/pages.yml`: builds + deploys on every push to
  main, with `concurrency.cancel-in-progress` so an in-flight build
  is cancelled when a new commit lands.
- Site URL: `https://hassard0.github.io/Mighty/`.
- Build verified locally with `mkdocs build --site-dir /tmp/site/`
  — produces `/tmp/site/index.html` plus per-tour/internals/spec
  pages.
- `--strict` is intentionally **not** enabled for v0.9. The docs
  corpus was assembled organically over 8 slices and includes a
  handful of stale RFC and example-source links. Flipping
  `--strict` on is a v0.10 follow-up (`KNOWN_ISSUES.md#5`).

## Task 3 — package signing

### Decision: stub vs. real sigstore

The user instruction explicitly authorised shipping signing as a
stub if the `sigstore` Rust crate's dep graph was surprising. It
is: tonic, Fulcio OpenAPI client, Rekor OpenAPI client, plus an
`openssl-sys` transitive on Windows. Shipping that into v0.9 would
have added ~10 minutes to a clean-cache build and a Windows-specific
OpenSSL footgun.

For v0.9 we ship a **deterministic SHA-256 envelope** with a
sigstore-compatible JSON sidecar. The envelope shape is documented
in `docs/internals/package-signing.md` and is forward-compatible
with the real signing path (just swap the `stub_signature` function
for an ECDSA call).

### Wire-up

- `crates/mty-pkg/src/signing.rs`: 6 tests, all passing
  (round-trip, determinism, tamper-bundle, tamper-signature, parse
  format, envelope JSON shape).
- `crates/mty-pkg/src/commands.rs`: `publish()` now calls
  `signing::sign_bundle(&outcome)` immediately after `publish::bundle`;
  the four-file artefact set is reported in both the "auth
  required" and "uploaded" messages.
- End-to-end verified by hand: `mty pkg publish` in a scratch
  package produced `smoketest-0.0.1.tar.gz{,.sha256,.sig,.bundle}`,
  and the `.bundle` JSON contains the documented sigstore-shape
  fields.

## Task 4 — CI hardening

`.github/workflows/ci.yml`:

| Job | Status | Notes |
| --- | --- | --- |
| `test (ubuntu/macos/windows)` | gate | + new demo smoke sweep (Linux only) |
| `test-minimal` | gate | `--no-default-features` on Linux |
| `clippy-strict` | advisory | `continue-on-error: true` for now; pedantic surface gated behind allow-list (`KNOWN_ISSUES.md#4`) |
| `msrv` | gate | builds (not tests) on Rust 1.85.0 |

Caching strategy:
- `actions/cache@v4` for `~/.cargo/registry/{index,cache}` +
  `~/.cargo/git/db` keyed on `Cargo.lock` hash.
- `Swatinem/rust-cache@v2` for `target/`, with `shared-key`
  per-job-type to avoid cross-job pollution.

The legacy CI workflow ran ubuntu+windows only on toolchain `1.82.0`
with rustfmt+clippy+build+test in a single job. The new workflow
splits those concerns + adds macOS.

## Task 5 — release scripts

`scripts/release.sh` + `scripts/release.ps1`:
- 6-step pipeline (clean tree → tests → bump version → changelog
  stub → commit/tag/push → optional publish).
- `--dry-run` / `-DryRun` short-circuits at step 2.
- Version bumper uses Python embedded in the bash script (we need
  reliable regex-with-lookbehind for the `[workspace.package]`
  block); the PowerShell version uses .NET `[regex]` for the same.
- Step 6 is a no-op in v0.9 ("marketplace upload comes in v0.10");
  it prints a hint to run `mty pkg publish` in any package root.

## Task 6 — KNOWN_ISSUES.md

7 entries across P0/P1/P2. Each one cross-references the slice
notes that introduced it and a v0.10/v1.0-RC fix plan. The P0
section is empty — demo 02 was the only release blocker, and it's
fixed.

## Gate status (final)

- `cargo fmt --all -- --check` → clean
- `cargo clippy --workspace --all-targets -- -D warnings` → clean
- `cargo test --workspace` → all green (~370 tests across 80
  binaries)
- `cargo test -p mty-codegen-wasm` → 11/11
- `cargo test -p mty-pkg` (incl. signing) → all green
- `bash demos/02_counter_web/smoke.sh` → **PASS**
- `python -c "import yaml; yaml.safe_load(open(...))"` on all 3
  new YAML files → clean
- `mkdocs build` → success (warnings about stale doc links;
  `--strict` deferred to v0.10)
- End-to-end `mty pkg publish` → produces `.tar.gz` + `.sha256` +
  `.sig` + `.bundle` sidecars

## Files touched

Owned (per the swarm-agent contract):
- `crates/mty-codegen-wasm/src/emit.rs` — cabi_realloc emission
- `crates/mty-pkg/src/signing.rs` — NEW
- `crates/mty-pkg/src/lib.rs` — register `signing` module
- `crates/mty-pkg/src/commands.rs` — wire signing into `publish()`
- `.github/workflows/ci.yml` — matrix + minimal + strict + msrv
- `.github/workflows/pages.yml` — NEW
- `mkdocs.yml` — NEW (repo root)
- `scripts/release.sh` — NEW
- `scripts/release.ps1` — NEW
- `KNOWN_ISSUES.md` — NEW (repo root)
- `docs/internals/package-signing.md` — NEW
- `RC_PREP_V0_9_NOTES.md` — this file
