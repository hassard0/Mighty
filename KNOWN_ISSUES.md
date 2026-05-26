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

## Spec-polish carryovers (surfaced v0.11, resolved v0.12)

### 10. Operator precedence not normative (resolved v1.0-RC3)

- **Where**: `docs/spec/v1.0-rc.md` §11.1 previously cross-referenced
  `docs/internals/parser.md` for exact operator precedence.
- **Symptom**: an independent implementer building only from the
  normative spec (e.g. the Python 2nd impl shipped in v0.11) cannot
  determine the precedence ladder for `&`, `|`, `^`, `<<`/`>>`, the
  comparison family, or `&&`/`||`. Cross-impl determinism breaks.
- **Resolution (v1.0-RC3)**: the Pratt precedence table from
  `crates/mty-syntax/src/parser/exprs.rs::infix_bp` is promoted
  verbatim to a new normative subsection §11.1.1. The
  internals doc now points at the spec for the authoritative table.
- **Status**: **resolved in v1.0-RC3** (v0.12, 2026-05-25).

### 11. Six FROZEN typeck codes are constructor-only (resolved v1.0-RC4)

- **Where**: `crates/mty-diagnostics/src/codes.rs` defines MT2003,
  MT2009, MT2022, MT2023, MT2024, MT2025 with full explain text;
  `docs/spec/conformance-coverage.md` lists them as `gap` (constructor
  only). No emit site exists in any crate.
- **Symptom**: the spec promises diagnostics that the v1.0 compiler
  does not produce; conformance coverage stays at 91%.
- **RC3 disposition (v0.12)**: every code reviewed and retained as
  "FROZEN — emit-site landing in v1.x" (the conditions are all real
  user-facing concerns; today's compiler funnels them into more
  general codes). The spec adds a new §33.1 documenting per-code
  current behaviour and the v1.x emit-landing plan. Code-points and
  explain text are stable; implementations MUST NOT recycle them.
- **Per-code emit-landing actions (closure history)**:

  | Code   | Today's funnel               | Action shipped                                                       |
  |--------|------------------------------|----------------------------------------------------------------------|
  | MT2003 | `{integer}`/`{float}` placeholder | empty-container shape in `check_stmt(HirStmt::Let)` (v0.14, commit `e5fb928`) |
  | MT2009 | MT2007 / MT2021              | enum-aware resolver split in `synth_path` (v0.12, commit `b05fe8f`)  |
  | MT2022 | MT2002                       | struct-init kind check in `synth_struct_literal` (v0.12, `b05fe8f`)  |
  | MT2023 | not reachable                | value-kind in type-arg position in `resolve_def_to_ty` (v0.14, `e5fb928`) — refined from "lifetime kind landing" since Mighty lifetimes are inferred not surface-syntax |
  | MT2024 | MT2005                       | lambda-arity refinement in `check_expr` (v0.12, `b05fe8f`)           |
  | MT2025 | implicit-promotion swallow   | stricter borrow pass in `HirExpr::Borrow` (v0.12, `b05fe8f`)         |
- **Status**: **resolved in v1.0-RC4** (v0.14, 2026-05-25). All six
  codes now have positive-fire conformance fixtures under
  `tests/conformance/type_checking/` (17, 18, 19, 20 from v0.12;
  03 and 21 from v0.14). The spec's §33.1 entry is superseded —
  these codes are now first-class typeck diagnostics, not deferred.

### 12. `package`, `export`, `requires` missing from §3.3 keyword set (resolved v1.0-RC3)

- **Where**: `docs/spec/v1.0-rc.md` §3.3 omitted these three keywords
  (plus 17 others actually present in
  `crates/mty-syntax/src/syntax_kind.rs`). The example corpus uses
  all three:
  - `examples/19_backend_service.mty` starts with `package search_api`
  - `examples/14_extern_c.mty` ships `export c fn _add(...)`
  - `examples/17_unsafe.mty` ships `unsafe fn ... requires <expr>`
- **Symptom**: a spec-only implementer (Python 2nd impl) cannot lex
  these example files because §3.3 does not list the words as
  keywords.
- **Resolution (v1.0-RC3)**: §3.3 is rewritten into three subsections:
  §3.3.1 reserved keywords (the full 63-word v1.0 list — including the
  two boolean literals — with all previously-missing lexer keywords
  added), §3.3.2 contextual keywords (the four
  positions where the parser upgrades an IDENT to a keyword), and
  §3.3.3 reserved for future use (`and`, `or`, `init`, `deinit`,
  `panic`, `static`, `union` — names the spec reserves but the v1.0
  lexer does NOT tokenise as keywords). §4.1 documents `package`
  syntax; §21.1 documents `requires` clauses; §26.2 documents
  `export <abi> fn`.
- **Status**: **resolved in v1.0-RC3** (v0.12, 2026-05-25).

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
