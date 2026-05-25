# Mighty — Known Issues (v0.9 → v1.0-RC)

This is the running catalog of known issues for the v0.9 → v1.0-RC
release window. Each entry links back to the originating slice's
`SLICE_V0_*.md` notes and (when known) the v1.0-RC follow-up plan.

A "workaround" entry lists the user-visible escape hatch; an "owner"
column flags the team/area that owns the fix.

---

## P0 — release blockers

_None outstanding for v0.9._ Demo 02 regression (see below) was
fixed in this prep; the rest are P1 or below.

---

## P1 — should-fix before v1.0-RC

### 1. `cabi_realloc` is a bump allocator with no `free`

- **Where**: `crates/mty-codegen-wasm/src/emit.rs::build_cabi_realloc_body`
- **Symptom**: long-running components that repeatedly invoke
  string-returning DOM imports (`dom.get-text`, `dom.query`) will
  grow linear memory monotonically. The 16-page (1 MiB) initial
  memory is enough for the v0.9 demos; pathological cases will need
  `memory.grow`.
- **Workaround**: nothing user-visible. The bump pointer wraps via
  the legacy `RETURN_BUF` region for the cases the JS shim cares
  about (see `dom-shim.js::writeStringToReturnArea`).
- **Fix plan (v0.10)**: replace with a real free-list / `wee_alloc`-
  style buddy allocator emitted as a small precompiled module
  imported by the codegen, *or* generate `cabi_realloc` from a
  vendored Rust source via `cargo-component` and link it in.

### 2. Package signing is a stub (no real OIDC / Rekor)

- **Where**: `crates/mty-pkg/src/signing.rs`
- **Symptom**: `mty pkg publish` produces `.sig` + `.bundle`
  sidecars that are **deterministic SHA-256 envelopes**, not real
  sigstore artifacts. They detect bundle tampering but offer no
  cryptographic identity guarantee.
- **Workaround**: treat the `.sig` file as advisory until v0.10.
  Re-verify a downloaded bundle with `mty pkg fetch` (verification
  is wired through `signing::verify_bundle` already).
- **Fix plan (v0.10)**: feature-gate the real sigstore path
  (`sigstore-real`); on GitHub Actions, fetch the OIDC token from
  `$ACTIONS_ID_TOKEN_REQUEST_URL`, exchange with Fulcio for a
  short-lived cert, sign with ECDSA, upload signing payload to
  Rekor, and embed the Rekor entry index in the `.bundle` envelope.

### 3. MSRV gate uses `cargo build`, not `cargo test`

- **Where**: `.github/workflows/ci.yml::msrv`
- **Symptom**: a dev-dep that requires a newer Rust silently
  bumps the floor without CI catching it (because we only build,
  not test, on MSRV).
- **Workaround**: bump cautiously; verify locally with `rustup
  override set 1.85.0 && cargo test --workspace`.
- **Fix plan**: use `--profile minimal` + a hermetic dev-dep
  resolution and run tests on MSRV. Tracking issue when filed.

### 4. `clippy-strict` job is `continue-on-error: true`

- **Where**: `.github/workflows/ci.yml::clippy-strict`
- **Symptom**: pedantic lint regressions slip past CI.
- **Workaround**: run locally before pushing —
  `cargo clippy --workspace --all-targets -- -D warnings -W clippy::pedantic`.
- **Fix plan**: shrink the allow-list iteratively until the job can
  be flipped to `continue-on-error: false` (target v1.0-RC).

---

## P2 — quality-of-life

### 5. `mkdocs build` runs without `--strict`

- **Where**: `.github/workflows/pages.yml`
- **Symptom**: broken intra-doc links don't fail CI.
- **Workaround**: build locally with `mkdocs build --strict` before
  big doc reorgs.
- **Fix plan (v0.10)**: audit + fix every stale link, then flip
  `--strict` on.

### 6. Demo 02 `web/index.html` does not yet exercise the new realloc

- **Where**: `demos/02_counter_web/web/dom-shim.js`
- **Symptom**: the JS shim still writes into the *fixed*
  `DOM_RETURN_AREA` at 8208 — it ignores the host-allocator pointer
  returned by `cabi_realloc`. This works because the canonical-ABI
  string lift on the JS side parses the same `(ptr, len)` pair
  format.
- **Workaround**: none needed; smoke passes.
- **Fix plan (v0.10)**: refactor `dom-shim.js` to call
  `instance.exports.cabi_realloc()` for each string return — would
  align fully with the canonical-ABI spec.

### 7. `--no-default-features` test job does not run the example sweep

- **Where**: `.github/workflows/ci.yml::test-minimal`
- **Symptom**: a feature-gated example regression goes undetected
  until someone runs the full matrix.
- **Workaround**: rerun the default test job for any change that
  touches `[features]` blocks.
- **Fix plan**: add an `examples-sweep` step under `test-minimal`.

---

## Reference: per-slice known-issues sections

Each slice's notes file at the repo root carries its own
"known issues" section; this document deduplicates and re-prioritizes
across slices.

- `SLICE_V0_2.md` — early codegen quirks (mostly resolved by v0.4)
- `SLICE_V0_3.md` — borrow-checker conservatism
- `SLICE_V0_4.md` — registry + demos
- `SLICE_V0_5.md` — DOM/web shim v1
- `SLICE_V0_6.md` — scheduler bring-up
- `SLICE_V0_8.md` — selfhost HIR + canonical-ABI return area
- `LOOSE_ENDS_V0_8.md` — v0.8 loose-end log
- `PERF_V0_8_NOTES.md` — perf microbenchmarks + honest-reset gotcha

When a slice's known-issue is fixed, strike it through there and
remove it from this file.
