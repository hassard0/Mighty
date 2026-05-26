# Conformance — kit, drivers, and harness (internals, v0.19)

**Spec doc:** `docs/spec/conformance.md` (normative).
**Kit manifest:** `tests/conformance/CONFORMANCE_KIT.md`.
**Builder script:** `scripts/build-conformance-kit.sh`.
**Test drivers:** `crates/mty-driver/tests/conformance_*.rs`.

This document covers the *implementation* side of Mighty conformance:
how the corpus on disk maps to test drivers, how the kit tarball is
built, and which drivers cover which categories. The companion
normative document (`docs/spec/conformance.md`) defines what
conformance *means* — read that first for the contract; read this for
how the Rust workspace produces and consumes the evidence.

## On-disk corpus

```
tests/conformance/
  README.md                — corpus overview (status of each category)
  CONFORMANCE_KIT.md       — kit manifest (categories, counts, version policy)
  <category>/
    <NN_case_name>/
      input.mty                  — source under test
      command.txt                — one of: check, run
      expected_diagnostics.txt   — optional, set of MTxxxx codes
      expected_stdout.txt        — optional, exact stdout (for `run`)
      expected_exit_code.txt     — optional, defaults to 0
```

The case ID is the *parent-directory name* (`<NN_case_name>`). The
two-digit prefix exists for human ordering only; the harness ignores
it. A renumbering is a non-breaking change as long as the case name
stays stable (the prefix is not part of the conformance contract).

## Categories at v0.19

The corpus carries **122 cases across 24 categories** (20 populated
+ 4 placeholder). The per-category cell count is the source of truth
for `docs/spec/conformance.md` §4. When adding a case, regenerate the
kit (see below) so the manifest and the spec doc stay in sync.

Populated categories drive the three live harness binaries
(`conformance_full`, `conformance_runtime`, `conformance_runtime_7`)
plus the WASM-component / native-codegen smoke
(`conformance_codegen`). Placeholder categories
(`deterministic_replay/`, `formatter_idempotence/`, `native_abi/`,
`wasm_component/`) are v1.0-backlog: spec wording lands first; the
case files arrive in v0.20 → v1.0-RC1.

## Test drivers

Four harness binaries live under `crates/mty-driver/tests/`. Together
they walk the populated corpus and assert the spec-defined
diagnostic + stdout + exit-code contracts.

| Driver                          | Categories                                                                                                                                                       | Notes                                              |
|---------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------|----------------------------------------------------|
| `conformance_full.rs`           | `lexical/`, `parser/`, `type_checking/`, `type_inference/`, `borrow_checking/`, `ownership_rejection/`, `effect_checking/`, `traits_derive/`, `macros/`, others… | Phase-1 harness (no runtime). Uses the SIR interpreter for `run` cases — deterministic, no tokio. |
| `conformance_runtime.rs`        | `agent_protocol/`, `mailbox_ordering/`, `supervisor_restart/`, `runtime/`, `runtime_traps/`, …                                                                   | Full agent runtime (tokio).                        |
| `conformance_runtime_7.rs`      | `runtime-7/`                                                                                                                                                     | v0.7-specific runtime additions.                   |
| `conformance_codegen.rs`        | `codegen/` + per-example sweeps (native, WASM core, WASM Component)                                                                                              | Builds + (where applicable) executes the artifact. |

The drivers all read the same case shape, so adding a `run` case to
`runtime/` doesn't require editing the harness — the next test run
picks it up automatically.

### Diagnostic comparison contract

The harness asserts **set membership, not equality**: every code listed
in `expected_diagnostics.txt` must appear in the produced error
diagnostics, but extra codes are tolerated. This keeps the corpus
robust against compiler enrichment (a typeck pass that adds a follow-up
`MT2018` hint to an `MT2001` site does not break conformance).

Exit-code comparison is exact when `expected_exit_code.txt` is
present; absent, the harness defaults to `0` for `run` cases and
`!= 0` for `check` cases that list any expected diagnostic.

### Intentionally-ignored cases

The harness keeps a small `INTENTIONALLY_IGNORED` allow-list for
cases that are intentionally broken pending an upstream fix. v0.19
inherits two from v0.18:

