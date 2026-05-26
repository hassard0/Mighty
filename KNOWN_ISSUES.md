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

### 1. `cabi_realloc` is a bump allocator with no `free` (RESOLVED v0.18)

- **Where**: `crates/mty-codegen-wasm/src/cabi_realloc.rs`
  (extracted from `emit.rs::build_cabi_realloc_body` in v0.18).
- **Status**: **RESOLVED**. The free-list allocator landed inline
  in v0.10; v0.18 extracts it to its own module + adds focused
  coverage tests (`tests/cabi_realloc.rs`, 8 tests; existing
  `tests/cabi_realloc_real.rs` keeps its 9 tests).
- **Implementation**: segregated free-list with 8 size classes
  (8B → 1024B, powers of 2) + a "large" bump path for requests
  > 1024B. State region at linear-memory `[32768, 32800)` holds
  one i32 free-list head per class; the link in each free block is
  the first 4 bytes (LIFO). Realloc copies `min(old_size, new)`
  bytes byte-by-byte and pushes `old` onto its class's free list.
- **Follow-ups (v0.19)**: see
  `dev/history/notes/CABI_REALLOC_V0_18_NOTES.md` — per-component
  allocator tuning, large-path coalescing, true in-place realloc
  when the size class wouldn't change.

### 2. Package signing is a stub (no real OIDC / Rekor) (RESOLVED v0.18)

- **Where**: `crates/mty-pkg/src/signing.rs`
- **Status**: **RESOLVED** in v0.18 (2026-05-26). The
  `sigstore-real` cargo feature now drives the real keyless flow:
  GitHub Actions OIDC → Fulcio short-lived ECDSA-P256 cert → Rekor
  `hashedrekord` transparency-log upload → full standard Sigstore
  Bundle JSON embedded under `verificationMaterial.sigstoreBundle`
  in the `.bundle` envelope. External tooling (`cosign verify-blob`,
  `rekor-cli`) consumes the embedded Bundle directly.
- **Implementation**: `sign_keyless` drives sigstore 0.14's
  `bundle::sign::SigningContext::async_production` against the
  public-good Sigstore deployment. The session generates an
  ephemeral ECDSA-P256 keypair, exchanges it (with the OIDC JWT)
  at `https://fulcio.sigstore.dev/api/v1/signingCert` for a
  ~10-minute cert, signs the bundle digest, and uploads the
  `hashedrekord` to `https://rekor.sigstore.dev/api/v1/log/entries`.
  `verify_bundle` cross-checks the embedded `messageDigest` against
  the recomputed bundle SHA-256 even on default builds (no
  `sigstore-real` feature needed for the consumer side). Three new
  structural verify tests live in `crates/mty-pkg/tests/signing_real.rs`.
  Default builds keep the deterministic stub envelope so Windows
  hosts without NASM still ship.
- **Follow-ups (v0.19)**: see
  `dev/history/notes/SIGSTORE_V0_18_NOTES.md` — full cryptographic
  cert-chain + Rekor inclusion-proof verify on `fetch`, device-flow
  OAuth for local signing, SLSA v1.0 provenance attestations, CI
  smoke against the public Sigstore trust root.

### 3. MSRV gate uses `cargo build`, not `cargo test` (resolved v0.18)

- **Where**: `.github/workflows/ci.yml::msrv`
- **Symptom**: a dev-dep that requires a newer Rust silently
  bumped the floor without CI catching it (because we only built,
  not test-compiled, on MSRV).
- **Resolution (v0.18)**: the MSRV job now runs `cargo build
  --workspace --tests` — a strictly larger compile surface than
  the old `cargo build --workspace` (covers every `[dev-dependencies]`
  graph too) — and continues to actually `cargo test` the bedrock
  crates (`mty-syntax`, `mty-types`, `mty-fmt`, `mty-diagnostics`)
  so behaviour regressions tied to the MSRV toolchain still get
  caught. The previous bare `cargo build --workspace` and the
  redundant `cargo test --workspace --no-run` steps are folded
  into the single `--tests` invocation.
- **Status**: **resolved in v0.18** — see commit history for the
  `ci.yml::msrv` block reshuffle.

### 4. `clippy-strict` job is `continue-on-error: true` (RESOLVED v0.11; re-verified v0.19)

- **Where**: `.github/workflows/ci.yml::clippy-strict`
- **Symptom**: pedantic lint regressions slipped past CI.
- **Status**: **RESOLVED**. v0.11 shrank the workspace allow-list far
  enough to flip the job to required; v0.19 audited the workflow
  file and confirmed there is no `continue-on-error: true` on
  `clippy-strict` (the comment block on the job spells out the gate
  is now hard-required). A clippy failure blocks merge.
- **Local repro**: `cargo clippy --workspace --all-targets -- -D
  warnings` — same command the CI job runs.

---

## P2 — quality-of-life

### 5. `mkdocs build` runs without `--strict` (RESOLVED v0.10; re-verified v0.19)

- **Where**: `.github/workflows/pages.yml`
- **Symptom**: broken intra-doc links didn't fail CI.
- **Status**: **RESOLVED**. v0.10 audited + fixed ~55 stale links and
  flipped the build step to `mkdocs build --strict`. v0.19 re-verified
  the workflow file still runs in strict mode (the step name is
  literally `mkdocs build (strict)` with `--strict --site-dir site/`).
- **Local repro**: `mkdocs build --strict` — same flag the CI job
  uses.

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

### 7. `--no-default-features` test job does not run the example sweep (RESOLVED v0.19)

- **Where**: `.github/workflows/ci.yml::test-minimal`
- **Symptom**: a feature-gated example regression went undetected
  until someone ran the full matrix.
- **Status**: **RESOLVED** in v0.19. The `test-minimal` job grew an
  `example sweep (no-default-features)` step that mirrors the
  default-features sweep (skipping `@typeck-pending` files) but
  invokes `cargo run --no-default-features -p mty-cli -- check
  <file>`. A `#[cfg(feature = "...")]` reach from the example
  corpus into an opt-in stdlib path now fails CI.
- **Local repro**: `cargo run --no-default-features -p mty-cli --
  check examples/<file>.mty` for each example.

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