- `capability_checking/03_narrow_to_ro` — pending cap-name resolver
  wiring (post-v1.0 backlog).
- `supervisor_restart/02_escalate` — pending the escalation-chain
  serialisation rework (post-v1.0 backlog).

Adding a case to `INTENTIONALLY_IGNORED` requires a rationale comment
naming the upstream tracking notes file.

## Building the kit

```
bash scripts/build-conformance-kit.sh [version]
```

The script packages the corpus + spec docs + the kit manifest into
`mty-conformance-kit-<version>.tar.gz`. Default version is
`git describe`; pass an explicit version for tagged releases (e.g.
`bash scripts/build-conformance-kit.sh v1.0.0`). Tarball layout:

```
mty-conformance-kit-<version>/
  tests/conformance/                     — full corpus
  docs/spec/v1.0-rc.md                   — language reference
  docs/spec/conformance.md               — normative conformance doc
  CONFORMANCE_KIT.md                     — manifest (categories, version)
```

The v0.19 kit weighs in at **~92 K compressed** with 645 tarball
entries (corpus + spec + manifest). The kit is attached to every
GitHub release from v0.19 onward.

## Cross-implementation conformance

The corpus is consumed by:

1. The Rust reference compiler (this repo) — the four `conformance_*`
   harness binaries above.
2. The Python 2nd-impl at [`impl-py/`](../../impl-py/) — via
   `python -m pytest tests/test_examples.py` and the per-category
   tests in `impl-py/tests/test_typeck.py` /
   `test_examples_typeck.py`. The Python impl claims conformance to
   the categories its scope covers (front-end + HIR + typeck through
   v0.19); runtime / codegen / borrow categories are explicitly out
   of v1.0 scope for the 2nd-impl.
3. Future independent implementations — the kit format is the
   contract.

The diagnostic-band-match policy (RFC 2119 §6.1) lets each
implementation pick its own exact MTxxxx code within the same band
(`MT[0-6]xxx`). The corpus' `expected_diagnostics.txt` files name the
*expected* code, and the spec-§6.1 prose allows any other code in the
same band to substitute if its semantics match.

## Version policy

The kit version is the spec version. v0.19 ships
`mty-conformance-kit-v0.19.tar.gz`; v1.0 will ship
`mty-conformance-kit-v1.0.0.tar.gz`. The script writes the version
into the tarball's filename and inside the embedded `CONFORMANCE_KIT.md`
header. A kit consumer can therefore self-describe its provenance
without consulting the GitHub release page.

Kit versions advance only when the corpus or spec changes — a
mty-codegen-only release that doesn't touch `tests/conformance/` or
`docs/spec/` keeps the prior kit. The CI release workflow re-runs the
builder on every tag push so the kit's filename always matches the
git tag.

## Adding a new case

1. Pick the category. If a new category is needed, add it to
   `CONFORMANCE_KIT.md` (the table is the v1.0 contract — bump the
   count delta in the slice notes).
2. Create `tests/conformance/<category>/NN_case_name/`. Two-digit
   prefix; underscored snake case for the name.
3. Drop `input.mty`, `command.txt`, and any expected outputs.
4. `cargo test -p mty-driver --test conformance_full` (or whichever
   harness owns the category) and confirm the new case is in the
   walked list and passes.
5. Update the category count in `CONFORMANCE_KIT.md` and in the
   spec's `docs/spec/conformance.md` §4 table.
6. Rebuild the kit and verify the tarball includes the new case file.

## v0.20 follow-ups

* Move the placeholder categories to populated. v0.20 is the first
  populated landing for `formatter_idempotence/` (the formatter is
  already fuzz-covered; the conformance shape is the prose
  formalisation).
* Diagnostic-code coverage report — surface "which MTxxxx codes are
  covered by *at least one* conformance case" so the corpus' coverage
  of the diagnostic registry is visible without spelunking. Tracked
  in the `tests/conformance/coverage.json` v0.20 plan.
* Implementer-CLI shim — a `mty conform <kit.tar.gz>` sub-command
  that an arbitrary implementation can wrap to drive its own harness
  without re-implementing the case walker.
